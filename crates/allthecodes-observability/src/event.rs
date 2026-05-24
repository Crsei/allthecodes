//! Runtime audit event model.
//!
//! Defines the stable, versioned `AuditEvent` schema used as the single runtime
//! fact source. All events share a common envelope (correlation IDs, timestamps,
//! stage, outcome) with business-specific data in the `data` field.
//!
//! See: docs/traceable-logging-plan.md §4.3

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Event kinds (§4.4)
// ---------------------------------------------------------------------------

/// Exhaustive list of audit event kinds.
///
/// New variants may be added; consumers should handle `Unknown(String)` for
/// forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    // Session boundary
    SessionStart,
    SessionEnd,

    // Submit boundary
    SubmitReceived,
    SubmitCompleted,
    SubmitAborted,

    // Query turn
    QueryTurnStart,
    QueryTurnContinue,
    QueryTurnStop,

    // Model request
    ModelRequestStart,
    ModelRequestRetry,
    ModelRequestFinish,
    ModelRequestError,

    // Messages
    AssistantMessage,
    UserMessage,
    SystemMessage,

    // Tool lifecycle
    ToolStart,
    ToolProgress,
    ToolFinish,
    ToolError,

    // Permission
    PermissionRequested,
    PermissionResolved,

    // Compaction
    CompactPre,
    CompactPost,

    // IPC / daemon
    IpcClientConnected,
    IpcClientDisconnected,
    DaemonSseConnected,
    DaemonSseReattach,

    // Background agents
    BackgroundAgentSpawned,
    BackgroundAgentCompleted,

    // Stream (opt-in)
    StreamStart,
    StreamStop,
    StreamDelta,

    // Plugin lifecycle
    PluginInstalled,
    PluginUninstalled,
    PluginUpdated,
    PluginDisabled,
    PluginError,

    // Telemetry
    TelemetryFlush,
    TelemetryExport,

    // Forward compat
    #[serde(other)]
    Unknown,
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use serde serialization for consistent snake_case output
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(s)) => f.write_str(&s),
            _ => write!(f, "{:?}", self),
        }
    }
}

// ---------------------------------------------------------------------------
// Execution stage
// ---------------------------------------------------------------------------

/// Which phase of the request lifecycle the event belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Session,
    Submit,
    QueryTurn,
    ModelCall,
    ToolExecution,
    Permission,
    Compaction,
    Ipc,
    Daemon,
    BackgroundAgent,
    Stream,
    Plugin,
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// High-level outcome of the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Started,
    Completed,
    Failed,
    Denied,
    Retried,
    Aborted,
    /// For informational events that don't have a success/failure semantic.
    Info,
}

// ---------------------------------------------------------------------------
// Log level
// ---------------------------------------------------------------------------

/// Severity level for the audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditLevel {
    Debug,
    Info,
    Warn,
    Error,
}

// ---------------------------------------------------------------------------
// AuditEvent — the core envelope
// ---------------------------------------------------------------------------

/// A single runtime audit event.
///
/// Top-level fields are stable; business-specific payload goes into `data`.
/// Large payloads should be written to `artifacts/` and referenced via
/// `artifact_path` inside `data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    // ── Identity ──────────────────────────────────────────────────────
    /// Unique event ID.
    pub event_id: String,
    /// Parent event for causal chains (e.g. `tool.finish` → `tool.start`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,

    // ── Timestamp ─────────────────────────────────────────────────────
    /// ISO 8601 timestamp with timezone.
    pub ts: DateTime<Utc>,

    // ── Correlation IDs ───────────────────────────────────────────────
    /// Session-level ID (stable for the entire process lifetime).
    pub session_id: String,
    /// Submit-level ID (one per user submission).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submit_id: Option<String>,
    /// Turn-level ID (one per query loop iteration).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Model request ID (one per API call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Message UUID (for assistant/user/system messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Tool use ID (for tool lifecycle events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,

    // ── Classification ────────────────────────────────────────────────
    /// Origin of the event (headless, tui, daemon, repl).
    pub source: String,
    /// Event kind (see `EventKind`).
    pub kind: EventKind,
    /// Lifecycle stage.
    pub stage: Stage,
    /// Severity.
    pub level: AuditLevel,
    /// High-level outcome.
    pub outcome: Outcome,

    // ── Metrics ───────────────────────────────────────────────────────
    /// Duration in milliseconds (for events that span time).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    // ── Business payload ──────────────────────────────────────────────
    /// Arbitrary structured data specific to this event kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl AuditEvent {
    /// Generate a new unique event ID.
    pub fn new_event_id() -> String {
        format!("evt_{}", Uuid::new_v4().as_simple())
    }
}

// ---------------------------------------------------------------------------
// Session metadata (meta.json)
// ---------------------------------------------------------------------------

/// Metadata written alongside `events.ndjson` for quick session identification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// Session ID.
    pub session_id: String,
    /// Process start time.
    pub started_at: DateTime<Utc>,
    /// Working directory.
    pub cwd: String,
    /// Executable version.
    pub version: String,
    /// Platform (e.g. "win32", "linux", "macos").
    pub platform: String,
    /// Source mode (tui, headless, daemon).
    pub source: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_to_stable_json() {
        let event = AuditEvent {
            event_id: "evt_test".into(),
            parent_event_id: None,
            ts: Utc::now(),
            session_id: "sess_01".into(),
            submit_id: Some("sub_01".into()),
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: "tui".into(),
            kind: EventKind::SessionStart,
            stage: Stage::Session,
            level: AuditLevel::Info,
            outcome: Outcome::Started,
            duration_ms: None,
            data: Some(serde_json::json!({"model": "claude-sonnet-4"})),
        };

        let json = serde_json::to_string(&event).unwrap();
        // Top-level fields must be present
        assert!(json.contains("\"event_id\""));
        assert!(json.contains("\"session_id\""));
        assert!(json.contains("\"kind\""));
        assert!(json.contains("\"session_start\""));
        // None fields should be absent
        assert!(!json.contains("\"turn_id\""));
    }

    #[test]
    fn event_roundtrips() {
        let event = AuditEvent {
            event_id: AuditEvent::new_event_id(),
            parent_event_id: Some("evt_parent".into()),
            ts: Utc::now(),
            session_id: "sess_02".into(),
            submit_id: None,
            turn_id: Some("turn_01".into()),
            request_id: Some("req_01".into()),
            message_id: None,
            tool_use_id: Some("tu_01".into()),
            source: "headless".into(),
            kind: EventKind::ToolFinish,
            stage: Stage::ToolExecution,
            level: AuditLevel::Info,
            outcome: Outcome::Completed,
            duration_ms: Some(142),
            data: Some(serde_json::json!({"tool_name": "Read"})),
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_id, event.event_id);
        assert_eq!(parsed.kind, EventKind::ToolFinish);
        assert_eq!(parsed.duration_ms, Some(142));
    }

    #[test]
    fn unknown_kind_deserializes() {
        let json = r#"{"event_id":"e","ts":"2026-04-16T00:00:00Z","session_id":"s","source":"x","kind":"future_event","stage":"session","level":"info","outcome":"info"}"#;
        let event: AuditEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.kind, EventKind::Unknown);
    }

    #[test]
    fn event_id_is_unique() {
        let a = AuditEvent::new_event_id();
        let b = AuditEvent::new_event_id();
        assert_ne!(a, b);
        assert!(a.starts_with("evt_"));
    }
}
