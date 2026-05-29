use super::*;

pub(crate) fn central_permission_decision_for_tool(
    tool_name: &str,
    input: &serde_json::Value,
    app_state: &AppState,
    hook_decision: Option<&crate::permissions::decision::HookPermissionDecision>,
    auto_classifier: Option<&AutoClassifierDecision>,
    denial_tracker: Option<&mut DenialTracker>,
) -> PermissionDecision {
    use crate::permissions::decision::{self, PermissionBehavior};

    let plan_file_write_allowed = app_state.tool_permission_context.mode == PermissionMode::Plan
        && is_plan_mode_plan_file_write(tool_name, input);

    let mut decision = if plan_file_write_allowed {
        PermissionDecision {
            behavior: PermissionBehavior::Allow,
            updated_input: None,
            message: None,
            reason: PermissionDecisionReason::Mode {
                mode: "plan_file".to_string(),
            },
        }
    } else {
        decision::has_permissions_to_use_tool_with_hook_and_auto_classifier(
            tool_name,
            input,
            &app_state.tool_permission_context,
            hook_decision,
            auto_classifier,
            denial_tracker,
        )
    };
    if matches!(&decision.behavior, PermissionBehavior::Ask)
        && matches!(&decision.reason, PermissionDecisionReason::Mode { .. })
        && app_state.tool_permission_context.mode != PermissionMode::Plan
        && sandbox_allowed_command_applies(tool_name, input, &app_state.to_tool_app_state())
    {
        decision = PermissionDecision {
            behavior: PermissionBehavior::Allow,
            updated_input: None,
            message: None,
            reason: PermissionDecisionReason::Mode {
                mode: "sandbox_allowed_command".to_string(),
            },
        };
    }
    decision
}

pub(crate) fn auto_classifier_needed(decision: &PermissionDecision) -> bool {
    matches!(
        (&decision.behavior, &decision.reason),
        (
            crate::permissions::decision::PermissionBehavior::Allow,
            PermissionDecisionReason::Mode { mode }
        ) if mode == "auto"
    )
}

pub(crate) fn permission_result_from_decision(
    tool_name: &str,
    input: &mut serde_json::Value,
    decision: PermissionDecision,
) -> crate::types::tool::PermissionResult {
    use crate::permissions::decision::PermissionBehavior;

    let behavior = decision.behavior;
    let message = decision.message;
    if let Some(updated_input) = decision.updated_input {
        *input = updated_input;
    }

    match behavior {
        PermissionBehavior::Allow => crate::types::tool::PermissionResult::Allow {
            updated_input: input.clone(),
        },
        PermissionBehavior::Deny => crate::types::tool::PermissionResult::Deny {
            message: message.unwrap_or_else(|| "Permission blocked by policy.".to_string()),
        },
        PermissionBehavior::Ask => crate::types::tool::PermissionResult::Ask {
            message: message.unwrap_or_else(|| format!("Allow tool '{}'?", tool_name)),
        },
    }
}

fn permission_behavior_label(
    behavior: &crate::permissions::decision::PermissionBehavior,
) -> &'static str {
    match behavior {
        crate::permissions::decision::PermissionBehavior::Allow => "allow",
        crate::permissions::decision::PermissionBehavior::Deny => "deny",
        crate::permissions::decision::PermissionBehavior::Ask => "ask",
    }
}

fn permission_reason_summary(
    reason: &PermissionDecisionReason,
) -> (String, String, Option<String>) {
    match reason {
        PermissionDecisionReason::Rule { source, pattern } => {
            (pattern.clone(), source.clone(), Some(pattern.clone()))
        }
        PermissionDecisionReason::Hook { detail } => {
            let matcher = detail
                .split_once(':')
                .map(|(_, tail)| tail.trim().to_string())
                .filter(|tail| !tail.is_empty())
                .unwrap_or_else(|| "*".to_string());
            (matcher, "hook".to_string(), None)
        }
        PermissionDecisionReason::Mode { mode } => (mode.clone(), "mode".to_string(), None),
        PermissionDecisionReason::PatternMatch { tool, pattern } => {
            (pattern.clone(), tool.clone(), Some(pattern.clone()))
        }
    }
}

pub(crate) fn emit_permission_decision_debug(
    ctx: &crate::types::tool::ToolUseContext,
    tool_name: &str,
    app_state: &AppState,
    decision: &PermissionDecision,
) {
    if !app_state.verbose {
        return;
    }
    let Some(callback) = ctx.permission_event_callback.as_ref() else {
        return;
    };
    let (matcher, source, matched_rule) = permission_reason_summary(&decision.reason);
    let reason = match &decision.reason {
        PermissionDecisionReason::Rule { source, pattern } => {
            format!("matched {pattern} from {source}")
        }
        PermissionDecisionReason::Hook { detail } => detail.clone(),
        PermissionDecisionReason::Mode { mode } => format!("permission mode {mode}"),
        PermissionDecisionReason::PatternMatch { tool, pattern } => {
            format!("{tool} matched {pattern}")
        }
    };
    callback(PermissionEventPayload::DecisionDebug {
        event: PermissionDecisionDebugEvent {
            tool_name: tool_name.to_string(),
            matcher,
            source,
            matched_rule,
            behavior: permission_behavior_label(&decision.behavior).to_string(),
            reason,
        },
    });
}

pub(crate) fn emit_hook_permission_decision(
    ctx: &crate::types::tool::ToolUseContext,
    hook_name: &str,
    hook_event: &str,
    matcher: impl Into<String>,
    decision: &str,
    notes: Vec<String>,
) {
    let Some(callback) = ctx.permission_event_callback.as_ref() else {
        return;
    };
    callback(PermissionEventPayload::HookDecision {
        event: HookPermissionDecisionEvent {
            hook_name: hook_name.to_string(),
            hook_event: hook_event.to_string(),
            matcher: matcher.into(),
            decision: decision.to_string(),
            notes,
        },
    });
}

pub(crate) fn hook_error_is_critical(
    tool_name: &str,
    hook_configs: &[allthecodes_types::hooks::HookEventConfig],
) -> bool {
    hook_configs.iter().any(|config| {
        config.critical
            && match config.matcher.as_deref() {
                None | Some("*") => true,
                Some(pattern) => tool_name == pattern || tool_name.starts_with(pattern),
            }
    })
}

pub(crate) fn permission_denied_message(response: &PermissionResponsePayload) -> String {
    match response.feedback.as_deref() {
        Some(feedback) => format!("Permission denied by user.\n\nUser feedback: {feedback}"),
        None => "Permission denied by user.".to_string(),
    }
}

pub(crate) fn permission_feedback_message(feedback: &str) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        role: "user".to_string(),
        content: MessageContent::Text(feedback.to_string()),
        is_meta: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}
