//! Bridge between cc-observability's `AuditEvent`/`AuditSink` and the
//! cc-services telemetry spans.
//!
//! The bridge converts span data into `AuditEvent`s for the NDJSON audit log
//! and (when the `telemetry` feature is enabled) forwards to the telemetry
//! backend via the `TelemetrySink` trait.
//!
//! # Feature gates
//!
//! - Core struct (`SessionTracingBridge`) is always available so the audit
//!   log path compiles unconditionally.
//! - The `TelemetrySink` trait and the `telemetry` field on the bridge are
//!   behind `#[cfg(feature = "telemetry")]` to avoid depending on span types
//!   defined in cc-services.
//!
//! See: docs/utils/telemetry-observability.md

use chrono::Utc;

use crate::event::{AuditEvent, AuditLevel, EventKind, Outcome, Stage};
use crate::sink::AuditSink;

// ---------------------------------------------------------------------------
// TelemetrySink trait (feature-gated)
// ---------------------------------------------------------------------------

/// Trait for telemetry backends to receive span data.
///
/// Defined in cc-observability rather than cc-services so the bridge can
/// reference it without creating a circular dependency. The concrete
/// `TelemetryHandle` in cc-services implements this trait.
#[cfg(feature = "telemetry")]
pub trait TelemetrySink: Send + Sync {
    /// Record a completed interaction (submit -> model response).
    fn record_interaction(
        &self,
        interaction_id: &str,
        duration_ms: Option<u64>,
        model: Option<&str>,
        error: Option<&str>,
    );

    /// Record a single model API call.
    fn record_model_call(
        &self,
        model: &str,
        duration_ms: Option<u64>,
        tokens_in: Option<u32>,
        tokens_out: Option<u32>,
    );

    /// Record a single tool execution.
    fn record_tool_execution(&self, tool_name: &str, duration_ms: Option<u64>, result: &str);
}

// ---------------------------------------------------------------------------
// SessionTracingBridge
// ---------------------------------------------------------------------------

/// Bridges audit events to telemetry spans.
///
/// Always available (even without the `telemetry` feature). When the feature
/// is enabled, it also forwards events to the telemetry system.
#[derive(Clone)]
pub struct SessionTracingBridge {
    audit_sink: AuditSink,
    #[cfg(feature = "telemetry")]
    telemetry: Option<std::sync::Arc<dyn TelemetrySink>>,
}

impl std::fmt::Debug for SessionTracingBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionTracingBridge")
            .field("audit_sink", &self.audit_sink)
            .finish()
    }
}

impl SessionTracingBridge {
    /// Create a new bridge with only the audit sink (no telemetry backend).
    pub fn new(audit_sink: AuditSink) -> Self {
        Self {
            audit_sink,
            #[cfg(feature = "telemetry")]
            telemetry: None,
        }
    }

    /// Attach a telemetry sink to the bridge.
    ///
    /// Only available when the `telemetry` feature is enabled.
    #[cfg(feature = "telemetry")]
    pub fn with_telemetry(mut self, telemetry: impl TelemetrySink + 'static) -> Self {
        self.telemetry = Some(std::sync::Arc::new(telemetry));
        self
    }

    // -- Event emission methods ------------------------------------------------

    /// Emit an interaction event (submit -> model response).
    ///
    /// Writes an `AuditEvent` to the NDJSON log and, if a telemetry sink is
    /// attached, forwards span data.
    pub fn emit_interaction_event(
        &self,
        interaction_id: &str,
        session_id: &str,
        submit_id: Option<&str>,
        duration_ms: Option<u64>,
        model: Option<&str>,
        error: Option<&str>,
    ) {
        let outcome = if error.is_some() {
            Outcome::Failed
        } else {
            Outcome::Completed
        };

        self.audit_sink.emit(AuditEvent {
            event_id: AuditEvent::new_event_id(),
            parent_event_id: None,
            ts: Utc::now(),
            session_id: session_id.to_string(),
            submit_id: submit_id.map(String::from),
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: "tui".to_string(),
            kind: EventKind::SubmitCompleted,
            stage: Stage::Submit,
            level: if error.is_some() {
                AuditLevel::Error
            } else {
                AuditLevel::Info
            },
            outcome,
            duration_ms,
            data: Some(serde_json::json!({
                "interaction_id": interaction_id,
                "model": model,
                "error": error,
            })),
        });

        #[cfg(feature = "telemetry")]
        if let Some(ref telemetry) = self.telemetry {
            telemetry.record_interaction(interaction_id, duration_ms, model, error);
        }
    }

    /// Emit a model call event.
    pub fn emit_model_event(
        &self,
        model: &str,
        duration_ms: Option<u64>,
        tokens_in: Option<u32>,
        tokens_out: Option<u32>,
        error: Option<&str>,
    ) {
        let outcome = if error.is_some() {
            Outcome::Failed
        } else {
            Outcome::Completed
        };

        self.audit_sink.emit(AuditEvent {
            event_id: AuditEvent::new_event_id(),
            parent_event_id: None,
            ts: Utc::now(),
            session_id: String::new(),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: "tui".to_string(),
            kind: EventKind::ModelRequestFinish,
            stage: Stage::ModelCall,
            level: if error.is_some() {
                AuditLevel::Error
            } else {
                AuditLevel::Info
            },
            outcome,
            duration_ms,
            data: Some(serde_json::json!({
                "model": model,
                "tokens_in": tokens_in,
                "tokens_out": tokens_out,
                "error": error,
            })),
        });

        #[cfg(feature = "telemetry")]
        if let Some(ref telemetry) = self.telemetry {
            telemetry.record_model_call(model, duration_ms, tokens_in, tokens_out);
        }
    }

    /// Emit a tool execution event.
    pub fn emit_tool_event(&self, tool_name: &str, duration_ms: Option<u64>, result: &str) {
        let outcome = match result {
            "error" | "denied" => Outcome::Failed,
            _ => Outcome::Completed,
        };

        self.audit_sink.emit(AuditEvent {
            event_id: AuditEvent::new_event_id(),
            parent_event_id: None,
            ts: Utc::now(),
            session_id: String::new(),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: "tui".to_string(),
            kind: EventKind::ToolFinish,
            stage: Stage::ToolExecution,
            level: AuditLevel::Info,
            outcome,
            duration_ms,
            data: Some(serde_json::json!({
                "tool_name": tool_name,
                "result": result,
            })),
        });

        #[cfg(feature = "telemetry")]
        if let Some(ref telemetry) = self.telemetry {
            telemetry.record_tool_execution(tool_name, duration_ms, result);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::AuditConfig;

    #[test]
    fn bridge_can_be_created_with_noop_sink() {
        let config = AuditConfig {
            enabled: false,
            stream_deltas: false,
            redaction: crate::sink::RedactionMode::Off,
        };
        let sink = AuditSink::noop(config);
        let bridge = SessionTracingBridge::new(sink);
        bridge.emit_interaction_event(
            "int_01",
            "sess_01",
            Some("sub_01"),
            Some(100),
            Some("claude-sonnet-4"),
            None,
        );
        bridge.emit_model_event("claude-sonnet-4", Some(50), Some(100), Some(200), None);
        bridge.emit_tool_event("Bash", Some(30), "ok");
        // Should not panic — events are silently discarded
    }
}
