//! Team protocol serialization tests.
//!
//! Verifies that team-related [`BackendMessage`] and [`FrontendMessage`]
//! variants round-trip correctly through JSON.

#[cfg(test)]
mod tests {
    use allthecodes_types::agent_events::TeamEvent as TE;

    use crate::protocol::{BackendMessage, FrontendMessage};

    #[test]
    fn backend_team_event_serializes() {
        let msg = BackendMessage::TeamEvent {
            event: TE::MemberLeft {
                team_name: "t1".into(),
                agent_id: "a1".into(),
                agent_name: "w".into(),
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "team_event");
        assert_eq!(json["event"]["kind"], "member_left");
    }

    #[test]
    fn frontend_team_command_deserializes() {
        let json =
            r#"{"type":"team_command","command":{"kind":"query_team_status","team_name":"t1"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::TeamCommand { .. }));
    }
}
