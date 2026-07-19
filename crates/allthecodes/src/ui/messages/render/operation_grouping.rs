//! Operation-based batch grouping for the render pipeline.
//!
//! Replaces the old `grouping.rs` (`apply_grouping` + `collapse_read_search_groups`)
//! with semantic operation batching driven by `ToolClassifier`.
//!
//! This module:
//! - Groups consecutive same-kind/subtype operations into batches
//! - Routes TodoWrite operations to a separate TodoList variant
//! - Suppresses tool results consumed by batches
//! - Skips all batching when `verbose` mode is active

use std::collections::{HashMap, HashSet};

use allthecodes_tool_display::{OperationKind, OperationSubtype, ToolOperation};
use allthecodes_types::message::{ContentBlock, Message, SystemSubtype};

use super::preprocessing::message_content_blocks;
use crate::ui::messages::render::context::{
    MessageRenderOptions, RenderableMessage, TodoListRenderRecord, ToolOperationBatchRenderRecord,
};

/// Apply operation-based grouping to the renderable message stream.
///
/// When `verbose`, messages pass through unchanged.
/// Otherwise, ToolUse blocks are classified and batched by OperationKind and subtype.
pub(crate) fn group_by_operation(
    messages: Vec<RenderableMessage>,
    options: MessageRenderOptions,
    tool_operations: &HashMap<String, ToolOperation>,
) -> Vec<RenderableMessage> {
    let messages = suppress_task_mutation_messages(messages, tool_operations);
    if options.verbose {
        return messages;
    }

    let mut result: Vec<RenderableMessage> = Vec::new();
    let mut i = 0;
    let mut consumed_tool_ids: HashSet<String> = HashSet::new();

    while i < messages.len() {
        // Check if this message is a ToolUse that we can classify
        if let Some((_tool_use_id, op)) = classify_message(&messages[i], tool_operations) {
            // Is it TodoWrite? Route to TodoList variant.
            if op.kind == OperationKind::Status && op.subtype == Some(OperationSubtype::Todo) {
                let (todo_batch, consumed_ids, count) =
                    collect_todo_batch(&messages, i, tool_operations);
                for id in &consumed_ids {
                    consumed_tool_ids.insert(id.clone());
                }
                result.push(todo_batch);
                i += count;
                continue;
            }

            // Otherwise, batch consecutive same-kind operations
            let (batch, consumed_ids, count) =
                collect_operation_batch(&messages, i, tool_operations);
            for id in &consumed_ids {
                consumed_tool_ids.insert(id.clone());
            }
            result.push(batch);
            i += count;
        } else {
            // Check if this is a tool result belonging to a consumed tool
            if let Some(consumed_id) = tool_result_id_for_message(&messages[i]) {
                if consumed_tool_ids.contains(consumed_id) {
                    i += 1;
                    continue;
                }
            }
            result.push(messages[i].clone());
            i += 1;
        }
    }

    result
}

fn suppress_task_mutation_messages(
    messages: Vec<RenderableMessage>,
    tool_operations: &HashMap<String, ToolOperation>,
) -> Vec<RenderableMessage> {
    let task_tool_ids = messages
        .iter()
        .filter_map(|message| classify_message(message, tool_operations))
        .filter_map(|(id, operation)| is_task_mutation_tool(&operation.raw_tool_name).then_some(id))
        .collect::<HashSet<_>>();

    messages
        .into_iter()
        .filter(|message| {
            if classify_message(message, tool_operations)
                .is_some_and(|(id, _)| task_tool_ids.contains(&id))
            {
                return false;
            }
            !tool_result_id_for_message(message).is_some_and(|id| task_tool_ids.contains(id))
        })
        .collect()
}

fn is_task_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "TaskCreate" | "TaskUpdate" | "task_create" | "task_update"
    )
}

/// Try to classify a RenderableMessage as a ToolUse with a known ToolOperation.
/// Returns Some((tool_use_id, ToolOperation)) or None.
fn classify_message<'a>(
    msg: &RenderableMessage,
    tool_operations: &'a HashMap<String, ToolOperation>,
) -> Option<(String, &'a ToolOperation)> {
    let RenderableMessage::Message {
        message: Message::Assistant(assistant),
        ..
    } = msg
    else {
        return None;
    };
    let (id, _name) = match assistant.content.first()? {
        ContentBlock::ToolUse { id, name, .. } | ContentBlock::ServerToolUse { id, name, .. } => {
            (id.as_str(), name.as_str())
        }
        _ => return None,
    };
    let op = tool_operations.get(id)?;
    Some((id.to_string(), op))
}

/// Collect consecutive operations of the same kind and subtype into a ToolOperationBatch.
///
/// Returns (batch_record, consumed_tool_use_ids, consumed_count).
/// consumed_count is the number of RenderableMessage items consumed from the input.
fn collect_operation_batch(
    messages: &[RenderableMessage],
    start: usize,
    tool_operations: &HashMap<String, ToolOperation>,
) -> (RenderableMessage, Vec<String>, usize) {
    let mut ops: Vec<ToolOperation> = Vec::new();
    let mut consumed_ids: Vec<String> = Vec::new();
    let mut source_indices: HashSet<usize> = HashSet::new();
    let mut i = start;
    let batch_kind;
    let batch_subtype;

    // Collect first operation
    if let Some((id, op)) = classify_message(&messages[i], tool_operations) {
        batch_kind = op.kind;
        batch_subtype = op.subtype;
        ops.push(op.clone());
        consumed_ids.push(id);
        if let Some(idx) = source_index_of(&messages[i]) {
            source_indices.insert(idx);
        }
        i += 1;
    } else {
        // Shouldn't happen if called correctly, but handle gracefully
        return (
            RenderableMessage::ToolOperationBatch(ToolOperationBatchRenderRecord {
                uuid: uuid::Uuid::nil(),
                timestamp: 0,
                source_indices: vec![0],
                operations: Vec::new(),
                is_batch: false,
            }),
            Vec::new(),
            1,
        );
    }

    // Collect consecutive messages with the same OperationKind and subtype
    while i < messages.len() {
        // If it's a tool result for a consumed tool, skip it (it's part of the batch)
        if let Some(result_id) = tool_result_id_for_message(&messages[i]) {
            if consumed_ids.iter().any(|id| id == result_id) {
                if let Some(idx) = source_index_of(&messages[i]) {
                    source_indices.insert(idx);
                }
                i += 1;
                continue;
            }
        }

        // Check if next message is a ToolUse with same kind
        if let Some((id, op)) = classify_message(&messages[i], tool_operations) {
            if op.kind == batch_kind && op.subtype == batch_subtype {
                ops.push(op.clone());
                consumed_ids.push(id);
                if let Some(idx) = source_index_of(&messages[i]) {
                    source_indices.insert(idx);
                }
                i += 1;
                // Skip immediately following tool results for this tool
                while i < messages.len() {
                    if let Some(result_id) = tool_result_id_for_message(&messages[i]) {
                        if consumed_ids.iter().any(|cid| cid == result_id) {
                            if let Some(idx) = source_index_of(&messages[i]) {
                                source_indices.insert(idx);
                            }
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                continue;
            }
        }
        break;
    }

    let count = ops.len();
    let is_batch = count >= 2;
    let mut source_indices = source_indices.into_iter().collect::<Vec<_>>();
    source_indices.sort_unstable();
    let parent_uuid = messages[start].uuid();
    let timestamp = messages[start].timestamp();

    let batch = RenderableMessage::ToolOperationBatch(ToolOperationBatchRenderRecord {
        uuid: uuid_from_parent_and_salt(parent_uuid, "operation-batch"),
        timestamp,
        source_indices,
        operations: ops,
        is_batch,
    });

    (batch, consumed_ids, i - start)
}

/// Collect consecutive TodoWrite operations into a TodoList.
///
/// Returns (todo_list_record, consumed_count).
fn collect_todo_batch(
    messages: &[RenderableMessage],
    start: usize,
    tool_operations: &HashMap<String, ToolOperation>,
) -> (RenderableMessage, Vec<String>, usize) {
    let mut ops: Vec<ToolOperation> = Vec::new();
    let mut consumed_ids: Vec<String> = Vec::new();
    let mut source_indices: HashSet<usize> = HashSet::new();
    let mut i = start;

    while i < messages.len() {
        if let Some((id, op)) = classify_message(&messages[i], tool_operations) {
            if op.kind == OperationKind::Status && op.subtype == Some(OperationSubtype::Todo) {
                ops.push(op.clone());
                consumed_ids.push(id);
                if let Some(idx) = source_index_of(&messages[i]) {
                    source_indices.insert(idx);
                }
                i += 1;
                // Skip tool results
                while i < messages.len() {
                    if let Some(result_id) = tool_result_id_for_message(&messages[i]) {
                        if consumed_ids.iter().any(|cid| cid == result_id) {
                            if let Some(idx) = source_index_of(&messages[i]) {
                                source_indices.insert(idx);
                            }
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                continue;
            }
        }
        break;
    }

    let mut source_indices = source_indices.into_iter().collect::<Vec<_>>();
    source_indices.sort_unstable();
    let parent_uuid = messages[start].uuid();
    let timestamp = messages[start].timestamp();
    let todo = RenderableMessage::TodoList(TodoListRenderRecord {
        uuid: uuid_from_parent_and_salt(parent_uuid, "todo-list"),
        timestamp,
        source_indices,
        operations: ops,
    });

    (todo, consumed_ids, i - start)
}

/// Extract the tool_use_id from a message that is a ToolResult.
fn tool_result_id_for_message(msg: &RenderableMessage) -> Option<&str> {
    let RenderableMessage::Message {
        message: Message::User(user),
        ..
    } = msg
    else {
        return None;
    };
    message_content_blocks(&user.content)
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
}

// ── Helpers moved from old grouping.rs ───────────────────────────────────

pub(crate) fn tool_use_id(msg: &RenderableMessage) -> Option<&str> {
    let RenderableMessage::Message {
        message: Message::Assistant(assistant),
        ..
    } = msg
    else {
        return None;
    };
    match assistant.content.first()? {
        ContentBlock::ToolUse { id, .. } | ContentBlock::ServerToolUse { id, .. } => Some(id),
        _ => None,
    }
}

pub(crate) fn tool_result_id(msg: &RenderableMessage) -> Option<&str> {
    let RenderableMessage::Message {
        message: Message::User(user),
        ..
    } = msg
    else {
        return None;
    };
    message_content_blocks(&user.content)
        .iter()
        .find_map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
}

pub(crate) fn source_index_of(msg: &RenderableMessage) -> Option<usize> {
    match msg {
        RenderableMessage::Message { source_index, .. } => Some(*source_index),
        RenderableMessage::ToolOperationBatch(batch) => batch.source_indices.first().copied(),
        RenderableMessage::TodoList(todo) => todo.source_indices.first().copied(),
    }
}

pub(crate) fn uuid_from_parent_and_salt(parent: uuid::Uuid, salt: &str) -> uuid::Uuid {
    let mut bytes = *parent.as_bytes();
    for (idx, byte) in salt.as_bytes().iter().enumerate() {
        bytes[idx % bytes.len()] ^= *byte;
    }
    uuid::Uuid::from_bytes(bytes)
}

pub(crate) fn is_api_error_message(msg: &RenderableMessage) -> bool {
    matches!(
        msg,
        RenderableMessage::Message {
            message: Message::System(allthecodes_types::message::SystemMessage {
                subtype: SystemSubtype::ApiError { .. },
                ..
            }),
            ..
        }
    )
}

/// Check whether a tool name should be suppressed from individual rendering
/// when the operation pipeline is active.
pub(crate) fn is_suppressed_operation_tool(name: &str) -> bool {
    matches!(
        name,
        "Bash"
            | "PowerShell"
            | "Read"
            | "Grep"
            | "Glob"
            | "WebSearch"
            | "WebFetch"
            | "Edit"
            | "Write"
            | "FileEdit"
            | "FileWrite"
            | "NotebookEdit"
            | "MultiEdit"
            | "TodoWrite"
            | "Task"
            | "Agent"
            | "SubAgent"
            | "LS"
            | "List"
            | "Permission"
    ) || name.contains("__") // MCP tools
}

/// Check whether a tool name is consumed by the operation pipeline (for
/// suppressing tool results).
pub(crate) fn is_suppressed_tool_result(name: &str) -> bool {
    matches!(
        name,
        "Read"
            | "Grep"
            | "Glob"
            | "WebSearch"
            | "WebFetch"
            | "Edit"
            | "Write"
            | "FileEdit"
            | "FileWrite"
            | "NotebookEdit"
            | "MultiEdit"
            | "LS"
            | "List"
    )
}
