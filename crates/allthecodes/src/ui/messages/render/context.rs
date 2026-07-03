use std::collections::{HashMap, HashSet};

use allthecodes_tool_display::{OperationStatus, ToolClassifier, ToolOperation};
use allthecodes_types::message::{ContentBlock, Message};

use super::operation_grouping::group_by_operation;
use super::preprocessing::{
    filter_brief_messages, filter_compact_boundary, find_last_thinking_block_id,
    find_latest_bash_output_uuid, is_not_empty_renderable_message, message_content_blocks,
    normalize_messages_for_render, reorder_messages_in_ui, should_show_renderable_message,
    truncate_transcript_messages,
};
use super::render_user::tool_result_content_text;

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageRenderContext {
    pub(crate) renderable_messages: Vec<RenderableMessage>,
    pub(crate) lookups: MessageLookups,
    pub(crate) selected_message: Option<usize>,
    pub(crate) selected_expanded: bool,
    pub(crate) options: MessageRenderOptions,
    cache_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MessageRenderOptions {
    pub(crate) verbose: bool,
    pub(crate) is_transcript_mode: bool,
    pub(crate) show_all_in_transcript: bool,
    pub(crate) thinking_animation_frame: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedMessages {
    pub(crate) renderable: Vec<RenderableMessage>,
    pub(crate) lookups: MessageLookups,
}

/// A batch of classified tool operations in a single assistant turn.
#[derive(Debug, Clone)]
pub(crate) struct ToolOperationBatchRenderRecord {
    pub(crate) uuid: uuid::Uuid,
    pub(crate) timestamp: i64,
    pub(crate) source_indices: Vec<usize>,
    /// The classified operations in this batch.
    pub(crate) operations: Vec<ToolOperation>,
    /// Whether this batch represents multiple operations (vs a singleton).
    pub(crate) is_batch: bool,
}

/// A rendered todo list produced from TodoWrite tool operations.
#[derive(Debug, Clone)]
pub(crate) struct TodoListRenderRecord {
    pub(crate) uuid: uuid::Uuid,
    pub(crate) timestamp: i64,
    pub(crate) source_indices: Vec<usize>,
    pub(crate) operations: Vec<ToolOperation>,
}

#[derive(Debug, Clone)]
pub(crate) enum RenderableMessage {
    Message {
        message: Message,
        source_index: usize,
    },
    ToolOperationBatch(ToolOperationBatchRenderRecord),
    TodoList(TodoListRenderRecord),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageLookups {
    pub(crate) tool_uses: HashMap<String, ToolUseRenderRecord>,
    pub(crate) tool_results: HashMap<String, ToolResultRenderRecord>,
    pub(crate) resolved_tool_use_ids: HashSet<String>,
    pub(crate) errored_tool_use_ids: HashSet<String>,
    pub(crate) in_progress_tool_use_ids: HashSet<String>,
    pub(crate) progress_messages_by_tool_use_id:
        HashMap<String, Vec<allthecodes_types::message::ProgressMessage>>,
    pub(crate) latest_shell_tool_result_id: Option<String>,
    pub(crate) latest_bash_output_uuid: Option<uuid::Uuid>,
    pub(crate) last_thinking_block_id: Option<String>,
    /// Classified ToolOperations keyed by tool_use_id.
    pub(crate) tool_operations: HashMap<String, ToolOperation>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolUseRenderRecord {
    pub(crate) tool_name: String,
    pub(crate) input: serde_json::Value,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolResultRenderRecord {
    pub(crate) tool_use_id: String,
    pub(crate) content: String,
    pub(crate) is_error: bool,
    pub(crate) tool_use_result: Option<String>,
    pub(crate) message_uuid: uuid::Uuid,
    pub(crate) source_index: usize,
}

impl MessageRenderContext {
    pub(crate) fn cache_key(&self) -> &str {
        &self.cache_key
    }

    pub(crate) fn renderable_messages(&self) -> &[RenderableMessage] {
        &self.renderable_messages
    }

    #[cfg(test)]
    pub(crate) fn selected_message(&self) -> Option<usize> {
        self.selected_message
    }

    #[cfg(test)]
    pub(crate) fn selected_expanded(&self) -> bool {
        self.selected_expanded
    }

    pub(crate) fn tool_use(&self, tool_use_id: &str) -> Option<&ToolUseRenderRecord> {
        self.lookups.tool_uses.get(tool_use_id)
    }

    pub(crate) fn shell_expanded(&self, msg_index: usize, tool_use_id: &str) -> bool {
        self.lookups.latest_shell_tool_result_id.as_deref() == Some(tool_use_id)
            || (self.selected_message == Some(msg_index) && self.selected_expanded)
    }
}

#[cfg(test)]
pub(crate) fn build_message_render_context(
    messages: &[Message],
    selected_message: Option<usize>,
    selected_expanded: bool,
) -> MessageRenderContext {
    build_message_render_context_with_options(
        messages,
        selected_message,
        selected_expanded,
        MessageRenderOptions::default(),
    )
}

pub(crate) fn build_message_render_context_with_options(
    messages: &[Message],
    selected_message: Option<usize>,
    selected_expanded: bool,
    options: MessageRenderOptions,
) -> MessageRenderContext {
    let prepared = prepare_renderable_messages(messages, options);
    let mut ctx = MessageRenderContext {
        renderable_messages: prepared.renderable,
        lookups: prepared.lookups,
        selected_message,
        selected_expanded,
        options,
        ..MessageRenderContext::default()
    };

    ctx.cache_key = render_context_cache_key(&ctx);
    ctx
}

pub(crate) fn prepare_renderable_messages(
    messages: &[Message],
    options: MessageRenderOptions,
) -> PreparedMessages {
    let normalized = normalize_messages_for_render(messages)
        .into_iter()
        .filter(is_not_empty_renderable_message)
        .collect::<Vec<_>>();
    let compact_aware = filter_compact_boundary(normalized.clone(), options);
    let visible = compact_aware
        .into_iter()
        .filter(|msg| should_show_renderable_message(msg, options))
        .collect::<Vec<_>>();
    let reordered = reorder_messages_in_ui(visible);
    let brief_filtered = filter_brief_messages(reordered, options);
    let transcript_limited = truncate_transcript_messages(brief_filtered, options);
    let lookups = build_message_lookups(&normalized, &[]);
    let tool_operations = lookups.tool_operations.clone();
    let grouped = group_by_operation(transcript_limited, options, &tool_operations);

    PreparedMessages {
        renderable: grouped,
        lookups,
    }
}

pub(crate) fn build_message_lookups(
    normalized: &[RenderableMessage],
    _renderable: &[RenderableMessage],
) -> MessageLookups {
    let mut lookups = MessageLookups {
        latest_bash_output_uuid: find_latest_bash_output_uuid(normalized),
        last_thinking_block_id: find_last_thinking_block_id(normalized),
        ..MessageLookups::default()
    };

    for render_msg in normalized {
        for (message, source_index) in render_msg.messages_for_lookup() {
            match message {
                Message::Assistant(assistant) => {
                    for block in &assistant.content {
                        match block {
                            ContentBlock::ToolUse { id, name, input }
                            | ContentBlock::ServerToolUse { id, name, input } => {
                                lookups.tool_uses.insert(
                                    id.clone(),
                                    ToolUseRenderRecord {
                                        tool_name: name.clone(),
                                        input: input.clone(),
                                    },
                                );
                            }
                            _ => {}
                        }
                    }
                }
                Message::User(user) => {
                    for block in message_content_blocks(&user.content) {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } = block
                        {
                            let record = ToolResultRenderRecord {
                                tool_use_id: tool_use_id.clone(),
                                content: tool_result_content_text(content),
                                is_error: *is_error,
                                tool_use_result: user.tool_use_result.clone(),
                                message_uuid: user.uuid,
                                source_index,
                            };
                            if *is_error {
                                lookups.errored_tool_use_ids.insert(tool_use_id.clone());
                            }
                            lookups.resolved_tool_use_ids.insert(tool_use_id.clone());
                            lookups.tool_results.insert(tool_use_id.clone(), record);
                        }
                    }
                }
                Message::Progress(progress) => {
                    lookups
                        .progress_messages_by_tool_use_id
                        .entry(progress.tool_use_id.clone())
                        .or_default()
                        .push(progress.clone());
                }
                _ => {}
            }
        }
    }

    for (id, tool_use) in &lookups.tool_uses {
        if !lookups.resolved_tool_use_ids.contains(id) {
            lookups.in_progress_tool_use_ids.insert(id.clone());
        }
        if tool_use.is_shell() && lookups.resolved_tool_use_ids.contains(id) {
            lookups.latest_shell_tool_result_id = Some(id.clone());
        }

        // Classify tool uses into operation summaries
        let status = if lookups.errored_tool_use_ids.contains(id) {
            OperationStatus::Error
        } else if lookups.resolved_tool_use_ids.contains(id) {
            OperationStatus::Resolved
        } else {
            OperationStatus::InProgress
        };
        let op = if let Some(result) = lookups.tool_results.get(id) {
            let output = result
                .tool_use_result
                .as_deref()
                .unwrap_or(result.content.as_str());
            ToolClassifier::classify_with_result(
                &tool_use.tool_name,
                &tool_use.input,
                status,
                Some(output),
                result.is_error,
            )
        } else {
            ToolClassifier::classify(&tool_use.tool_name, &tool_use.input, status)
        };
        lookups.tool_operations.insert(id.clone(), op);
    }

    lookups
}

fn render_context_cache_key(ctx: &MessageRenderContext) -> String {
    let mut parts = vec![
        format!(
            "mode=v{}t{}",
            u8::from(ctx.options.verbose),
            u8::from(ctx.options.is_transcript_mode)
        ),
        format!(
            "shell={}",
            ctx.lookups
                .latest_shell_tool_result_id
                .as_deref()
                .unwrap_or("")
        ),
        format!("selected={:?}", ctx.selected_message),
        format!("expanded={}", ctx.selected_expanded),
        format!(
            "thinking={}",
            ctx.lookups.last_thinking_block_id.as_deref().unwrap_or("")
        ),
    ];
    parts.extend(ctx.lookups.tool_results.values().map(|result| {
        format!(
            "r:{}:{}:{}:{}:{}:{}",
            result.tool_use_id,
            result.content.len(),
            result.is_error,
            result.tool_use_result.as_deref().unwrap_or("").len(),
            result.message_uuid,
            result.source_index
        )
    }));
    parts.extend(
        ctx.renderable_messages
            .iter()
            .map(RenderableMessage::cache_part),
    );
    parts.join("|")
}

impl RenderableMessage {
    pub(crate) fn uuid(&self) -> uuid::Uuid {
        match self {
            RenderableMessage::Message { message, .. } => message.uuid(),
            RenderableMessage::ToolOperationBatch(batch) => batch.uuid,
            RenderableMessage::TodoList(todo) => todo.uuid,
        }
    }

    pub(crate) fn timestamp(&self) -> i64 {
        match self {
            RenderableMessage::Message { message, .. } => message.timestamp(),
            RenderableMessage::ToolOperationBatch(batch) => batch.timestamp,
            RenderableMessage::TodoList(todo) => todo.timestamp,
        }
    }

    fn cache_part(&self) -> String {
        match self {
            RenderableMessage::Message { message, .. } => {
                format!("m:{}:{}", message_type_key(message), message.uuid())
            }
            RenderableMessage::ToolOperationBatch(batch) => {
                format!("b:{}:{}", batch.operations.len(), batch.uuid)
            }
            RenderableMessage::TodoList(todo) => {
                format!("t:{}:{}", todo.operations.len(), todo.uuid)
            }
        }
    }

    pub(crate) fn is_assistant_message(&self) -> bool {
        matches!(
            self,
            RenderableMessage::Message {
                message: Message::Assistant(_),
                ..
            }
        )
    }

    pub(crate) fn has_source_index(&self, selected: Option<usize>) -> bool {
        let Some(selected) = selected else {
            return false;
        };
        match self {
            RenderableMessage::Message { source_index, .. } => *source_index == selected,
            RenderableMessage::ToolOperationBatch(batch) => {
                batch.source_indices.contains(&selected)
            }
            RenderableMessage::TodoList(todo) => todo.source_indices.contains(&selected),
        }
    }

    pub(crate) fn messages_for_lookup(&self) -> Vec<(&Message, usize)> {
        match self {
            RenderableMessage::Message {
                message,
                source_index,
            } => vec![(message, *source_index)],
            RenderableMessage::ToolOperationBatch(_) | RenderableMessage::TodoList(_) => Vec::new(),
        }
    }
}

fn message_type_key(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "u",
        Message::Assistant(_) => "a",
        Message::System(_) => "s",
        Message::Progress(_) => "p",
        Message::Attachment(_) => "t",
    }
}

impl ToolUseRenderRecord {
    pub(crate) fn is_shell(&self) -> bool {
        matches!(self.tool_name.as_str(), "Bash" | "PowerShell")
    }

    pub(crate) fn command(&self) -> String {
        self.input
            .get("command")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.tool_name.clone())
    }
}
