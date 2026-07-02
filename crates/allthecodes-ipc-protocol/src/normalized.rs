//! Normalized IPC payloads shared by non-JSONL transports.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::{
    BackendMessage, CompletionItemDTO, ConversationMessage, InstallProgress, LspRecommendationDTO,
    ToolResultContentInfo,
};
use crate::subsystem_types::SubsystemStatusSnapshot;
use allthecodes_types::permission_events::{
    HookPermissionDecisionEvent, PermissionAutoReviewEvent, PermissionDecisionDebugEvent,
};
use allthecodes_types::tool_operation::{OperationResultSummary, ToolOperation};

/// Backward-compatible alias for the legacy backend wire enum.
pub type LegacyBackendMessage = BackendMessage;
/// Backward-compatible alias for the legacy frontend wire enum.
pub type LegacyFrontendMessage = crate::protocol::FrontendMessage;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleEvent {
    Ready {
        session_id: String,
        model: String,
        cwd: String,
        permission_mode: String,
    },
    Shutdown {
        reason: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEvent {
    StreamStart {
        message_id: String,
    },
    StreamDelta {
        message_id: String,
        text: String,
    },
    ThinkingDelta {
        message_id: String,
        thinking: String,
    },
    StreamEnd {
        message_id: String,
    },
    Tombstone {
        message_id: String,
    },
    AssistantMessage {
        id: String,
        content: serde_json::Value,
        cost_usd: f64,
    },
    ConversationReplaced {
        messages: Vec<ConversationMessage>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolEvent {
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    ToolResult {
        tool_use_id: String,
        output: String,
        is_error: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        content_blocks: Option<Vec<ToolResultContentInfo>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result_summary: Option<OperationResultSummary>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    ToolProgress {
        tool_use_id: String,
        tool: String,
        output: String,
        elapsed_seconds: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_lines: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_bytes: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PermissionEvent {
    PermissionRequest {
        tool_use_id: String,
        tool: String,
        command: String,
        #[serde(default)]
        input: Value,
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        operation: Option<ToolOperation>,
    },
    QuestionRequest {
        id: String,
        text: String,
        #[serde(default)]
        choices: Vec<String>,
        #[serde(default = "default_true")]
        allow_free_text: bool,
    },
    HookPermissionDecision {
        event: HookPermissionDecisionEvent,
    },
    PermissionDecisionDebug {
        event: PermissionDecisionDebugEvent,
    },
    PermissionAutoReview {
        event: PermissionAutoReviewEvent,
    },
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlCommand {
    SubmitPrompt { text: String, id: String },
    AbortQuery,
    Quit,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlowControlEvent {
    SystemInfo {
        text: String,
        level: String,
    },
    UsageUpdate {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    GoalUpdated {
        event: String,
        goal: Value,
    },
    StatusLineUpdate {
        payload: serde_json::Value,
        #[serde(default)]
        lines: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Suggestions {
        items: Vec<String>,
    },
    SubsystemStatus {
        status: SubsystemStatusSnapshot,
    },
    Completions {
        items: Vec<CompletionItemDTO>,
        request_id: String,
    },
    PluginInstallProgress {
        plugin_id: String,
        status: InstallProgress,
    },
    TelemetryStatus {
        enabled: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default)]
        errors: Vec<String>,
    },
    LspRecommendations {
        recommendations: Vec<LspRecommendationDTO>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProtocolError {
    pub message: String,
    pub recoverable: bool,
}

/// Normalized payload produced from legacy backend messages.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "category", content = "event", rename_all = "snake_case")]
pub enum LegacyBackendPayload {
    Lifecycle(LifecycleEvent),
    Conversation(ConversationEvent),
    Tool(ToolEvent),
    Permission(PermissionEvent),
    FlowControl(FlowControlEvent),
    Error(ProtocolError),
    Unsupported { legacy_type: &'static str },
}

/// Stable legacy protocol type name for adapter diagnostics.
pub fn legacy_backend_type(message: &BackendMessage) -> &'static str {
    match message {
        BackendMessage::Ready { .. } => "ready",
        BackendMessage::StreamStart { .. } => "stream_start",
        BackendMessage::AssistantMessage { .. } => "assistant_message",
        BackendMessage::StreamDelta { .. } => "stream_delta",
        BackendMessage::ThinkingDelta { .. } => "thinking_delta",
        BackendMessage::StreamEnd { .. } => "stream_end",
        BackendMessage::ToolUse { .. } => "tool_use",
        BackendMessage::ToolResult { .. } => "tool_result",
        BackendMessage::Tombstone { .. } => "tombstone",
        BackendMessage::PermissionRequest { .. } => "permission_request",
        BackendMessage::QuestionRequest { .. } => "question_request",
        BackendMessage::HookPermissionDecision { .. } => "hook_permission_decision",
        BackendMessage::PermissionDecisionDebug { .. } => "permission_decision_debug",
        BackendMessage::PermissionAutoReview { .. } => "permission_auto_review",
        BackendMessage::ToolProgress { .. } => "tool_progress",
        BackendMessage::BackgroundAgentComplete { .. } => "background_agent_complete",
        BackendMessage::SystemInfo { .. } => "system_info",
        BackendMessage::Error { .. } => "error",
        BackendMessage::FileSearchResult { .. } => "file_search_result",
        BackendMessage::ConversationReplaced { .. } => "conversation_replaced",
        BackendMessage::PlanWorkflowEvent { .. } => "plan_workflow_event",
        BackendMessage::GoalUpdated { .. } => "goal_updated",
        BackendMessage::UsageUpdate { .. } => "usage_update",
        BackendMessage::StatusLineUpdate { .. } => "status_line_update",
        BackendMessage::Suggestions { .. } => "suggestions",
        BackendMessage::BriefMessage { .. } => "brief_message",
        BackendMessage::AutonomousStart { .. } => "autonomous_start",
        BackendMessage::NotificationSent { .. } => "notification_sent",
        BackendMessage::SubsystemStatus { .. } => "subsystem_status",
        BackendMessage::LspEvent { .. } => "lsp_event",
        BackendMessage::McpEvent { .. } => "mcp_event",
        BackendMessage::PluginEvent { .. } => "plugin_event",
        BackendMessage::SkillEvent { .. } => "skill_event",
        BackendMessage::IdeEvent { .. } => "ide_event",
        BackendMessage::AgentSettingsEvent { .. } => "agent_settings_event",
        BackendMessage::AgentEvent { .. } => "agent_event",
        BackendMessage::TeamEvent { .. } => "team_event",
        BackendMessage::Completions { .. } => "completions",
        BackendMessage::PluginInstallProgress { .. } => "plugin_install_progress",
        BackendMessage::TelemetryStatus { .. } => "telemetry_status",
        BackendMessage::LspRecommendations { .. } => "lsp_recommendations",
    }
}

/// Convert a legacy backend message into a normalized envelope payload.
pub fn legacy_backend_to_payload(message: &BackendMessage) -> LegacyBackendPayload {
    match message {
        BackendMessage::Ready {
            session_id,
            model,
            cwd,
            permission_mode,
            ..
        } => LegacyBackendPayload::Lifecycle(LifecycleEvent::Ready {
            session_id: session_id.clone(),
            model: model.clone(),
            cwd: cwd.clone(),
            permission_mode: permission_mode.clone(),
        }),
        BackendMessage::StreamStart { message_id } => {
            LegacyBackendPayload::Conversation(ConversationEvent::StreamStart {
                message_id: message_id.clone(),
            })
        }
        BackendMessage::StreamDelta { message_id, text } => {
            LegacyBackendPayload::Conversation(ConversationEvent::StreamDelta {
                message_id: message_id.clone(),
                text: text.clone(),
            })
        }
        BackendMessage::ThinkingDelta {
            message_id,
            thinking,
        } => LegacyBackendPayload::Conversation(ConversationEvent::ThinkingDelta {
            message_id: message_id.clone(),
            thinking: thinking.clone(),
        }),
        BackendMessage::StreamEnd { message_id } => {
            LegacyBackendPayload::Conversation(ConversationEvent::StreamEnd {
                message_id: message_id.clone(),
            })
        }
        BackendMessage::Tombstone { message_id } => {
            LegacyBackendPayload::Conversation(ConversationEvent::Tombstone {
                message_id: message_id.clone(),
            })
        }
        BackendMessage::AssistantMessage {
            id,
            content,
            cost_usd,
        } => LegacyBackendPayload::Conversation(ConversationEvent::AssistantMessage {
            id: id.clone(),
            content: content.clone(),
            cost_usd: *cost_usd,
        }),
        BackendMessage::ConversationReplaced { messages } => {
            LegacyBackendPayload::Conversation(ConversationEvent::ConversationReplaced {
                messages: messages.clone(),
            })
        }
        BackendMessage::ToolUse {
            id,
            name,
            input,
            operation,
        } => LegacyBackendPayload::Tool(ToolEvent::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
            operation: operation.clone(),
        }),
        BackendMessage::ToolResult {
            tool_use_id,
            output,
            is_error,
            content_blocks,
            result_summary,
            operation,
        } => LegacyBackendPayload::Tool(ToolEvent::ToolResult {
            tool_use_id: tool_use_id.clone(),
            output: output.clone(),
            is_error: *is_error,
            content_blocks: content_blocks.clone(),
            result_summary: result_summary.clone(),
            operation: operation.clone(),
        }),
        BackendMessage::ToolProgress {
            tool_use_id,
            tool,
            output,
            elapsed_seconds,
            total_lines,
            total_bytes,
            timeout_ms,
            operation,
        } => LegacyBackendPayload::Tool(ToolEvent::ToolProgress {
            tool_use_id: tool_use_id.clone(),
            tool: tool.clone(),
            output: output.clone(),
            elapsed_seconds: *elapsed_seconds,
            total_lines: *total_lines,
            total_bytes: *total_bytes,
            timeout_ms: *timeout_ms,
            operation: operation.clone(),
        }),
        BackendMessage::PermissionRequest {
            tool_use_id,
            tool,
            command,
            input,
            options,
            operation,
        } => LegacyBackendPayload::Permission(PermissionEvent::PermissionRequest {
            tool_use_id: tool_use_id.clone(),
            tool: tool.clone(),
            command: command.clone(),
            input: input.clone(),
            options: options.clone(),
            operation: operation.clone(),
        }),
        BackendMessage::QuestionRequest {
            id,
            text,
            choices,
            allow_free_text,
        } => LegacyBackendPayload::Permission(PermissionEvent::QuestionRequest {
            id: id.clone(),
            text: text.clone(),
            choices: choices.clone(),
            allow_free_text: *allow_free_text,
        }),
        BackendMessage::HookPermissionDecision { event } => {
            LegacyBackendPayload::Permission(PermissionEvent::HookPermissionDecision {
                event: event.clone(),
            })
        }
        BackendMessage::PermissionDecisionDebug { event } => {
            LegacyBackendPayload::Permission(PermissionEvent::PermissionDecisionDebug {
                event: event.clone(),
            })
        }
        BackendMessage::PermissionAutoReview { event } => {
            LegacyBackendPayload::Permission(PermissionEvent::PermissionAutoReview {
                event: event.clone(),
            })
        }
        BackendMessage::SystemInfo { text, level } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::SystemInfo {
                text: text.clone(),
                level: level.clone(),
            })
        }
        BackendMessage::UsageUpdate {
            input_tokens,
            output_tokens,
            cost_usd,
        } => LegacyBackendPayload::FlowControl(FlowControlEvent::UsageUpdate {
            input_tokens: *input_tokens,
            output_tokens: *output_tokens,
            cost_usd: *cost_usd,
        }),
        BackendMessage::GoalUpdated { event, goal } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::GoalUpdated {
                event: event.clone(),
                goal: goal.clone(),
            })
        }
        BackendMessage::StatusLineUpdate {
            payload,
            lines,
            error,
        } => LegacyBackendPayload::FlowControl(FlowControlEvent::StatusLineUpdate {
            payload: payload.clone(),
            lines: lines.clone(),
            error: error.clone(),
        }),
        BackendMessage::Suggestions { items } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::Suggestions {
                items: items.clone(),
            })
        }
        BackendMessage::SubsystemStatus { status } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::SubsystemStatus {
                status: status.clone(),
            })
        }
        BackendMessage::Completions { items, request_id } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::Completions {
                items: items.clone(),
                request_id: request_id.clone(),
            })
        }
        BackendMessage::PluginInstallProgress { plugin_id, status } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::PluginInstallProgress {
                plugin_id: plugin_id.clone(),
                status: status.clone(),
            })
        }
        BackendMessage::TelemetryStatus {
            enabled,
            session_id,
            errors,
        } => LegacyBackendPayload::FlowControl(FlowControlEvent::TelemetryStatus {
            enabled: *enabled,
            session_id: session_id.clone(),
            errors: errors.clone(),
        }),
        BackendMessage::LspRecommendations { recommendations } => {
            LegacyBackendPayload::FlowControl(FlowControlEvent::LspRecommendations {
                recommendations: recommendations.clone(),
            })
        }
        BackendMessage::Error {
            message,
            recoverable,
        } => LegacyBackendPayload::Error(ProtocolError {
            message: message.clone(),
            recoverable: *recoverable,
        }),
        other => LegacyBackendPayload::Unsupported {
            legacy_type: legacy_backend_type(other),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IpcEnvelope;

    #[test]
    fn legacy_tool_use_maps_to_normalized_payload() {
        let msg = BackendMessage::ToolUse {
            id: "tool-1".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"file_path":"Cargo.toml"}),
            operation: None,
        };

        let payload = legacy_backend_to_payload(&msg);

        assert!(matches!(
            payload,
            LegacyBackendPayload::Tool(ToolEvent::ToolUse { id, name, input, .. })
                if id == "tool-1" && name == "Read" && input["file_path"] == "Cargo.toml"
        ));
    }

    #[test]
    fn legacy_tool_result_maps_to_normalized_payload() {
        let msg = BackendMessage::ToolResult {
            tool_use_id: "tool-1".to_string(),
            output: "done".to_string(),
            is_error: false,
            content_blocks: None,
            result_summary: None,
            operation: None,
        };

        let payload = legacy_backend_to_payload(&msg);

        assert!(matches!(
            payload,
            LegacyBackendPayload::Tool(ToolEvent::ToolResult {
                tool_use_id,
                output,
                is_error: false,
                ..
            }) if tool_use_id == "tool-1" && output == "done"
        ));
    }

    #[test]
    fn legacy_permission_request_maps_to_normalized_payload() {
        let msg = BackendMessage::PermissionRequest {
            tool_use_id: "tool-1".to_string(),
            tool: "Bash".to_string(),
            command: "rm -rf target".to_string(),
            input: serde_json::json!({"command":"rm -rf target"}),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
        };

        let payload = legacy_backend_to_payload(&msg);

        assert!(matches!(
            payload,
            LegacyBackendPayload::Permission(PermissionEvent::PermissionRequest {
                tool_use_id,
                tool,
                ..
            }) if tool_use_id == "tool-1" && tool == "Bash"
        ));
    }

    #[test]
    fn phase2_flow_control_messages_map_to_normalized_payload() {
        let completions = legacy_backend_to_payload(&BackendMessage::Completions {
            request_id: "req-1".to_string(),
            items: vec![CompletionItemDTO {
                label: "/help".to_string(),
                insert_text: "/help ".to_string(),
                kind: "command".to_string(),
                detail: Some("Show help".to_string()),
            }],
        });
        assert!(matches!(
            completions,
            LegacyBackendPayload::FlowControl(FlowControlEvent::Completions {
                request_id,
                items,
            }) if request_id == "req-1" && items.len() == 1
        ));

        let telemetry = legacy_backend_to_payload(&BackendMessage::TelemetryStatus {
            enabled: true,
            session_id: Some("session-1".to_string()),
            errors: Vec::new(),
        });
        assert!(matches!(
            telemetry,
            LegacyBackendPayload::FlowControl(FlowControlEvent::TelemetryStatus {
                enabled: true,
                session_id: Some(session_id),
                ..
            }) if session_id == "session-1"
        ));

        let install = legacy_backend_to_payload(&BackendMessage::PluginInstallProgress {
            plugin_id: "rust-lsp".to_string(),
            status: InstallProgress::Installed,
        });
        assert!(matches!(
            install,
            LegacyBackendPayload::FlowControl(FlowControlEvent::PluginInstallProgress {
                plugin_id,
                status: InstallProgress::Installed,
            }) if plugin_id == "rust-lsp"
        ));

        let lsp = legacy_backend_to_payload(&BackendMessage::LspRecommendations {
            recommendations: vec![LspRecommendationDTO {
                plugin_id: "rust-lsp".to_string(),
                plugin_name: "Rust LSP".to_string(),
                description: None,
                languages: vec!["rust".to_string()],
                confidence: 0.9,
                is_installed: false,
                is_dismissed: false,
            }],
        });
        assert!(matches!(
            lsp,
            LegacyBackendPayload::FlowControl(FlowControlEvent::LspRecommendations {
                recommendations,
            }) if recommendations.len() == 1
        ));
    }

    #[test]
    fn legacy_stream_and_error_can_be_wrapped_in_envelope() {
        let payload = legacy_backend_to_payload(&BackendMessage::StreamDelta {
            message_id: "message-1".to_string(),
            text: "hello".to_string(),
        });
        let envelope = IpcEnvelope::new("event-1", 1, 10, payload);

        assert!(matches!(
            envelope.payload,
            LegacyBackendPayload::Conversation(ConversationEvent::StreamDelta {
                message_id,
                text,
            }) if message_id == "message-1" && text == "hello"
        ));

        let payload = legacy_backend_to_payload(&BackendMessage::Error {
            message: "bad input".to_string(),
            recoverable: true,
        });

        assert!(matches!(
            payload,
            LegacyBackendPayload::Error(ProtocolError {
                message,
                recoverable: true,
            }) if message == "bad input"
        ));
    }
}
