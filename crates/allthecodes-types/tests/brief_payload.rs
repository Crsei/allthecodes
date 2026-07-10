#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use allthecodes_types::brief::{
    brief_payload_from_tool_result, BriefMessageLevel, BriefMessageStatus, BRIEF_TOOL_NAME,
    SEND_USER_MESSAGE_TOOL_NAME,
};
use serde_json::json;

#[test]
fn extracts_brief_tool_payload() {
    let payload = brief_payload_from_tool_result(
        BRIEF_TOOL_NAME,
        "toolu_brief",
        "session-1",
        &json!({
            "is_brief_message": true,
            "message": "Build finished",
            "status": "proactive",
            "attachments": ["target/report.txt"]
        }),
    )
    .expect("brief payload should extract");

    assert_eq!(payload.message, "Build finished");
    assert_eq!(payload.status, BriefMessageStatus::Proactive);
    assert_eq!(payload.attachments, vec!["target/report.txt"]);
    assert_eq!(payload.source_tool_name.as_deref(), Some(BRIEF_TOOL_NAME));
    assert_eq!(payload.tool_use_id.as_deref(), Some("toolu_brief"));
    assert_eq!(payload.session_id.as_deref(), Some("session-1"));
    assert!(payload.timestamp.is_some());
}

#[test]
fn rejects_invalid_brief_status() {
    let payload = brief_payload_from_tool_result(
        BRIEF_TOOL_NAME,
        "toolu_brief",
        "session-1",
        &json!({
            "is_brief_message": true,
            "message": "Build finished",
            "status": "loud"
        }),
    );

    assert!(payload.is_none());
}

#[test]
fn extracts_send_user_message_payload() {
    let payload = brief_payload_from_tool_result(
        SEND_USER_MESSAGE_TOOL_NAME,
        "toolu_user_msg",
        "session-1",
        &json!({
            "is_brief_message": true,
            "message": "Need attention",
            "level": "warning"
        }),
    )
    .expect("send user message payload should extract");

    assert_eq!(payload.message, "Need attention");
    assert_eq!(payload.status, BriefMessageStatus::Normal);
    assert_eq!(payload.level, Some(BriefMessageLevel::Warning));
    assert!(payload.attachments.is_empty());
}

#[test]
fn rejects_empty_payload_message() {
    let payload = brief_payload_from_tool_result(
        SEND_USER_MESSAGE_TOOL_NAME,
        "toolu_user_msg",
        "session-1",
        &json!({
            "is_brief_message": true,
            "message": "",
            "level": "info"
        }),
    );

    assert!(payload.is_none());
}
