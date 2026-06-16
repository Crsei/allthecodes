use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootProbeResponse {
    pub status: &'static str,
    pub probe: &'static str,
    pub service: &'static str,
    pub pid: u32,
    pub timestamp_ms: u64,
}

impl RootProbeResponse {
    pub fn ok(probe: &'static str, service: &'static str) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
            .min(u64::MAX as u128) as u64;
        Self {
            status: "ok",
            probe,
            service,
            pid: std::process::id(),
            timestamp_ms,
        }
    }
}
