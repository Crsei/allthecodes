use super::*;
use crate::session::record_replay::types::{
    PermissionRequestRecord, PermissionResponseRecord, QuestionRequestRecord,
    QuestionResponseRecord, RecordItem,
};
use allthecodes_types::agent_runtime_record::AgentRuntimePermissionDecision;

impl QueryEngineDeps {
    async fn record_replay_items(&self, items: Vec<RecordItem>, context: &'static str) {
        record_replay_items_for_handle(&self.session_recorder, &self.session_id, items, context)
            .await;
    }

    pub(super) async fn record_tool_permission_request(
        &self,
        request: &ToolExecRequest,
        input: &serde_json::Value,
        message: &str,
        options: &[String],
    ) {
        self.record_replay_items(
            vec![RecordItem::PermissionRequest(PermissionRequestRecord {
                request_id: request.tool_use_id.clone(),
                tool_name: request.tool_name.clone(),
                context: Some(serde_json::json!({
                    "message": message,
                    "tool_input": input.clone(),
                    "options": options,
                })),
            })],
            "permission_request",
        )
        .await;
    }

    pub(super) async fn record_tool_permission_response(
        &self,
        request_id: &str,
        decision: impl Into<String>,
        reason: Option<String>,
    ) {
        self.record_replay_items(
            vec![RecordItem::PermissionResponse(PermissionResponseRecord {
                request_id: request_id.to_string(),
                decision: decision.into(),
                reason,
            })],
            "permission_response",
        )
        .await;
    }

    fn recordable_ask_user_callback(&self) -> Option<crate::types::tool::AskUserCallback> {
        let callback = self.state.read().permissions.ask_user_callback.clone()?;
        let session_recorder = self.session_recorder.clone();
        let session_id = self.session_id.clone();
        Some(Arc::new(
            move |request: allthecodes_types::callbacks::AskUserRequestPayload| {
                let callback = callback.clone();
                let session_recorder = session_recorder.clone();
                let session_id = session_id.clone();
                Box::pin(async move {
                    let request_id = Uuid::new_v4().to_string();
                    let prompt = request.question.clone();
                    let options = serde_json::json!({
                        "choices": request.choices.clone(),
                        "allow_free_text": request.allow_free_text,
                    });
                    record_replay_items_for_handle(
                        &session_recorder,
                        &session_id,
                        vec![RecordItem::QuestionRequest(QuestionRequestRecord {
                            request_id: request_id.clone(),
                            prompt,
                            options: Some(options),
                        })],
                        "question_request",
                    )
                    .await;

                    let response = callback(request).await;
                    record_replay_items_for_handle(
                        &session_recorder,
                        &session_id,
                        vec![RecordItem::QuestionResponse(QuestionResponseRecord {
                            request_id,
                            response: response.clone(),
                        })],
                        "question_response",
                    )
                    .await;
                    response
                })
            },
        ))
    }

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

        // Hook dispatcher trait object — decouples the engine from the concrete
        // concrete shell-hook runner (see issue #74, full-build parity).
        let hooks = self.hook_runner.as_ref();

        let tool = find_tool(&request.tool_name, tools)
            .ok_or_else(|| anyhow::anyhow!("tool not found: {}", request.tool_name))?;

        let available_tools = self.submit_tools.clone().unwrap_or_else(|| {
            let state = self.state.read();
            let app_state = self.get_app_state();
            let capability_filtered = allthecodes_tools::media::filter_tools_for_model_capabilities(
                state.tools.registry.clone(),
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
            read_file_state: self.state.read().tools.file_state_cache.clone(),
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
            ask_user_callback: self.recordable_ask_user_callback(),
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
        let mut permission_decision = AgentRuntimePermissionDecision::NotRequired;
        let pipeline = ToolExecutionPipeline::new(
            self,
            &request,
            tool.clone(),
            &ctx,
            hooks,
            &hooks_map,
            &pre_configs,
            &post_configs,
            &failure_configs,
            execution_started,
        );

        if let PipelineStageResult::Finish(result) = pipeline
            .validate_input(&request.input, InputValidationKind::Original)
            .await
        {
            return Ok(result);
        }

        let SanitizedInput {
            value: sanitized_input,
        } = pipeline.sanitize_input();

        if let PipelineStageResult::Finish(result) = pipeline.security_validate(&sanitized_input) {
            return Ok(result);
        }

        let pre_hook = match pipeline.run_pre_hooks(&sanitized_input).await {
            PipelineStageResult::Continue(pre_hook) => pre_hook,
            PipelineStageResult::Finish(result) => return Ok(result),
        };
        let mut effective_input = pre_hook.effective_input;

        // Permission check (tool-local checks first, then central rules/mode).
        let hook_decision = match pipeline
            .resolve_permission(
                pre_hook.permission_override.as_ref(),
                &effective_input,
                &mut permission_decision,
            )
            .await
        {
            PermissionOverrideStageResult::Continue(result) => result.hook_decision,
            PermissionOverrideStageResult::Finish(result) => return Ok(result),
        };

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
                            &self.runtime_services,
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
                                .permissions
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
                                    Some(&mut state.permissions.auto_denial_tracker),
                                    &self.runtime_services,
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
                                    Some(&mut state.permissions.auto_denial_tracker),
                                    &self.runtime_services,
                                );
                            }
                        }

                        emit_permission_decision_debug(
                            &ctx,
                            &request.tool_name,
                            &app_state,
                            &decision,
                        );
                        permission_decision = runtime_permission_decision_label(&decision);
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
                    if permission_decision != AgentRuntimePermissionDecision::NotRequired {
                        self.record_tool_permission_response(
                            &request.tool_use_id,
                            "allow",
                            Some(format!("{permission_decision:?}")),
                        )
                        .await;
                    }
                }
                PermissionResult::Deny { message } => {
                    if permission_decision == AgentRuntimePermissionDecision::NotRequired {
                        permission_decision = AgentRuntimePermissionDecision::DeniedByPolicy;
                    }
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

                    self.record_tool_permission_response(
                        &request.tool_use_id,
                        "deny",
                        Some(message.clone()),
                    )
                    .await;
                    return Ok(tool_exec_result(
                        &request,
                        crate::types::tool::ToolResult {
                            data: serde_json::json!(format!("Permission denied: {}", message)),
                            new_messages: vec![],
                            ..Default::default()
                        },
                        true,
                        false,
                        effective_input.clone(),
                        elapsed_ms(execution_started),
                        Some(permission_decision),
                    ));
                }
                PermissionResult::Ask { message } => {
                    let prompt_plan =
                        ToolExecutionPlan::new(effective_input.clone(), permission_decision);
                    match pipeline.maybe_prompt_user(message, &prompt_plan).await {
                        PipelineStageResult::Continue(prompt) => {
                            permission_decision = prompt.permission_decision;
                            accepted_permission_feedback = prompt.accepted_permission_feedback;
                        }
                        PipelineStageResult::Finish(result) => return Ok(result),
                    }
                }
            }
        }

        // Tool execution with post-hooks.
        let mut plan = ToolExecutionPlan::new(effective_input, permission_decision);
        plan.accepted_permission_feedback = accepted_permission_feedback;

        let call_outcome = match pipeline.call_tool(&plan, parent_message, on_progress).await {
            PipelineStageResult::Continue(outcome) => outcome,
            PipelineStageResult::Finish(result) => return Ok(result),
        };

        let post_hook = match pipeline.run_post_hooks(&plan, call_outcome).await {
            PipelineStageResult::Continue(post_hook) => post_hook,
            PipelineStageResult::Finish(result) => return Ok(result),
        };

        if let PipelineStageResult::Finish(result) = pipeline.record_and_audit(
            post_hook.result,
            &plan,
            post_hook.hook_stopped_continuation,
            post_hook.tool_start,
        ) {
            return Ok(result);
        }

        unreachable!("record_and_audit always finalizes successful tool execution");
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

async fn record_replay_items_for_handle(
    session_recorder: &Arc<Mutex<Option<crate::session::record_replay::SessionRecorderHandle>>>,
    session_id: &str,
    items: Vec<RecordItem>,
    context: &'static str,
) {
    if items.is_empty() {
        return;
    }
    let handle = session_recorder.lock().clone();
    if let Some(handle) = handle {
        if let Err(error) = handle.add(items).await {
            tracing::warn!(
                session_id = %session_id,
                context,
                %error,
                "failed to record session replay item"
            );
        }
    }
}

pub(super) fn elapsed_ms(started: std::time::Instant) -> Option<u64> {
    Some(started.elapsed().as_millis() as u64)
}

pub(super) fn tool_exec_result(
    request: &ToolExecRequest,
    result: crate::types::tool::ToolResult,
    is_error: bool,
    hook_stopped_continuation: bool,
    effective_input: serde_json::Value,
    duration_ms: Option<u64>,
    permission_decision: Option<AgentRuntimePermissionDecision>,
) -> ToolExecResult {
    let brief_message = allthecodes_types::brief::brief_payload_from_tool_result(
        &request.tool_name,
        &request.tool_use_id,
        "",
        &result.data,
    );
    tool_exec_result_with_brief(
        request,
        result,
        is_error,
        hook_stopped_continuation,
        effective_input,
        duration_ms,
        permission_decision,
        brief_message,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn tool_exec_result_with_brief(
    request: &ToolExecRequest,
    result: crate::types::tool::ToolResult,
    is_error: bool,
    hook_stopped_continuation: bool,
    effective_input: serde_json::Value,
    duration_ms: Option<u64>,
    permission_decision: Option<AgentRuntimePermissionDecision>,
    brief_message: Option<allthecodes_types::brief::BriefMessagePayload>,
) -> ToolExecResult {
    ToolExecResult {
        tool_use_id: request.tool_use_id.clone(),
        tool_name: request.tool_name.clone(),
        effective_input,
        result,
        is_error,
        hook_stopped_continuation,
        duration_ms,
        permission_decision,
        brief_message,
    }
}

pub(super) fn permission_denied_exec_result(
    request: &ToolExecRequest,
    message: String,
    effective_input: serde_json::Value,
    started: std::time::Instant,
    permission_decision: AgentRuntimePermissionDecision,
) -> ToolExecResult {
    tool_exec_result(
        request,
        crate::types::tool::ToolResult {
            data: serde_json::json!(message),
            new_messages: vec![],
            ..Default::default()
        },
        true,
        false,
        effective_input,
        elapsed_ms(started),
        Some(permission_decision),
    )
}

pub(super) fn exact_always_allow_rule(tool_name: &str, input: &serde_json::Value) -> String {
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

pub(super) fn persist_local_always_allow_rule(cwd: &str, rule: &str) -> anyhow::Result<()> {
    let cwd = std::path::Path::new(cwd);
    let mut raw = allthecodes_config::settings::load_local_config(cwd)?;
    let permissions = raw.permissions.get_or_insert_with(Default::default);
    if !permissions.allow.iter().any(|existing| existing == rule) {
        permissions.allow.push(rule.to_string());
        allthecodes_config::settings::write_local_settings(cwd, &raw)?;
    }
    Ok(())
}

pub(super) fn add_local_always_allow_rule(state: &mut QueryEngineState, rule: &str) {
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

pub(super) fn permission_auto_review_event(
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

pub(super) fn permission_action_summary(tool_name: &str, input: &serde_json::Value) -> String {
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

pub(super) fn permission_risk_level(tool_name: &str, input: &serde_json::Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tool::ToolResult;
    use serde_json::json;

    // ------------------------------------------------------------------
    // exact_always_allow_rule
    // ------------------------------------------------------------------

    #[test]
    fn exact_always_allow_rule_constructs_for_bash() {
        let input = json!({"command": "ls -la"});
        let rule = exact_always_allow_rule("Bash", &input);
        // rule: Bash(exact-hex:<hex of "ls -la">)
        assert!(rule.starts_with("Bash(exact-hex:"));
        assert!(rule.ends_with(")"));
        assert!(rule.len() > "Bash(exact-hex:)".len() + 1);
    }

    #[test]
    fn tool_exec_result_extracts_brief_payload() {
        let request = ToolExecRequest {
            tool_use_id: "toolu-brief".to_string(),
            tool_name: allthecodes_types::brief::BRIEF_TOOL_NAME.to_string(),
            input: json!({"message": "brief body"}),
            langfuse_batch_span: None,
        };
        let result = tool_exec_result(
            &request,
            ToolResult {
                data: json!({
                    "is_brief_message": true,
                    "message": "brief body",
                    "status": "proactive",
                    "attachments": ["docs/brief.md"],
                }),
                display_preview: Some("brief body".to_string()),
                ..Default::default()
            },
            false,
            false,
            request.input.clone(),
            Some(42),
            None,
        );

        let brief = result
            .brief_message
            .as_ref()
            .expect("Brief tool result should expose brief payload");
        assert_eq!(brief.message, "brief body");
        assert_eq!(
            brief.status,
            allthecodes_types::brief::BriefMessageStatus::Proactive
        );
        assert_eq!(brief.attachments, vec!["docs/brief.md".to_string()]);
        assert_eq!(brief.tool_use_id.as_deref(), Some("toolu-brief"));
    }

    #[test]
    fn exact_always_allow_rule_constructs_for_read() {
        let input = json!({"file_path": "/tmp/test.txt"});
        let rule = exact_always_allow_rule("Read", &input);
        assert!(rule.starts_with("Read(exact-hex:"));
        assert!(rule.len() > "Read(exact-hex:)".len() + 1);
    }

    #[test]
    fn exact_always_allow_rule_constructs_for_edit() {
        let input = json!({"file_path": "/tmp/foo.rs"});
        let rule = exact_always_allow_rule("Edit", &input);
        assert!(rule.starts_with("Edit(exact-hex:"));
    }

    #[test]
    fn exact_always_allow_rule_falls_back_to_json_for_unknown_tool() {
        let input = json!({"key": "value"});
        let rule = exact_always_allow_rule("CustomTool", &input);
        assert!(rule.starts_with("CustomTool(exact-hex:"));
        assert!(rule.len() > "CustomTool(exact-hex:)".len() + 1);
    }

    // ------------------------------------------------------------------
    // canonical_permission_rule_tool
    // ------------------------------------------------------------------

    #[test]
    fn permission_rule_tool_maps_bash() {
        assert_eq!(canonical_permission_rule_tool("bash"), "Bash");
        assert_eq!(canonical_permission_rule_tool("Bash"), "Bash");
    }

    #[test]
    fn permission_rule_tool_maps_powershell() {
        for name in &["PowerShell", "powershell", "pwsh", "Pwsh"] {
            assert_eq!(
                canonical_permission_rule_tool(name),
                "PowerShell",
                "unexpected mapping for {name}"
            );
        }
    }

    #[test]
    fn permission_rule_tool_maps_read_like() {
        for name in &["read", "read_file", "FileRead"] {
            assert_eq!(
                canonical_permission_rule_tool(name),
                "Read",
                "unexpected mapping for {name}"
            );
        }
    }

    #[test]
    fn permission_rule_tool_maps_edit_like() {
        for name in &["edit_file", "file_edit", "FileEdit"] {
            assert_eq!(
                canonical_permission_rule_tool(name),
                "Edit",
                "unexpected mapping for {name}"
            );
        }
    }

    #[test]
    fn permission_rule_tool_maps_write_like() {
        for name in &["write_file", "file_write", "FileWrite"] {
            assert_eq!(
                canonical_permission_rule_tool(name),
                "Write",
                "unexpected mapping for {name}"
            );
        }
    }

    #[test]
    fn permission_rule_tool_maps_multi_edit() {
        assert_eq!(canonical_permission_rule_tool("FileMultiEdit"), "MultiEdit");
    }

    #[test]
    fn permission_rule_tool_passthrough_for_unknown() {
        assert_eq!(canonical_permission_rule_tool("WebSearch"), "WebSearch");
        assert_eq!(canonical_permission_rule_tool("Agent"), "Agent");
    }

    // ------------------------------------------------------------------
    // exact_rule_subject
    // ------------------------------------------------------------------

    #[test]
    fn exact_rule_subject_extracts_command_for_bash() {
        let input = json!({"command": "cargo build"});
        let subject = exact_rule_subject("Bash", &input);
        assert_eq!(subject, "cargo build");
    }

    #[test]
    fn exact_rule_subject_extracts_file_path_for_read() {
        let input = json!({"file_path": "src/main.rs"});
        let subject = exact_rule_subject("Read", &input);
        assert_eq!(subject, "src/main.rs");
    }

    #[test]
    fn exact_rule_subject_falls_back_to_json() {
        let input = json!({"a": 1, "b": 2});
        let subject = exact_rule_subject("CustomTool", &input);
        let parsed: serde_json::Value = serde_json::from_str(&subject).unwrap();
        assert_eq!(parsed["a"], 1);
        assert_eq!(parsed["b"], 2);
    }

    // ------------------------------------------------------------------
    // shell_command_subject
    // ------------------------------------------------------------------

    #[test]
    fn shell_command_subject_prefers_command_field() {
        let input = json!({"command": "ls", "cmd": "rm"});
        assert_eq!(shell_command_subject(&input), "ls");
    }

    #[test]
    fn shell_command_subject_falls_back_to_cmd() {
        let input = json!({"cmd": "echo hello"});
        assert_eq!(shell_command_subject(&input), "echo hello");
    }

    #[test]
    fn shell_command_subject_falls_back_to_script() {
        let input = json!({"script": "#!/bin/bash"});
        assert_eq!(shell_command_subject(&input), "#!/bin/bash");
    }

    #[test]
    fn shell_command_subject_falls_back_to_json() {
        let input = json!({"other": "value"});
        assert!(shell_command_subject(&input).contains("other"));
    }

    // ------------------------------------------------------------------
    // input_string_field
    // ------------------------------------------------------------------

    #[test]
    fn input_string_field_finds_first_match() {
        let input = json!({"file_path": "a.rs", "path": "b.rs"});
        assert_eq!(
            input_string_field(&input, &["file_path", "path"]),
            Some("a.rs".to_string())
        );
    }

    #[test]
    fn input_string_field_skips_empty_strings() {
        let input = json!({"file_path": "", "path": "real.rs"});
        assert_eq!(
            input_string_field(&input, &["file_path", "path"]),
            Some("real.rs".to_string())
        );
    }

    #[test]
    fn input_string_field_returns_none_for_missing_keys() {
        let input = json!({});
        assert_eq!(input_string_field(&input, &["file_path"]), None);
    }

    #[test]
    fn input_string_field_returns_none_for_non_string_values() {
        let input = json!({"file_path": 42});
        assert_eq!(input_string_field(&input, &["file_path"]), None);
    }

    // ------------------------------------------------------------------
    // stable_json_subject
    // ------------------------------------------------------------------

    #[test]
    fn stable_json_subject_serializes_object() {
        let input = json!({"x": 1, "y": [2]});
        let subject = stable_json_subject(&input);
        let parsed: serde_json::Value = serde_json::from_str(&subject).unwrap();
        assert_eq!(parsed["x"], 1);
    }

    #[test]
    fn stable_json_subject_serializes_string_value() {
        let input = json!("hello");
        let subject = stable_json_subject(&input);
        assert_eq!(subject, r#""hello""#);
    }

    // ------------------------------------------------------------------
    // permission_action_summary
    // ------------------------------------------------------------------

    #[test]
    fn permission_action_summary_for_bash() {
        let input = json!({"command": "cargo test"});
        let summary = permission_action_summary("Bash", &input);
        assert_eq!(summary, "Run cargo test");
    }

    #[test]
    fn permission_action_summary_for_read() {
        let input = json!({"file_path": "/tmp/log.txt"});
        let summary = permission_action_summary("Read", &input);
        assert_eq!(summary, "Read /tmp/log.txt");
    }

    #[test]
    fn permission_action_summary_for_read_fallback() {
        let input = json!({"other": "value"});
        let summary = permission_action_summary("Read", &input);
        assert_eq!(summary, "Use Read");
    }

    #[test]
    fn permission_action_summary_for_edit() {
        let input = json!({"file_path": "src/lib.rs"});
        let summary = permission_action_summary("Edit", &input);
        assert_eq!(summary, "Edit src/lib.rs");
    }

    #[test]
    fn permission_action_summary_for_write() {
        let input = json!({"file_path": "/tmp/out.txt"});
        let summary = permission_action_summary("Write", &input);
        assert_eq!(summary, "Write /tmp/out.txt");
    }

    #[test]
    fn permission_action_summary_for_webfetch() {
        let input = json!({"url": "https://example.com"});
        let summary = permission_action_summary("WebFetch", &input);
        assert_eq!(summary, "Fetch https://example.com");
    }

    #[test]
    fn permission_action_summary_for_websearch() {
        let input = json!({"query": "rust async"});
        let summary = permission_action_summary("WebSearch", &input);
        assert_eq!(summary, "Search rust async");
    }

    #[test]
    fn permission_action_summary_unknown_tool() {
        let input = json!({"any": "val"});
        let summary = permission_action_summary("CustomTool", &input);
        assert_eq!(summary, "Use CustomTool");
    }

    // ------------------------------------------------------------------
    // permission_risk_level
    // ------------------------------------------------------------------

    #[test]
    fn permission_risk_level_bash_destructive_rm_rf() {
        let input = json!({"command": "rm -rf /"});
        assert_eq!(
            permission_risk_level("Bash", &input),
            Some("destructive".to_string())
        );
    }

    #[test]
    fn permission_risk_level_bash_destructive_remove_item() {
        let input = json!({"command": "Remove-Item -Recurse C:\\"});
        assert_eq!(
            permission_risk_level("PowerShell", &input),
            Some("destructive".to_string())
        );
    }

    #[test]
    fn permission_risk_level_bash_destructive_shutdown() {
        let input = json!({"command": "shutdown -h now"});
        assert_eq!(
            permission_risk_level("Bash", &input),
            Some("destructive".to_string())
        );
    }

    #[test]
    fn permission_risk_level_bash_high_sudo() {
        let input = json!({"command": "sudo apt install"});
        assert_eq!(
            permission_risk_level("bash", &input),
            Some("high".to_string())
        );
    }

    #[test]
    fn permission_risk_level_bash_high_curl() {
        let input = json!({"command": "curl https://example.com"});
        assert_eq!(
            permission_risk_level("Bash", &input),
            Some("high".to_string())
        );
    }

    #[test]
    fn permission_risk_level_bash_medium() {
        let input = json!({"command": "ls -la"});
        assert_eq!(
            permission_risk_level("Bash", &input),
            Some("medium".to_string())
        );
    }

    #[test]
    fn permission_risk_level_edit_medium() {
        let input = json!({});
        assert_eq!(
            permission_risk_level("Edit", &input),
            Some("medium".to_string())
        );
    }

    #[test]
    fn permission_risk_level_write_medium() {
        let input = json!({});
        assert_eq!(
            permission_risk_level("FileWrite", &input),
            Some("medium".to_string())
        );
    }

    #[test]
    fn permission_risk_level_webfetch_safe() {
        let input = json!({"url": "https://example.com"});
        assert_eq!(
            permission_risk_level("WebFetch", &input),
            Some("safe".to_string())
        );
    }

    #[test]
    fn permission_risk_level_websearch_safe() {
        let input = json!({"query": "test"});
        assert_eq!(
            permission_risk_level("WebSearch", &input),
            Some("safe".to_string())
        );
    }

    #[test]
    fn permission_risk_level_unknown_tool_returns_none() {
        let input = json!({});
        assert_eq!(permission_risk_level("Read", &input), None);
    }

    // ------------------------------------------------------------------
    // elapsed_ms
    // ------------------------------------------------------------------

    #[test]
    fn elapsed_ms_returns_some_nonzero() {
        let started = std::time::Instant::now();
        let ms = elapsed_ms(started).unwrap();
        assert!(ms < 1000, "fresh Instant should be < 1s");
    }

    // ------------------------------------------------------------------
    // filter_tools_for_allowed_override
    // ------------------------------------------------------------------

    #[test]
    fn filter_tools_for_allowed_override_returns_all_when_no_override() {
        let tools: Tools = Vec::new();
        let result = filter_tools_for_allowed_override(tools, None);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_tools_for_allowed_override_filters_by_name_case_insensitive() {
        struct NamedTool(&'static str);
        #[async_trait::async_trait]
        impl crate::types::tool::Tool for NamedTool {
            fn name(&self) -> &str {
                self.0
            }
            fn input_json_schema(&self) -> serde_json::Value {
                json!({})
            }
            async fn call(
                &self,
                _input: serde_json::Value,
                _ctx: &crate::types::tool::ToolUseContext,
                _parent_message: &crate::types::message::AssistantMessage,
                _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult::default())
            }
            async fn description(&self, _input: &serde_json::Value) -> String {
                self.0.to_string()
            }
            async fn prompt(&self) -> String {
                String::new()
            }
        }

        let tools: Tools = vec![
            Arc::new(NamedTool("Bash")),
            Arc::new(NamedTool("Read")),
            Arc::new(NamedTool("Edit")),
        ];

        let allowed = vec!["bash".to_string(), "read".to_string()];
        let filtered = filter_tools_for_allowed_override(tools, Some(&allowed));
        let names: Vec<String> = filtered.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(names, vec!["Bash", "Read"]);
    }

    // ------------------------------------------------------------------
    // permission_auto_review_event
    // ------------------------------------------------------------------

    #[test]
    fn permission_auto_review_event_constructs() {
        let event = permission_auto_review_event(
            "review-1",
            "tool-use-1",
            "started",
            Some("high".to_string()),
            true,
            Some("user authorized".to_string()),
            Some("Run ls".to_string()),
        );
        assert_eq!(event.review_id, "review-1");
        assert_eq!(event.target_tool_use_id, "tool-use-1");
        assert_eq!(event.status, "started");
        assert_eq!(event.risk_level, Some("high".to_string()));
        assert!(event.user_authorization);
        assert_eq!(event.rationale, Some("user authorized".to_string()));
        assert_eq!(event.action, Some("Run ls".to_string()));
        assert_eq!(event.decision_source, "auto_review");
    }

    #[test]
    fn permission_auto_review_event_without_optionals() {
        let event = permission_auto_review_event(
            "review-2",
            "tool-use-2",
            "failed",
            None,
            false,
            None,
            None,
        );
        assert_eq!(event.status, "failed");
        assert!(event.risk_level.is_none());
        assert!(!event.user_authorization);
        assert!(event.rationale.is_none());
        assert!(event.action.is_none());
    }
}
