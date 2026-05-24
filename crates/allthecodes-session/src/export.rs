//! Session export — convert saved sessions to Markdown.
//!
//! Exports conversation history as a readable `.md` file with:
//! - Session metadata header
//! - User/assistant messages with timestamps
//! - Tool use blocks formatted as code fences
//! - Cost and token usage summary

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use tracing::{debug, info};

use crate::storage::{self, SerializableMessage, SessionFile};
use allthecodes_types::message::{ContentBlock, Message, MessageContent};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Export a session (by ID) to a Markdown file.
///
/// If `output_path` is None, writes to `~/.allthecodes/exports/<session_id>.md`.
/// Returns the path of the written file.
pub fn export_session_markdown(session_id: &str, output_path: Option<&Path>) -> Result<PathBuf> {
    let session_file = load_session_file_raw(session_id)?;
    let md = render_session_markdown(&session_file);

    let path = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = get_export_dir();
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create export directory {}", dir.display()))?;
            dir.join(format!("{}.md", session_id))
        }
    };

    std::fs::write(&path, md)
        .with_context(|| format!("Failed to write export file {}", path.display()))?;

    info!(session_id = session_id, path = %path.display(), "session exported to markdown");

    Ok(path)
}

/// Export the current in-memory conversation to Markdown.
///
/// Used by the `/export` command to export the live session without
/// requiring it to be saved first.
pub fn export_messages_markdown(
    session_id: &str,
    messages: &[Message],
    cwd: &str,
    output_path: Option<&Path>,
) -> Result<PathBuf> {
    let md = render_messages_markdown(session_id, messages, cwd);

    let path = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let dir = get_export_dir();
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create export directory {}", dir.display()))?;
            dir.join(format!("{}.md", session_id))
        }
    };

    std::fs::write(&path, md)
        .with_context(|| format!("Failed to write export file {}", path.display()))?;

    debug!(session_id = session_id, path = %path.display(), "live session exported");

    Ok(path)
}

/// List all available exports.
pub fn list_exports() -> Result<Vec<PathBuf>> {
    let dir = get_export_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn get_export_dir() -> PathBuf {
    allthecodes_config::paths::exports_dir()
}

// ---------------------------------------------------------------------------
// Markdown rendering (from saved session file)
// ---------------------------------------------------------------------------

fn render_session_markdown(session: &SessionFile) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Session {}\n\n", &session.session_id[..8]));

    let created = format_timestamp_secs(session.created_at);
    let modified = format_timestamp_secs(session.last_modified);
    md.push_str(&format!("- **Created**: {}\n", created));
    md.push_str(&format!("- **Last Modified**: {}\n", modified));
    if !session.cwd.is_empty() {
        md.push_str(&format!("- **Working Directory**: `{}`\n", session.cwd));
    }
    md.push_str(&format!("- **Messages**: {}\n", session.messages.len()));
    md.push_str("\n---\n\n");

    // Messages
    let mut total_cost = 0.0_f64;
    for msg in &session.messages {
        total_cost += render_serialized_msg(msg, &mut md);
    }

    // Footer
    if total_cost > 0.0 {
        md.push_str("---\n\n");
        md.push_str(&format!("**Total Cost**: ${:.4}\n", total_cost));
    }

    md
}

// ---------------------------------------------------------------------------
// Markdown rendering (from live Message vec)
// ---------------------------------------------------------------------------

fn render_messages_markdown(session_id: &str, messages: &[Message], cwd: &str) -> String {
    let mut md = String::new();

    md.push_str(&format!(
        "# Session {}\n\n",
        &session_id[..std::cmp::min(8, session_id.len())]
    ));

    let now = format_datetime(Utc::now());
    md.push_str(&format!("- **Exported**: {}\n", now));
    if !cwd.is_empty() {
        md.push_str(&format!("- **Working Directory**: `{}`\n", cwd));
    }
    md.push_str(&format!("- **Messages**: {}\n", messages.len()));
    md.push_str("\n---\n\n");

    let mut total_cost = 0.0_f64;

    for msg in messages {
        match msg {
            Message::User(u) => {
                let text = match &u.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Blocks(blocks) => {
                        let mut s = String::new();
                        for b in blocks {
                            if let ContentBlock::Text { text } = b {
                                s.push_str(text);
                                s.push('\n');
                            }
                        }
                        s
                    }
                };
                if text.trim().is_empty() {
                    continue;
                }
                let ts = format_timestamp_millis(u.timestamp);
                md.push_str("## You\n\n");
                md.push_str(&format!("<sub>{}</sub>\n\n", ts));
                md.push_str(text.trim());
                md.push_str("\n\n");
            }
            Message::Assistant(a) => {
                let ts = format_timestamp_millis(a.timestamp);
                md.push_str("## Assistant\n\n");
                md.push_str(&format!("<sub>{}</sub>\n\n", ts));

                for block in &a.content {
                    render_content_block(block, &mut md);
                }

                if a.cost_usd > 0.0 {
                    total_cost += a.cost_usd;
                    md.push_str(&format!("\n<sub>Cost: ${:.4}</sub>\n", a.cost_usd));
                }
                md.push('\n');
            }
            Message::System(s) => {
                if !s.content.is_empty() {
                    md.push_str(&format!("> **System**: {}\n\n", s.content));
                }
            }
            _ => {}
        }
    }

    if total_cost > 0.0 {
        md.push_str("---\n\n");
        md.push_str(&format!("**Total Cost**: ${:.4}\n", total_cost));
    }

    md
}

// ---------------------------------------------------------------------------
// Serialized message rendering (from saved session JSON)
// ---------------------------------------------------------------------------

/// Render a single `SerializableMessage` into Markdown. Returns the cost (USD)
/// of this message so the caller can accumulate it for the footer.
fn render_serialized_msg(msg: &SerializableMessage, md: &mut String) -> f64 {
    let mut cost = 0.0;
    match msg.msg_type.as_str() {
        "user" => {
            let text = extract_user_text_from_data(&msg.data);
            if !text.is_empty() {
                let ts = format_timestamp_millis(msg.timestamp);
                md.push_str("## You\n\n");
                md.push_str(&format!("<sub>{}</sub>\n\n", ts));
                md.push_str(&text);
                md.push_str("\n\n");
            }
        }
        "assistant" => {
            let ts = format_timestamp_millis(msg.timestamp);
            md.push_str("## Assistant\n\n");
            md.push_str(&format!("<sub>{}</sub>\n\n", ts));

            if let Some(content) = msg.data.get("content") {
                if let Some(blocks) = content.as_array() {
                    for block in blocks {
                        render_content_block_from_json(block, md);
                    }
                }
            }

            if let Some(c) = msg.data.get("cost_usd").and_then(|v| v.as_f64()) {
                if c > 0.0 {
                    cost = c;
                    md.push_str(&format!("\n<sub>Cost: ${:.4}</sub>\n", c));
                }
            }
            md.push('\n');
        }
        "system" => {
            if let Some(content) = msg.data.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    md.push_str(&format!("> **System**: {}\n\n", content));
                }
            }
        }
        _ => {} // skip progress, attachment
    }
    cost
}

// ---------------------------------------------------------------------------
// Content block rendering helpers
// ---------------------------------------------------------------------------

fn render_content_block(block: &ContentBlock, md: &mut String) {
    match block {
        ContentBlock::Text { text } => {
            md.push_str(text);
            md.push_str("\n\n");
        }
        ContentBlock::ToolUse { id: _, name, input } => {
            md.push_str(&format!("**Tool**: `{}`\n\n", name));
            let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
            if !input_str.is_empty() && input_str != "{}" {
                md.push_str("```json\n");
                md.push_str(&input_str);
                md.push_str("\n```\n\n");
            }
        }
        ContentBlock::ToolResult {
            tool_use_id: _,
            content,
            is_error,
        } => {
            let label = if *is_error {
                "Tool Error"
            } else {
                "Tool Result"
            };
            md.push_str(&format!("**{}**:\n\n", label));
            let text = match content {
                allthecodes_types::message::ToolResultContent::Text(t) => t.clone(),
                allthecodes_types::message::ToolResultContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::Text { text } = b {
                            Some(text.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if !text.is_empty() {
                md.push_str("```\n");
                md.push_str(&text);
                md.push_str("\n```\n\n");
            }
        }
        ContentBlock::Thinking { thinking, .. } => {
            md.push_str("<details>\n<summary>Thinking</summary>\n\n");
            md.push_str(thinking);
            md.push_str("\n\n</details>\n\n");
        }
        _ => {}
    }
}

fn render_content_block_from_json(block: &serde_json::Value, md: &mut String) {
    match block.get("type").and_then(|t| t.as_str()) {
        Some("text") => {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                md.push_str(text);
                md.push_str("\n\n");
            }
        }
        Some("tool_use") => {
            let name = block
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            md.push_str(&format!("**Tool**: `{}`\n\n", name));
            if let Some(input) = block.get("input") {
                let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
                if !input_str.is_empty() && input_str != "{}" {
                    md.push_str("```json\n");
                    md.push_str(&input_str);
                    md.push_str("\n```\n\n");
                }
            }
        }
        Some("tool_result") => {
            let is_error = block
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let label = if is_error {
                "Tool Error"
            } else {
                "Tool Result"
            };
            md.push_str(&format!("**{}**:\n\n", label));
            if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                md.push_str("```\n");
                md.push_str(content);
                md.push_str("\n```\n\n");
            }
        }
        Some("thinking") => {
            if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                md.push_str("<details>\n<summary>Thinking</summary>\n\n");
                md.push_str(thinking);
                md.push_str("\n\n</details>\n\n");
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_user_text_from_data(data: &serde_json::Value) -> String {
    match data.get("content") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b.get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn format_datetime(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn format_timestamp_secs(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(format_datetime)
        .unwrap_or_else(|| format!("{}", ts))
}

fn format_timestamp_millis(ts: i64) -> String {
    let secs = ts / 1000;
    Utc.timestamp_opt(secs, 0)
        .single()
        .map(format_datetime)
        .unwrap_or_else(|| format!("{}", ts))
}

// ---------------------------------------------------------------------------
// Internal: load raw session file (reuse storage internals)
// ---------------------------------------------------------------------------

fn load_session_file_raw(session_id: &str) -> Result<SessionFile> {
    let path = storage::get_session_file(session_id);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read session file {}", path.display()))?;
    let file: SessionFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session file {}", path.display()))?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        let ts = format_timestamp_secs(1700000000);
        assert!(ts.contains("2023"));
    }

    #[test]
    fn test_export_dir() {
        let dir = get_export_dir();
        assert!(dir.to_string_lossy().contains("exports"));
    }
}
