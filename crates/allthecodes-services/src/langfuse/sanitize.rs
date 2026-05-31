pub use allthecodes_langfuse::sanitize::*;

/// Apply telemetry redaction configuration to a sanitized value.
///
/// Called during telemetry export to apply additional redaction rules on top
/// of the default Langfuse sanitization.
#[cfg(feature = "telemetry")]
pub fn apply_redaction_config(
    value: &mut serde_json::Value,
    config: &crate::telemetry::TelemetryRedaction,
) {
    crate::telemetry::privacy::redact_for_telemetry(value, config);
}
