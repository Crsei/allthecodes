use allthecodes_types::message::Message;

use super::SerializableMessage;

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Convert the internal `Message` enum to a serializable form.
pub(super) fn messages_to_serializable(messages: &[Message]) -> Vec<SerializableMessage> {
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
pub(super) fn serializable_to_messages(msgs: &[SerializableMessage]) -> Vec<Message> {
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
