use std::collections::{HashMap, HashSet};

use allthecodes_types::message::{
    Attachment, ContentBlock, Message, MessageContent, SystemSubtype,
};

use super::context::{MessageRenderOptions, RenderableMessage};
use super::grouping::{is_api_error_message, tool_result_id, tool_use_id};

pub(crate) fn normalize_messages_for_render(messages: &[Message]) -> Vec<RenderableMessage> {
    let mut normalized = Vec::new();
    let mut split_seen = false;

    for (source_index, message) in messages.iter().enumerate() {
        match message {
            Message::Assistant(assistant) => {
                split_seen |= assistant.content.len() > 1;
                for (block_index, block) in assistant.content.iter().cloned().enumerate() {
                    let mut msg = assistant.clone();
                    msg.uuid = if split_seen {
                        derive_child_uuid(assistant.uuid, block_index)
                    } else {
                        assistant.uuid
                    };
                    msg.content = vec![block];
                    normalized.push(RenderableMessage::Message {
                        message: Message::Assistant(msg),
                        source_index,
                    });
                }
            }
            Message::User(user) => {
                let blocks = match &user.content {
                    MessageContent::Text(text) => vec![ContentBlock::Text { text: text.clone() }],
                    MessageContent::Blocks(blocks) => blocks.clone(),
                };
                split_seen |= blocks.len() > 1;
                for (block_index, block) in blocks.into_iter().enumerate() {
                    let mut msg = user.clone();
                    msg.uuid = if split_seen {
                        derive_child_uuid(user.uuid, block_index)
                    } else {
                        user.uuid
                    };
                    msg.content = MessageContent::Blocks(vec![block]);
                    normalized.push(RenderableMessage::Message {
                        message: Message::User(msg),
                        source_index,
                    });
                }
            }
            Message::System(_) | Message::Progress(_) | Message::Attachment(_) => {
                normalized.push(RenderableMessage::Message {
                    message: message.clone(),
                    source_index,
                });
            }
        }
    }

    normalized
}

pub(crate) fn derive_child_uuid(parent: uuid::Uuid, index: usize) -> uuid::Uuid {
    let parent = parent.to_string();
    let derived = format!("{}{:012x}", &parent[..24], index);
    uuid::Uuid::parse_str(&derived)
        .unwrap_or_else(|_| super::grouping::uuid_from_parent_and_salt(uuid::Uuid::nil(), &derived))
}

pub(crate) fn is_not_empty_renderable_message(msg: &RenderableMessage) -> bool {
    let RenderableMessage::Message { message, .. } = msg else {
        return true;
    };
    match message {
        Message::Progress(_) | Message::Attachment(_) | Message::System(_) => true,
        Message::Assistant(assistant) => {
            if assistant.content.is_empty() {
                return false;
            }
            if assistant.content.len() > 1 {
                return true;
            }
            match &assistant.content[0] {
                ContentBlock::Text { text } => !is_empty_message_text(text),
                _ => true,
            }
        }
        Message::User(user) => match &user.content {
            MessageContent::Text(text) => !is_empty_message_text(text),
            MessageContent::Blocks(blocks) => {
                if blocks.is_empty() {
                    return false;
                }
                if blocks.len() > 1 {
                    return true;
                }
                match &blocks[0] {
                    ContentBlock::Text { text } => {
                        !is_empty_message_text(text)
                            && text.trim()
                                != crate::ui::messages::user_tool_result_message::utils::INTERRUPT_MESSAGE_FOR_TOOL_USE
                    }
                    _ => true,
                }
            }
        },
    }
}

pub(crate) fn is_empty_message_text(text: &str) -> bool {
    let stripped = strip_prompt_xml_tags(text);
    stripped.trim().is_empty() || stripped.trim() == "[NO_CONTENT]"
}

pub(crate) fn strip_prompt_xml_tags(text: &str) -> String {
    let mut out = text.to_string();
    for tag in [
        "commit_analysis",
        "context",
        "function_analysis",
        "pr_analysis",
    ] {
        out = strip_simple_tag(&out, tag);
    }
    out.trim().to_string()
}

fn strip_simple_tag(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = text;
    let mut out = String::new();
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        let Some(end) = after_open.find(&close) else {
            out.push_str(after_open);
            return out;
        };
        rest = &after_open[end + close.len()..];
    }
    out.push_str(rest);
    out
}

pub(crate) fn filter_compact_boundary(
    messages: Vec<RenderableMessage>,
    options: MessageRenderOptions,
) -> Vec<RenderableMessage> {
    if options.verbose || options.is_transcript_mode {
        return messages;
    }
    let boundary_index = messages.iter().rposition(|msg| {
        matches!(
            msg,
            RenderableMessage::Message {
                message: Message::System(allthecodes_types::message::SystemMessage {
                    subtype: SystemSubtype::CompactBoundary { .. },
                    ..
                }),
                ..
            }
        )
    });
    boundary_index
        .map(|idx| messages[idx..].to_vec())
        .unwrap_or(messages)
}

pub(crate) fn should_show_renderable_message(
    msg: &RenderableMessage,
    options: MessageRenderOptions,
) -> bool {
    let RenderableMessage::Message { message, .. } = msg else {
        return true;
    };
    match message {
        Message::Progress(_) => false,
        Message::Attachment(attachment) => !is_null_rendering_attachment(&attachment.attachment),
        Message::User(user) => {
            if user.is_meta && !options.is_transcript_mode && !user_contains_tool_result(user) {
                return false;
            }
            true
        }
        Message::Assistant(_) | Message::System(_) => true,
    }
}

fn is_null_rendering_attachment(attachment: &Attachment) -> bool {
    matches!(
        attachment,
        Attachment::EditedTextFile { .. }
            | Attachment::MaxTurnsReached { .. }
            | Attachment::StructuredOutput { .. }
    )
}

fn user_contains_tool_result(user: &allthecodes_types::message::UserMessage) -> bool {
    message_content_blocks(&user.content)
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
}

pub(crate) fn reorder_messages_in_ui(messages: Vec<RenderableMessage>) -> Vec<RenderableMessage> {
    let mut tool_results = HashMap::<String, RenderableMessage>::new();
    for msg in &messages {
        if let Some(tool_use_id_val) = tool_result_id(msg) {
            tool_results.insert(tool_use_id_val.to_string(), msg.clone());
        }
    }

    let mut result = Vec::new();
    let mut consumed_results = HashSet::<String>::new();
    for msg in messages {
        if let Some(tool_use_id_val) = tool_use_id(&msg).map(str::to_string) {
            result.push(msg);
            if let Some(result_msg) = tool_results.get(&tool_use_id_val) {
                result.push(result_msg.clone());
                consumed_results.insert(tool_use_id_val);
            }
            continue;
        }
        if let Some(tool_use_id_val) = tool_result_id(&msg) {
            if consumed_results.contains(tool_use_id_val) {
                continue;
            }
        }
        if is_api_error_message(&msg) {
            if result.last().is_some_and(is_api_error_message) {
                result.pop();
            }
            result.push(msg);
            continue;
        }
        result.push(msg);
    }

    let last_idx = result.len().saturating_sub(1);
    result
        .into_iter()
        .enumerate()
        .filter_map(|(idx, msg)| {
            if is_api_error_message(&msg) && idx != last_idx {
                None
            } else {
                Some(msg)
            }
        })
        .collect()
}

pub(crate) fn filter_brief_messages(
    messages: Vec<RenderableMessage>,
    _options: MessageRenderOptions,
) -> Vec<RenderableMessage> {
    messages
}

pub(crate) fn truncate_transcript_messages(
    messages: Vec<RenderableMessage>,
    options: MessageRenderOptions,
) -> Vec<RenderableMessage> {
    const MAX_MESSAGES_TO_SHOW_IN_TRANSCRIPT_MODE: usize = 30;
    if options.is_transcript_mode
        && !options.show_all_in_transcript
        && messages.len() > MAX_MESSAGES_TO_SHOW_IN_TRANSCRIPT_MODE
    {
        messages[messages.len() - MAX_MESSAGES_TO_SHOW_IN_TRANSCRIPT_MODE..].to_vec()
    } else {
        messages
    }
}

pub(crate) fn message_content_blocks(content: &MessageContent) -> Vec<&ContentBlock> {
    match content {
        MessageContent::Text(_) => Vec::new(),
        MessageContent::Blocks(blocks) => blocks.iter().collect(),
    }
}

pub(crate) fn find_latest_bash_output_uuid(messages: &[RenderableMessage]) -> Option<uuid::Uuid> {
    messages.iter().rev().find_map(|msg| {
        let RenderableMessage::Message {
            message: Message::User(user),
            ..
        } = msg
        else {
            return None;
        };
        message_content_blocks(&user.content)
            .iter()
            .find_map(|block| {
                let ContentBlock::Text { text } = block else {
                    return None;
                };
                (text.starts_with("<bash-stdout") || text.starts_with("<bash-stderr"))
                    .then_some(user.uuid)
            })
    })
}

pub(crate) fn find_last_thinking_block_id(messages: &[RenderableMessage]) -> Option<String> {
    messages.iter().rev().find_map(|msg| {
        let RenderableMessage::Message {
            message: Message::Assistant(assistant),
            ..
        } = msg
        else {
            return None;
        };
        assistant
            .content
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, block)| {
                matches!(
                    block,
                    ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
                )
                .then(|| format!("{}:{idx}", assistant.uuid))
            })
    })
}
