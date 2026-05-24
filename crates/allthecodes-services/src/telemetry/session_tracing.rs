//! Session-level tracing — wraps session lifecycle events with `tracing` spans.
//!
//! The `SessionTracer` bridges between the session lifecycle
//! (start/end/create/end-interaction) and the `tracing` crate's span system.
//! This allows existing tracing subscribers (e.g. OpenTelemetry, Langfuse) to
//! observe session boundaries without additional wiring.
//!
//! `TracingContext` carries trace/session/parent-span identity through async
//! boundaries so child spans can be correctly attributed.

use tracing::Span;

use crate::telemetry::instrumentation::{InteractionSpan, ToolSpan};
use crate::telemetry::{TelemetryConfig, TelemetryEvent, TelemetryHandle};

// ---------------------------------------------------------------------------
// TracingContext
// ---------------------------------------------------------------------------

/// Carries trace identity through async boundaries.
///
/// Fields are cheaply cloneable strings so the context can be passed across
/// await points or thread boundaries without lifetime complexity.
#[derive(Debug, Clone)]
pub struct TracingContext {
    /// High-level trace ID (typically the session ID).
    trace_id: String,
    /// Session ID (stable for the process lifetime).
    session_id: String,
    /// Parent span ID for attribution of child spans.
    parent_span_id: Option<String>,
}

impl TracingContext {
    /// Create a new tracing context from a session ID.
    pub fn new(session_id: impl Into<String>) -> Self {
        let session_id = session_id.into();
        Self {
            trace_id: session_id.clone(),
            session_id: session_id.clone(),
            parent_span_id: None,
        }
    }

    /// Create a child context linked to a parent span.
    pub fn with_parent(&self, parent_span_id: impl Into<String>) -> Self {
        Self {
            parent_span_id: Some(parent_span_id.into()),
            ..self.clone()
        }
    }

    /// The trace ID.
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// The session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The parent span ID, if set.
    pub fn parent_span_id(&self) -> Option<&str> {
        self.parent_span_id.as_deref()
    }
}

// ---------------------------------------------------------------------------
// SessionTracer
// ---------------------------------------------------------------------------

/// Wraps session lifecycle events with `tracing` spans.
///
/// Each method creates a `tracing::Span` at the appropriate level
/// (`info` for session boundaries, `debug` for details) and records
/// relevant metadata as span attributes.
#[derive(Debug, Clone)]
pub struct SessionTracer {
    session_id: String,
    /// Telemetry configuration.
    ///
    /// Intentional: Stored for future use when sampling decisions need to be
    ///     made per-session (e.g., adjusting sampling rate for long-running
    ///     sessions, or checking `TelemetryConfig::enabled` before recording
    ///     events in the tracer). Not currently referenced in method bodies.
    /// Owner: https://github.com/Crsei/allthecodes/issues (telemetry)
    /// Removal: When a method body references `self.config` for sampling or
    ///     feature-gating, remove this allow-dead-code.
    #[allow(dead_code)]
    config: TelemetryConfig,
    handle: Option<TelemetryHandle>,
    /// The root tracing span for this session.
    root_span: Option<Span>,
}

impl SessionTracer {
    /// Create a new session tracer.
    pub fn new(
        session_id: impl Into<String>,
        config: TelemetryConfig,
        handle: Option<TelemetryHandle>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            config,
            handle,
            root_span: None,
        }
    }

    /// Trace the start of a session.
    ///
    /// Creates a root tracing span for the session.
    pub fn trace_session_start(&mut self) {
        let span = tracing::info_span!(
            "session.start",
            session_id = %self.session_id,
        );
        self.root_span = Some(span);

        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::InputEvent {
                input_summary: "session_start".to_string(),
                input_type: "SessionLifecycle".to_string(),
            });
        }
    }

    /// Trace the end of a session.
    pub fn trace_session_end(&self) {
        // The root span is dropped here — tracing will close it.
        if let Some(ref handle) = self.handle {
            handle.record(TelemetryEvent::InputEvent {
                input_summary: "session_end".to_string(),
                input_type: "SessionLifecycle".to_string(),
            });
        }
    }

    /// Trace an interaction within the session.
    pub fn trace_interaction(&self, interaction: &InteractionSpan) {
        let span = tracing::info_span!(
            "interaction",
            interaction_id = %interaction.interaction_id(),
            session_id = %interaction.session_id(),
            submit_id = %interaction.submit_id(),
        );
        let _guard = span.enter();
        tracing::debug!(
            duration_ms = ?interaction.duration_ms(),
            model = ?interaction.model(),
            error = ?interaction.error(),
            "interaction traced"
        );
    }

    /// Trace a tool call within the session.
    pub fn trace_tool_call(&self, tool: &ToolSpan) {
        let span = tracing::debug_span!(
            "tool.call",
            tool_name = %tool.tool_name(),
            span_id = %tool.span_id(),
        );
        let _guard = span.enter();
        tracing::debug!("tool call traced");
    }

    /// Access the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Access the root tracing span, if set.
    pub fn root_span(&self) -> Option<&Span> {
        self.root_span.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_context_creates_hierarchy() {
        let root = TracingContext::new("sess_01");
        assert_eq!(root.trace_id(), "sess_01");
        assert_eq!(root.session_id(), "sess_01");
        assert!(root.parent_span_id().is_none());

        let child = root.with_parent("span_01");
        assert_eq!(child.parent_span_id(), Some("span_01"));
    }

    #[test]
    fn session_tracer_start_and_end_no_panic() {
        let config = TelemetryConfig {
            enabled: false,
            ..Default::default()
        };
        let mut tracer = SessionTracer::new("sess_01", config, None);
        tracer.trace_session_start();
        tracer.trace_session_end();
        // Should not panic
    }
}
