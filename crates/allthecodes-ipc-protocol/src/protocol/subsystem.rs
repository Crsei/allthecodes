//! Subsystem protocol serialization tests.
//!
//! Verifies that subsystem-related [`BackendMessage`] and [`FrontendMessage`]
//! variants round-trip correctly through JSON.

#[cfg(test)]
mod tests {
    use crate::protocol::{BackendMessage, FrontendMessage};

    #[test]
    fn backend_lsp_event_serializes() {
        use crate::subsystem_events::LspEvent;
        let msg = BackendMessage::LspEvent {
            event: LspEvent::ServerStateChanged {
                language_id: "rust".to_string(),
                state: "running".to_string(),
                error: None,
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "lsp_event");
        assert_eq!(json["event"]["kind"], "server_state_changed");
    }

    #[test]
    fn backend_lsp_completion_event_serializes() {
        use crate::subsystem_events::LspEvent;
        let msg = BackendMessage::LspEvent {
            event: LspEvent::CompletionResults {
                request_id: "c1".to_string(),
                uri: "file:///src/main.rs".to_string(),
                items: vec![],
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "lsp_event");
        assert_eq!(json["event"]["kind"], "completion_results");
        assert_eq!(json["event"]["request_id"], "c1");
    }

    #[test]
    fn backend_subsystem_status_serializes() {
        use crate::subsystem_types::SubsystemStatusSnapshot;
        let msg = BackendMessage::SubsystemStatus {
            status: SubsystemStatusSnapshot {
                lsp: vec![],
                mcp: vec![],
                plugins: vec![],
                skills: vec![],
                ides: vec![],
                timestamp: 100,
            },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "subsystem_status");
        assert_eq!(json["status"]["timestamp"], 100);
    }

    #[test]
    fn backend_ide_event_serializes() {
        use crate::subsystem_events::IdeEvent;
        let msg = BackendMessage::IdeEvent {
            event: IdeEvent::IdeList { ides: vec![] },
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "ide_event");
        assert_eq!(json["event"]["kind"], "ide_list");
    }

    #[test]
    fn frontend_ide_command_select_deserializes() {
        let json = r#"{"type":"ide_command","command":{"kind":"select","ide_id":"vscode"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::IdeCommand { .. }));
    }

    #[test]
    fn frontend_mcp_command_upsert_config_deserializes() {
        let json = r#"{
            "type":"mcp_command",
            "command":{
                "kind":"upsert_config",
                "entry":{
                    "name":"ctx7",
                    "transport":"stdio",
                    "command":"npx",
                    "scope":{"kind":"user"}
                }
            }
        }"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::McpCommand { .. }));
    }

    #[test]
    fn frontend_plugin_command_reload_deserializes() {
        let json = r#"{"type":"plugin_command","command":{"kind":"reload"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::PluginCommand { .. }));
    }

    #[test]
    fn frontend_lsp_command_deserializes() {
        let json =
            r#"{"type":"lsp_command","command":{"kind":"start_server","language_id":"rust"}}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::LspCommand { .. }));
    }

    #[test]
    fn frontend_lsp_change_document_deserializes() {
        let json = r#"{
            "type":"lsp_command",
            "command":{
                "kind":"change_document",
                "uri":"file:///src/main.rs",
                "version":2,
                "changes":[{
                    "range":{
                        "start_line":1,
                        "start_character":1,
                        "end_line":1,
                        "end_character":1
                    },
                    "text":"pub "
                }]
            }
        }"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::LspCommand { .. }));
    }

    #[test]
    fn frontend_query_subsystem_status_deserializes() {
        let json = r#"{"type":"query_subsystem_status"}"#;
        let msg: FrontendMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, FrontendMessage::QuerySubsystemStatus));
    }
}
