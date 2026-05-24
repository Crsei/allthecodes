//! Agent protocol serialization tests.
//!
//! Verifies that agent-related [`BackendMessage`] and [`FrontendMessage`]
//! variants round-trip correctly through JSON.

#[cfg(test)]
mod tests {
    use allthecodes_types::agent_events::AgentEvent as AE;

    use crate::protocol::{BackendMessage, FrontendMessage};

    #[test]
    fn backend_agent_event_serializes() {
        let msg = BackendMessage::AgentEvent {
            event: AE::Spawned {
                agent_id: "a1".into(),
                parent_agent_id: None,
                description: "test".into(),
                agent_type: None,
                model: None,
                is_background: false,
                depth: 1,
                chain_id: "c1".into(),
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "agent_event");
        assert_eq!(json["event"]["kind"], "spawned");
    }

    #[test]
    fn frontend_agent_command_deserializes() {
        let json = r#"{"type":"agent_command","command":{"kind":"abort_agent","agent_id":"a1"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::AgentCommand { .. }));
    }
}
