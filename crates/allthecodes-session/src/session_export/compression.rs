//! Tool timeline reconstruction and compression event extraction.

use std::collections::HashMap;

use regex::Regex;

use allthecodes_types::message::{
    ContentBlock, Message, MessageContent, SystemSubtype, ToolResultContent,
};

use super::builders::tool_result_content_to_json;
use super::format_ts_millis;
use super::{
    CompactBoundaryRecord, CompressionData, ContentReplacementRecord, MicrocompactRecord,
    PreservedSegmentRecord, ToolCallRecord,
};

// ---------------------------------------------------------------------------
// Tool timeline reconstruction
// ---------------------------------------------------------------------------

/// Reconstruct the tool call timeline by matching tool_use blocks in
/// assistant messages to tool_result blocks in subsequent user messages.
pub fn reconstruct_tool_timeline(messages: &[Message]) -> Vec<ToolCallRecord> {
    // Phase 1: collect all tool_use entries from assistant messages
    struct PendingToolUse {
        assistant_uuid: String,
        tool_use_id: String,
        tool_name: String,
        input: serde_json::Value,
        timestamp: i64,
    }

    let mut pending: HashMap<String, PendingToolUse> = HashMap::new();
    let mut timeline: Vec<ToolCallRecord> = Vec::new();
    let mut seq = 0usize;

    for msg in messages {
        match msg {
            Message::Assistant(a) => {
                for block in &a.content {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        pending.insert(
                            id.clone(),
                            PendingToolUse {
                                assistant_uuid: a.uuid.to_string(),
                                tool_use_id: id.clone(),
                                tool_name: name.clone(),
                                input: input.clone(),
                                timestamp: a.timestamp,
                            },
                        );
                    }
                }
            }
            Message::User(u) => {
                let blocks = match &u.content {
                    MessageContent::Blocks(b) => b.as_slice(),
                    _ => &[],
                };
                for block in blocks {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = block
                    {
                        let result_text = tool_result_content_to_json(content);
                        let was_replaced = detect_content_replacement_in_result(content);

                        if let Some(p) = pending.remove(tool_use_id) {
                            timeline.push(ToolCallRecord {
                                sequence: seq,
                                assistant_uuid: p.assistant_uuid,
                                tool_use_id: p.tool_use_id,
                                tool_name: p.tool_name,
                                input: p.input,
                                result: Some(result_text),
                                is_error: *is_error,
                                timestamp: format_ts_millis(p.timestamp),
                                result_timestamp: Some(format_ts_millis(u.timestamp)),
                                was_content_replaced: was_replaced,
                            });
                            seq += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Phase 2: any unmatched tool_uses (no result received — user interrupt, etc.)
    let mut unmatched: Vec<_> = pending.into_values().collect();
    unmatched.sort_by_key(|p| p.timestamp);
    for p in unmatched {
        timeline.push(ToolCallRecord {
            sequence: seq,
            assistant_uuid: p.assistant_uuid,
            tool_use_id: p.tool_use_id,
            tool_name: p.tool_name,
            input: p.input,
            result: None,
            is_error: false,
            timestamp: format_ts_millis(p.timestamp),
            result_timestamp: None,
            was_content_replaced: false,
        });
        seq += 1;
    }

    timeline
}

// ---------------------------------------------------------------------------
// Compression event extraction
// ---------------------------------------------------------------------------

/// Extract compression events from the message history.
pub fn extract_compression_events(messages: &[Message]) -> CompressionData {
    let mut compact_boundaries = Vec::new();
    let mut content_replacements = Vec::new();
    let mut microcompact_replacements = Vec::new();

    for msg in messages {
        // Extract compact boundaries from system messages
        if let Message::System(s) = msg {
            if let SystemSubtype::CompactBoundary { compact_metadata } = &s.subtype {
                compact_boundaries.push(CompactBoundaryRecord {
                    uuid: s.uuid.to_string(),
                    timestamp: format_ts_millis(s.timestamp),
                    pre_compact_tokens: compact_metadata
                        .as_ref()
                        .map(|m| m.pre_compact_token_count),
                    post_compact_tokens: compact_metadata
                        .as_ref()
                        .map(|m| m.post_compact_token_count),
                    preserved_segment: compact_metadata
                        .as_ref()
                        .and_then(|m| m.preserved_segment.as_ref())
                        .map(|segment| PreservedSegmentRecord {
                            summary_message_uuid: segment.summary_message_uuid.clone(),
                            preserved_message_uuids: segment.preserved_message_uuids.clone(),
                        }),
                    boundary_text: s.content.clone(),
                });
            }
        }

        // Detect content replacements and microcompact events in user messages
        if let Message::User(u) = msg {
            let blocks = match &u.content {
                MessageContent::Blocks(b) => b.as_slice(),
                _ => &[],
            };
            for block in blocks {
                if let ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } = block
                {
                    if let Some(record) = detect_content_replacement(tool_use_id, content) {
                        content_replacements.push(record);
                    }
                    if let Some(record) = detect_microcompact(tool_use_id, content) {
                        microcompact_replacements.push(record);
                    }
                }
            }
        }
    }

    let total_compactions = compact_boundaries.len();

    CompressionData {
        compact_boundaries,
        content_replacements,
        microcompact_replacements,
        total_compactions,
    }
}

/// Detect if a tool_result was replaced by the tool_result_budget system.
///
/// Looks for the pattern:
/// `[... N characters omitted. Full output saved to: <path> ...]`
pub fn detect_content_replacement(
    tool_use_id: &str,
    content: &ToolResultContent,
) -> Option<ContentReplacementRecord> {
    let text = match content {
        ToolResultContent::Text(t) => t.as_str(),
        ToolResultContent::Blocks(blocks) => {
            // Check each text block
            for block in blocks {
                if let ContentBlock::Text { text } = block {
                    if let Some(record) = parse_replacement_marker(tool_use_id, text) {
                        return Some(record);
                    }
                }
            }
            return None;
        }
    };
    parse_replacement_marker(tool_use_id, text)
}

// ---------------------------------------------------------------------------
// Private parse/detect helpers
// ---------------------------------------------------------------------------

/// Parse the replacement marker from text content.
fn parse_replacement_marker(tool_use_id: &str, text: &str) -> Option<ContentReplacementRecord> {
    // Match: [... N characters omitted. Full output saved to: <path> ...]
    let re = Regex::new(
        r"\[\.\.\.\s+(\d+)\s+characters omitted\.\s+Full output saved to:\s+(.+?)\s*\.\.\.\]",
    )
    .ok()?;

    let caps = re.captures(text)?;
    let omitted_chars: usize = caps.get(1)?.as_str().parse().ok()?;
    let file_path = caps.get(2)?.as_str().to_string();

    Some(ContentReplacementRecord {
        tool_use_id: tool_use_id.to_string(),
        original_size_hint: Some(omitted_chars),
        file_path: Some(file_path),
    })
}

/// Detect if a tool result was microcompacted (old large result truncated in-place).
///
/// Looks for the pattern:
/// `[... N characters omitted (microcompacted) ...]`
pub(super) fn detect_microcompact(
    tool_use_id: &str,
    content: &ToolResultContent,
) -> Option<MicrocompactRecord> {
    let text = match content {
        ToolResultContent::Text(t) => t.as_str(),
        ToolResultContent::Blocks(blocks) => {
            for block in blocks {
                if let ContentBlock::Text { text } = block {
                    if let Some(record) = parse_microcompact_marker(tool_use_id, text) {
                        return Some(record);
                    }
                }
            }
            return None;
        }
    };
    parse_microcompact_marker(tool_use_id, text)
}

/// Parse the microcompact marker from text content.
fn parse_microcompact_marker(tool_use_id: &str, text: &str) -> Option<MicrocompactRecord> {
    let re =
        Regex::new(r"\[\.\.\.\s+(\d+)\s+characters omitted \(microcompacted\)\s*\.\.\.\]").ok()?;

    let caps = re.captures(text)?;
    let omitted_chars: usize = caps.get(1)?.as_str().parse().ok()?;

    Some(MicrocompactRecord {
        tool_use_id: tool_use_id.to_string(),
        omitted_chars,
    })
}

/// Check if a tool result content has any replacement marker (used by
/// tool timeline reconstruction).
pub(super) fn detect_content_replacement_in_result(content: &ToolResultContent) -> bool {
    let text = match content {
        ToolResultContent::Text(t) => t.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    text.contains("characters omitted. Full output saved to:")
        || text.contains("characters omitted (truncated in place)")
        || text.contains("characters omitted (microcompacted)")
}
