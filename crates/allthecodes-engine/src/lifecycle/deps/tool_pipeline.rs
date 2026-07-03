use super::execute::{elapsed_ms, tool_exec_result};
use super::*;
use allthecodes_types::agent_runtime_record::AgentRuntimePermissionDecision;
use allthecodes_types::hooks::{
    HookEventConfig, HookRunner, HooksMap, PermissionOverride, PreToolHookResult,
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

#[derive(Debug, Clone)]
pub(crate) enum PipelineStageResult<T> {
    Continue(T),
    Finish(ToolExecResult),
}

pub(crate) type PreHookStageResult = PipelineStageResult<PreHookContinue>;
pub(crate) type PermissionOverrideStageResult = PipelineStageResult<PermissionOverrideContinue>;

pub(crate) struct ToolExecutionPipeline<'a> {
    pub(crate) deps: &'a QueryEngineDeps,
    pub(crate) request: &'a ToolExecRequest,
    pub(crate) tool: Arc<dyn crate::types::tool::Tool>,
    pub(crate) ctx: &'a crate::types::tool::ToolUseContext,
    pub(crate) hooks: &'a dyn HookRunner,
    pub(crate) hooks_map: &'a HooksMap,
    pub(crate) pre_configs: &'a [HookEventConfig],
    pub(crate) started: std::time::Instant,
}

impl<'a> ToolExecutionPipeline<'a> {
    pub(crate) fn new(
        deps: &'a QueryEngineDeps,
        request: &'a ToolExecRequest,
        tool: Arc<dyn crate::types::tool::Tool>,
        ctx: &'a crate::types::tool::ToolUseContext,
        hooks: &'a dyn HookRunner,
        hooks_map: &'a HooksMap,
        pre_configs: &'a [HookEventConfig],
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

    pub(crate) async fn maybe_prompt_user(&self) -> PipelineStageResult<()> {
        PipelineStageResult::Continue(())
    }

    pub(crate) async fn call_tool(&self) -> PipelineStageResult<()> {
        PipelineStageResult::Continue(())
    }

    pub(crate) async fn run_post_hooks(&self) -> PipelineStageResult<()> {
        PipelineStageResult::Continue(())
    }

    pub(crate) fn record_and_audit(&self) -> PipelineStageResult<()> {
        PipelineStageResult::Continue(())
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
