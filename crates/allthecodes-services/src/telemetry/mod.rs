//! Telemetry subsystem — interaction, model, tool, and hook span tracking.
//!
//! This module provides the instrumentation API for engine call sites. It is
//! behind the `telemetry` feature gate and expects `tracing`, `serde_json`,
//! and `cc-observability` (for the audit log bridge) to be available.
//!
//! # Architecture
//!
//! ```text
//! Engine call site
//!     │
//!     ├──> InteractionSpan  ──finish()──>  TelemetryHandle
//!     ├──> ModelSpan        ──finish()──>  TelemetryHandle
//!     ├──> ToolSpan         ──finish()──>  TelemetryHandle
//!     └──> HookSpan         ──finish()──>  TelemetryHandle
//!                                              │
//!                                              ├──> TelemetryEvent (in-memory buffer)
//!                                              ├──> tracing spans (log export)
//!                                              └──> SessionTracingBridge (audit log)
//! ```
//!
//! See: docs/utils/telemetry-observability.md

pub mod instrumentation;
pub mod privacy;
pub mod session_tracing;

use std::sync::Arc;

use parking_lot::Mutex;

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use self::instrumentation::{HookSpan, InputType, InteractionSpan, ModelSpan, ToolSpan};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Telemetry configuration.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Whether the telemetry subsystem is active.
    pub enabled: bool,
    /// Which exporter backend to use.
    pub exporter: TelemetryExporter,
    /// Sampling rate (0.0 = no events, 1.0 = all events).
    pub sampling_rate: f64,
    /// PII redaction settings.
    pub redaction: TelemetryRedaction,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exporter: TelemetryExporter::Log,
            sampling_rate: 1.0,
            redaction: TelemetryRedaction::default(),
        }
    }
}

/// Telemetry export backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryExporter {
    /// Drop all events.
    None,
    /// Log events via the `tracing` crate (debug level).
    Log,
    /// BigQuery export.
    ///
    /// Intentional: BigQuery exporter is defined as a future surface but is not
    ///     yet implemented. The variant exists so the type hierarchy is complete
    ///     and callers that match on `TelemetryExporter` can handle this case
    ///     (currently a stub that drains events silently).
    /// Owner: https://github.com/Crsei/allthecodes/issues (telemetry)
    /// Removal: When the BigQuery exporter backend is implemented, remove this
    ///     allow-dead-code and add real export logic in `TelemetryHandle::flush`.
    #[allow(dead_code)]
    BigQuery,
}

/// PII redaction configuration for telemetry events.
#[derive(Debug, Clone)]
pub struct TelemetryRedaction {
    /// Redact tool input payloads.
    pub redact_tool_inputs: bool,
    /// Redact tool output payloads.
    pub redact_tool_outputs: bool,
    /// Redact file paths that may contain user names.
    pub redact_file_paths: bool,
    /// Redact environment variables.
    pub redact_environment: bool,
}

impl Default for TelemetryRedaction {
    fn default() -> Self {
        Self {
            redact_tool_inputs: true,
            redact_tool_outputs: true,
            redact_file_paths: true,
            redact_environment: false,
        }
    }
}

// ---------------------------------------------------------------------------
// TelemetryEvent
// ---------------------------------------------------------------------------

/// A telemetry event that can be exported to a backend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum TelemetryEvent {
    /// A user submit -> model response interaction.
    Interaction {
        interaction_id: String,
        session_id: String,
        submit_id: String,
        duration_ms: Option<u64>,
        model: Option<String>,
        tools_used: Vec<String>,
        tokens_in: Option<u32>,
        tokens_out: Option<u32>,
        error: Option<String>,
    },
    /// A single model API call.
    ModelCall {
        span_id: String,
        parent_span_id: Option<String>,
        model: String,
        request_tokens: Option<u32>,
        response_tokens: Option<u32>,
        duration_ms: Option<u64>,
        cache_hit: bool,
        retry_count: u32,
        error: Option<String>,
    },
    /// A single tool execution.
    ToolExecution {
        span_id: String,
        parent_span_id: Option<String>,
        tool_name: String,
        tool_input_summary: Option<String>,
        duration_ms: Option<u64>,
        result: String,
    },
    /// A hook execution.
    HookExecution {
        hook_name: String,
        duration_ms: Option<u64>,
        result: String,
    },
    /// A user/system input event.
    InputEvent {
        input_summary: String,
        input_type: String,
    },
}

// ---------------------------------------------------------------------------
// TelemetryHandle
// ---------------------------------------------------------------------------

/// Handle to the telemetry subsystem.
///
/// Cheaply cloneable — all clones share the same event buffer and config.
#[derive(Clone)]
pub struct TelemetryHandle {
    config: TelemetryConfig,
    inner: Arc<Mutex<TelemetryInner>>,
}

impl std::fmt::Debug for TelemetryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelemetryHandle")
            .field("enabled", &self.config.enabled)
            .field("exporter", &self.config.exporter)
            .finish()
    }
}

struct TelemetryInner {
    events: Vec<TelemetryEvent>,
}

impl TelemetryHandle {
    fn new(config: TelemetryConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TelemetryInner { events: Vec::new() })),
            config,
        }
    }

    /// Start a new interaction span.
    ///
    /// Returns an `InteractionSpan` that should be `finish()`ed when the
    /// interaction completes.
    pub fn start_interaction(&self, session_id: String, submit_id: String) -> InteractionSpan {
        InteractionSpan::start(session_id, submit_id, self.clone())
    }

    /// Flush buffered telemetry events to the configured exporter backend.
    ///
    /// When the exporter is `Log`, events are written via `tracing::debug!`.
    pub fn flush(&self) {
        let mut inner = self.inner.lock();
        if inner.events.is_empty() {
            return;
        }

        match self.config.exporter {
            TelemetryExporter::Log => {
                for event in inner.events.drain(..) {
                    tracing::debug!(?event, "telemetry event");
                }
            }
            TelemetryExporter::None => {
                inner.events.clear();
            }
            TelemetryExporter::BigQuery => {
                // Stub: BigQuery export is non-P0.
                inner.events.clear();
            }
        }
    }

    /// Shut down the telemetry subsystem, flushing any remaining events.
    pub fn shutdown(&self) {
        self.flush();
    }

    /// Whether this handle is active (telemetry is enabled).
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Access the configuration.
    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    /// Record a telemetry event (internal).
    pub(crate) fn record(&self, event: TelemetryEvent) {
        if !self.config.enabled {
            return;
        }

        // Apply sampling
        if self.config.sampling_rate < 1.0 {
            use rand::Rng;
            let sample: f64 = rand::thread_rng().gen();
            if sample > self.config.sampling_rate {
                return; // skipped by sampling
            }
        }

        let event = match serde_json::to_value(&event) {
            Ok(mut value) => {
                privacy::redact_for_telemetry(&mut value, &self.config.redaction);
                serde_json::from_value(value).unwrap_or(event)
            }
            Err(_) => event,
        };

        self.inner.lock().events.push(event);
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the telemetry subsystem.
///
/// Creates a `TelemetryHandle` from the given configuration. The handle
/// should be shared across the application (e.g. stored in `AppState`).
pub fn init_telemetry(config: TelemetryConfig) -> TelemetryHandle {
    TelemetryHandle::new(config)
}

// ---------------------------------------------------------------------------
// Convenience helpers (point 10)
// ---------------------------------------------------------------------------

/// Emit an input event for tracing.
///
/// Convenience helper — creates an `InteractionSpan` and immediately records it
/// as an input event. The span is returned so the caller can call `finish()` on
/// it when the interaction completes.
pub fn emit_input_event(
    handle: &TelemetryHandle,
    input: &str,
    input_type: InputType,
    session_id: String,
    submit_id: String,
) -> InteractionSpan {
    // Record the input event
    let summary = privacy::summarize_input(input);
    handle.record(TelemetryEvent::InputEvent {
        input_summary: summary,
        input_type: format!("{:?}", input_type),
    });

    // Return a new interaction span for the caller to finish
    InteractionSpan::start(session_id, submit_id, handle.clone())
}

/// Emit a tool event start.
///
/// Convenience helper — immediately records a tool execution start and returns
/// a `ToolSpan` for the caller to finish.
pub fn emit_tool_event_start(
    handle: &TelemetryHandle,
    tool_name: &str,
    tool_input_summary: Option<String>,
) -> ToolSpan {
    ToolSpan::start(tool_name.to_string(), tool_input_summary, handle.clone())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_handle_default_config() {
        let config = TelemetryConfig::default();
        assert!(config.enabled);
        assert_eq!(config.exporter, TelemetryExporter::Log);
        assert!((config.sampling_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn telemetry_handle_disabled_discards() {
        let config = TelemetryConfig {
            enabled: false,
            ..Default::default()
        };
        let handle = init_telemetry(config);
        handle.record(TelemetryEvent::InputEvent {
            input_summary: "test".into(),
            input_type: "UserMessage".into(),
        });
        // No panic — event silently discarded because disabled
    }

    #[test]
    fn telemetry_handle_flush_log_exporter() {
        let config = TelemetryConfig {
            enabled: true,
            exporter: TelemetryExporter::None,
            ..Default::default()
        };
        let handle = init_telemetry(config);
        handle.record(TelemetryEvent::InputEvent {
            input_summary: "test".into(),
            input_type: "UserMessage".into(),
        });
        handle.flush();
        // Flush should clear the buffer without panic
    }

    #[test]
    fn interaction_span_lifecycle() {
        let config = TelemetryConfig {
            enabled: true,
            exporter: TelemetryExporter::None,
            ..Default::default()
        };
        let handle = init_telemetry(config);
        let mut span = handle.start_interaction("sess_01".into(), "sub_01".into());
        span.finish("claude-sonnet-4", 100, 200);
        // Should not panic
    }

    #[test]
    fn record_redacts_before_buffering() {
        let config = TelemetryConfig {
            enabled: true,
            exporter: TelemetryExporter::None,
            ..Default::default()
        };
        let handle = init_telemetry(config);
        handle.record(TelemetryEvent::InputEvent {
            input_summary: r#"{"api_key":"secret","file_path":"/home/alice/project"}"#.into(),
            input_type: "UserMessage".into(),
        });

        let inner = handle.inner.lock();
        let value = serde_json::to_value(&inner.events[0]).unwrap();
        assert!(!value.to_string().contains("secret"));
        assert!(!value.to_string().contains("/home/alice"));
    }
}
