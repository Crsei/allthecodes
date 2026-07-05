use super::execute::{
    add_local_always_allow_rule, elapsed_ms, exact_always_allow_rule, permission_action_summary,
    permission_auto_review_event, permission_denied_exec_result, permission_risk_level,
    persist_local_always_allow_rule, tool_exec_result, tool_exec_result_with_brief,
};
use super::*;
use allthecodes_types::agent_runtime_record::AgentRuntimePermissionDecision;
use allthecodes_types::hooks::{
    HookEventConfig, HookRunner, HooksMap, PermissionOverride, PostToolHookResult,
    PreToolHookResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputValidationKind {
    Original,
    PreHookProduced,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SanitizedInput {
    pub(crate) value: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(crate) struct PreHookContinue {
    pub(crate) effective_input: serde_json::Value,
    pub(crate) permission_override: Option<PermissionOverride>,
}

#[derive(Debug, Clone)]
pub(crate) struct PermissionOverrideContinue {
    pub(crate) hook_decision: Option<crate::permissions::decision::HookPermissionDecision>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolExecutionPlan {
    pub(crate) effective_input: serde_json::Value,
    pub(crate) permission_decision: AgentRuntimePermissionDecision,
    pub(crate) accepted_permission_feedback: Option<String>,
}

impl ToolExecutionPlan {
    pub(crate) fn new(
        effective_input: serde_json::Value,
        permission_decision: AgentRuntimePermissionDecision,
    ) -> Self {
        Self {
            effective_input,
            permission_decision,
            accepted_permission_feedback: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PermissionPromptContinue {
    pub(crate) permission_decision: AgentRuntimePermissionDecision,
    pub(crate) accepted_permission_feedback: Option<String>,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ToolCallOutcome {
    Success {
        result: crate::types::tool::ToolResult,
        tool_start: std::time::Instant,
    },
    Error {
        message: String,
        tool_start: std::time::Instant,
    },
}

#[derive(Debug)]
pub(crate) struct PostHookContinue {
    pub(crate) result: crate::types::tool::ToolResult,
    pub(crate) hook_stopped_continuation: bool,
    pub(crate) tool_start: std::time::Instant,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PipelineStageResult<T> {
    Continue(T),
    Finish(ToolExecResult),
}

pub(crate) type PreHookStageResult = PipelineStageResult<PreHookContinue>;
pub(crate) type PermissionOverrideStageResult = PipelineStageResult<PermissionOverrideContinue>;
pub(crate) type PermissionPromptStageResult = PipelineStageResult<PermissionPromptContinue>;
pub(crate) type ToolCallStageResult = PipelineStageResult<ToolCallOutcome>;
pub(crate) type PostHookStageResult = PipelineStageResult<PostHookContinue>;

pub(crate) struct ToolExecutionPipeline<'a> {
    pub(crate) deps: &'a QueryEngineDeps,
    pub(crate) request: &'a ToolExecRequest,
    pub(crate) tool: Arc<dyn crate::types::tool::Tool>,
    pub(crate) ctx: &'a crate::types::tool::ToolUseContext,
    pub(crate) hooks: &'a dyn HookRunner,
    pub(crate) hooks_map: &'a HooksMap,
    pub(crate) pre_configs: &'a [HookEventConfig],
    pub(crate) post_configs: &'a [HookEventConfig],
    pub(crate) failure_configs: &'a [HookEventConfig],
    pub(crate) started: std::time::Instant,
}

impl<'a> ToolExecutionPipeline<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        deps: &'a QueryEngineDeps,
        request: &'a ToolExecRequest,
        tool: Arc<dyn crate::types::tool::Tool>,
        ctx: &'a crate::types::tool::ToolUseContext,
        hooks: &'a dyn HookRunner,
        hooks_map: &'a HooksMap,
        pre_configs: &'a [HookEventConfig],
        post_configs: &'a [HookEventConfig],
        failure_configs: &'a [HookEventConfig],
        started: std::time::Instant,
    ) -> Self {
        Self {
            deps,
            request,
            tool,
            ctx,
            hooks,
            hooks_map,
            pre_configs,
            post_configs,
            failure_configs,
            started,
        }
    }

    pub(crate) async fn validate_input(
        &self,
        input: &serde_json::Value,
        kind: InputValidationKind,
    ) -> PipelineStageResult<()> {
        match self.tool.validate_input(input, self.ctx).await {
            ValidationResult::Ok => PipelineStageResult::Continue(()),
            ValidationResult::Error { message, .. } => {
                let data = match kind {
                    InputValidationKind::Original => serde_json::json!(format!(
                        "Input validation error: {}. The schema was not sent - please check the tool's input requirements.",
                        message
                    )),
                    InputValidationKind::PreHookProduced => {
                        serde_json::json!(format!("Pre-tool hook produced invalid input: {}.", message))
                    }
                };
                PipelineStageResult::Finish(tool_exec_result(
                    self.request,
                    crate::types::tool::ToolResult {
                        data,
                        new_messages: vec![],
                        ..Default::default()
                    },
                    true,
                    false,
                    input.clone(),
                    elapsed_ms(self.started),
                    Some(match kind {
                        InputValidationKind::Original => {
                            AgentRuntimePermissionDecision::NotRequired
                        }
                        InputValidationKind::PreHookProduced => {
                            AgentRuntimePermissionDecision::DeniedByHook
                        }
                    }),
                ))
            }
        }
    }

    pub(crate) fn sanitize_input(&self) -> SanitizedInput {
        let mut value = self.request.input.clone();
        sanitize_tool_input(&mut value);
        SanitizedInput { value }
    }

    pub(crate) fn security_validate(&self, input: &serde_json::Value) -> PipelineStageResult<()> {
        if let Some(result) = security_validate(
            &self.request.tool_use_id,
            &self.request.tool_name,
            input,
            self.tool.as_ref(),
            self.ctx,
            self.started,
        ) {
            return PipelineStageResult::Finish(
                tool_execution_result_to_exec_result(result).with_runtime_metadata(
                    input.clone(),
                    elapsed_ms(self.started),
                    Some(AgentRuntimePermissionDecision::DeniedByPolicy),
                ),
            );
        }
        PipelineStageResult::Continue(())
    }

    pub(crate) async fn run_pre_hooks(
        &self,
        sanitized_input: &serde_json::Value,
    ) -> PreHookStageResult {
        let (effective_input, permission_override) = match self
            .hooks
            .run_pre_tool_hooks(&self.request.tool_name, sanitized_input, self.pre_configs)
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
                return PipelineStageResult::Finish(tool_exec_result(
                    self.request,
                    crate::types::tool::ToolResult {
                        data: serde_json::json!(format!("Pre-tool hook stopped: {}", message)),
                        new_messages: vec![],
                        ..Default::default()
                    },
                    true,
                    false,
                    sanitized_input.clone(),
                    elapsed_ms(self.started),
                    Some(AgentRuntimePermissionDecision::DeniedByHook),
                ));
            }
            Err(e) => {
                if hook_error_is_critical(&self.request.tool_name, self.pre_configs) {
                    tracing::warn!(error = %e, tool = %self.request.tool_name, "critical pre-tool hook error, blocking tool execution");
                    return PipelineStageResult::Finish(tool_exec_result(
                        self.request,
                        crate::types::tool::ToolResult {
                            data: serde_json::json!(format!(
                                "Critical pre-tool hook failed: {}",
                                e
                            )),
                            new_messages: vec![],
                            ..Default::default()
                        },
                        true,
                        false,
                        sanitized_input.clone(),
                        elapsed_ms(self.started),
                        Some(AgentRuntimePermissionDecision::DeniedByHook),
                    ));
                }
                tracing::warn!(error = %e, "optional pre-tool hook error, continuing");
                (sanitized_input.clone(), None)
            }
        };

        if effective_input != *sanitized_input {
            if let PipelineStageResult::Finish(result) = self
                .validate_input(&effective_input, InputValidationKind::PreHookProduced)
                .await
            {
                return PipelineStageResult::Finish(result);
            }
            if let PipelineStageResult::Finish(result) = self.security_validate(&effective_input) {
                return PipelineStageResult::Finish(result);
            }
        }

        PipelineStageResult::Continue(PreHookContinue {
            effective_input,
            permission_override,
        })
    }

    pub(crate) async fn resolve_permission(
        &self,
        permission_override: Option<&PermissionOverride>,
        effective_input: &serde_json::Value,
        permission_decision: &mut AgentRuntimePermissionDecision,
    ) -> PermissionOverrideStageResult {
        let hook_decision = match permission_override {
            Some(PermissionOverride::Allow) => {
                *permission_decision = AgentRuntimePermissionDecision::AllowedByHook;
                tracing::debug!(
                    tool = %self.request.tool_name,
                    "Permission allow requested by hook override"
                );
                emit_hook_permission_decision(
                    self.ctx,
                    "PreToolUse",
                    "PreToolUse",
                    "*",
                    "allow",
                    vec![format!("tool: {}", self.request.tool_name)],
                );
                Some(crate::permissions::decision::HookPermissionDecision {
                    allow: true,
                    source: Some("PreToolUse".to_string()),
                    ..Default::default()
                })
            }
            Some(PermissionOverride::Deny { .. }) | None => None,
        };

        if let Some(PermissionOverride::Deny { reason }) = permission_override {
            *permission_decision = AgentRuntimePermissionDecision::DeniedByHook;
            emit_hook_permission_decision(
                self.ctx,
                "PreToolUse",
                "PreToolUse",
                "*",
                "deny",
                vec![reason.clone()],
            );
            self.fire_permission_denied_hook(
                effective_input,
                format!("Permission denied by hook: {}", reason),
            )
            .await;
            self.deps
                .record_tool_permission_response(
                    &self.request.tool_use_id,
                    "deny",
                    Some(format!("Permission denied by hook: {reason}")),
                )
                .await;
            return PipelineStageResult::Finish(tool_exec_result(
                self.request,
                crate::types::tool::ToolResult {
                    data: serde_json::json!(format!("Permission denied by hook: {}", reason)),
                    new_messages: vec![],
                    ..Default::default()
                },
                true,
                false,
                effective_input.clone(),
                elapsed_ms(self.started),
                Some(*permission_decision),
            ));
        }

        PipelineStageResult::Continue(PermissionOverrideContinue { hook_decision })
    }

    pub(crate) async fn maybe_prompt_user(
        &self,
        message: String,
        plan: &ToolExecutionPlan,
    ) -> PermissionPromptStageResult {
        let perm_audit_ctx = self.deps.audit_ctx.with_tool_use(&self.request.tool_use_id);
        {
            use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
            perm_audit_ctx.emit(
                EventKind::PermissionRequested,
                Stage::Permission,
                AuditLevel::Info,
                Outcome::Info,
                None,
                Some(serde_json::json!({
                    "tool_name": self.request.tool_name,
                    "message": message,
                })),
            );
        }

        let options = vec![
            "Allow".to_string(),
            "Deny".to_string(),
            "Always Allow".to_string(),
            "Auto Review".to_string(),
        ];
        self.deps
            .record_tool_permission_request(self.request, &plan.effective_input, &message, &options)
            .await;

        let mut permission_decision = plan.permission_decision;
        let mut accepted_permission_feedback = plan.accepted_permission_feedback.clone();

        let mut hook_allowed = false;
        let perm_req_configs = self
            .hooks
            .load_hook_configs(self.hooks_map, "PermissionRequest");
        if !perm_req_configs.is_empty() {
            let payload = serde_json::json!({
                "tool_name": self.request.tool_name,
                "tool_input": plan.effective_input.clone(),
                "message": message,
            });
            if let Ok(output) = self
                .hooks
                .run_event_hooks("PermissionRequest", &payload, &perm_req_configs)
                .await
            {
                if let Some(ref decision) = output.permission_decision {
                    emit_hook_permission_decision(
                        self.ctx,
                        "PermissionRequest",
                        "PermissionRequest",
                        "*",
                        decision,
                        vec![format!("tool: {}", self.request.tool_name)],
                    );
                    match decision.as_str() {
                        "allow" => {
                            tracing::debug!(
                                tool = %self.request.tool_name,
                                "PermissionRequest hook allowed tool execution"
                            );
                            permission_decision = AgentRuntimePermissionDecision::AllowedByHook;
                            self.deps
                                .record_tool_permission_response(
                                    &self.request.tool_use_id,
                                    "allow",
                                    Some("PermissionRequest hook".to_string()),
                                )
                                .await;
                            hook_allowed = true;
                        }
                        "deny" => {
                            self.fire_permission_denied_hook(
                                &plan.effective_input,
                                "Permission denied by PermissionRequest hook".to_string(),
                            )
                            .await;
                            self.deps
                                .record_tool_permission_response(
                                    &self.request.tool_use_id,
                                    "deny",
                                    Some("PermissionRequest hook".to_string()),
                                )
                                .await;
                            return PipelineStageResult::Finish(tool_exec_result(
                                self.request,
                                crate::types::tool::ToolResult {
                                    data: serde_json::json!("Permission denied by hook"),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                true,
                                false,
                                plan.effective_input.clone(),
                                elapsed_ms(self.started),
                                Some(AgentRuntimePermissionDecision::DeniedByHook),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }

        if hook_allowed {
            return PipelineStageResult::Continue(PermissionPromptContinue {
                permission_decision,
                accepted_permission_feedback,
            });
        }

        let Some(ref callback) = self.ctx.permission_callback else {
            self.fire_permission_denied_hook(
                &plan.effective_input,
                format!("Permission required (no callback): {}", message),
            )
            .await;

            self.deps
                .record_tool_permission_response(
                    &self.request.tool_use_id,
                    "deny",
                    Some(format!("Permission required: {message}")),
                )
                .await;
            return PipelineStageResult::Finish(permission_denied_exec_result(
                self.request,
                format!("Permission required: {}", message),
                plan.effective_input.clone(),
                self.started,
                AgentRuntimePermissionDecision::DeniedByPolicy,
            ));
        };

        let response = callback(PermissionRequestPayload {
            tool_use_id: self.request.tool_use_id.clone(),
            tool_name: self.request.tool_name.clone(),
            tool_input: plan.effective_input.clone(),
            message: message.clone(),
            options: options.clone(),
            operation: None,
        })
        .await;

        let decision = response.normalized_decision();
        self.deps
            .record_tool_permission_response(
                &self.request.tool_use_id,
                decision.clone(),
                response.feedback.clone(),
            )
            .await;
        match decision.as_str() {
            "allow" => {
                permission_decision = AgentRuntimePermissionDecision::AllowedByUser;
                accepted_permission_feedback = response.feedback.clone();
                use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                perm_audit_ctx.emit(
                    EventKind::PermissionResolved,
                    Stage::Permission,
                    AuditLevel::Info,
                    Outcome::Completed,
                    None,
                    Some(serde_json::json!({
                        "tool_name": self.request.tool_name,
                        "decision": "allow",
                    })),
                );
            }
            "always_allow" => {
                permission_decision = AgentRuntimePermissionDecision::AllowedByUser;
                let rule = exact_always_allow_rule(&self.request.tool_name, &plan.effective_input);
                if let Err(error) = persist_local_always_allow_rule(&self.deps.cwd, &rule) {
                    let error_message =
                        format!("Permission denied: failed to persist Always Allow rule: {error}");
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    perm_audit_ctx.emit(
                        EventKind::PermissionResolved,
                        Stage::Permission,
                        AuditLevel::Warn,
                        Outcome::Denied,
                        None,
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "decision": "always_allow",
                            "source": "user",
                            "error": error.to_string(),
                        })),
                    );
                    return PipelineStageResult::Finish(permission_denied_exec_result(
                        self.request,
                        error_message,
                        plan.effective_input.clone(),
                        self.started,
                        AgentRuntimePermissionDecision::DeniedByUser,
                    ));
                }
                {
                    let mut state = self.deps.state.write();
                    add_local_always_allow_rule(&mut state, &rule);
                }
                tracing::debug!(
                    tool = %self.request.tool_name,
                    rule = %rule,
                    "local exact always_allow rule recorded"
                );
                accepted_permission_feedback = response.feedback.clone();
                use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                perm_audit_ctx.emit(
                    EventKind::PermissionResolved,
                    Stage::Permission,
                    AuditLevel::Info,
                    Outcome::Completed,
                    None,
                    Some(serde_json::json!({
                        "tool_name": self.request.tool_name,
                        "decision": "always_allow",
                        "source": "user",
                        "rule": rule,
                    })),
                );
            }
            "auto_review" => {
                permission_decision = AgentRuntimePermissionDecision::AllowedByUser;
                let risk_level =
                    permission_risk_level(&self.request.tool_name, &plan.effective_input);
                let action = Some(permission_action_summary(
                    &self.request.tool_name,
                    &plan.effective_input,
                ));
                let review_id = Uuid::new_v4().to_string();
                let start_result = {
                    let mut state = self.deps.state.write();
                    state
                        .permissions
                        .auto_review_tracker
                        .try_start(&self.request.tool_use_id)
                };
                if let Err(reason) = start_result {
                    emit_permission_auto_review(
                        self.ctx,
                        permission_auto_review_event(
                            &review_id,
                            &self.request.tool_use_id,
                            "circuit_open",
                            risk_level.clone(),
                            true,
                            Some(reason.to_string()),
                            action.clone(),
                        ),
                    );
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    perm_audit_ctx.emit(
                        EventKind::PermissionResolved,
                        Stage::Permission,
                        AuditLevel::Warn,
                        Outcome::Denied,
                        None,
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "decision": "auto_review",
                            "source": "user",
                            "reason": reason,
                        })),
                    );
                    return PipelineStageResult::Finish(permission_denied_exec_result(
                        self.request,
                        format!("Permission denied: auto review unavailable ({reason})."),
                        plan.effective_input.clone(),
                        self.started,
                        AgentRuntimePermissionDecision::DeniedByUser,
                    ));
                }

                emit_permission_auto_review(
                    self.ctx,
                    permission_auto_review_event(
                        &review_id,
                        &self.request.tool_use_id,
                        "started",
                        risk_level.clone(),
                        true,
                        response.feedback.clone(),
                        action.clone(),
                    ),
                );

                let mut classifier_input =
                    self.tool.to_auto_classifier_input(&plan.effective_input);
                if matches!(&classifier_input, serde_json::Value::String(s) if s.is_empty()) {
                    classifier_input = plan.effective_input.clone();
                }
                let classifier = self
                    .deps
                    .compute_permission_auto_review(
                        &self.request.tool_name,
                        &plan.effective_input,
                        &classifier_input,
                    )
                    .await;
                let Some(classifier) = classifier else {
                    {
                        let mut state = self.deps.state.write();
                        state.permissions.auto_review_tracker.record_denied();
                    }
                    emit_permission_auto_review(
                        self.ctx,
                        permission_auto_review_event(
                            &review_id,
                            &self.request.tool_use_id,
                            "failed",
                            risk_level.clone(),
                            true,
                            Some("classifier unavailable".to_string()),
                            action.clone(),
                        ),
                    );
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    perm_audit_ctx.emit(
                        EventKind::PermissionResolved,
                        Stage::Permission,
                        AuditLevel::Warn,
                        Outcome::Denied,
                        None,
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "decision": "auto_review",
                            "source": "user",
                            "reason": "classifier unavailable",
                        })),
                    );
                    return PipelineStageResult::Finish(permission_denied_exec_result(
                        self.request,
                        "Permission denied: auto review classifier is unavailable.".to_string(),
                        plan.effective_input.clone(),
                        self.started,
                        AgentRuntimePermissionDecision::DeniedByUser,
                    ));
                };

                let rationale = if classifier.reason.trim().is_empty() {
                    None
                } else {
                    Some(classifier.reason.clone())
                };
                if classifier.unavailable || classifier.transcript_too_long {
                    {
                        let mut state = self.deps.state.write();
                        state.permissions.auto_review_tracker.record_denied();
                    }
                    emit_permission_auto_review(
                        self.ctx,
                        permission_auto_review_event(
                            &review_id,
                            &self.request.tool_use_id,
                            "failed",
                            risk_level.clone(),
                            true,
                            rationale.clone(),
                            action.clone(),
                        ),
                    );
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    perm_audit_ctx.emit(
                        EventKind::PermissionResolved,
                        Stage::Permission,
                        AuditLevel::Warn,
                        Outcome::Denied,
                        None,
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "decision": "auto_review",
                            "source": "user",
                            "reason": classifier.reason.clone(),
                        })),
                    );
                    return PipelineStageResult::Finish(permission_denied_exec_result(
                        self.request,
                        format!(
                            "Permission denied: auto review could not classify this request: {}",
                            classifier.reason
                        ),
                        plan.effective_input.clone(),
                        self.started,
                        AgentRuntimePermissionDecision::DeniedByUser,
                    ));
                }

                match classifier.verdict {
                    AutoClassifierVerdict::Allow => {
                        {
                            let mut state = self.deps.state.write();
                            state.permissions.auto_review_tracker.record_allowed();
                        }
                        emit_permission_auto_review(
                            self.ctx,
                            permission_auto_review_event(
                                &review_id,
                                &self.request.tool_use_id,
                                "approved",
                                risk_level,
                                true,
                                rationale,
                                action,
                            ),
                        );
                        accepted_permission_feedback = response.feedback.clone();
                        use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                        perm_audit_ctx.emit(
                            EventKind::PermissionResolved,
                            Stage::Permission,
                            AuditLevel::Info,
                            Outcome::Completed,
                            None,
                            Some(serde_json::json!({
                                "tool_name": self.request.tool_name,
                                "decision": "auto_review",
                                "source": "user",
                                "classifier_model": classifier.model,
                            })),
                        );
                    }
                    AutoClassifierVerdict::Deny | AutoClassifierVerdict::Ask => {
                        {
                            let mut state = self.deps.state.write();
                            state.permissions.auto_review_tracker.record_denied();
                        }
                        let reason = if classifier.reason.trim().is_empty() {
                            "auto review did not approve this request".to_string()
                        } else {
                            classifier.reason.clone()
                        };
                        emit_permission_auto_review(
                            self.ctx,
                            permission_auto_review_event(
                                &review_id,
                                &self.request.tool_use_id,
                                "denied",
                                risk_level,
                                true,
                                Some(reason.clone()),
                                action,
                            ),
                        );
                        use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                        perm_audit_ctx.emit(
                            EventKind::PermissionResolved,
                            Stage::Permission,
                            AuditLevel::Warn,
                            Outcome::Denied,
                            None,
                            Some(serde_json::json!({
                                "tool_name": self.request.tool_name,
                                "decision": "auto_review",
                                "source": "user",
                                "reason": reason.clone(),
                            })),
                        );

                        self.fire_permission_denied_hook(
                            &plan.effective_input,
                            "Permission denied by auto review".to_string(),
                        )
                        .await;

                        return PipelineStageResult::Finish(permission_denied_exec_result(
                            self.request,
                            format!("Permission denied by auto review: {reason}"),
                            plan.effective_input.clone(),
                            self.started,
                            AgentRuntimePermissionDecision::DeniedByUser,
                        ));
                    }
                }
            }
            _ => {
                let denial_message = permission_denied_message(&response);
                {
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    perm_audit_ctx.emit(
                        EventKind::PermissionResolved,
                        Stage::Permission,
                        AuditLevel::Warn,
                        Outcome::Denied,
                        None,
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "decision": "deny",
                            "source": "user",
                        })),
                    );
                }

                self.fire_permission_denied_hook(
                    &plan.effective_input,
                    "Permission denied by user".to_string(),
                )
                .await;

                return PipelineStageResult::Finish(permission_denied_exec_result(
                    self.request,
                    denial_message,
                    plan.effective_input.clone(),
                    self.started,
                    AgentRuntimePermissionDecision::DeniedByUser,
                ));
            }
        }

        PipelineStageResult::Continue(PermissionPromptContinue {
            permission_decision,
            accepted_permission_feedback,
        })
    }

    pub(crate) async fn call_tool(
        &self,
        plan: &ToolExecutionPlan,
        parent_message: &crate::types::message::AssistantMessage,
        on_progress: Option<Arc<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> ToolCallStageResult {
        let tool_audit_ctx = self.deps.audit_ctx.with_tool_use(&self.request.tool_use_id);
        let tool_langfuse_span = self.deps.langfuse_trace.as_ref().and_then(|trace| {
            crate::services::langfuse::create_tool_span(
                trace,
                &self.request.tool_name,
                &self.request.tool_use_id,
                &plan.effective_input,
                self.request.langfuse_batch_span.as_ref(),
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
                    "tool_name": self.request.tool_name,
                })),
            );
        }
        let tool_start = std::time::Instant::now();

        let tool_use_id_for_progress = self.request.tool_use_id.clone();
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

        match self
            .tool
            .call(
                plan.effective_input.clone(),
                self.ctx,
                parent_message,
                boxed_progress,
            )
            .await
        {
            Ok(result) => {
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
                    &self.request.tool_name,
                    &result_preview,
                    false,
                );
                {
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    tool_audit_ctx.emit(
                        EventKind::ToolFinish,
                        Stage::ToolExecution,
                        AuditLevel::Info,
                        Outcome::Completed,
                        Some(tool_start.elapsed().as_millis() as u64),
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                        })),
                    );
                }
                PipelineStageResult::Continue(ToolCallOutcome::Success { result, tool_start })
            }
            Err(e) => {
                let message = e.to_string();
                crate::services::langfuse::finish_tool_span(
                    tool_langfuse_span,
                    &self.request.tool_name,
                    &message,
                    true,
                );
                {
                    use crate::observability::{AuditLevel, EventKind, Outcome, Stage};
                    tool_audit_ctx.emit(
                        EventKind::ToolError,
                        Stage::ToolExecution,
                        AuditLevel::Error,
                        Outcome::Failed,
                        Some(tool_start.elapsed().as_millis() as u64),
                        Some(serde_json::json!({
                            "tool_name": self.request.tool_name,
                            "error": message,
                        })),
                    );
                }
                PipelineStageResult::Continue(ToolCallOutcome::Error {
                    message,
                    tool_start,
                })
            }
        }
    }

    pub(crate) async fn run_post_hooks(
        &self,
        plan: &ToolExecutionPlan,
        outcome: ToolCallOutcome,
    ) -> PostHookStageResult {
        match outcome {
            ToolCallOutcome::Success { result, tool_start } => {
                let mut hook_stopped_continuation = false;
                if !self.post_configs.is_empty() {
                    match self
                        .hooks
                        .run_post_tool_hooks(
                            &self.request.tool_name,
                            &plan.effective_input,
                            &result.data,
                            self.post_configs,
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
                        Err(e)
                            if hook_error_is_critical(
                                &self.request.tool_name,
                                self.post_configs,
                            ) =>
                        {
                            tracing::warn!(error = %e, tool = %self.request.tool_name, "critical post-tool hook error, failing tool execution");
                            return PipelineStageResult::Finish(tool_exec_result(
                                self.request,
                                crate::types::tool::ToolResult {
                                    data: serde_json::json!(format!(
                                        "Critical post-tool hook failed: {}",
                                        e
                                    )),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                true,
                                false,
                                plan.effective_input.clone(),
                                elapsed_ms(tool_start),
                                Some(plan.permission_decision),
                            ));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, tool = %self.request.tool_name, "optional post-tool hook error, continuing");
                        }
                    }
                }

                PipelineStageResult::Continue(PostHookContinue {
                    result,
                    hook_stopped_continuation,
                    tool_start,
                })
            }
            ToolCallOutcome::Error {
                message,
                tool_start,
            } => {
                if !self.failure_configs.is_empty() {
                    match self
                        .hooks
                        .run_post_tool_failure_hooks(
                            &self.request.tool_name,
                            &plan.effective_input,
                            &message,
                            self.failure_configs,
                        )
                        .await
                    {
                        Ok(()) => {}
                        Err(hook_error)
                            if hook_error_is_critical(
                                &self.request.tool_name,
                                self.failure_configs,
                            ) =>
                        {
                            tracing::warn!(error = %hook_error, tool = %self.request.tool_name, "critical post-failure hook error, failing tool execution");
                            return PipelineStageResult::Finish(tool_exec_result(
                                self.request,
                                crate::types::tool::ToolResult {
                                    data: serde_json::json!(format!(
                                        "Critical post-failure hook failed after tool error ({}): {}",
                                        message, hook_error
                                    )),
                                    new_messages: vec![],
                                    ..Default::default()
                                },
                                true,
                                false,
                                plan.effective_input.clone(),
                                elapsed_ms(tool_start),
                                Some(plan.permission_decision),
                            ));
                        }
                        Err(hook_error) => {
                            tracing::warn!(error = %hook_error, tool = %self.request.tool_name, "optional post-failure hook error, continuing");
                        }
                    }
                }

                PipelineStageResult::Finish(tool_exec_result(
                    self.request,
                    crate::types::tool::ToolResult {
                        data: serde_json::json!(format!("Error: {}", message)),
                        new_messages: vec![],
                        ..Default::default()
                    },
                    true,
                    false,
                    plan.effective_input.clone(),
                    elapsed_ms(tool_start),
                    Some(plan.permission_decision),
                ))
            }
        }
    }

    pub(crate) fn record_and_audit(
        &self,
        mut result: crate::types::tool::ToolResult,
        plan: &ToolExecutionPlan,
        hook_stopped_continuation: bool,
        tool_start: std::time::Instant,
    ) -> PipelineStageResult<()> {
        if let Some(feedback) = plan.accepted_permission_feedback.as_deref() {
            result
                .new_messages
                .push(permission_feedback_message(feedback));
        }

        let brief_message = allthecodes_types::brief::brief_payload_from_tool_result(
            &self.request.tool_name,
            &self.request.tool_use_id,
            &self.deps.session_id,
            &result.data,
        );

        result.data = allthecodes_tools::result::enforce_result_size(
            result.data,
            self.tool.max_result_size_chars(),
        );

        PipelineStageResult::Finish(tool_exec_result_with_brief(
            self.request,
            result,
            false,
            hook_stopped_continuation,
            plan.effective_input.clone(),
            elapsed_ms(tool_start),
            Some(plan.permission_decision),
            brief_message,
        ))
    }

    async fn fire_permission_denied_hook(&self, input: &serde_json::Value, reason: String) {
        let deny_configs = self
            .hooks
            .load_hook_configs(self.hooks_map, "PermissionDenied");
        if deny_configs.is_empty() {
            return;
        }
        let payload = serde_json::json!({
            "tool_name": self.request.tool_name,
            "tool_input": input.clone(),
            "reason": reason,
        });
        let _ = self
            .hooks
            .run_event_hooks("PermissionDenied", &payload, &deny_configs)
            .await;
    }
}

pub(crate) fn sanitize_tool_input(input: &mut serde_json::Value) {
    if let Some(obj) = input.as_object_mut() {
        obj.remove("_simulatedSedEdit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_execution_pipeline_sanitizes_simulated_sed_edit() {
        let mut input = serde_json::json!({
            "file_path": "src/lib.rs",
            "_simulatedSedEdit": true
        });

        sanitize_tool_input(&mut input);

        assert_eq!(input, serde_json::json!({"file_path": "src/lib.rs"}));
    }
}
