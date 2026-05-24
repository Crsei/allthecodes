use std::collections::HashMap;

use allthecodes_types::message::{ContentBlock, Message, MessageContent, ToolResultContent};

/// A record of a tool result that was replaced with a truncated preview.
#[derive(Debug)]
pub struct ReplacementRecord {
    /// The tool_use_id of the replaced result.
    pub tool_use_id: String,
    /// The original character count of the content.
    pub original_size: usize,
    /// The file path where the full content was saved.
    pub file_path: String,
}

/// State for tracking content replacements (large results saved to disk).
#[derive(Debug, Default)]
pub struct ContentReplacementState {
    /// Map from tool_use_id to its replacement record.
    pub replacements: HashMap<String, ReplacementRecord>,
}

/// Default maximum size for tool results before they get budgeted.
pub const DEFAULT_MAX_SIZE_CHARS: usize = 100_000;

/// Number of preview characters to keep at the head.
const PREVIEW_HEAD_CHARS: usize = 500;

/// Number of preview characters to keep at the tail.
const PREVIEW_TAIL_CHARS: usize = 200;

/// Apply tool result budget: if a tool result exceeds `max_size_chars`,
/// save it to disk and replace with a truncated preview + file path.
///
/// This prevents extremely large tool results (e.g., huge file reads or
/// command outputs) from consuming the entire context window.
pub async fn apply_tool_result_budget(
    messages: Vec<Message>,
    state: &mut ContentReplacementState,
    max_size_chars: usize,
) -> Vec<Message> {
    let mut result = Vec::with_capacity(messages.len());

    for msg in messages {
        match msg {
            Message::User(mut user) => {
                match &mut user.content {
                    MessageContent::Blocks(blocks) => {
                        for block in blocks.iter_mut() {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                ref mut content,
                                ..
                            } = block
                            {
                                let content_len = tool_result_content_len(content);
                                if content_len > max_size_chars {
                                    // Save to disk and replace
                                    let full_text = extract_tool_result_text(content);
                                    match save_to_disk(tool_use_id, &full_text).await {
                                        Ok(file_path) => {
                                            let preview =
                                                make_preview(&full_text, content_len, &file_path);

                                            state.replacements.insert(
                                                tool_use_id.clone(),
                                                ReplacementRecord {
                                                    tool_use_id: tool_use_id.clone(),
                                                    original_size: content_len,
                                                    file_path: file_path.clone(),
                                                },
                                            );

                                            *content = ToolResultContent::Text(preview);
                                        }
                                        Err(e) => {
                                            // If we can't save to disk, truncate in place
                                            tracing::warn!(
                                                "Failed to save large tool result to disk: {}",
                                                e
                                            );
                                            let truncated =
                                                truncate_in_place(&full_text, max_size_chars);
                                            *content = ToolResultContent::Text(truncated);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    MessageContent::Text(_) => {}
                }
                result.push(Message::User(user));
            }
            other => result.push(other),
        }
    }

    result
}

/// Get the character length of tool result content.
fn tool_result_content_len(content: &ToolResultContent) -> usize {
    match content {
        ToolResultContent::Text(s) => s.len(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .map(|b| match b {
                ContentBlock::Text { text } => text.len(),
                _ => 0,
            })
            .sum(),
    }
}

/// Extract the full text from tool result content.
fn extract_tool_result_text(content: &ToolResultContent) -> String {
    match content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Save tool result content to a temporary file on disk.
/// Returns the file path where the content was saved.
async fn save_to_disk(tool_use_id: &str, content: &str) -> Result<String, std::io::Error> {
    let dir = std::env::temp_dir()
        .join("allthecodes")
        .join("tool-results");
    tokio::fs::create_dir_all(&dir).await?;

    let file_name = format!("{}.txt", tool_use_id);
    let file_path = dir.join(&file_name);
    tokio::fs::write(&file_path, content).await?;

    Ok(file_path.to_string_lossy().to_string())
}

/// Create a preview string for a budgeted tool result.
fn make_preview(full_text: &str, original_len: usize, file_path: &str) -> String {
    let head_len = PREVIEW_HEAD_CHARS.min(full_text.len());
    let tail_len = PREVIEW_TAIL_CHARS.min(full_text.len().saturating_sub(head_len));

    let head = &full_text[..head_len];
    let tail = if tail_len > 0 {
        &full_text[full_text.len() - tail_len..]
    } else {
        ""
    };

    let omitted = original_len.saturating_sub(head_len + tail_len);

    format!(
        "{head}\n\n[... {omitted} characters omitted. Full output saved to: {file_path} ...]\n\n{tail}"
    )
}

/// Truncate content in place when we can't save to disk.
fn truncate_in_place(text: &str, max_size: usize) -> String {
    if text.len() <= max_size {
        return text.to_string();
    }

    let head_len = PREVIEW_HEAD_CHARS.min(max_size / 2);
    let tail_len = PREVIEW_TAIL_CHARS.min(max_size.saturating_sub(head_len) / 2);

    let head = &text[..head_len];
    let tail = if tail_len > 0 && text.len() > tail_len {
        &text[text.len() - tail_len..]
    } else {
        ""
    };

    let omitted = text.len().saturating_sub(head_len + tail_len);
    format!("{head}\n\n[... {omitted} characters omitted (truncated in place) ...]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::create_tool_result_message;

    #[tokio::test]
    async fn test_small_results_unchanged() {
        let messages = vec![create_tool_result_message("tu_1", "small", false)];
        let mut state = ContentReplacementState::default();

        let result = apply_tool_result_budget(messages.clone(), &mut state, 1000).await;
        assert_eq!(result.len(), 1);
        assert!(state.replacements.is_empty());
    }

    #[tokio::test]
    async fn test_large_result_gets_budgeted() {
        let large_content = "x".repeat(5000);
        let messages = vec![create_tool_result_message("tu_big", &large_content, false)];
        let mut state = ContentReplacementState::default();

        let result = apply_tool_result_budget(messages, &mut state, 1000).await;
        assert_eq!(result.len(), 1);
        assert!(state.replacements.contains_key("tu_big"));

        let record = &state.replacements["tu_big"];
        assert_eq!(record.original_size, 5000);
        assert!(!record.file_path.is_empty());
    }
}
