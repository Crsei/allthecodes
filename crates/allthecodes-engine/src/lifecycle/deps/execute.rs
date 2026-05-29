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
        // concrete shell-hook runner (see issue #74, Phase 5b).
        let hooks = self.hook_runner.as_ref();

        let tool = find_tool(&request.tool_name, tools)
            .ok_or_else(|| anyhow::anyhow!("tool not found: {}", request.tool_name))?;

        let available_tools = self.state.read().tools.clone();
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
                main_loop_model: self.state.read().app_state.main_loop_model.clone(),
                verbose: self.state.read().app_state.verbose,
                is_non_interactive_session: false,
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
                Arc::new(move || state.read().app_state.to_tool_app_state())
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
                self.state.read().app_state.tool_permission_context.mode == PermissionMode::Bypass;
            let perm_result = if bypass_permissions {
                PermissionResult::Allow {
                    updated_input: effective_input.clone(),
                }
            } else {
                match tool.check_permissions(&effective_input, &ctx).await {
                    PermissionResult::Allow { updated_input } => {
                        effective_input = updated_input;
                        let app_state = self.state.read().app_state.clone();
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
                                let mut state = self.state.write();
                                let app_state = state.app_state.clone();
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
                                let mut state = self.state.write();
                                let app_state = state.app_state.clone();
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
                            ];
                            let response = callback(PermissionRequestPayload {
                                tool_use_id: request.tool_use_id.clone(),
                                tool_name: request.tool_name.clone(),
                                tool_input: effective_input.clone(),
                                message,
                                options,
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
                                    // Record a session-level grant so subsequent
                                    // calls to this tool don't re-prompt.
                                    self.state
                                        .write()
                                        .app_state
                                        .tool_permission_context
                                        .grant_session_allow(&request.tool_name);
                                    tracing::debug!(
                                        tool = %request.tool_name,
                                        "session-level always_allow grant recorded"
                                    );
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
