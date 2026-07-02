//! Agent protocol serialization tests.
//!
//! Verifies that agent-related [`BackendMessage`] and [`FrontendMessage`]
//! variants round-trip correctly through JSON.

#[cfg(test)]
mod tests {
    use allthecodes_types::agent_events::AgentEvent as AE;
    use allthecodes_types::agent_runtime_record::{
        AgentRuntimeExecutionRecord, AgentRuntimePermissionDecision,
    };

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
    fn backend_execution_record_agent_event_serializes() {
        let msg = BackendMessage::AgentEvent {
            event: AE::ExecutionRecord {
                agent_id: "a1".into(),
                record: Box::new(AgentRuntimeExecutionRecord {
                    session_id: "session-1".to_string(),
                    agent_id: "a1".to_string(),
                    tool: "shell".to_string(),
                    tool_use_id: Some("toolu-1".to_string()),
                    permission_decision: Some(AgentRuntimePermissionDecision::AllowedByUser),
                    exit_code: Some(0),
                    ..Default::default()
                }),
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "agent_event");
        assert_eq!(json["event"]["kind"], "execution_record");
        assert_eq!(json["event"]["agent_id"], "a1");
        assert_eq!(json["event"]["record"]["session_id"], "session-1");
        assert_eq!(json["event"]["record"]["agent_id"], "a1");
        assert_eq!(json["event"]["record"]["tool"], "shell");
        assert_eq!(json["event"]["record"]["tool_use_id"], "toolu-1");
        assert_eq!(
            json["event"]["record"]["permission_decision"],
            "allowed_by_user"
        );
    }

    #[test]
    fn frontend_agent_command_deserializes() {
        let json = r#"{"type":"agent_command","command":{"kind":"abort_agent","agent_id":"a1"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::AgentCommand { .. }));
    }
}
