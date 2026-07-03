use allthecodes_types::message::Message;

use super::SerializableMessage;

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Convert the internal `Message` enum to a serializable form.
pub(crate) fn messages_to_serializable(messages: &[Message]) -> Vec<SerializableMessage> {
    messages
        .iter()
        .map(|msg| {
            let (msg_type, data) = match msg {
                Message::User(u) => {
                    let content_value = match &u.content {
                        allthecodes_types::message::MessageContent::Text(t) => {
                            serde_json::json!(t)
                        }
                        allthecodes_types::message::MessageContent::Blocks(blocks) => {
                            serde_json::json!(blocks)
                        }
                    };
                    (
                        "user".to_string(),
                        serde_json::json!({
                            "content": content_value,
                            "is_meta": u.is_meta,
                        }),
                    )
                }
                Message::Assistant(a) => (
                    "assistant".to_string(),
                    serde_json::json!({
                        "content": a.content,
                        "stop_reason": a.stop_reason,
                        "cost_usd": a.cost_usd,
                        "usage": a.usage,
                    }),
                ),
                Message::System(s) => {
                    ("system".to_string(), system_message_to_serializable_data(s))
                }
                Message::Progress(p) => (
                    "progress".to_string(),
                    serde_json::json!({
                        "tool_use_id": p.tool_use_id,
                        "data": p.data,
                    }),
                ),
                Message::Attachment(a) => (
                    "attachment".to_string(),
                    serde_json::json!({
                        "attachment": a.attachment,
                    }),
                ),
            };
            SerializableMessage {
                msg_type,
                uuid: msg.uuid().to_string(),
                timestamp: msg.timestamp(),
                data,
            }
        })
        .collect()
}

fn system_message_to_serializable_data(
    system: &allthecodes_types::message::SystemMessage,
) -> serde_json::Value {
    use allthecodes_types::message::SystemSubtype;

    let mut data = serde_json::json!({
        "content": system.content,
    });
    match &system.subtype {
        SystemSubtype::CompactBoundary { compact_metadata } => {
            data["subtype"] = serde_json::json!("CompactBoundary");
            if let Some(metadata) = compact_metadata {
                data["compact_metadata"] = serde_json::json!(metadata);
            }
        }
        SystemSubtype::MicrocompactBoundary {
            microcompact_metadata,
        } => {
            data["subtype"] = serde_json::json!("MicrocompactBoundary");
            if let Some(metadata) = microcompact_metadata {
                data["microcompact_metadata"] = serde_json::json!(metadata);
            }
        }
        SystemSubtype::LocalCommand { content } => {
            data["subtype"] = serde_json::json!("LocalCommand");
            data["local_command_content"] = serde_json::json!(content);
        }
        SystemSubtype::Warning => {
            data["subtype"] = serde_json::json!("Warning");
        }
        _ => {}
    }
    data
}

/// Convert serializable messages back to `Message` instances.
///
/// This is a best-effort reconstruction. Fields that cannot be recovered from
/// the simplified serialization are set to defaults. A production
/// implementation would store the full typed data.
///
/// Phase 0 record/replay baseline: legacy `SerializableMessage` currently
/// reconstructs user/assistant/system messages only. Progress and attachment
/// entries remain persisted in JSON/SQLite but are dropped when loading through
/// this compatibility path; new record logs store a typed message envelope
/// instead.
pub(crate) fn serializable_to_messages(msgs: &[SerializableMessage]) -> Vec<Message> {
    use allthecodes_types::message::*;
    use uuid::Uuid;

    msgs.iter()
        .filter_map(|sm| {
            let uuid = Uuid::parse_str(&sm.uuid).unwrap_or_else(|_| Uuid::new_v4());

            match sm.msg_type.as_str() {
                "user" => Some(Message::User(UserMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    role: "user".into(),
                    content: match sm.data.get("content") {
                        Some(serde_json::Value::String(s)) => MessageContent::Text(s.clone()),
                        Some(serde_json::Value::Array(blocks)) => {
                            match serde_json::from_value::<
                                Vec<allthecodes_types::message::ContentBlock>,
                            >(serde_json::Value::Array(
                                blocks.clone(),
                            )) {
                                Ok(cb) => MessageContent::Blocks(cb),
                                Err(_) => MessageContent::Text(
                                    blocks
                                        .iter()
                                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                ),
                            }
                        }
                        // Backwards compat: old Debug format like Text("hello")
                        _ => MessageContent::Text(
                            sm.data
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ),
                    },
                    is_meta: sm
                        .data
                        .get("is_meta")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                })),
                "assistant" => Some(Message::Assistant(AssistantMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    role: "assistant".into(),
                    content: sm
                        .data
                        .get("content")
                        .and_then(|v| {
                            serde_json::from_value::<Vec<allthecodes_types::message::ContentBlock>>(
                                v.clone(),
                            )
                            .ok()
                        })
                        .unwrap_or_default(),
                    usage: sm.data.get("usage").and_then(|v| {
                        serde_json::from_value::<allthecodes_types::message::Usage>(v.clone()).ok()
                    }),
                    stop_reason: sm
                        .data
                        .get("stop_reason")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    is_api_error_message: false,
                    api_error: None,
                    cost_usd: sm
                        .data
                        .get("cost_usd")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                })),
                "system" => Some(Message::System(SystemMessage {
                    uuid,
                    timestamp: sm.timestamp,
                    subtype: system_subtype_from_serialized_data(&sm.data),
                    content: sm
                        .data
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })),
                _ => None,
            }
        })
        .collect()
}

fn system_subtype_from_serialized_data(
    data: &serde_json::Value,
) -> allthecodes_types::message::SystemSubtype {
    use allthecodes_types::message::{InfoLevel, SystemSubtype};

    match data.get("subtype").and_then(|v| v.as_str()) {
        Some("CompactBoundary") => SystemSubtype::CompactBoundary {
            compact_metadata: data
                .get("compact_metadata")
                .and_then(compact_metadata_from_value),
        },
        Some("LocalCommand") => SystemSubtype::LocalCommand {
            content: data
                .get("local_command_content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
        Some("Warning") => SystemSubtype::Warning,
        _ => SystemSubtype::Informational {
            level: InfoLevel::Info,
        },
    }
}

fn compact_metadata_from_value(
    value: &serde_json::Value,
) -> Option<allthecodes_types::message::CompactMetadata> {
    Some(allthecodes_types::message::CompactMetadata {
        pre_compact_token_count: value.get("pre_compact_token_count")?.as_u64()?,
        post_compact_token_count: value.get("post_compact_token_count")?.as_u64()?,
        preserved_segment: value
            .get("preserved_segment")
            .and_then(preserved_segment_from_value),
        pre_compact_discovered_tools: value
            .get("pre_compact_discovered_tools")
            .and_then(string_vec_from_value),
    })
}

fn preserved_segment_from_value(
    value: &serde_json::Value,
) -> Option<allthecodes_types::message::PreservedSegment> {
    let preserved_message_uuids = value
        .get("preserved_message_uuids")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect::<Vec<_>>();

    Some(allthecodes_types::message::PreservedSegment {
        summary_message_uuid: value
            .get("summary_message_uuid")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        preserved_message_uuids,
    })
}

fn string_vec_from_value(value: &serde_json::Value) -> Option<Vec<String>> {
    Some(
        value
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(ToString::to_string))
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{
        AssistantMessage, ContentBlock, Message, MessageContent, SystemMessage, SystemSubtype,
        ToolResultContent, UserMessage,
    };
    use uuid::Uuid;

    /// Helper: build a minimal SerializableMessage from parts.
    fn sm(msg_type: &str, uuid: &str, ts: i64, data: serde_json::Value) -> SerializableMessage {
        SerializableMessage {
            msg_type: msg_type.into(),
            uuid: uuid.into(),
            timestamp: ts,
            data,
        }
    }

    // ------------------------------------------------------------------
    // Roundtrip: messages_to_serializable -> serializable_to_messages
    // ------------------------------------------------------------------

    #[test]
    fn roundtrip_user_text_message() {
        let msgs = vec![Message::User(UserMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000001").unwrap(),
            timestamp: 100,
            role: "user".into(),
            content: MessageContent::Text("hello".into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        assert_eq!(deserialized.len(), 1);
        match &deserialized[0] {
            Message::User(u) => {
                assert!(matches!(&u.content, MessageContent::Text(t) if t == "hello"));
                assert!(!u.is_meta);
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_user_meta_message() {
        let msgs = vec![Message::User(UserMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000002").unwrap(),
            timestamp: 101,
            role: "user".into(),
            content: MessageContent::Text("meta".into()),
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::User(u) => assert!(u.is_meta),
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_user_with_tool_result_blocks() {
        let msgs = vec![Message::User(UserMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000003").unwrap(),
            timestamp: 102,
            role: "user".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tu_roundtrip".into(),
                content: ToolResultContent::Text("result data".into()),
                is_error: false,
            }]),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::User(u) => match &u.content {
                MessageContent::Blocks(blocks) => {
                    assert!(blocks.iter().any(|b| matches!(
                        b,
                        ContentBlock::ToolResult { tool_use_id, .. }
                            if tool_use_id == "tu_roundtrip"
                    )));
                }
                other => panic!("expected Blocks, got {other:?}"),
            },
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_assistant_with_tool_use() {
        let msgs = vec![Message::Assistant(AssistantMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000004").unwrap(),
            timestamp: 103,
            role: "assistant".into(),
            content: vec![
                ContentBlock::Text {
                    text: "Let me look that up.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tu_assistant_1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({ "path": "Cargo.toml" }),
                },
            ],
            usage: None,
            stop_reason: Some("tool_use".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.02,
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::Assistant(a) => {
                assert!(a.content.iter().any(|b| matches!(
                    b,
                    ContentBlock::Text { text } if text == "Let me look that up."
                )));
                assert!(a.content.iter().any(|b| matches!(
                    b,
                    ContentBlock::ToolUse { id, name, .. }
                        if id == "tu_assistant_1" && name == "Read"
                )));
                assert_eq!(a.stop_reason.as_deref(), Some("tool_use"));
                // cost_usd may be serialized via serde_json::Number; compare approximately
                assert!((a.cost_usd - 0.02).abs() < 1e-9);
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_system_warning() {
        let msgs = vec![Message::System(SystemMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000005").unwrap(),
            timestamp: 104,
            subtype: SystemSubtype::Warning,
            content: "test warning".into(),
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::System(s) => {
                assert_eq!(s.content, "test warning");
                assert!(matches!(s.subtype, SystemSubtype::Warning));
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_system_compact_boundary() {
        use allthecodes_types::message::CompactMetadata;
        let msgs = vec![Message::System(SystemMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000006").unwrap(),
            timestamp: 105,
            subtype: SystemSubtype::CompactBoundary {
                compact_metadata: Some(CompactMetadata {
                    pre_compact_token_count: 1000,
                    post_compact_token_count: 200,
                    preserved_segment: None,
                    pre_compact_discovered_tools: None,
                }),
            },
            content: "compacted".into(),
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::System(s) => {
                assert!(matches!(s.subtype, SystemSubtype::CompactBoundary { .. }));
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_system_local_command() {
        let msgs = vec![Message::System(SystemMessage {
            uuid: Uuid::parse_str("a0000000-0000-0000-0000-000000000007").unwrap(),
            timestamp: 106,
            subtype: SystemSubtype::LocalCommand {
                content: "echo hello".into(),
            },
            content: "local".into(),
        })];
        let serialized = messages_to_serializable(&msgs);
        let deserialized = serializable_to_messages(&serialized);
        match &deserialized[0] {
            Message::System(s) => {
                assert!(matches!(s.subtype, SystemSubtype::LocalCommand { .. }));
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // Progress and attachment deserialization: legacy compatibility path.
    //
    // TODO(full-build): callers that need full-fidelity replay should use typed
    // record logs. This legacy SerializableMessage loader still drops progress
    // and attachment entries when reconstructing chat messages.
    // ------------------------------------------------------------------

    #[test]
    fn deserialize_progress_is_dropped() {
        let sms = vec![sm(
            "progress",
            "b0000000-0000-0000-0000-000000000001",
            200,
            serde_json::json!({
                "tool_use_id": "tu_progress",
                "data": { "progress": 0.5 }
            }),
        )];
        let messages = serializable_to_messages(&sms);
        assert!(
            messages.is_empty(),
            "progress entries are dropped by serializable_to_messages; got {} messages",
            messages.len()
        );
    }

    #[test]
    fn deserialize_attachment_is_dropped() {
        let sms = vec![sm(
            "attachment",
            "b0000000-0000-0000-0000-000000000002",
            201,
            serde_json::json!({
                "attachment": { "type": "edited_text_file", "path": "src/main.rs" }
            }),
        )];
        let messages = serializable_to_messages(&sms);
        assert!(
            messages.is_empty(),
            "attachment entries are dropped by serializable_to_messages; got {} messages",
            messages.len()
        );
    }

    // ------------------------------------------------------------------
    // Phase 0: SerializableMessage deserialization baseline tests
    // ------------------------------------------------------------------
    // These tests directly construct SerializableMessage values and pass
    // them through serializable_to_messages, covering the exact names
    // listed in the TDD test plan.

    #[test]
    fn deserialize_user_message() {
        let sm = sm(
            "user",
            "f0000000-0000-0000-0000-000000000001",
            700,
            serde_json::json!({ "content": "hello user", "is_meta": false }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::User(u) => {
                assert!(matches!(&u.content, MessageContent::Text(t) if t == "hello user"));
                assert!(!u.is_meta);
                assert_eq!(u.timestamp, 700);
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_assistant_message() {
        let sm = sm(
            "assistant",
            "f0000000-0000-0000-0000-000000000002",
            701,
            serde_json::json!({
                "content": [
                    { "type": "text", "text": "hello assistant" }
                ],
                "stop_reason": "end_turn",
                "cost_usd": 0.01
            }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::Assistant(a) => {
                assert!(a.content.len() == 1);
                assert!(
                    matches!(&a.content[0], ContentBlock::Text { text } if text == "hello assistant")
                );
                assert_eq!(a.stop_reason.as_deref(), Some("end_turn"));
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_system_message() {
        let sm = sm(
            "system",
            "f0000000-0000-0000-0000-000000000003",
            702,
            serde_json::json!({ "content": "system note", "subtype": "Warning" }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::System(s) => {
                assert_eq!(s.content, "system note");
                assert!(matches!(s.subtype, SystemSubtype::Warning));
                assert_eq!(s.timestamp, 702);
            }
            other => panic!("expected System, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_tool_use() {
        let sm = sm(
            "assistant",
            "f0000000-0000-0000-0000-000000000004",
            703,
            serde_json::json!({
                "content": [
                    { "type": "text", "text": "Let me check" },
                    {
                        "type": "tool_use",
                        "id": "toolu_deser",
                        "name": "Bash",
                        "input": { "command": "ls" }
                    }
                ],
                "stop_reason": "tool_use"
            }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::Assistant(a) => {
                let tool_blocks: Vec<_> = a
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::ToolUse { id, name, .. } = b {
                            Some((id.as_str(), name.as_str()))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(tool_blocks, vec![("toolu_deser", "Bash")]);
            }
            other => panic!("expected Assistant, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_tool_result() {
        let sm = sm(
            "user",
            "f0000000-0000-0000-0000-000000000005",
            704,
            serde_json::json!({
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_deser",
                        "content": "ls output here",
                        "is_error": false
                    }
                ],
                "is_meta": false
            }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::User(u) => {
                let tool_results: Vec<_> = match &u.content {
                    MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } = b
                            {
                                let text = match content {
                                    ToolResultContent::Text(t) => t.as_str(),
                                    _ => "",
                                };
                                Some((tool_use_id.as_str(), text, *is_error))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => vec![],
                };
                assert_eq!(tool_results, vec![("toolu_deser", "ls output here", false)]);
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    /// Legacy compatibility behavior: roundtrip of a full SerializableMessage list that
    /// contains user, assistant (with tool_use), tool_result user, system,
    /// progress, and attachment. This compatibility loader drops progress and
    /// attachment; typed record logs are responsible for preserving them.
    #[test]
    fn deserialize_legacy_session_with_tool_messages() {
        let sms = vec![
            // 0: user
            sm(
                "user",
                "g0000000-0000-0000-0000-000000000001",
                800,
                serde_json::json!({ "content": "read the file", "is_meta": false }),
            ),
            // 1: assistant with tool_use
            sm(
                "assistant",
                "g0000000-0000-0000-0000-000000000002",
                801,
                serde_json::json!({
                    "content": [
                        { "type": "text", "text": "sure" },
                        { "type": "tool_use", "id": "tu_legacy", "name": "Read", "input": { "file_path": "foo.txt" } }
                    ],
                    "stop_reason": "tool_use",
                    "cost_usd": 0.02
                }),
            ),
            // 2: user with tool_result
            sm(
                "user",
                "g0000000-0000-0000-0000-000000000003",
                802,
                serde_json::json!({
                    "content": [
                        { "type": "tool_result", "tool_use_id": "tu_legacy", "content": "file content", "is_error": false }
                    ],
                    "is_meta": false
                }),
            ),
            // 3: system
            sm(
                "system",
                "g0000000-0000-0000-0000-000000000004",
                803,
                serde_json::json!({ "content": "system msg", "subtype": "Warning" }),
            ),
            // 4: progress — expected to be dropped
            sm(
                "progress",
                "g0000000-0000-0000-0000-000000000005",
                804,
                serde_json::json!({ "tool_use_id": "tu_legacy", "data": {} }),
            ),
            // 5: attachment — expected to be dropped
            sm(
                "attachment",
                "g0000000-0000-0000-0000-000000000006",
                805,
                serde_json::json!({ "attachment": { "type": "edited_text_file", "path": "foo.txt" } }),
            ),
        ];
        let messages = serializable_to_messages(&sms);
        // 6 input → 4 survive (user, assistant, user, system)
        assert_eq!(messages.len(), 4);
        assert!(matches!(&messages[0], Message::User(_)));
        assert!(matches!(&messages[1], Message::Assistant(_)));
        assert!(matches!(&messages[2], Message::User(_)));
        assert!(matches!(&messages[3], Message::System(_)));

        // Verify tool_use content survived in assistant
        match &messages[1] {
            Message::Assistant(a) => {
                assert!(a.content.iter().any(|b| matches!(
                    b,
                    ContentBlock::ToolUse { id, name, .. }
                        if id == "tu_legacy" && name == "Read"
                )));
            }
            other => panic!("expected Assistant, got {other:?}"),
        }

        // Verify tool_result content survived in user
        match &messages[2] {
            Message::User(u) => {
                assert!(matches!(&u.content, MessageContent::Blocks(blocks)
                    if blocks.iter().any(|b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tu_legacy"))));
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_progress_legacy_compat_path_drops_entry_todo() {
        // SerializableMessage with type "progress" is dropped by
        // serializable_to_messages in the legacy compatibility path.
        // TODO(full-build): keep full-fidelity progress replay in typed
        // record logs instead of relying on this loader.
        let sm = sm(
            "progress",
            "f0000000-0000-0000-0000-000000000006",
            705,
            serde_json::json!({
                "tool_use_id": "toolu_progress",
                "data": { "progress": 0.75 }
            }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert!(
            messages.is_empty(),
            "progress entries are dropped by serializable_to_messages; got {}",
            messages.len()
        );
    }

    #[test]
    fn deserialize_attachment_legacy_compat_path_drops_entry_todo() {
        // SerializableMessage with type "attachment" is dropped by
        // serializable_to_messages in the legacy compatibility path.
        // TODO(full-build): keep full-fidelity attachment replay in typed
        // record logs instead of relying on this loader.
        let sm = sm(
            "attachment",
            "f0000000-0000-0000-0000-000000000007",
            706,
            serde_json::json!({
                "attachment": { "type": "edited_text_file", "path": "Cargo.toml" }
            }),
        );
        let messages = serializable_to_messages(&[sm]);
        assert!(
            messages.is_empty(),
            "attachment entries are dropped by serializable_to_messages; got {}",
            messages.len()
        );
    }

    // ------------------------------------------------------------------
    // Edge cases: unknowns, empty data, missing fields
    // ------------------------------------------------------------------

    #[test]
    fn deserialize_unknown_type_is_dropped() {
        let sms = vec![sm(
            "bogus_type",
            "c0000000-0000-0000-0000-000000000001",
            300,
            serde_json::json!({ "content": "whatever" }),
        )];
        let messages = serializable_to_messages(&sms);
        assert!(
            messages.is_empty(),
            "unknown msg_type entries are dropped; got {} messages",
            messages.len()
        );
    }

    #[test]
    fn deserialize_invalid_uuid_falls_back_and_does_not_panic() {
        let sms = vec![SerializableMessage {
            msg_type: "user".into(),
            uuid: "not-a-uuid".into(),
            timestamp: 400,
            data: serde_json::json!({ "content": "hello", "is_meta": false }),
        }];
        let messages = serializable_to_messages(&sms);
        // Should not panic; falls back to a random UUID
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn deserialize_user_missing_content_does_not_panic() {
        let sms = vec![sm(
            "user",
            "d0000000-0000-0000-0000-000000000001",
            500,
            serde_json::json!({ "is_meta": false }),
        )];
        let messages = serializable_to_messages(&sms);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::User(u) => match &u.content {
                MessageContent::Text(t) => assert_eq!(t, ""),
                other => panic!("expected empty Text, got {other:?}"),
            },
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_mixed_types_preserves_known_drops_other() {
        let sms = vec![
            sm(
                "user",
                "e0000000-0000-0000-0000-000000000001",
                600,
                serde_json::json!({ "content": "first", "is_meta": false }),
            ),
            sm(
                "progress",
                "e0000000-0000-0000-0000-000000000002",
                601,
                serde_json::json!({ "tool_use_id": "x", "data": {} }),
            ),
            sm(
                "attachment",
                "e0000000-0000-0000-0000-000000000003",
                602,
                serde_json::json!({ "attachment": { "type": "max_turns_reached", "max_turns": 10, "turn_count": 10 } }),
            ),
            sm(
                "system",
                "e0000000-0000-0000-0000-000000000004",
                603,
                serde_json::json!({ "content": "sys", "subtype": "Warning" }),
            ),
        ];
        let messages = serializable_to_messages(&sms);
        assert_eq!(messages.len(), 2); // only user + system survive
        assert!(matches!(&messages[0], Message::User(_)));
        assert!(matches!(&messages[1], Message::System(_)));
    }
}
