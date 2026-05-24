//! Pure adapters from internal events to IPC protocol payloads.

use allthecodes_ipc_protocol::{
    legacy_backend_to_payload, legacy_backend_type, BackendMessage, IpcEnvelope,
    LegacyBackendPayload, ToolResultContentInfo,
};
use allthecodes_types::message::{ContentBlock, StreamEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadySnapshot {
    pub session_id: String,
    pub model: String,
    pub cwd: String,
}

pub fn ready_snapshot(message: &BackendMessage) -> Option<ReadySnapshot> {
    match message {
        BackendMessage::Ready {
            session_id,
            model,
            cwd,
            ..
        } => Some(ReadySnapshot {
            session_id: session_id.clone(),
            model: model.clone(),
            cwd: cwd.clone(),
        }),
        _ => None,
    }
}

pub fn backend_message_kind(message: &BackendMessage) -> &'static str {
    legacy_backend_type(message)
}

pub fn legacy_to_envelope(
    message: &BackendMessage,
    id: impl Into<String>,
    seq: u64,
    timestamp: i64,
) -> IpcEnvelope<LegacyBackendPayload> {
    IpcEnvelope::new(id, seq, timestamp, legacy_backend_to_payload(message))
}

pub fn stream_event_to_backend_message(
    event: &StreamEvent,
    message_id: &str,
) -> Option<BackendMessage> {
    match event {
        StreamEvent::MessageStart { .. } => Some(BackendMessage::StreamStart {
            message_id: message_id.to_string(),
        }),
        StreamEvent::ContentBlockStart { .. } => None,
        StreamEvent::ContentBlockDelta { ref delta, .. } => {
            if stream_delta_type_matches(delta, "text_delta") {
                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                    return Some(BackendMessage::StreamDelta {
                        message_id: message_id.to_string(),
                        text: text.to_string(),
                    });
                }
            }
            if stream_delta_type_matches(delta, "thinking_delta") {
                if let Some(thinking) = delta.get("thinking").and_then(|v| v.as_str()) {
                    return Some(BackendMessage::ThinkingDelta {
                        message_id: message_id.to_string(),
                        thinking: thinking.to_string(),
                    });
                }
            }
            None
        }
        StreamEvent::MessageStop => Some(BackendMessage::StreamEnd {
            message_id: message_id.to_string(),
        }),
        _ => None,
    }
}

pub fn stream_event_to_envelope(
    event: &StreamEvent,
    message_id: &str,
    id: impl Into<String>,
    seq: u64,
    timestamp: i64,
) -> Option<IpcEnvelope<LegacyBackendPayload>> {
    stream_event_to_backend_message(event, message_id)
        .map(|message| legacy_to_envelope(&message, id, seq, timestamp))
}

fn stream_delta_type_matches(delta: &serde_json::Value, expected: &str) -> bool {
    delta
        .get("type")
        .and_then(|v| v.as_str())
        .is_none_or(|actual| actual == expected)
}

/// Extract human-readable output text and optional structured content info
/// from block-style tool results.
pub fn extract_tool_result_output(
    blocks: &[ContentBlock],
) -> (String, Option<Vec<ToolResultContentInfo>>) {
    let mut text_parts: Vec<String> = Vec::new();
    let mut infos: Vec<ToolResultContentInfo> = Vec::new();
    let mut has_non_text = false;

    for block in blocks {
        match block {
            ContentBlock::Text { text } => {
                text_parts.push(text.clone());
                infos.push(ToolResultContentInfo::Text { text: text.clone() });
            }
            ContentBlock::Image { source } => {
                has_non_text = true;
                let media_type = source.media_type.clone();
                let size_bytes = Some(source.data.len() * 3 / 4);
                text_parts.push(format!("[image: {}]", media_type));
                infos.push(ToolResultContentInfo::Image {
                    media_type,
                    size_bytes,
                    data: Some(source.data.clone()),
                });
            }
            _ => {
                text_parts.push("[...]".to_string());
            }
        }
    }

    let output = if text_parts.is_empty() {
        "(no output)".to_string()
    } else {
        text_parts.join("\n")
    };

    let content_infos = if has_non_text { Some(infos) } else { None };
    (output, content_infos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_ipc_protocol::{ConversationEvent, LegacyBackendPayload};
    use allthecodes_types::message::ImageSource;

    fn ready() -> BackendMessage {
        BackendMessage::Ready {
            session_id: "session".to_string(),
            model: "model".to_string(),
            cwd: "/tmp/project".to_string(),
            permission_mode: "default".to_string(),
            available_models: Vec::new(),
            plan_workflow: None,
            editor_mode: None,
            view_mode: None,
            keybindings: None,
        }
    }

    #[test]
    fn extracts_ready_snapshot() {
        let snapshot = ready_snapshot(&ready()).expect("ready snapshot");
        assert_eq!(snapshot.session_id, "session");
        assert_eq!(snapshot.model, "model");
        assert_eq!(snapshot.cwd, "/tmp/project");
    }

    #[test]
    fn names_backend_message_kind() {
        assert_eq!(backend_message_kind(&ready()), "ready");
    }

    #[test]
    fn maps_text_stream_delta() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: serde_json::json!({"type":"text_delta","text":"hello"}),
        };

        let mapped = stream_event_to_backend_message(&event, "message-1");

        assert!(matches!(
            mapped,
            Some(BackendMessage::StreamDelta { message_id, text })
                if message_id == "message-1" && text == "hello"
        ));
    }

    #[test]
    fn maps_stream_delta_to_envelope() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: serde_json::json!({"type":"text_delta","text":"hello"}),
        };

        let envelope =
            stream_event_to_envelope(&event, "message-1", "event-1", 1, 99).expect("envelope");

        assert_eq!(envelope.id, "event-1");
        assert_eq!(envelope.seq, 1);
        assert_eq!(envelope.timestamp, 99);
        assert!(matches!(
            envelope.payload,
            LegacyBackendPayload::Conversation(ConversationEvent::StreamDelta {
                message_id,
                text,
            }) if message_id == "message-1" && text == "hello"
        ));
    }

    #[test]
    fn test_extract_text_only_blocks() {
        let blocks = vec![
            ContentBlock::Text {
                text: "line 1".into(),
            },
            ContentBlock::Text {
                text: "line 2".into(),
            },
        ];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "line 1\nline 2");
        assert!(infos.is_none(), "no non-text blocks -> None");
    }

    #[test]
    fn test_extract_image_block_shows_placeholder() {
        let blocks = vec![ContentBlock::Image {
            source: ImageSource {
                source_type: "base64".into(),
                media_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            },
        }];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "[image: image/png]");
        let infos = infos.expect("should have content_infos");
        assert_eq!(infos.len(), 1);
        match &infos[0] {
            ToolResultContentInfo::Image { media_type, .. } => {
                assert_eq!(media_type, "image/png");
            }
            _ => panic!("expected Image info"),
        }
    }

    #[test]
    fn test_extract_mixed_text_and_image() {
        let blocks = vec![
            ContentBlock::Text {
                text: "screenshot taken".into(),
            },
            ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: "image/jpeg".into(),
                    data: "AAAA".into(),
                },
            },
        ];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert!(output.contains("screenshot taken"));
        assert!(output.contains("[image: image/jpeg]"));
        let infos = infos.expect("has image -> Some");
        assert_eq!(infos.len(), 2);
    }

    #[test]
    fn test_extract_empty_blocks() {
        let blocks: Vec<ContentBlock> = vec![];
        let (output, infos) = extract_tool_result_output(&blocks);
        assert_eq!(output, "(no output)");
        assert!(infos.is_none());
    }
}
