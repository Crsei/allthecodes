//! Canonical IPC v2 payloads and legacy adapter helpers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use allthecodes_protocol::{ApiErrorBody, ClientRequest};
use allthecodes_types::tool_operation::ToolOperation;

use crate::normalized::{
    legacy_backend_to_payload as legacy_backend_to_normalized_payload, LegacyBackendPayload,
};
use crate::protocol::{BackendMessage, FrontendMessage};

pub const IPC_V2_PROTOCOL_VERSION: u16 = 2;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum IpcPayload {
    Hello(ClientHello),
    Ready(ServerReady),
    ClientRequest(ClientRequestEnvelope),
    ClientNotification(ClientNotificationEnvelope),
    ClientResponse(ClientResponseEnvelope),
    ClientError(ClientErrorEnvelope),
    ServerNotification(ServerNotificationEnvelope),
    ServerRequest(ServerRequestEnvelope),
    ServerError(ServerErrorEnvelope),
    Lagged(LaggedEvent),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ClientHello {
    pub client_name: String,
    pub client_version: String,
    pub supported_versions: Vec<u16>,
    pub capabilities: ClientCapabilities,
    #[serde(default)]
    pub notification_subscriptions: Vec<String>,
}

impl ClientHello {
    pub fn supports_current_version(&self) -> bool {
        self.supported_versions.contains(&IPC_V2_PROTOCOL_VERSION)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ClientCapabilities {
    pub server_requests: bool,
    pub lagged_events: bool,
    pub typed_client_requests: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerReady {
    pub accepted_version: u16,
    pub session_id: String,
    pub model: String,
    pub cwd: String,
    pub permission_mode: String,
    #[serde(default)]
    pub available_models: Vec<String>,
    pub capabilities: ServerCapabilities,
    pub legacy_compatibility_mode: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub server_requests: bool,
    pub lagged_events: bool,
    pub typed_client_requests: bool,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            server_requests: true,
            lagged_events: true,
            typed_client_requests: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientRequestEnvelope {
    pub request_id: String,
    pub request: ClientRequest,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientNotificationEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
    pub method: ClientNotificationMethod,
    #[serde(default)]
    pub params: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientNotificationMethod {
    SubmitPrompt,
    AbortQuery,
    SlashCommand,
    Quit,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientResponseEnvelope {
    pub request_id: String,
    pub result: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientErrorEnvelope {
    pub request_id: String,
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerNotificationEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
    pub payload: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerRequestEnvelope {
    pub request_id: String,
    pub method: ServerRequestMethod,
    #[serde(default)]
    pub params: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerRequestMethod {
    PermissionDecision,
    AskUserQuestion,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ServerErrorEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<ApiErrorBody> for ServerErrorEnvelope {
    fn from(error: ApiErrorBody) -> Self {
        Self {
            request_id: None,
            code: -32000,
            message: error.error,
            data: Some(json!({
                "code": error.code,
                "details": error.details,
            })),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LaggedEvent {
    pub skipped: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_dropped_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcPayloadAdapterError {
    UnsupportedLegacyFrontend { legacy_type: &'static str },
    UnsupportedLegacyBackend { legacy_type: &'static str },
    UnsupportedPayload { kind: &'static str },
    InvalidPayload { message: String },
}

/// Stable legacy frontend protocol type name for adapter diagnostics.
pub fn legacy_frontend_type(message: &FrontendMessage) -> &'static str {
    match message {
        FrontendMessage::SubmitPrompt { .. } => "submit_prompt",
        FrontendMessage::AbortQuery => "abort_query",
        FrontendMessage::PermissionResponse { .. } => "permission_response",
        FrontendMessage::SlashCommand { .. } => "slash_command",
        FrontendMessage::Resize { .. } => "resize",
        FrontendMessage::QuestionResponse { .. } => "question_response",
        FrontendMessage::Quit => "quit",
        FrontendMessage::LspCommand { .. } => "lsp_command",
        FrontendMessage::McpCommand { .. } => "mcp_command",
        FrontendMessage::PluginCommand { .. } => "plugin_command",
        FrontendMessage::SkillCommand { .. } => "skill_command",
        FrontendMessage::IdeCommand { .. } => "ide_command",
        FrontendMessage::AgentSettingsCommand { .. } => "agent_settings_command",
        FrontendMessage::QuerySubsystemStatus => "query_subsystem_status",
        FrontendMessage::AgentCommand { .. } => "agent_command",
        FrontendMessage::TeamCommand { .. } => "team_command",
        FrontendMessage::SearchFiles { .. } => "search_files",
        FrontendMessage::RequestCompletions { .. } => "request_completions",
        FrontendMessage::AcceptCompletion { .. } => "accept_completion",
        FrontendMessage::InstallRecommendedPlugin { .. } => "install_recommended_plugin",
        FrontendMessage::RefreshPluginTelemetry => "refresh_plugin_telemetry",
        FrontendMessage::RequestLspRecommendations { .. } => "request_lsp_recommendations",
    }
}

/// Convert a legacy frontend message into an IPC v2 payload.
pub fn legacy_frontend_to_payload(
    message: &FrontendMessage,
) -> Result<IpcPayload, IpcPayloadAdapterError> {
    match message {
        FrontendMessage::SubmitPrompt { text, id } => {
            Ok(IpcPayload::ClientNotification(ClientNotificationEnvelope {
                notification_id: Some(id.clone()),
                method: ClientNotificationMethod::SubmitPrompt,
                params: json!({
                    "text": text,
                    "id": id,
                }),
            }))
        }
        FrontendMessage::AbortQuery => {
            Ok(IpcPayload::ClientNotification(ClientNotificationEnvelope {
                notification_id: None,
                method: ClientNotificationMethod::AbortQuery,
                params: Value::Object(serde_json::Map::new()),
            }))
        }
        FrontendMessage::SlashCommand { raw } => {
            Ok(IpcPayload::ClientNotification(ClientNotificationEnvelope {
                notification_id: None,
                method: ClientNotificationMethod::SlashCommand,
                params: json!({
                    "raw": raw,
                }),
            }))
        }
        FrontendMessage::Quit => Ok(IpcPayload::ClientNotification(ClientNotificationEnvelope {
            notification_id: None,
            method: ClientNotificationMethod::Quit,
            params: Value::Object(serde_json::Map::new()),
        })),
        FrontendMessage::PermissionResponse {
            tool_use_id,
            decision,
            feedback,
            session_id,
            turn_id,
        } => Ok(IpcPayload::ClientResponse(ClientResponseEnvelope {
            request_id: tool_use_id.clone(),
            result: json!({
                "tool_use_id": tool_use_id,
                "decision": decision,
                "feedback": feedback,
                "session_id": session_id,
                "turn_id": turn_id,
            }),
        })),
        FrontendMessage::QuestionResponse {
            id,
            text,
            session_id,
            turn_id,
        } => Ok(IpcPayload::ClientResponse(ClientResponseEnvelope {
            request_id: id.clone(),
            result: json!({
                "id": id,
                "text": text,
                "session_id": session_id,
                "turn_id": turn_id,
            }),
        })),
        other => Err(IpcPayloadAdapterError::UnsupportedLegacyFrontend {
            legacy_type: legacy_frontend_type(other),
        }),
    }
}

/// Convert a legacy backend message into an IPC v2 payload.
pub fn legacy_backend_to_payload(
    message: &BackendMessage,
) -> Result<IpcPayload, IpcPayloadAdapterError> {
    match message {
        BackendMessage::Ready {
            session_id,
            model,
            cwd,
            permission_mode,
            available_models,
            ..
        } => Ok(IpcPayload::Ready(ServerReady {
            accepted_version: IPC_V2_PROTOCOL_VERSION,
            session_id: session_id.clone(),
            model: model.clone(),
            cwd: cwd.clone(),
            permission_mode: permission_mode.clone(),
            available_models: available_models.clone(),
            capabilities: ServerCapabilities::default(),
            legacy_compatibility_mode: true,
        })),
        BackendMessage::PermissionRequest {
            tool_use_id,
            tool,
            command,
            input,
            options,
            security,
            operation,
        } => {
            let mut params = json!({
                "tool_use_id": tool_use_id,
                "tool": tool,
                "command": command,
                "input": input,
                "options": options,
            });
            if let Some(security) = security {
                params["security"] = serde_json::to_value(security).map_err(|error| {
                    IpcPayloadAdapterError::InvalidPayload {
                        message: error.to_string(),
                    }
                })?;
            }
            if let Some(operation) = operation {
                params["operation"] = serde_json::to_value(operation).map_err(|error| {
                    IpcPayloadAdapterError::InvalidPayload {
                        message: error.to_string(),
                    }
                })?;
            }

            Ok(IpcPayload::ServerRequest(ServerRequestEnvelope {
                request_id: tool_use_id.clone(),
                method: ServerRequestMethod::PermissionDecision,
                params,
                timeout_ms: None,
            }))
        }
        BackendMessage::QuestionRequest {
            id,
            text,
            choices,
            allow_free_text,
        } => Ok(IpcPayload::ServerRequest(ServerRequestEnvelope {
            request_id: id.clone(),
            method: ServerRequestMethod::AskUserQuestion,
            params: json!({
                "id": id,
                "text": text,
                "choices": choices,
                "allow_free_text": allow_free_text,
            }),
            timeout_ms: None,
        })),
        BackendMessage::Error {
            message,
            recoverable,
        } => Ok(IpcPayload::ServerError(ServerErrorEnvelope {
            request_id: None,
            code: -32000,
            message: message.clone(),
            data: Some(json!({
                "recoverable": recoverable,
            })),
        })),
        other => {
            let normalized = legacy_backend_to_normalized_payload(other);
            if let LegacyBackendPayload::Unsupported { legacy_type } = normalized {
                return Err(IpcPayloadAdapterError::UnsupportedLegacyBackend { legacy_type });
            }

            let payload = serde_json::to_value(normalized).map_err(|error| {
                IpcPayloadAdapterError::InvalidPayload {
                    message: error.to_string(),
                }
            })?;
            Ok(IpcPayload::ServerNotification(ServerNotificationEnvelope {
                notification_id: None,
                payload,
            }))
        }
    }
}

/// Convert an IPC v2 payload into a legacy backend message for compatibility.
pub fn payload_to_legacy_backend(
    payload: &IpcPayload,
) -> Result<BackendMessage, IpcPayloadAdapterError> {
    match payload {
        IpcPayload::Ready(ready) => Ok(BackendMessage::Ready {
            session_id: ready.session_id.clone(),
            model: ready.model.clone(),
            cwd: ready.cwd.clone(),
            permission_mode: ready.permission_mode.clone(),
            available_models: ready.available_models.clone(),
            plan_workflow: None,
            editor_mode: None,
            view_mode: None,
            keybindings: None,
        }),
        IpcPayload::ServerRequest(request) => server_request_to_legacy_backend(request),
        IpcPayload::ServerError(error) => Ok(BackendMessage::Error {
            message: error.message.clone(),
            recoverable: true,
        }),
        IpcPayload::Lagged(event) => Ok(BackendMessage::Error {
            message: format!(
                "ipc client lagged: skipped={} last_dropped_type={}",
                event.skipped,
                event.last_dropped_type.as_deref().unwrap_or("none")
            ),
            recoverable: true,
        }),
        other => Err(IpcPayloadAdapterError::UnsupportedPayload {
            kind: payload_kind(other),
        }),
    }
}

fn server_request_to_legacy_backend(
    request: &ServerRequestEnvelope,
) -> Result<BackendMessage, IpcPayloadAdapterError> {
    match request.method {
        ServerRequestMethod::PermissionDecision => Ok(BackendMessage::PermissionRequest {
            tool_use_id: required_string(&request.params, "tool_use_id")?,
            tool: required_string(&request.params, "tool")?,
            command: required_string(&request.params, "command")?,
            input: request
                .params
                .get("input")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new())),
            options: required_string_vec(&request.params, "options")?,
            security: request
                .params
                .get("security")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| IpcPayloadAdapterError::InvalidPayload {
                    message: error.to_string(),
                })?,
            operation: optional_operation(&request.params)?,
        }),
        ServerRequestMethod::AskUserQuestion => Ok(BackendMessage::QuestionRequest {
            id: required_string(&request.params, "id")?,
            text: required_string(&request.params, "text")?,
            choices: optional_string_vec(&request.params, "choices")?.unwrap_or_default(),
            allow_free_text: optional_bool(&request.params, "allow_free_text")?.unwrap_or(true),
        }),
    }
}

fn payload_kind(payload: &IpcPayload) -> &'static str {
    match payload {
        IpcPayload::Hello(_) => "hello",
        IpcPayload::Ready(_) => "ready",
        IpcPayload::ClientRequest(_) => "client_request",
        IpcPayload::ClientNotification(_) => "client_notification",
        IpcPayload::ClientResponse(_) => "client_response",
        IpcPayload::ClientError(_) => "client_error",
        IpcPayload::ServerNotification(_) => "server_notification",
        IpcPayload::ServerRequest(_) => "server_request",
        IpcPayload::ServerError(_) => "server_error",
        IpcPayload::Lagged(_) => "lagged",
    }
}

fn required_string(params: &Value, field: &'static str) -> Result<String, IpcPayloadAdapterError> {
    params
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| IpcPayloadAdapterError::InvalidPayload {
            message: format!("missing or invalid string field: {field}"),
        })
}

fn optional_bool(
    params: &Value,
    field: &'static str,
) -> Result<Option<bool>, IpcPayloadAdapterError> {
    params
        .get(field)
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| IpcPayloadAdapterError::InvalidPayload {
                    message: format!("invalid bool field: {field}"),
                })
        })
        .transpose()
}

fn required_string_vec(
    params: &Value,
    field: &'static str,
) -> Result<Vec<String>, IpcPayloadAdapterError> {
    optional_string_vec(params, field)?.ok_or_else(|| IpcPayloadAdapterError::InvalidPayload {
        message: format!("missing string array field: {field}"),
    })
}

fn optional_string_vec(
    params: &Value,
    field: &'static str,
) -> Result<Option<Vec<String>>, IpcPayloadAdapterError> {
    params
        .get(field)
        .map(|value| {
            let array = value
                .as_array()
                .ok_or_else(|| IpcPayloadAdapterError::InvalidPayload {
                    message: format!("invalid string array field: {field}"),
                })?;
            array
                .iter()
                .map(|item| {
                    item.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        IpcPayloadAdapterError::InvalidPayload {
                            message: format!("invalid string array item: {field}"),
                        }
                    })
                })
                .collect()
        })
        .transpose()
}

fn optional_operation(params: &Value) -> Result<Option<ToolOperation>, IpcPayloadAdapterError> {
    let Some(value) = params.get("operation") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| IpcPayloadAdapterError::InvalidPayload {
            message: format!("invalid operation field: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IpcEnvelope;
    use allthecodes_protocol::ClientResponse;
    use allthecodes_types::tool_operation::{
        OperationConfidence, OperationKind, OperationRisk, OperationStatus, ToolOperation,
    };

    fn ready_message() -> BackendMessage {
        BackendMessage::Ready {
            session_id: "session-1".to_string(),
            model: "test-model".to_string(),
            cwd: "/repo".to_string(),
            permission_mode: "default".to_string(),
            available_models: vec!["test-model".to_string()],
            plan_workflow: None,
            editor_mode: None,
            view_mode: None,
            keybindings: None,
        }
    }

    #[test]
    fn hello_payload_roundtrips_in_envelope() {
        let envelope = IpcEnvelope::new(
            "env-1",
            1,
            1_700_000_000,
            IpcPayload::Hello(ClientHello {
                client_name: "allthecodes-web".to_string(),
                client_version: "0.1.0".to_string(),
                supported_versions: vec![1, IPC_V2_PROTOCOL_VERSION],
                capabilities: ClientCapabilities {
                    server_requests: true,
                    lagged_events: true,
                    typed_client_requests: true,
                },
                notification_subscriptions: vec!["conversation".to_string()],
            }),
        );

        let encoded = serde_json::to_string(&envelope).unwrap();
        let decoded: IpcEnvelope<IpcPayload> = serde_json::from_str(&encoded).unwrap();

        if let IpcPayload::Hello(hello) = &decoded.payload {
            assert!(hello.supports_current_version());
        }
        assert!(matches!(
            decoded.payload,
            IpcPayload::Hello(ClientHello {
                client_name,
                capabilities: ClientCapabilities { server_requests: true, .. },
                ..
            }) if client_name == "allthecodes-web"
        ));
    }

    #[test]
    fn legacy_ready_maps_to_v2_ready_and_back() {
        let payload = legacy_backend_to_payload(&ready_message()).unwrap();

        assert!(matches!(
            &payload,
            IpcPayload::Ready(ServerReady {
                accepted_version: IPC_V2_PROTOCOL_VERSION,
                session_id,
                legacy_compatibility_mode: true,
                ..
            }) if session_id == "session-1"
        ));

        let legacy = payload_to_legacy_backend(&payload).unwrap();
        assert!(matches!(
            legacy,
            BackendMessage::Ready { session_id, available_models, .. }
                if session_id == "session-1" && available_models == vec!["test-model"]
        ));
    }

    #[test]
    fn legacy_permission_request_maps_to_server_request_and_back() {
        let operation = ToolOperation {
            kind: OperationKind::Permission,
            subtype: None,
            status: OperationStatus::InProgress,
            risk: OperationRisk::Low,
            confidence: OperationConfidence::High,
            label: "Permission: ls".to_string(),
            target: None,
            command_summary: Some("ls".to_string()),
            result_summary: None,
            raw_tool_name: "Bash".to_string(),
            raw_input: json!({ "command": "ls" }),
            raw_output: None,
            side_channels: vec![],
        };
        let legacy = BackendMessage::PermissionRequest {
            tool_use_id: "tool-1".to_string(),
            tool: "Bash".to_string(),
            command: "ls".to_string(),
            input: json!({ "command": "ls" }),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: Some(operation.clone()),
            security: None,
        };
        let payload = legacy_backend_to_payload(&legacy).unwrap();

        assert!(matches!(
            &payload,
            IpcPayload::ServerRequest(ServerRequestEnvelope {
                request_id,
                method: ServerRequestMethod::PermissionDecision,
                params,
                ..
            }) if request_id == "tool-1"
                && params["operation"]["kind"] == "permission"
        ));

        let remapped = payload_to_legacy_backend(&payload).unwrap();
        assert!(matches!(
            remapped,
            BackendMessage::PermissionRequest {
                tool_use_id,
                tool,
                command,
                options,
                operation: Some(remapped_operation),
                ..
            }
                if tool_use_id == "tool-1"
                    && tool == "Bash"
                    && command == "ls"
                    && options == vec!["allow", "deny"]
                    && remapped_operation == operation
        ));
    }

    #[test]
    fn legacy_question_response_maps_to_client_response() {
        let legacy = FrontendMessage::QuestionResponse {
            id: "question-1".to_string(),
            text: "yes".to_string(),
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
        };

        let payload = legacy_frontend_to_payload(&legacy).unwrap();

        assert!(matches!(
            payload,
            IpcPayload::ClientResponse(ClientResponseEnvelope { request_id, result })
                if request_id == "question-1"
                    && result["text"] == "yes"
                    && result["session_id"] == "session-1"
                    && result["turn_id"] == "turn-1"
        ));
    }

    #[test]
    fn legacy_submit_prompt_maps_to_client_notification() {
        let legacy = FrontendMessage::SubmitPrompt {
            text: "hello".to_string(),
            id: "ui-1".to_string(),
        };

        let payload = legacy_frontend_to_payload(&legacy).unwrap();

        assert!(matches!(
            payload,
            IpcPayload::ClientNotification(ClientNotificationEnvelope {
                notification_id: Some(id),
                method: ClientNotificationMethod::SubmitPrompt,
                params,
            }) if id == "ui-1" && params["text"] == "hello"
        ));
    }

    #[test]
    fn unsupported_legacy_frontend_message_returns_typed_error() {
        let legacy = FrontendMessage::Resize { cols: 80, rows: 24 };

        let error = legacy_frontend_to_payload(&legacy).unwrap_err();

        assert_eq!(
            error,
            IpcPayloadAdapterError::UnsupportedLegacyFrontend {
                legacy_type: "resize",
            }
        );
    }

    #[test]
    fn lagged_maps_to_recoverable_legacy_error() {
        let legacy = payload_to_legacy_backend(&IpcPayload::Lagged(LaggedEvent {
            skipped: 3,
            last_dropped_type: Some("tool_progress".to_string()),
        }))
        .unwrap();

        assert!(matches!(
            legacy,
            BackendMessage::Error { message, recoverable: true }
                if message.contains("skipped=3") && message.contains("tool_progress")
        ));
    }

    #[test]
    fn client_request_payload_carries_protocol_request() {
        let payload = IpcPayload::ClientRequest(ClientRequestEnvelope {
            request_id: "req-1".to_string(),
            request: ClientRequest::Capabilities(allthecodes_protocol::NoParams {}),
        });
        let encoded = serde_json::to_string(&payload).unwrap();
        let decoded: IpcPayload = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            IpcPayload::ClientRequest(ClientRequestEnvelope { request_id, .. })
                if request_id == "req-1"
        ));
    }

    #[test]
    fn client_response_payload_carries_protocol_response() {
        let response = ClientResponse::Capabilities(
            allthecodes_protocol::v1::capabilities::CapabilityDiscoveryResponse {
                capabilities: std::collections::HashMap::new(),
                backend: None,
                protocol: None,
                schema: None,
                build: None,
                features: None,
                checked_at: None,
            },
        );
        let payload = IpcPayload::ClientResponse(ClientResponseEnvelope {
            request_id: "req-2".to_string(),
            result: serde_json::to_value(response).unwrap(),
        });

        let encoded = serde_json::to_string(&payload).unwrap();
        let decoded: IpcPayload = serde_json::from_str(&encoded).unwrap();

        assert!(matches!(
            decoded,
            IpcPayload::ClientResponse(ClientResponseEnvelope { request_id, .. })
                if request_id == "req-2"
        ));
    }
}
