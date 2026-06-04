use std::collections::{HashMap, HashSet};

use allthecodes_types::message::{ContentBlock, Message, SystemSubtype};

use super::context::{
    CollapsedReadSearchRenderRecord, GroupedToolUseRenderRecord, MessageRenderOptions,
    RenderableMessage,
};
use super::copy_text::tool_primary_input;
use super::preprocessing::message_content_blocks;

pub(crate) fn apply_grouping(
    messages: Vec<RenderableMessage>,
    options: MessageRenderOptions,
) -> Vec<RenderableMessage> {
    if options.verbose {
        return messages;
    }

    let mut groups: HashMap<(usize, String), Vec<RenderableMessage>> = HashMap::new();
    for msg in &messages {
        if let Some((source_index, name)) = grouping_tool_use_key(msg) {
            groups
                .entry((source_index, name.to_string()))
                .or_default()
                .push(msg.clone());
        }
    }
    let valid = groups
        .into_iter()
        .filter(|(_, group)| group.len() >= 2)
        .collect::<HashMap<_, _>>();

    if valid.is_empty() {
        return messages;
    }

    let mut emitted = HashSet::<(usize, String)>::new();
    let mut grouped_tool_ids = HashSet::<String>::new();
    for group in valid.values() {
        for msg in group {
            if let Some(id) = tool_use_id(msg) {
                grouped_tool_ids.insert(id.to_string());
            }
        }
    }

    let mut result = Vec::new();
    for msg in messages {
        if let Some((source_index, name)) = grouping_tool_use_key(&msg) {
            let key = (source_index, name.to_string());
            if let Some(group) = valid.get(&key) {
                if emitted.insert(key.clone()) {
                    let tool_use_ids = group
                        .iter()
                        .filter_map(|msg| tool_use_id(msg).map(str::to_string))
                        .collect::<Vec<_>>();
                    result.push(RenderableMessage::GroupedToolUse(
                        GroupedToolUseRenderRecord {
                            uuid: derive_group_uuid(group[0].uuid(), "grouped"),
                            timestamp: group[0].timestamp(),
                            source_indices: group
                                .iter()
                                .filter_map(source_index_of)
                                .collect::<HashSet<_>>()
                                .into_iter()
                                .collect(),
                            tool_name: name.to_string(),
                            tool_use_ids,
                        },
                    ));
                }
                continue;
            }
        }
        if let Some(tool_use_id_val) = tool_result_id(&msg) {
            if grouped_tool_ids.contains(tool_use_id_val) {
                continue;
            }
        }
        result.push(msg);
    }

    result
}

pub(crate) fn collapse_read_search_groups(
    messages: Vec<RenderableMessage>,
    options: MessageRenderOptions,
) -> Vec<RenderableMessage> {
    if options.verbose {
        return messages;
    }

    let mut result = Vec::new();
    let mut i = 0usize;
    while i < messages.len() {
        let Some(first_info) = collapsible_tool_info(&messages[i]) else {
            result.push(messages[i].clone());
            i += 1;
            continue;
        };

        let mut originals = vec![messages[i].clone()];
        let mut source_indices = HashSet::new();
        let mut tool_use_ids = Vec::new();
        let mut read_count = 0usize;
        let mut search_count = 0usize;
        let mut list_count = 0usize;
        let mut latest_hint = None;
        add_collapsible_info(
            first_info,
            &messages[i],
            &mut source_indices,
            &mut tool_use_ids,
            &mut read_count,
            &mut search_count,
            &mut list_count,
            &mut latest_hint,
        );
        i += 1;

        while i < messages.len() {
            if let Some(result_id) = tool_result_id(&messages[i]) {
                if tool_use_ids.iter().any(|id| id == result_id) {
                    originals.push(messages[i].clone());
                    if let Some(source_index) = source_index_of(&messages[i]) {
                        source_indices.insert(source_index);
                    }
                    i += 1;
                    continue;
                }
            }
            let Some(info) = collapsible_tool_info(&messages[i]) else {
                break;
            };
            add_collapsible_info(
                info,
                &messages[i],
                &mut source_indices,
                &mut tool_use_ids,
                &mut read_count,
                &mut search_count,
                &mut list_count,
                &mut latest_hint,
            );
            originals.push(messages[i].clone());
            i += 1;
        }

        let tool_count = read_count + search_count + list_count;
        if tool_count >= 2 {
            result.push(RenderableMessage::CollapsedReadSearch(
                CollapsedReadSearchRenderRecord {
                    uuid: derive_group_uuid(originals[0].uuid(), "collapsed"),
                    timestamp: originals[0].timestamp(),
                    source_indices: source_indices.into_iter().collect(),
                    tool_use_ids,
                    read_count,
                    search_count,
                    list_count,
                    latest_hint,
                },
            ));
        } else {
            result.extend(originals);
        }
    }
    result
}

#[derive(Debug, Clone)]
pub(crate) struct CollapsibleToolInfo {
    tool_use_ids: Vec<String>,
    read_count: usize,
    search_count: usize,
    list_count: usize,
    hint: Option<String>,
}

fn add_collapsible_info(
    info: CollapsibleToolInfo,
    msg: &RenderableMessage,
    source_indices: &mut HashSet<usize>,
    tool_use_ids: &mut Vec<String>,
    read_count: &mut usize,
    search_count: &mut usize,
    list_count: &mut usize,
    latest_hint: &mut Option<String>,
) {
    if let Some(source_index) = source_index_of(msg) {
        source_indices.insert(source_index);
    }
    tool_use_ids.extend(info.tool_use_ids);
    *read_count += info.read_count;
    *search_count += info.search_count;
    *list_count += info.list_count;
    if info.hint.is_some() {
        *latest_hint = info.hint;
    }
}

fn collapsible_tool_info(msg: &RenderableMessage) -> Option<CollapsibleToolInfo> {
    match msg {
        RenderableMessage::Message {
            message: Message::Assistant(assistant),
            ..
        } => {
            let block = assistant.content.first()?;
            let (ContentBlock::ToolUse { id, name, input }
            | ContentBlock::ServerToolUse { id, name, input }) = block
            else {
                return None;
            };
            collapsible_info_for_tool(id, name, input)
        }
        RenderableMessage::GroupedToolUse(group) => {
            let kind = collapsible_kind_for_tool(&group.tool_name)?;
            let (read_count, search_count, list_count) =
                counts_for_kind(kind, group.tool_use_ids.len());
            Some(CollapsibleToolInfo {
                tool_use_ids: group.tool_use_ids.clone(),
                read_count,
                search_count,
                list_count,
                hint: Some(group.tool_name.clone()),
            })
        }
        _ => None,
    }
}

fn collapsible_info_for_tool(
    id: &str,
    name: &str,
    input: &serde_json::Value,
) -> Option<CollapsibleToolInfo> {
    let kind = collapsible_kind_for_tool(name)?;
    let (read_count, search_count, list_count) = counts_for_kind(kind, 1);
    Some(CollapsibleToolInfo {
        tool_use_ids: vec![id.to_string()],
        read_count,
        search_count,
        list_count,
        hint: tool_primary_input(name, input),
    })
}

#[derive(Debug, Clone, Copy)]
enum CollapsibleKind {
    Read,
    Search,
    List,
}

fn collapsible_kind_for_tool(name: &str) -> Option<CollapsibleKind> {
    match name {
        "Read" => Some(CollapsibleKind::Read),
        "Grep" | "Glob" | "WebSearch" => Some(CollapsibleKind::Search),
        "LS" | "List" => Some(CollapsibleKind::List),
        _ => None,
    }
}

fn counts_for_kind(kind: CollapsibleKind, count: usize) -> (usize, usize, usize) {
    match kind {
        CollapsibleKind::Read => (count, 0, 0),
        CollapsibleKind::Search => (0, count, 0),
        CollapsibleKind::List => (0, 0, count),
    }
}

pub(crate) fn grouping_tool_use_key(msg: &RenderableMessage) -> Option<(usize, &str)> {
    let RenderableMessage::Message {
        message: Message::Assistant(assistant),
        source_index,
    } = msg
    else {
        return None;
    };
    let (ContentBlock::ToolUse { name, .. } | ContentBlock::ServerToolUse { name, .. }) =
        assistant.content.first()?
    else {
        return None;
    };
    if is_groupable_tool(name) {
        Some((*source_index, name.as_str()))
    } else {
        None
    }
}

pub(crate) fn is_groupable_tool(name: &str) -> bool {
    matches!(
        name,
        "Task"
            | "Agent"
            | "Read"
            | "Grep"
            | "Glob"
            | "Bash"
            | "PowerShell"
            | "Edit"
            | "Write"
            | "FileEdit"
            | "FileWrite"
            | "MultiEdit"
            | "NotebookEdit"
    )
}

pub(crate) fn source_index_of(msg: &RenderableMessage) -> Option<usize> {
    match msg {
        RenderableMessage::Message { source_index, .. } => Some(*source_index),
        RenderableMessage::GroupedToolUse(group) => group.source_indices.first().copied(),
        RenderableMessage::CollapsedReadSearch(group) => group.source_indices.first().copied(),
    }
}

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

pub(crate) fn derive_group_uuid(parent: uuid::Uuid, salt: &str) -> uuid::Uuid {
    uuid_from_parent_and_salt(parent, salt)
}

pub(crate) fn uuid_from_parent_and_salt(parent: uuid::Uuid, salt: &str) -> uuid::Uuid {
    let mut bytes = *parent.as_bytes();
    for (idx, byte) in salt.as_bytes().iter().enumerate() {
        bytes[idx % bytes.len()] ^= *byte;
    }
    uuid::Uuid::from_bytes(bytes)
}
