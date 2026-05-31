pub use allthecodes_langfuse::stub::*;

/// Stub: no-op when telemetry feature is disabled.
pub fn bridge_from_telemetry() -> Option<LangfuseTrace> {
    None
}

/// Stub: no-op when telemetry feature is disabled.
pub fn flush_telemetry_to_langfuse() {}
