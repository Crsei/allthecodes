//! Shared IPC protocol DTOs.

pub mod envelope;
pub mod lsp;
pub mod normalized;
pub mod payload;
pub mod protocol;
pub mod subsystem_events;
pub mod subsystem_types;

pub use envelope::{
    is_supported_envelope_version, IpcEnvelope, IPC_ENVELOPE_MIN_COMPAT_VERSION,
    IPC_ENVELOPE_VERSION,
};
pub use lsp::{CompletionItemInfo, DocumentChange, SourceRange};
pub use normalized::{
    legacy_backend_to_payload, legacy_backend_type, ControlCommand, ConversationEvent,
    FlowControlEvent, LegacyBackendMessage, LegacyBackendPayload, LegacyFrontendMessage,
    LifecycleEvent, PermissionEvent, ProtocolError, ToolEvent,
};
pub use payload::{
    legacy_backend_to_payload as legacy_backend_to_ipc_payload, legacy_frontend_to_payload,
    legacy_frontend_type, payload_to_legacy_backend, ClientCapabilities, ClientErrorEnvelope,
    ClientHello, ClientNotificationEnvelope, ClientNotificationMethod, ClientRequestEnvelope,
    ClientResponseEnvelope, IpcPayload, IpcPayloadAdapterError, LaggedEvent, ServerCapabilities,
    ServerErrorEnvelope, ServerNotificationEnvelope, ServerReady, ServerRequestEnvelope,
    ServerRequestMethod, IPC_V2_PROTOCOL_VERSION,
};
pub use protocol::{
    BackendMessage, ConversationMessage, FileSearchMatch, FrontendMessage, ToolResultContentInfo,
};
