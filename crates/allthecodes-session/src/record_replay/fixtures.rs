use chrono::{TimeZone, Utc};
use uuid::Uuid;

use allthecodes_types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, UserMessage,
};

use super::types::{MessageRecord, RecordItem, RecordLine, SessionMetaRecord};

pub(crate) fn sample_session_meta() -> SessionMetaRecord {
    SessionMetaRecord {
        created_at: Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap(),
        cwd: "/repo".into(),
        workspace_key: Some("/repo/.git".into()),
        workspace_root: Some("/repo".into()),
        workspace_name: Some("repo".into()),
        model: Some("claude-sonnet-4-5".into()),
        config_summary: None,
        parent_session_id: None,
        branch_from_seq: None,
        migrated_from: None,
    }
}

pub(crate) fn sample_record_lines(session_id: &str) -> Vec<RecordLine> {
    vec![
        RecordLine::new(
            session_id,
            0,
            RecordItem::SessionMeta(sample_session_meta()),
        ),
        RecordLine::new(
            session_id,
            1,
            RecordItem::Message(MessageRecord::from_message(&Message::User(UserMessage {
                uuid: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
                timestamp: 1,
                role: "user".into(),
                content: MessageContent::Text("hello".into()),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            }))),
        ),
        RecordLine::new(
            session_id,
            2,
            RecordItem::Message(MessageRecord::from_message(&Message::Assistant(
                AssistantMessage {
                    uuid: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                    timestamp: 2,
                    role: "assistant".into(),
                    content: vec![ContentBlock::Text { text: "hi".into() }],
                    usage: None,
                    stop_reason: Some("end_turn".into()),
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: 0.0,
                },
            ))),
        ),
    ]
}
