//! Versioned IPC event envelope.

use serde::{Deserialize, Serialize};

/// Current envelope schema version.
pub const IPC_ENVELOPE_VERSION: u16 = 1;
/// Oldest envelope schema version accepted by this build.
pub const IPC_ENVELOPE_MIN_COMPAT_VERSION: u16 = 1;

fn default_version() -> u16 {
    IPC_ENVELOPE_VERSION
}

pub fn is_supported_envelope_version(version: u16) -> bool {
    (IPC_ENVELOPE_MIN_COMPAT_VERSION..=IPC_ENVELOPE_VERSION).contains(&version)
}

/// Transport-neutral wrapper for IPC events and commands.
///
/// The legacy JSONL protocol still sends bare [`crate::BackendMessage`] and
/// [`crate::FrontendMessage`] values. This envelope is additive: WebUI,
/// gateway, SSE, and future transports can carry the same normalized payloads
/// while preserving session/turn/run correlation metadata.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct IpcEnvelope<T> {
    #[serde(default = "default_version")]
    pub version: u16,
    pub id: String,
    pub seq: u64,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub payload: T,
}

impl<T> IpcEnvelope<T> {
    pub fn new(id: impl Into<String>, seq: u64, timestamp: i64, payload: T) -> Self {
        Self {
            version: IPC_ENVELOPE_VERSION,
            id: id.into(),
            seq,
            timestamp,
            session_id: None,
            turn_id: None,
            run_id: None,
            correlation_id: None,
            payload,
        }
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn is_supported_version(&self) -> bool {
        is_supported_envelope_version(self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConversationEvent;

    #[test]
    fn envelope_roundtrip_preserves_correlation_fields() {
        let original = IpcEnvelope::new(
            "event-1",
            7,
            1_700_000_000,
            ConversationEvent::StreamStart {
                message_id: "message-1".to_string(),
            },
        )
        .with_session_id("session-1")
        .with_turn_id("turn-1")
        .with_run_id("run-1")
        .with_correlation_id("corr-1");

        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: IpcEnvelope<ConversationEvent> = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.version, IPC_ENVELOPE_VERSION);
        assert_eq!(decoded.id, "event-1");
        assert_eq!(decoded.seq, 7);
        assert_eq!(decoded.timestamp, 1_700_000_000);
        assert_eq!(decoded.session_id.as_deref(), Some("session-1"));
        assert_eq!(decoded.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(decoded.run_id.as_deref(), Some("run-1"));
        assert_eq!(decoded.correlation_id.as_deref(), Some("corr-1"));
        assert!(matches!(
            decoded.payload,
            ConversationEvent::StreamStart { message_id } if message_id == "message-1"
        ));
    }

    #[test]
    fn missing_version_defaults_to_current_wire_contract() {
        let decoded: IpcEnvelope<ConversationEvent> = serde_json::from_value(serde_json::json!({
            "id": "event-2",
            "seq": 8,
            "timestamp": 1_700_000_001,
            "payload": {
                "type": "stream_start",
                "message_id": "message-2"
            }
        }))
        .unwrap();

        assert_eq!(decoded.version, IPC_ENVELOPE_VERSION);
        assert!(decoded.is_supported_version());
    }

    #[test]
    fn future_version_is_detected_as_incompatible() {
        let decoded: IpcEnvelope<ConversationEvent> = serde_json::from_value(serde_json::json!({
            "version": IPC_ENVELOPE_VERSION + 1,
            "id": "event-3",
            "seq": 9,
            "timestamp": 1_700_000_002,
            "payload": {
                "type": "stream_start",
                "message_id": "message-3"
            }
        }))
        .unwrap();

        assert!(!decoded.is_supported_version());
    }
}
