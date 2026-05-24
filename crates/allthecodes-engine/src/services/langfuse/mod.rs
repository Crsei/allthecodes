//! Langfuse observability via OpenTelemetry.
//!
//! This module keeps Langfuse-specific wiring out of the business logic and
//! exposes a small set of helpers for lifecycle boundaries (submit/model/tool).
//!
//! The real telemetry path lives behind the `telemetry` feature. When that
//! feature is disabled, `stub` provides drop-in no-op types and functions
//! with identical signatures so call sites don't need `#[cfg]` guards.

pub mod convert;
pub mod sanitize;

#[cfg(feature = "telemetry")]
pub mod client;
#[cfg(feature = "telemetry")]
pub mod tracing;

#[cfg(not(feature = "telemetry"))]
mod stub;

#[cfg(feature = "telemetry")]
pub use client::{init_langfuse, shutdown_langfuse};
#[cfg(feature = "telemetry")]
pub use tracing::{
    create_generation_span, create_subagent_trace, create_tool_batch_span, create_tool_span,
    create_trace, end_span, end_trace, finish_generation_span, finish_tool_span, LangfuseSpan,
    LangfuseTrace, TraceStatus,
};

#[cfg(not(feature = "telemetry"))]
pub use stub::{
    create_generation_span, create_subagent_trace, create_tool_batch_span, create_tool_span,
    create_trace, end_span, end_trace, finish_generation_span, finish_tool_span, shutdown_langfuse,
    LangfuseSpan, LangfuseTrace, TraceStatus,
};
