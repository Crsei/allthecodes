//! Internal builder functions for session export components.

use std::collections::HashSet;
use std::path::Path;

use allthecodes_bootstrap::PROCESS_STATE;
use allthecodes_compact::auto_compact::get_context_window_size;
use allthecodes_types::message::{ContentBlock, Message, MessageContent, ToolResultContent};
use allthecodes_utils::tokens::estimate_messages_tokens;

use super::{
    format_ts_millis, ApiRequestSnapshot, ApiViewData, ContextSnapshot, SessionMeta, TranscriptData,
};

// ---------------------------------------------------------------------------
// Session metadata
// ---------------------------------------------------------------------------

pub(super) fn build_session_meta(session_id: &str, messages: &[Message], cwd: &str) -> SessionMeta {
    let cwd_path = Path::new(cwd);

    let git_branch = allthecodes_utils::git::current_branch(cwd_path).ok();
    let git_head_sha = allthecodes_utils::git::head_sha(cwd_path).ok();

    let model = PROCESS_STATE
        .read()
        .effective_model()
        .map(|s| s.to_string());

    let project_path = {
        let p = PROCESS_STATE
            .read()
            .project_root
            .to_string_lossy()
            .to_string();
        if p.is_empty() {
            None
        } else {
            Some(p)
        }
    };

    let started_at = messages.first().map(|m| format_ts_millis(m.timestamp()));
    let ended_at = messages.last().map(|m| format_ts_millis(m.timestamp()));

    SessionMeta {
        session_id: session_id.to_string(),
        project_path,
        git_branch,
        git_head_sha,
        model,
        mode: None,
        permission_mode: None,
        custom_title: None,
        tags: Vec::new(),
        started_at,
        ended_at,
    }
}

// ---------------------------------------------------------------------------
// Transcript data
// ---------------------------------------------------------------------------

pub(super) fn build_transcript_data(messages: &[Message]) -> TranscriptData {
    let mut user_count = 0usize;
    let mut assistant_count = 0usize;
    let mut system_count = 0usize;

    let json_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|msg| {
            match msg {
                Message::User(_) => user_count += 1,
                Message::Assistant(_) => assistant_count += 1,
                Message::System(_) => system_count += 1,
                _ => {}
            }
            message_to_json(msg)
        })
        .collect();

    TranscriptData {
        message_count: json_messages.len(),
        messages: json_messages,
        user_message_count: user_count,
        assistant_message_count: assistant_count,
        system_message_count: system_count,
    }
}

// ---------------------------------------------------------------------------
// Context snapshot
// ---------------------------------------------------------------------------

/// Build a context snapshot with token/cost/tool statistics.
pub fn build_context_snapshot(messages: &[Message]) -> ContextSnapshot {
    let estimated_total_tokens = estimate_messages_tokens(messages);

    let model = PROCESS_STATE
        .read()
        .effective_model()
        .map(|s| s.to_string())
        .unwrap_or_else(allthecodes_types::models::default_fallback_model_id);
    let context_window = get_context_window_size(&model);

    let utilization_pct = if context_window > 0 {
        (estimated_total_tokens as f64 / context_window as f64) * 100.0
    } else {
        0.0
    };

    let mut total_cost = 0.0_f64;
    let mut total_input = 0_u64;
    let mut total_output = 0_u64;
    let mut cache_read = 0_u64;
    let mut api_calls = 0_usize;
    let mut tool_use_count = 0_usize;
    let mut tools_seen: HashSet<String> = HashSet::new();

    for msg in messages {
        if let Message::Assistant(a) = msg {
            total_cost += a.cost_usd;
            if let Some(ref usage) = a.usage {
                total_input += usage.input_tokens;
                total_output += usage.output_tokens;
                cache_read += usage.cache_read_input_tokens;
            }
            // Count this as an API call if it has usage data or non-empty content
            if a.usage.is_some() || !a.content.is_empty() {
                api_calls += 1;
            }

            for block in &a.content {
                if let ContentBlock::ToolUse { name, .. } = block {
                    tool_use_count += 1;
                    tools_seen.insert(name.clone());
                }
            }
        }
    }

    let mut unique_tools: Vec<String> = tools_seen.into_iter().collect();
    unique_tools.sort();

    ContextSnapshot {
        estimated_total_tokens,
        context_window_size: context_window,
        utilization_pct,
        total_cost_usd: total_cost,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        cache_read_tokens: cache_read,
        api_call_count: api_calls,
        tool_use_count,
        unique_tools_used: unique_tools,
    }
}

pub(super) fn build_api_view_data(requests: &[ApiRequestSnapshot]) -> ApiViewData {
    let mut providers: Vec<String> = requests
        .iter()
        .map(|snapshot| snapshot.provider.clone())
        .collect();
    providers.sort();
    providers.dedup();

    let mut models: Vec<String> = requests
        .iter()
        .map(|snapshot| snapshot.model.clone())
        .collect();
    models.sort();
    models.dedup();

    ApiViewData {
        request_count: requests.len(),
        providers,
        models,
        last_request_id: requests.last().map(|snapshot| snapshot.request_id.clone()),
        diagnostics: Vec::new(),
        rollout_path: None,
        last_seq: 0,
        record_schema_version: 0,
    }
}

// ---------------------------------------------------------------------------
// Message serialization
// ---------------------------------------------------------------------------

/// Convert a Message to a full JSON value (preserving all data).
fn message_to_json(msg: &Message) -> serde_json::Value {
    match msg {
        Message::User(u) => {
            let content_value = match &u.content {
                MessageContent::Text(t) => serde_json::json!(t),
                MessageContent::Blocks(blocks) => content_blocks_to_export_json(blocks),
            };
            serde_json::json!({
                "type": "user",
                "uuid": u.uuid.to_string(),
                "timestamp": u.timestamp,
                "role": "user",
                "content": content_value,
                "is_meta": u.is_meta,
                "tool_use_result": u.tool_use_result,
                "source_tool_assistant_uuid": u.source_tool_assistant_uuid,
            })
        }
        Message::Assistant(a) => serde_json::json!({
            "type": "assistant",
            "uuid": a.uuid.to_string(),
            "timestamp": a.timestamp,
            "role": "assistant",
            "content": content_blocks_to_export_json(&a.content),
            "usage": a.usage,
            "stop_reason": a.stop_reason,
            "is_api_error_message": a.is_api_error_message,
            "api_error": a.api_error,
            "cost_usd": a.cost_usd,
        }),
        Message::System(s) => serde_json::json!({
            "type": "system",
            "uuid": s.uuid.to_string(),
            "timestamp": s.timestamp,
            "subtype": format!("{:?}", s.subtype),
            "content": s.content,
        }),
        Message::Progress(p) => serde_json::json!({
            "type": "progress",
            "uuid": p.uuid.to_string(),
            "timestamp": p.timestamp,
            "tool_use_id": p.tool_use_id,
            "data": p.data,
        }),
        Message::Attachment(a) => serde_json::json!({
            "type": "attachment",
            "uuid": a.uuid.to_string(),
            "timestamp": a.timestamp,
            "attachment": a.attachment,
        }),
    }
}

/// Convert ToolResultContent to a JSON value.
pub(super) fn tool_result_content_to_json(content: &ToolResultContent) -> serde_json::Value {
    match content {
        ToolResultContent::Text(t) => serde_json::json!(t),
        ToolResultContent::Blocks(blocks) => content_blocks_to_export_json(blocks),
    }
}

fn content_blocks_to_export_json(blocks: &[ContentBlock]) -> serde_json::Value {
    serde_json::Value::Array(blocks.iter().map(content_block_to_export_json).collect())
}

fn content_block_to_export_json(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Image { source } => serde_json::json!({
            "type": "image",
            "source": {
                "type": source.source_type,
                "media_type": source.media_type,
                "data": format!(
                    "[image omitted: {}, base64 length {}]",
                    source.media_type,
                    source.data.len()
                ),
                "metadata": {
                    "omitted": true,
                    "encoding": source.source_type,
                    "base64_length": source.data.len(),
                }
            }
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => serde_json::json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": tool_result_content_to_json(content),
            "is_error": is_error,
        }),
        other => serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
    }
}
