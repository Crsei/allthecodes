//! Audit context — propagates correlation IDs through the call chain.
//!
//! `AuditContext` is created at the submit boundary and threaded through
//! query turns, model calls, and tool executions. It avoids every layer
//! having to manually assemble IDs.
//!
//! See: docs/traceable-logging-plan.md §4.2

use chrono::Utc;
use uuid::Uuid;

use super::event::{AuditEvent, AuditLevel, EventKind, Outcome, Stage};
use super::sink::AuditSink;

// ---------------------------------------------------------------------------
// AuditContext
// ---------------------------------------------------------------------------

/// Carries correlation IDs for a single request chain.
///
/// Clone is cheap (all fields are `String` / `Option<String>` + an `Arc`).
/// Child scopes are created via `with_turn`, `with_request`, etc.
#[derive(Debug, Clone)]
pub struct AuditContext {
    /// Session ID (stable for process lifetime).
    pub session_id: String,
    /// Submit ID (one per user submission).
    pub submit_id: Option<String>,
    /// Turn ID (one per query loop iteration).
    pub turn_id: Option<String>,
    /// Request ID (one per model API call).
    pub request_id: Option<String>,
    /// Message ID (for message-level events).
    pub message_id: Option<String>,
    /// Tool use ID (for tool lifecycle events).
    pub tool_use_id: Option<String>,
    /// Source mode (tui, headless, daemon, repl).
    pub source: String,
    /// Shared sink for emitting events.
    sink: Option<AuditSink>,
}

/// Input fields for emitting an audit event.
#[derive(Debug, Clone)]
pub struct AuditEmitInput {
    pub kind: EventKind,
    pub stage: Stage,
    pub level: AuditLevel,
    pub outcome: Outcome,
    pub duration_ms: Option<u64>,
    pub data: Option<serde_json::Value>,
    pub parent_event_id: Option<String>,
}

impl AuditContext {
    /// Create a root context for a session.
    pub fn new(session_id: impl Into<String>, source: impl Into<String>, sink: AuditSink) -> Self {
        Self {
            session_id: session_id.into(),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: source.into(),
            sink: Some(sink),
        }
    }

    /// Create a no-op context that discards all events (for tests / disabled mode).
    pub fn noop(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            submit_id: None,
            turn_id: None,
            request_id: None,
            message_id: None,
            tool_use_id: None,
            source: "noop".into(),
            sink: None,
        }
    }

    // -- Child scope constructors -----------------------------------------

    /// Create a child context for a new submit.
    pub fn with_submit(&self) -> Self {
        let mut ctx = self.clone();
        ctx.submit_id = Some(new_id("sub"));
        ctx.turn_id = None;
        ctx.request_id = None;
        ctx.message_id = None;
        ctx.tool_use_id = None;
        ctx
    }

    /// Create a child context for a new query turn.
    pub fn with_turn(&self) -> Self {
        let mut ctx = self.clone();
        ctx.turn_id = Some(new_id("turn"));
        ctx.request_id = None;
        ctx.message_id = None;
        ctx.tool_use_id = None;
        ctx
    }

    /// Create a child context for a new model request.
    pub fn with_request(&self) -> Self {
        let mut ctx = self.clone();
        ctx.request_id = Some(new_id("req"));
        ctx
    }

    /// Create a child context scoped to a specific message.
    pub fn with_message(&self, message_id: impl Into<String>) -> Self {
        let mut ctx = self.clone();
        ctx.message_id = Some(message_id.into());
        ctx
    }

    /// Create a child context scoped to a specific tool use.
    pub fn with_tool_use(&self, tool_use_id: impl Into<String>) -> Self {
        let mut ctx = self.clone();
        ctx.tool_use_id = Some(tool_use_id.into());
        ctx
    }

    // -- Event emission ---------------------------------------------------

    /// Emit an audit event, populating correlation IDs from this context.
    pub fn emit(
        &self,
        kind: EventKind,
        stage: Stage,
        level: AuditLevel,
        outcome: Outcome,
        duration_ms: Option<u64>,
        data: Option<serde_json::Value>,
    ) {
        self.emit_event(AuditEmitInput {
            kind,
            stage,
            level,
            outcome,
            duration_ms,
            data,
            parent_event_id: None,
        });
    }

    /// Emit an audit event from a structured input payload.
    pub fn emit_event(&self, input: AuditEmitInput) {
        let sink = match &self.sink {
            Some(s) => s,
            None => return, // noop context
        };

        let event = AuditEvent {
            event_id: AuditEvent::new_event_id(),
            parent_event_id: input.parent_event_id,
            ts: Utc::now(),
            session_id: self.session_id.clone(),
            submit_id: self.submit_id.clone(),
            turn_id: self.turn_id.clone(),
            request_id: self.request_id.clone(),
            message_id: self.message_id.clone(),
            tool_use_id: self.tool_use_id.clone(),
            source: self.source.clone(),
            kind: input.kind,
            stage: input.stage,
            level: input.level,
            outcome: input.outcome,
            duration_ms: input.duration_ms,
            data: input.data,
        };

        sink.emit(event);
    }

    /// Quick helper: emit an info-level event with no duration or data.
    pub fn emit_simple(&self, kind: EventKind, stage: Stage, outcome: Outcome) {
        self.emit(kind, stage, AuditLevel::Info, outcome, None, None);
    }

    /// Flush buffered events to disk. Call at submit boundaries.
    pub fn flush(&self) {
        if let Some(ref sink) = self.sink {
            sink.flush();
        }
    }

    /// Sync to disk (flush + fsync). Call at shutdown.
    pub fn sync(&self) {
        if let Some(ref sink) = self.sink {
            sink.sync();
        }
    }

    /// Check if this context is active (has a sink).
    pub fn is_active(&self) -> bool {
        self.sink.is_some()
    }

    /// Get the current submit_id (if set).
    pub fn submit_id(&self) -> Option<&str> {
        self.submit_id.as_deref()
    }

    /// Get the current turn_id (if set).
    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }
}

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

fn new_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4().as_simple())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_context_does_not_panic() {
        let ctx = AuditContext::noop("test-session");
        ctx.emit_simple(EventKind::SessionStart, Stage::Session, Outcome::Started);
        // Should silently discard
    }

    #[test]
    fn child_contexts_inherit_ids() {
        let ctx = AuditContext::noop("sess_01");
        let submit_ctx = ctx.with_submit();
        assert!(submit_ctx.submit_id.is_some());
        assert_eq!(submit_ctx.session_id, "sess_01");

        let turn_ctx = submit_ctx.with_turn();
        assert!(turn_ctx.turn_id.is_some());
        assert_eq!(turn_ctx.submit_id, submit_ctx.submit_id);

        let req_ctx = turn_ctx.with_request();
        assert!(req_ctx.request_id.is_some());
        assert_eq!(req_ctx.turn_id, turn_ctx.turn_id);
    }

    #[test]
    fn child_turn_clears_downstream_ids() {
        let ctx = AuditContext::noop("sess_01");
        let submit_ctx = ctx.with_submit();
        let turn_ctx = submit_ctx.with_turn().with_request().with_tool_use("tu_01");

        // New turn should clear request, message, tool_use
        let new_turn = turn_ctx.with_turn();
        assert!(new_turn.request_id.is_none());
        assert!(new_turn.tool_use_id.is_none());
        // But keep submit
        assert!(new_turn.submit_id.is_some());
    }
}
