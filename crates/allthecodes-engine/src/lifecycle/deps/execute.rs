use super::*;

impl QueryEngineDeps {
    // Production implementation of the canonical query-loop tool execution
    // boundary declared in `QueryDeps`. Main-loop and future stream-time
    // scheduling must stay routed here so permission, hook, progress, audit,
    // security, and result handling remain single-sourced.
    pub(crate) async fn execute_tool_impl(
        &self,
        request: ToolExecRequest,
        tools: &Tools,
        parent_message: &crate::types::message::AssistantMessage,
        on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolExecResult> {
        use crate::types::tool::PermissionResult;
        use allthecodes_types::hooks::{PermissionOverride, PostToolHookResult, PreToolHookResult};

        // Hook dispatcher trait object — decouples the engine from the concrete
        // concrete shell-hook runner (see issue #74, full-build parity).
        let hooks = self.hook_runner.as_ref();

        let tool = find_tool(&request.tool_name, tools)
            .ok_or_else(|| anyhow::anyhow!("tool not found: {}", request.tool_name))?;

        let available_tools = self.submit_tools.clone().unwrap_or_else(|| {
            let state = self.state.read();
            let app_state = self.get_app_state();
            let capability_filtered = allthecodes_tools::media::filter_tools_for_model_capabilities(
                state.tools.clone(),
                &app_state.settings,
                &app_state.main_loop_model,
            );
            let settings_filtered = allthecodes_tools::registry::filter_tools_for_runtime_settings(
                capability_filtered,
                &app_state.settings,
            );
            let tools = allthecodes_tools::registry::dedupe_tools_by_name(
                allthecodes_tools::registry::filter_tools_for_session_gates(
                    settings_filtered,
                    allthecodes_tools::registry::ToolSessionGates {
                        non_interactive: self.query_source.is_non_interactive(),
                        subagent: self.query_source.starts_with_agent(),
                    },
                ),
            );
            filter_tools_for_allowed_override(tools, self.submit_overrides.allowed_tools.as_ref())
        });
        let execute_deferred_tool: crate::types::tool::DeferredToolExecutor = {
            let deps = self.clone();
            let tools = available_tools.clone();
            let parent_message = parent_message.clone();
            let on_progress = on_progress.clone();
            Arc::new(
                move |deferred_request: crate::types::tool::DeferredToolExecutionRequest| {
                    let deps = deps.clone();
                    let tools = tools.clone();
                    let parent_message = parent_message.clone();
                    let on_progress = on_progress.clone();
                    Box::pin(async move {
                        let result = QueryDeps::execute_tool(
                            &deps,
                            ToolExecRequest {
                                tool_use_id: deferred_request.tool_use_id,
                                tool_name: deferred_request.tool_name,
                                input: deferred_request.input,
                                langfuse_batch_span: None,
                            },
                            &tools,
                            &parent_message,
                            on_progress,
                        )
                        .await?;
                        Ok(crate::types::tool::DeferredToolExecutionResult {
                            tool_use_id: result.tool_use_id,
                            tool_name: result.tool_name,
                            result: result.result,
                            is_error: result.is_error,
                        })
                    })
                },
            )
        };

        let ctx = crate::types::tool::ToolUseContext {
            options: crate::types::tool::ToolUseOptions {
                debug: false,
                main_loop_model: self.get_app_state().main_loop_model,
                verbose: self.get_app_state().verbose,
                is_non_interactive_session: self.query_source.is_non_interactive(),
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: {
                let (tx, rx) = tokio::sync::watch::channel(false);
                if self.aborted.load(Ordering::Relaxed) {
                    let _ = tx.send(true);
                }
                rx
            },
            read_file_state: self.state.read().file_state_cache.clone(),
            get_app_state: {
                let state = self.state.clone();
                let overrides = self.submit_overrides.clone();
                Arc::new(move || {
                    let mut app_state = state.read().app_state.clone();
                    if let Some(model) = overrides.model.as_ref() {
                        app_state.main_loop_model = model.clone();
                    }
                    if overrides.thinking_enabled.is_some() {
                        app_state.thinking_enabled = overrides.thinking_enabled;
                    }
                    if overrides.effort.is_some() {
                        app_state.effort_value = overrides.effort.clone();
                    }
                    app_state.to_tool_app_state()
                })
            },
            set_app_state: {
                let state = self.state.clone();
                Arc::new(move |updater: crate::types::tool::AppStateUpdater| {
                    let mut s = state.write();
                    let old = s.app_state.to_tool_app_state();
                    let updated = updater(old);
                    s.app_state.apply_tool_app_state(updated);
                })
            },
            session_id: self.audit_ctx.session_id.clone(),
            langfuse_session_id: self
                .langfuse_trace
                .as_ref()
                .map(|trace| trace.session_id.clone())
                .unwrap_or_else(|| self.audit_ctx.session_id.clone()),
            messages: vec![],
            agent_id: self.agent_context.as_ref().map(|ac| ac.agent_id.clone()),
            agent_type: self
                .agent_context
                .as_ref()
                .and_then(|ac| ac.agent_type.clone()),
            query_tracking: self
                .agent_context
                .as_ref()
                .map(|ac| ac.query_tracking.clone()),
            permission_callback: self.permission_callback.clone(),
            ask_user_callback: self.state.read().ask_user_callback.clone(),
            permission_event_callback: self.permission_event_callback.clone(),
            bg_agent_tx: self.bg_agent_tx.clone(),
            hook_runner: self.hook_runner.clone(),
            command_dispatcher: self.command_dispatcher.clone(),
            available_tools,
            execute_deferred_tool: Some(execute_deferred_tool),
        };

        // Load hook configs from AppState.
        let hooks_map = self.state.read().app_state.hooks.clone();
        let pre_configs = hooks.load_hook_configs(&hooks_map, "PreToolUse");
        let post_configs = hooks.load_hook_configs(&hooks_map, "PostToolUse");
        let failure_configs = hooks.load_hook_configs(&hooks_map, "PostToolUseFailure");

        // Pre-tool hooks.
        let execution_started = std::time::Instant::now();

        match tool.validate_input(&request.input, &ctx).await {
            ValidationResult::Ok => {}
            ValidationResult::Error { message, .. } => {
                return Ok(ToolExecResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result: crate::types::tool::ToolResult {
                        data: serde_json::json!(format!(
                            "Input validation error: {}. The schema was not sent - please check the tool's input requirements.",
                            message
                        )),
                        new_messages: vec![],
                        ..Default::default()
                    },
                    is_error: true,
                    hook_stopped_continuation: false,
                });
            }
        }

        let mut sanitized_input = request.input.clone();
        if let Some(obj) = sanitized_input.as_object_mut() {
            obj.remove("_simulatedSedEdit");
        }

        if let Some(result) = security_validate(
            &request.tool_use_id,
            &request.tool_name,
            &sanitized_input,
            tool.as_ref(),
            &ctx,
            execution_started,
        ) {
            return Ok(tool_execution_result_to_exec_result(result));
        }

        let (mut effective_input, permission_override) = match hooks
            .run_pre_tool_hooks(&request.tool_name, &sanitized_input, &pre_configs)
            .await
        {
            Ok(PreToolHookResult::Continue {
                updated_input,
                permission_override,
            }) => (
                updated_input.unwrap_or_else(|| sanitized_input.clone()),
                permission_override,
            ),
            Ok(PreToolHookResult::Stop { message }) => {
                return Ok(ToolExecResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result: crate::types::tool::ToolResult {
                        data: serde_json::json!(format!("Pre-tool hook stopped: {}", message)),
                        new_messages: vec![],
                        ..Default::default()
                    },
                    is_error: true,
                    hook_stopped_continuation: false,
                });
            }
            Err(e) => {
                if hook_error_is_critical(&request.tool_name, &pre_configs) {
                    tracing::warn!(error = %e, tool = %request.tool_name, "critical pre-tool hook error, blocking tool execution");
                    return Ok(ToolExecResult {
                        tool_use_id: request.tool_use_id,
                        tool_name: request.tool_name,
                        result: crate::types::tool::ToolResult {
                            data: serde_json::json!(format!(
                                "Critical pre-tool hook failed: {}",
                                e
                            )),
                            new_messages: vec![],
                            ..Default::default()
                        },
                        is_error: true,
                        hook_stopped_continuation: false,
                    });
                } else {
                    tracing::warn!(error = %e, "optional pre-tool hook error, continuing");
                    (sanitized_input.clone(), None)
                }
            }
        };

        if effective_input != sanitized_input {
            match tool.validate_input(&effective_input, &ctx).await {
                ValidationResult::Ok => {}
                ValidationResult::Error { message, .. } => {
                    return Ok(ToolExecResult {
                        tool_use_id: request.tool_use_id,
                        tool_name: request.tool_name,
                        result: crate::types::tool::ToolResult {
                            data: serde_json::json!(format!(
                                "Pre-tool hook produced invalid input: {}.",
                                message
                            )),
                            new_messages: vec![],
                            ..Default::default()
                        },
                        is_error: true,
                        hook_stopped_continuation: false,
                    });
                }
            }

            if let Some(result) = security_validate(
                &request.tool_use_id,
                &request.tool_name,
                &effective_input,
                tool.as_ref(),
                &ctx,
                execution_started,
            ) {
                return Ok(tool_execution_result_to_exec_result(result));
            }
        }

        // Permission check (tool-local checks first, then central rules/mode).
        let hook_decision = match permission_override.as_ref() {
            Some(PermissionOverride::Allow) => {
                tracing::debug!(
                    tool = %request.tool_name,
                    "Permission allow requested by hook override"
                );
                emit_hook_permission_decision(
                    &ctx,
                    "PreToolUse",
                    "PreToolUse",
                    "*",
                    "allow",
                    vec![format!("tool: {}", request.tool_name)],
                );
                Some(crate::permissions::decision::HookPermissionDecision {
                    allow: true,
                    source: Some("PreToolUse".to_string()),
                    ..Default::default()
                })
            }
            Some(PermissionOverride::Deny { .. }) | None => None,
        };

        if let Some(PermissionOverride::Deny { reason }) = permission_override.as_ref() {
            emit_hook_permission_decision(
                &ctx,
                "PreToolUse",
                "PreToolUse",
                "*",
                "deny",
                vec![reason.clone()],
            );
            // Fire PermissionDenied hook
            let deny_configs = hooks.load_hook_configs(&hooks_map, "PermissionDenied");
            if !deny_configs.is_empty() {
                let payload = serde_json::json!({
                    "tool_name": request.tool_name,
                    "tool_input": effective_input.clone(),
                    "reason": format!("Permission denied by hook: {}", reason),
                });
                let _ = hooks
                    .run_event_hooks("PermissionDenied", &payload, &deny_configs)
                    .await;
            }

            return Ok(ToolExecResult {
                tool_use_id: request.tool_use_id,
                tool_name: request.tool_name,
                result: crate::types::tool::ToolResult {
                    data: serde_json::json!(format!("Permission denied by hook: {}", reason)),
                    new_messages: vec![],
                    ..Default::default()
                },
                is_error: true,
                hook_stopped_continuation: false,
            });
        }

        let mut accepted_permission_feedback: Option<String> = None;
        {
            // Normal permission check via tool-local checks and the central rule engine
            let perm_audit_ctx = self.audit_ctx.with_tool_use(&request.tool_use_id);
            let bypass_permissions =
                self.get_app_state().tool_permission_context.mode == PermissionMode::Bypass;
            let perm_result = if bypass_permissions {
                PermissionResult::Allow {
                    updated_input: effective_input.clone(),
                }
            } else {
                match tool.check_permissions(&effective_input, &ctx).await {
                    PermissionResult::Allow { updated_input } => {
                        effective_input = updated_input;
                        let app_state = self.get_app_state();
                        let mut decision = central_permission_decision_for_tool(
                            &request.tool_name,
                            &effective_input,
                            &app_state,
                            hook_decision.as_ref(),
                            None,
                            None,
                        );

                        if auto_classifier_needed(&decision) {
                            let mut classifier_input =
                                tool.to_auto_classifier_input(&effective_input);
                            if matches!(&classifier_input, serde_json::Value::String(s) if s.is_empty())
                            {
                                classifier_input = effective_input.clone();
                            }
                            if self
                                .state
                                .read()
                                .auto_denial_tracker
                                .should_fallback_to_interactive()
                            {
                                let app_state = self.get_app_state();
                                let mut state = self.state.write();
                                decision = central_permission_decision_for_tool(
                                    &request.tool_name,
                                    &effective_input,
                                    &app_state,
                                    hook_decision.as_ref(),
                                    None,
                                    Some(&mut state.auto_denial_tracker),
                                );
                            } else if let Some(auto_classifier) = self
                                .compute_auto_classifier(
                                    &request.tool_name,
                                    &effective_input,
                                    &classifier_input,
                                )
                                .await
                            {
                                let app_state = self.get_app_state();
                                let mut state = self.state.write();
                                decision = central_permission_decision_for_tool(
                                    &request.tool_name,
                                    &effective_input,
                                    &app_state,
                                    hook_decision.as_ref(),
                                    Some(&auto_classifier),
                                    Some(&mut state.auto_denial_tracker),
                                );
                            }
                        }

                        emit_permission_decision_debug(
                            &ctx,
                            &request.tool_name,
                            &app_state,
                            &decision,
                        );
                        permission_result_from_decision(
                            &request.tool_name,
                            &mut effective_input,
                            decision,
                        )
                    }
                    other => other,
                }
            };
            match perm_result {
                PermissionResult::Allow { updated_input } => {
                    effective_input = updated_input;
                }
                PermissionResult::Deny { message } => {
                    // Emit permission.resolved(denied) audit event
                    {
                        use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                        perm_audit_ctx.emit(
                            EventKind::PermissionResolved,
                            Stage::Permission,
                            AuditLevel::Warn,
                            Outcome::Denied,
                            None,
                            Some(serde_json::json!({
                                "tool_name": request.tool_name,
                                "decision": "deny",
                                "reason": message,
                            })),
                        );
                    }
                    // Fire PermissionDenied hook
                    let deny_configs = hooks.load_hook_configs(&hooks_map, "PermissionDenied");
                    if !deny_configs.is_empty() {
                        let payload = serde_json::json!({
                            "tool_name": request.tool_name,
                            "tool_input": effective_input.clone(),
                            "reason": format!("Permission denied: {}", message),
                        });
                        let _ = hooks
                            .run_event_hooks("PermissionDenied", &payload, &deny_configs)
                            .await;
                    }

                    return Ok(ToolExecResult {
                        tool_use_id: request.tool_use_id,
                        tool_name: request.tool_name,
                        result: crate::types::tool::ToolResult {
                            data: serde_json::json!(format!("Permission denied: {}", message)),
                            new_messages: vec![],
                            ..Default::default()
                        },
                        is_error: true,
                        hook_stopped_continuation: false,
                    });
                }
                PermissionResult::Ask { message } => {
                    // Emit permission.requested audit event
                    {
                        use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                        perm_audit_ctx.emit(
                            EventKind::PermissionRequested,
                            Stage::Permission,
                            AuditLevel::Info,
                            Outcome::Info,
                            None,
                            Some(serde_json::json!({
                                "tool_name": request.tool_name,
                                "message": message,
                            })),
                        );
                    }

                    // Fire PermissionRequest hook before interactive prompt
                    let mut hook_allowed = false;
                    let perm_req_configs = hooks.load_hook_configs(&hooks_map, "PermissionRequest");
                    if !perm_req_configs.is_empty() {
                        let payload = serde_json::json!({
                            "tool_name": request.tool_name,
                            "tool_input": effective_input.clone(),
                            "message": message,
                        });
                        if let Ok(output) = hooks
                            .run_event_hooks("PermissionRequest", &payload, &perm_req_configs)
                            .await
                        {
                            // If hook provides a permission decision, use it
                            if let Some(ref decision) = output.permission_decision {
                                emit_hook_permission_decision(
                                    &ctx,
                                    "PermissionRequest",
                                    "PermissionRequest",
                                    "*",
                                    decision,
                                    vec![format!("tool: {}", request.tool_name)],
                                );
                                match decision.as_str() {
                                    "allow" => {
                                        // Skip the interactive prompt, proceed to execution
                                        tracing::debug!(
                                            tool = %request.tool_name,
                                            "PermissionRequest hook allowed tool execution"
                                        );
                                        hook_allowed = true;
                                    }
                                    "deny" => {
                                        // Fire PermissionDenied hook
                                        let deny_configs =
                                            hooks.load_hook_configs(&hooks_map, "PermissionDenied");
                                        if !deny_configs.is_empty() {
                                            let deny_payload = serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "tool_input": effective_input.clone(),
                                                "reason": "Permission denied by PermissionRequest hook",
                                            });
                                            let _ = hooks
                                                .run_event_hooks(
                                                    "PermissionDenied",
                                                    &deny_payload,
                                                    &deny_configs,
                                                )
                                                .await;
                                        }

                                        return Ok(ToolExecResult {
                                            tool_use_id: request.tool_use_id,
                                            tool_name: request.tool_name,
                                            result: crate::types::tool::ToolResult {
                                                data: serde_json::json!(
                                                    "Permission denied by hook"
                                                ),
                                                new_messages: vec![],
                                                ..Default::default()
                                            },
                                            is_error: true,
                                            hook_stopped_continuation: false,
                                        });
                                    }
                                    _ => {} // unknown decision, continue with normal prompt
                                }
                            }
                        }
                    }

                    if !hook_allowed {
                        if let Some(ref callback) = ctx.permission_callback {
                            let options = vec![
                                "Allow".to_string(),
                                "Deny".to_string(),
                                "Always Allow".to_string(),
                                "Auto Review".to_string(),
                            ];
                            let response = callback(PermissionRequestPayload {
                                tool_use_id: request.tool_use_id.clone(),
                                tool_name: request.tool_name.clone(),
                                tool_input: effective_input.clone(),
                                message,
                                options,
                                operation: None,
                            })
                            .await;

                            let decision = response.normalized_decision();
                            match decision.as_str() {
                                "allow" => {
                                    accepted_permission_feedback = response.feedback.clone();
                                    // Emit permission.resolved(allow) audit event
                                    use crate::observability::{
                                        AuditLevel, EventKind, Outcome, Stage,
                                    };
                                    perm_audit_ctx.emit(
                                        EventKind::PermissionResolved,
                                        Stage::Permission,
                                        AuditLevel::Info,
                                        Outcome::Completed,
                                        None,
                                        Some(serde_json::json!({
                                            "tool_name": request.tool_name,
                                            "decision": "allow",
                                        })),
                                    );
                                }
                                "always_allow" => {
                                    accepted_permission_feedback = response.feedback.clone();
                                    let rule = exact_always_allow_rule(
                                        &request.tool_name,
                                        &effective_input,
                                    );
                                    if let Err(error) =
                                        persist_local_always_allow_rule(&self.cwd, &rule)
                                    {
                                        let error_message = format!(
                                            "Permission denied: failed to persist Always Allow rule: {error}"
                                        );
                                        use crate::observability::{
                                            AuditLevel, EventKind, Outcome, Stage,
                                        };
                                        perm_audit_ctx.emit(
                                            EventKind::PermissionResolved,
                                            Stage::Permission,
                                            AuditLevel::Warn,
                                            Outcome::Denied,
                                            None,
                                            Some(serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "decision": "always_allow",
                                                "source": "user",
                                                "error": error.to_string(),
                                            })),
                                        );
                                        return Ok(permission_denied_exec_result(
                                            &request,
                                            error_message,
                                        ));
                                    }
                                    {
                                        let mut state = self.state.write();
                                        add_local_always_allow_rule(&mut state, &rule);
                                    }
                                    tracing::debug!(
                                        tool = %request.tool_name,
                                        rule = %rule,
                                        "local exact always_allow rule recorded"
                                    );
                                    use crate::observability::{
                                        AuditLevel, EventKind, Outcome, Stage,
                                    };
                                    perm_audit_ctx.emit(
                                        EventKind::PermissionResolved,
                                        Stage::Permission,
                                        AuditLevel::Info,
                                        Outcome::Completed,
                                        None,
                                        Some(serde_json::json!({
                                            "tool_name": request.tool_name,
                                            "decision": "always_allow",
                                            "source": "user",
                                            "rule": rule,
                                        })),
                                    );
                                }
                                "auto_review" => {
                                    let risk_level =
                                        permission_risk_level(&request.tool_name, &effective_input);
                                    let action = Some(permission_action_summary(
                                        &request.tool_name,
                                        &effective_input,
                                    ));
                                    let review_id = Uuid::new_v4().to_string();
                                    let start_result = {
                                        let mut state = self.state.write();
                                        state.auto_review_tracker.try_start(&request.tool_use_id)
                                    };
                                    if let Err(reason) = start_result {
                                        emit_permission_auto_review(
                                            &ctx,
                                            permission_auto_review_event(
                                                &review_id,
                                                &request.tool_use_id,
                                                "circuit_open",
                                                risk_level.clone(),
                                                true,
                                                Some(reason.to_string()),
                                                action.clone(),
                                            ),
                                        );
                                        use crate::observability::{
                                            AuditLevel, EventKind, Outcome, Stage,
                                        };
                                        perm_audit_ctx.emit(
                                            EventKind::PermissionResolved,
                                            Stage::Permission,
                                            AuditLevel::Warn,
                                            Outcome::Denied,
                                            None,
                                            Some(serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "decision": "auto_review",
                                                "source": "user",
                                                "reason": reason,
                                            })),
                                        );
                                        return Ok(permission_denied_exec_result(
                                            &request,
                                            format!(
                                                "Permission denied: auto review unavailable ({reason})."
                                            ),
                                        ));
                                    }

                                    emit_permission_auto_review(
                                        &ctx,
                                        permission_auto_review_event(
                                            &review_id,
                                            &request.tool_use_id,
                                            "started",
                                            risk_level.clone(),
                                            true,
                                            response.feedback.clone(),
                                            action.clone(),
                                        ),
                                    );

                                    let mut classifier_input =
                                        tool.to_auto_classifier_input(&effective_input);
                                    if matches!(&classifier_input, serde_json::Value::String(s) if s.is_empty())
                                    {
                                        classifier_input = effective_input.clone();
                                    }
                                    let classifier = self
                                        .compute_permission_auto_review(
                                            &request.tool_name,
                                            &effective_input,
                                            &classifier_input,
                                        )
                                        .await;
                                    let Some(classifier) = classifier else {
                                        {
                                            let mut state = self.state.write();
                                            state.auto_review_tracker.record_denied();
                                        }
                                        emit_permission_auto_review(
                                            &ctx,
                                            permission_auto_review_event(
                                                &review_id,
                                                &request.tool_use_id,
                                                "failed",
                                                risk_level.clone(),
                                                true,
                                                Some("classifier unavailable".to_string()),
                                                action.clone(),
                                            ),
                                        );
                                        use crate::observability::{
                                            AuditLevel, EventKind, Outcome, Stage,
                                        };
                                        perm_audit_ctx.emit(
                                            EventKind::PermissionResolved,
                                            Stage::Permission,
                                            AuditLevel::Warn,
                                            Outcome::Denied,
                                            None,
                                            Some(serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "decision": "auto_review",
                                                "source": "user",
                                                "reason": "classifier unavailable",
                                            })),
                                        );
                                        return Ok(permission_denied_exec_result(
                                            &request,
                                            "Permission denied: auto review classifier is unavailable."
                                                .to_string(),
                                        ));
                                    };

                                    let rationale = if classifier.reason.trim().is_empty() {
                                        None
                                    } else {
                                        Some(classifier.reason.clone())
                                    };
                                    if classifier.unavailable || classifier.transcript_too_long {
                                        {
                                            let mut state = self.state.write();
                                            state.auto_review_tracker.record_denied();
                                        }
                                        emit_permission_auto_review(
                                            &ctx,
                                            permission_auto_review_event(
                                                &review_id,
                                                &request.tool_use_id,
                                                "failed",
                                                risk_level.clone(),
                                                true,
                                                rationale.clone(),
                                                action.clone(),
                                            ),
                                        );
                                        use crate::observability::{
                                            AuditLevel, EventKind, Outcome, Stage,
                                        };
                                        perm_audit_ctx.emit(
                                            EventKind::PermissionResolved,
                                            Stage::Permission,
                                            AuditLevel::Warn,
                                            Outcome::Denied,
                                            None,
                                            Some(serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "decision": "auto_review",
                                                "source": "user",
                                                "reason": classifier.reason.clone(),
                                            })),
                                        );
                                        return Ok(permission_denied_exec_result(
                                            &request,
                                            format!(
                                                "Permission denied: auto review could not classify this request: {}",
                                                classifier.reason
                                            ),
                                        ));
                                    }

                                    match classifier.verdict {
                                        AutoClassifierVerdict::Allow => {
                                            {
                                                let mut state = self.state.write();
                                                state.auto_review_tracker.record_allowed();
                                            }
                                            emit_permission_auto_review(
                                                &ctx,
                                                permission_auto_review_event(
                                                    &review_id,
                                                    &request.tool_use_id,
                                                    "approved",
                                                    risk_level,
                                                    true,
                                                    rationale,
                                                    action,
                                                ),
                                            );
                                            accepted_permission_feedback =
                                                response.feedback.clone();
                                            use crate::observability::{
                                                AuditLevel, EventKind, Outcome, Stage,
                                            };
                                            perm_audit_ctx.emit(
                                                EventKind::PermissionResolved,
                                                Stage::Permission,
                                                AuditLevel::Info,
                                                Outcome::Completed,
                                                None,
                                                Some(serde_json::json!({
                                                    "tool_name": request.tool_name,
                                                    "decision": "auto_review",
                                                    "source": "user",
                                                    "classifier_model": classifier.model,
                                                })),
                                            );
                                        }
                                        AutoClassifierVerdict::Deny
                                        | AutoClassifierVerdict::Ask => {
                                            {
                                                let mut state = self.state.write();
                                                state.auto_review_tracker.record_denied();
                                            }
                                            let reason = if classifier.reason.trim().is_empty() {
                                                "auto review did not approve this request"
                                                    .to_string()
                                            } else {
                                                classifier.reason.clone()
                                            };
                                            emit_permission_auto_review(
                                                &ctx,
                                                permission_auto_review_event(
                                                    &review_id,
                                                    &request.tool_use_id,
                                                    "denied",
                                                    risk_level,
                                                    true,
                                                    Some(reason.clone()),
                                                    action,
                                                ),
                                            );
                                            use crate::observability::{
                                                AuditLevel, EventKind, Outcome, Stage,
                                            };
                                            perm_audit_ctx.emit(
                                                EventKind::PermissionResolved,
                                                Stage::Permission,
                                                AuditLevel::Warn,
                                                Outcome::Denied,
                                                None,
                                                Some(serde_json::json!({
                                                    "tool_name": request.tool_name,
                                                    "decision": "auto_review",
                                                    "source": "user",
                                                    "reason": reason.clone(),
                                                })),
                                            );

                                            let deny_configs = hooks
                                                .load_hook_configs(&hooks_map, "PermissionDenied");
                                            if !deny_configs.is_empty() {
                                                let payload = serde_json::json!({
                                                    "tool_name": request.tool_name,
                                                    "tool_input": effective_input.clone(),
                                                    "reason": "Permission denied by auto review",
                                                });
                                                let _ = hooks
                                                    .run_event_hooks(
                                                        "PermissionDenied",
                                                        &payload,
                                                        &deny_configs,
                                                    )
                                                    .await;
                                            }

                                            return Ok(permission_denied_exec_result(
                                                &request,
                                                format!(
                                                    "Permission denied by auto review: {reason}"
                                                ),
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    let denial_message = permission_denied_message(&response);
                                    // Emit permission.resolved(denied) audit event
                                    {
                                        use crate::observability::{
                                            AuditLevel, EventKind, Outcome, Stage,
                                        };
                                        perm_audit_ctx.emit(
                                            EventKind::PermissionResolved,
                                            Stage::Permission,
                                            AuditLevel::Warn,
                                            Outcome::Denied,
                                            None,
                                            Some(serde_json::json!({
                                                "tool_name": request.tool_name,
                                                "decision": "deny",
                                                "source": "user",
                                            })),
                                        );
                                    }

                                    // Fire PermissionDenied hook (user chose deny)
                                    let deny_configs =
                                        hooks.load_hook_configs(&hooks_map, "PermissionDenied");
                                    if !deny_configs.is_empty() {
                                        let payload = serde_json::json!({
                                            "tool_name": request.tool_name,
                                            "tool_input": effective_input.clone(),
                                            "reason": "Permission denied by user",
                                        });
                                        let _ = hooks
                                            .run_event_hooks(
                                                "PermissionDenied",
                                                &payload,
                                                &deny_configs,
                                            )
                                            .await;
                                    }

                                    return Ok(ToolExecResult {
                                        tool_use_id: request.tool_use_id,
                                        tool_name: request.tool_name,
                                        result: crate::types::tool::ToolResult {
                                            data: serde_json::json!(denial_message),
                                            new_messages: vec![],
                                            ..Default::default()
                                        },
                                        is_error: true,
                                        hook_stopped_continuation: false,
                                    });
                                }
                            }
                        } else {
                            // Fire PermissionDenied hook (no callback available)
                            let deny_configs =
                                hooks.load_hook_configs(&hooks_map, "PermissionDenied");
                            if !deny_configs.is_empty() {
                                let payload = serde_json::json!({
                                    "tool_name": request.tool_name,
                                    "tool_input": effective_input.clone(),
                                    "reason": format!("Permission required (no callback): {}", message),
                                });
                                let _ = hooks
                                    .run_event_hooks("PermissionDenied", &payload, &deny_configs)
                                    .await;
                            }

                            return Ok(ToolExecResult {
                                tool_use_id: request.tool_use_id,
                                tool_name: request.tool_name,
                                result: crate::types::tool::ToolResult {
                                    data: serde_json::json!(format!(
                                        "Permission required: {}",
                                        message
                                    )),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                is_error: true,
                                hook_stopped_continuation: false,
                            });
                        }
                    } // if !hook_allowed
                }
            }
        }

        // Tool execution with post-hooks.

        // Emit tool.start audit event
        let tool_audit_ctx = self.audit_ctx.with_tool_use(&request.tool_use_id);
        let tool_langfuse_span = self.langfuse_trace.as_ref().and_then(|trace| {
            crate::services::langfuse::create_tool_span(
                trace,
                &request.tool_name,
                &request.tool_use_id,
                &effective_input,
                request.langfuse_batch_span.as_ref(),
            )
        });
        {
            use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
            tool_audit_ctx.emit(
                EventKind::ToolStart,
                Stage::ToolExecution,
                AuditLevel::Info,
                Outcome::Started,
                None,
                Some(serde_json::json!({
                    "tool_name": request.tool_name,
                })),
            );
        }
        let tool_start = std::time::Instant::now();

        // Adapt the `Arc` from `QueryDeps::execute_tool` into the `Box`
        // the `Tool::call` contract expects. The wrapper also stamps the
        // current `request.tool_use_id` onto each `ToolProgress` so
        // downstream tools don't need to know it themselves.
        let tool_use_id_for_progress = request.tool_use_id.clone();
        let boxed_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>> =
            on_progress.as_ref().map(|arc| {
                let arc = arc.clone();
                let tool_use_id = tool_use_id_for_progress.clone();
                Box::new(move |mut p: ToolProgress| {
                    if p.tool_use_id.is_empty() {
                        p.tool_use_id = tool_use_id.clone();
                    }
                    arc(p)
                }) as Box<dyn Fn(ToolProgress) + Send + Sync>
            });

        match tool
            .call(
                effective_input.clone(),
                &ctx,
                parent_message,
                boxed_progress,
            )
            .await
        {
            Ok(mut result) => {
                let result_preview =
                    result
                        .display_preview
                        .clone()
                        .unwrap_or_else(|| match &result.data {
                            serde_json::Value::String(value) => value.clone(),
                            other => {
                                serde_json::to_string(other).unwrap_or_else(|_| "null".to_string())
                            }
                        });
                crate::services::langfuse::finish_tool_span(
                    tool_langfuse_span,
                    &request.tool_name,
                    &result_preview,
                    false,
                );
                // Emit tool.finish audit event
                {
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    tool_audit_ctx.emit(
                        EventKind::ToolFinish,
                        Stage::ToolExecution,
                        AuditLevel::Info,
                        Outcome::Completed,
                        Some(tool_start.elapsed().as_millis() as u64),
                        Some(serde_json::json!({
                            "tool_name": request.tool_name,
                        })),
                    );
                }

                // Run post-tool hooks on success
                let mut hook_stopped_continuation = false;
                if !post_configs.is_empty() {
                    match hooks
                        .run_post_tool_hooks(
                            &request.tool_name,
                            &effective_input,
                            &result.data,
                            &post_configs,
                        )
                        .await
                    {
                        Ok(PostToolHookResult::Continue) => {}
                        Ok(PostToolHookResult::StopContinuation { message }) => {
                            tracing::debug!(
                                message = %message,
                                "post-tool hook stopped continuation"
                            );
                            hook_stopped_continuation = true;
                        }
                        Err(e) if hook_error_is_critical(&request.tool_name, &post_configs) => {
                            tracing::warn!(error = %e, tool = %request.tool_name, "critical post-tool hook error, failing tool execution");
                            return Ok(ToolExecResult {
                                tool_use_id: request.tool_use_id,
                                tool_name: request.tool_name,
                                result: crate::types::tool::ToolResult {
                                    data: serde_json::json!(format!(
                                        "Critical post-tool hook failed: {}",
                                        e
                                    )),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                is_error: true,
                                hook_stopped_continuation: false,
                            });
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, tool = %request.tool_name, "optional post-tool hook error, continuing");
                        }
                    }
                }

                if let Some(feedback) = accepted_permission_feedback.as_deref() {
                    result
                        .new_messages
                        .push(permission_feedback_message(feedback));
                }

                result.data = allthecodes_tools::result::enforce_result_size(
                    result.data,
                    tool.max_result_size_chars(),
                );

                Ok(ToolExecResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result,
                    is_error: false,
                    hook_stopped_continuation,
                })
            }
            Err(e) => {
                crate::services::langfuse::finish_tool_span(
                    tool_langfuse_span,
                    &request.tool_name,
                    &e.to_string(),
                    true,
                );
                // Emit tool.error audit event
                {
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    tool_audit_ctx.emit(
                        EventKind::ToolError,
                        Stage::ToolExecution,
                        AuditLevel::Error,
                        Outcome::Failed,
                        Some(tool_start.elapsed().as_millis() as u64),
                        Some(serde_json::json!({
                            "tool_name": request.tool_name,
                            "error": e.to_string(),
                        })),
                    );
                }

                // Run post-failure hooks on error
                if !failure_configs.is_empty() {
                    match hooks
                        .run_post_tool_failure_hooks(
                            &request.tool_name,
                            &effective_input,
                            &e.to_string(),
                            &failure_configs,
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(hook_error)
                            if hook_error_is_critical(&request.tool_name, &failure_configs) =>
                        {
                            tracing::warn!(error = %hook_error, tool = %request.tool_name, "critical post-failure hook error, failing tool execution");
                            return Ok(ToolExecResult {
                                tool_use_id: request.tool_use_id,
                                tool_name: request.tool_name,
                                result: crate::types::tool::ToolResult {
                                    data: serde_json::json!(format!(
                                        "Critical post-failure hook failed after tool error ({}): {}",
                                        e, hook_error
                                    )),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                is_error: true,
                                hook_stopped_continuation: false,
                            });
                        }
                        Err(hook_error) => {
                            tracing::warn!(error = %hook_error, tool = %request.tool_name, "optional post-failure hook error, continuing");
                        }
                    }
                }

                Ok(ToolExecResult {
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name,
                    result: crate::types::tool::ToolResult {
                        data: serde_json::json!(format!("Error: {}", e)),
                        new_messages: vec![],
                        ..Default::default()
                    },
                    is_error: true,
                    hook_stopped_continuation: false,
                })
            }
        }
    }
}


fn permission_denied_exec_result(request: &ToolExecRequest, message: String) -> ToolExecResult {
    ToolExecResult {
        tool_use_id: request.tool_use_id.clone(),
        tool_name: request.tool_name.clone(),
        result: crate::types::tool::ToolResult {
            data: serde_json::json!(message),
            new_messages: vec![],
            ..Default::default()
        },
        is_error: true,
        hook_stopped_continuation: false,
    }
}

fn exact_always_allow_rule(tool_name: &str, input: &serde_json::Value) -> String {
    let rule_tool = canonical_permission_rule_tool(tool_name);
    let specifier = exact_rule_subject(&rule_tool, input);
    format!(
        "{}({})",
        rule_tool,
        allthecodes_permissions::rules::literal_specifier(&specifier)
    )
}

fn canonical_permission_rule_tool(tool_name: &str) -> String {
    match tool_name {
        "bash" => "Bash",
        "PowerShell" | "powershell" | "pwsh" | "Pwsh" => "PowerShell",
        "read" | "read_file" | "FileRead" => "Read",
        "edit_file" | "file_edit" | "FileEdit" => "Edit",
        "write_file" | "file_write" | "FileWrite" => "Write",
        "FileMultiEdit" => "MultiEdit",
        other => other,
    }
    .to_string()
}

fn exact_rule_subject(rule_tool: &str, input: &serde_json::Value) -> String {
    match rule_tool {
        "Bash" | "PowerShell" => shell_command_subject(input),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => input_string_field(
            input,
            &[
                "file_path",
                "notebook_path",
                "path",
                "relative_path",
                "absolute_path",
            ],
        )
        .unwrap_or_else(|| stable_json_subject(input)),
        "Glob" => {
            input_string_field(input, &["pattern"]).unwrap_or_else(|| stable_json_subject(input))
        }
        "WebFetch" => {
            input_string_field(input, &["url"]).unwrap_or_else(|| stable_json_subject(input))
        }
        "WebSearch" => {
            input_string_field(input, &["query"]).unwrap_or_else(|| stable_json_subject(input))
        }
        "Agent" => input_string_field(input, &["subagent_type", "agent_type"])
            .unwrap_or_else(|| stable_json_subject(input)),
        _ => stable_json_subject(input),
    }
}

fn shell_command_subject(input: &serde_json::Value) -> String {
    input_string_field(input, &["command", "cmd", "script"])
        .unwrap_or_else(|| stable_json_subject(input))
}

fn input_string_field(input: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| input.get(*key))
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

fn stable_json_subject(input: &serde_json::Value) -> String {
    serde_json::to_string(input).unwrap_or_else(|_| input.to_string())
}

fn persist_local_always_allow_rule(cwd: &str, rule: &str) -> anyhow::Result<()> {
    let cwd = std::path::Path::new(cwd);
    let mut raw = allthecodes_config::settings::load_local_config(cwd)?;
    let permissions = raw.permissions.get_or_insert_with(Default::default);
    if !permissions.allow.iter().any(|existing| existing == rule) {
        permissions.allow.push(rule.to_string());
        allthecodes_config::settings::write_local_settings(cwd, &raw)?;
    }
    Ok(())
}

fn add_local_always_allow_rule(state: &mut QueryEngineState, rule: &str) {
    let rules = state
        .app_state
        .tool_permission_context
        .always_allow_rules
        .entry("local".to_string())
        .or_default();
    if !rules.iter().any(|existing| existing == rule) {
        rules.push(rule.to_string());
    }
}

fn permission_auto_review_event(
    review_id: &str,
    target_tool_use_id: &str,
    status: &str,
    risk_level: Option<String>,
    user_authorization: bool,
    rationale: Option<String>,
    action: Option<String>,
) -> PermissionAutoReviewEvent {
    PermissionAutoReviewEvent {
        review_id: review_id.to_string(),
        target_tool_use_id: target_tool_use_id.to_string(),
        status: status.to_string(),
        risk_level,
        user_authorization,
        rationale,
        action,
        decision_source: "auto_review".to_string(),
    }
}

fn permission_action_summary(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "Bash" | "bash" | "PowerShell" | "powershell" | "pwsh" | "Pwsh" => {
            format!("Run {}", shell_command_subject(input))
        }
        "Read" | "FileRead" | "read" | "read_file" => input_string_field(
            input,
            &["file_path", "path", "relative_path", "absolute_path"],
        )
        .map(|path| format!("Read {path}"))
        .unwrap_or_else(|| format!("Use {tool_name}")),
        "Edit" | "FileEdit" | "MultiEdit" | "FileMultiEdit" | "NotebookEdit" => {
            input_string_field(input, &["file_path", "notebook_path", "path"])
                .map(|path| format!("Edit {path}"))
                .unwrap_or_else(|| format!("Use {tool_name}"))
        }
        "Write" | "FileWrite" => input_string_field(input, &["file_path", "path"])
            .map(|path| format!("Write {path}"))
            .unwrap_or_else(|| format!("Use {tool_name}")),
        "WebFetch" => input_string_field(input, &["url"])
            .map(|url| format!("Fetch {url}"))
            .unwrap_or_else(|| "Fetch URL".to_string()),
        "WebSearch" => input_string_field(input, &["query"])
            .map(|query| format!("Search {query}"))
            .unwrap_or_else(|| "Search web".to_string()),
        _ => format!("Use {tool_name}"),
    }
}

fn permission_risk_level(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    match tool_name {
        "Bash" | "bash" | "PowerShell" | "powershell" | "pwsh" | "Pwsh" => {
            let command = shell_command_subject(input).to_ascii_lowercase();
            if command.contains("rm -rf")
                || command.contains("remove-item")
                || command.contains("del ")
                || command.contains(" format ")
                || command.contains("shutdown")
            {
                Some("destructive".to_string())
            } else if command.contains("sudo")
                || command.contains("chmod")
                || command.contains("chown")
                || command.contains("curl ")
                || command.contains("wget ")
            {
                Some("high".to_string())
            } else {
                Some("medium".to_string())
            }
        }
        "Edit" | "FileEdit" | "MultiEdit" | "FileMultiEdit" | "NotebookEdit" | "Write"
        | "FileWrite" => Some("medium".to_string()),
        "WebFetch" | "WebSearch" => Some("safe".to_string()),
        _ => None,
    }
}

fn filter_tools_for_allowed_override(tools: Tools, allowed_tools: Option<&Vec<String>>) -> Tools {
    let Some(allowed_tools) = allowed_tools else {
        return tools;
    };
    let allowed: HashSet<String> = allowed_tools
        .iter()
        .map(|tool| tool.to_ascii_lowercase())
        .collect();
    tools
        .into_iter()
        .filter(|tool| allowed.contains(&tool.name().to_ascii_lowercase()))
        .collect()
}
