use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::{build_session_info, get_session_dir, workspace_key, SerializableMessage, SessionFile};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const SNIPPET_CHARS: usize = 240;
const JSON_SUMMARY_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSearchResult {
    pub session_id: String,
    pub message_index: usize,
    pub role: Option<String>,
    pub msg_type: String,
    pub timestamp: i64,
    pub title: String,
    pub cwd: String,
    pub workspace_key: String,
    pub workspace_name: String,
    pub snippet: String,
}

pub(super) fn clamp_search_limit(limit: usize) -> usize {
    if limit == 0 {
        DEFAULT_LIMIT
    } else {
        limit.min(MAX_LIMIT)
    }
}

pub(super) fn searchable_message_text(message: &SerializableMessage) -> String {
    let mut parts = Vec::new();
    match message.msg_type.as_str() {
        "user" => append_content_text(message.data.get("content"), &mut parts),
        "assistant" => append_assistant_content_text(message.data.get("content"), &mut parts),
        "system" => append_string_field(&message.data, "content", &mut parts),
        "progress" | "attachment" => parts.push(json_summary(&message.data)),
        _ => parts.push(json_summary(&message.data)),
    }
    normalize_text(parts)
}

pub(super) fn make_snippet(text: &str, query: &str) -> String {
    let text = normalize_whitespace(text);
    if text.chars().count() <= SNIPPET_CHARS {
        return text;
    }

    let query_len = query.trim().chars().count().max(1);
    let match_pos = find_case_insensitive_char_pos(&text, query.trim()).unwrap_or(0);
    let context_chars = SNIPPET_CHARS.saturating_sub(query_len) / 2;
    let start = match_pos.saturating_sub(context_chars);
    let end = start
        .saturating_add(SNIPPET_CHARS)
        .min(text.chars().count());

    let mut snippet = String::new();
    if start > 0 {
        snippet.push_str("...");
    }
    snippet.extend(text.chars().skip(start).take(end - start));
    if end < text.chars().count() {
        snippet.push_str("...");
    }
    snippet
}

pub(super) fn json_search_results(
    query: &str,
    limit: usize,
    workspace_key_filter: Option<&str>,
    excluded_ids: &HashSet<String>,
) -> Result<Vec<SessionSearchResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let limit = clamp_search_limit(limit);
    let query_lower = query.to_lowercase();
    let mut hits = Vec::new();
    let dir = get_session_dir();
    if !dir.exists() {
        return Ok(hits);
    }

    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read session directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(_) => continue,
        };
        let file: SessionFile = match serde_json::from_str(&contents) {
            Ok(file) => file,
            Err(_) => continue,
        };
        if excluded_ids.contains(&file.session_id) {
            continue;
        }

        let info = build_session_info(file.clone());
        if workspace_key_filter.is_some_and(|expected| {
            let actual = if info.workspace_key.is_empty() {
                workspace_key(std::path::Path::new(&file.cwd))
            } else {
                info.workspace_key.clone()
            };
            actual != expected
        }) {
            continue;
        }

        for (position, message) in file.messages.iter().enumerate() {
            let text = searchable_message_text(message);
            if text.to_lowercase().contains(&query_lower) {
                hits.push(SessionSearchResult {
                    session_id: file.session_id.clone(),
                    message_index: position,
                    role: role_for_message(message).map(ToString::to_string),
                    msg_type: message.msg_type.clone(),
                    timestamp: message.timestamp,
                    title: info.title.clone(),
                    cwd: info.cwd.clone(),
                    workspace_key: info.workspace_key.clone(),
                    workspace_name: info.workspace_name.clone(),
                    snippet: make_snippet(&text, query),
                });
                if hits.len() >= limit {
                    return Ok(hits);
                }
            }
        }
    }

    hits.sort_by(|a, b| {
        b.timestamp
            .cmp(&a.timestamp)
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.message_index.cmp(&b.message_index))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn append_content_text(value: Option<&serde_json::Value>, parts: &mut Vec<String>) {
    match value {
        Some(serde_json::Value::String(text)) => parts.push(text.clone()),
        Some(serde_json::Value::Array(blocks)) => {
            for block in blocks {
                append_block_text(block, parts);
            }
        }
        Some(value) => parts.push(json_summary(value)),
        None => {}
    }
}

fn append_assistant_content_text(value: Option<&serde_json::Value>, parts: &mut Vec<String>) {
    match value {
        Some(serde_json::Value::Array(blocks)) => {
            for block in blocks {
                let block_type = block
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                match block_type {
                    "text" => append_string_field(block, "text", parts),
                    "tool_use" => {
                        append_string_field(block, "name", parts);
                        if let Some(input) = block.get("input") {
                            parts.push(json_summary(input));
                        }
                    }
                    _ => append_block_text(block, parts),
                }
            }
        }
        Some(value) => append_content_text(Some(value), parts),
        None => {}
    }
}

fn append_block_text(block: &serde_json::Value, parts: &mut Vec<String>) {
    let block_type = block
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    match block_type {
        "text" => append_string_field(block, "text", parts),
        "tool_result" => {
            append_string_field(block, "tool_use_id", parts);
            append_tool_result_content(block.get("content"), parts);
        }
        "tool_use" => {
            append_string_field(block, "name", parts);
            if let Some(input) = block.get("input") {
                parts.push(json_summary(input));
            }
        }
        _ => parts.push(json_summary(block)),
    }
}

fn append_tool_result_content(value: Option<&serde_json::Value>, parts: &mut Vec<String>) {
    match value {
        Some(serde_json::Value::String(text)) => parts.push(text.clone()),
        Some(serde_json::Value::Array(blocks)) => {
            for block in blocks {
                append_block_text(block, parts);
            }
        }
        Some(value) => parts.push(json_summary(value)),
        None => {}
    }
}

fn append_string_field(value: &serde_json::Value, field: &str, parts: &mut Vec<String>) {
    if let Some(text) = value.get(field).and_then(|value| value.as_str()) {
        parts.push(text.to_string());
    }
}

fn role_for_message(message: &SerializableMessage) -> Option<&str> {
    match message.msg_type.as_str() {
        "user" => Some("user"),
        "assistant" => Some("assistant"),
        "system" => Some("system"),
        _ => None,
    }
}

fn normalize_text(parts: Vec<String>) -> String {
    normalize_whitespace(
        &parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn json_summary(value: &serde_json::Value) -> String {
    let raw = value.to_string();
    raw.chars().take(JSON_SUMMARY_CHARS).collect()
}

fn find_case_insensitive_char_pos(text: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_query = query.to_ascii_lowercase();
    let byte_pos = lower_text.find(&lower_query)?;
    Some(text[..byte_pos.min(text.len())].chars().count())
}
