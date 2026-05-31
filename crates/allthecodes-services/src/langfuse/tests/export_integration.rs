#[cfg(not(feature = "telemetry"))]
#[test]
fn disabled_telemetry_surface_is_noop() {
    let trace = crate::langfuse::create_trace("session", "model", "provider", "input", None);

    assert!(trace.is_none());
    crate::langfuse::shutdown_langfuse();
}

#[cfg(feature = "telemetry")]
#[test]
fn empty_telemetry_export_is_noop() {
    crate::langfuse::export_telemetry_events(Vec::new());
}
