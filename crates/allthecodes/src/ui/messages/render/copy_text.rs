use allthecodes_types::message::{Attachment, ContentBlock, MessageContent, ToolResultContent};

use super::render_user::tool_result_content_text;

// ── Utility ─────────────────────────────────────────────────────────────

/// Create an abbreviated string representation of a JSON value, capped at
/// `max_chars` characters.
pub(super) fn abbreviate_json(value: &serde_json::Value, max_chars: usize) -> String {
    let full = match serde_json::to_string(value) {
        Ok(s) => s,
        Err(_) => value.to_string(),
    };
    if full.len() <= max_chars {
        full
    } else if max_chars > 3 {
        format!("{}...", &full[..max_chars - 3])
    } else {
        full[..max_chars].to_string()
    }
}
pub(crate) fn tool_input_summary(
    name: &str,
    input: &serde_json::Value,
    max_chars: usize,
) -> String {
    if let Some(primary) = tool_primary_input(name, input) {
        let json = abbreviate_json(input, max_chars);
        return format!("{primary} {json}").trim().to_string();
    }
    abbreviate_json(input, max_chars)
}

pub(super) fn tool_primary_input(name: &str, input: &serde_json::Value) -> Option<String> {
    let key = match name {
        "Read" | "Edit" | "Write" | "FileEdit" | "FileWrite" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Bash" => "command",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "WebSearch" => "query",
        "Task" | "Agent" => "prompt",
        _ => return None,
    };
    input
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(|value| {
            let label = if key.ends_with("path") || key == "file_path" {
                "path"
            } else {
                key
            };
            format!("{label}={value}")
        })
}

pub(super) fn message_content_copy_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => strip_system_reminders(text),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(content_block_copy_text)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub(super) fn message_content_reference(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Text(text) => extract_code_path_reference(text),
        MessageContent::Blocks(blocks) => blocks.iter().find_map(content_block_reference),
    }
}

pub(super) fn content_block_copy_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(strip_system_reminders(text)),
        ContentBlock::ConnectorText { connector_text, .. } => Some(connector_text.clone()),
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => {
            Some(tool_input_summary(name, input, 240))
        }
        ContentBlock::ToolResult { content, .. } => Some(tool_result_content_text(content)),
        ContentBlock::Image { source } => Some(image_reference(source)),
        ContentBlock::Thinking { thinking, .. } => (!thinking.is_empty()).then(|| thinking.clone()),
        ContentBlock::RedactedThinking { .. } => Some("[redacted thinking]".to_string()),
    }
}

pub(super) fn content_block_reference(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::ToolUse { name, input, .. }
        | ContentBlock::ServerToolUse { name, input, .. } => tool_primary_input(name, input),
        ContentBlock::Image { source } => Some(image_reference(source)),
        ContentBlock::ToolResult { content, .. } => match content {
            ToolResultContent::Text(text) => extract_code_path_reference(text),
            ToolResultContent::Blocks(blocks) => blocks.iter().find_map(content_block_reference),
        },
        ContentBlock::Text { text }
        | ContentBlock::ConnectorText {
            connector_text: text,
            ..
        } => extract_code_path_reference(text),
        _ => None,
    }
}

fn extract_code_path_reference(text: &str) -> Option<String> {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '<' | '>' | '(' | ')'))
        .filter_map(clean_path_candidate)
        .find(|candidate| looks_like_code_path(candidate))
        .map(|path| format!("path={path}"))
}

fn clean_path_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | ',' | ';' | ':' | '.' | '!' | '?' | '[' | ']' | '{' | '}'
        )
    });
    if trimmed.is_empty() || trimmed.contains("://") {
        return None;
    }
    Some(trimmed.to_string())
}

fn looks_like_code_path(candidate: &str) -> bool {
    let without_line = candidate
        .rsplit_once(':')
        .and_then(|(path, line)| line.parse::<u32>().ok().map(|_| path))
        .unwrap_or(candidate);
    let Some(file_name) = without_line.rsplit('/').next() else {
        return false;
    };
    let Some((_, ext)) = file_name.rsplit_once('.') else {
        return false;
    };
    let known_ext = matches!(
        ext.to_ascii_lowercase().as_str(),
        "rs" | "toml"
            | "lock"
            | "json"
            | "jsonl"
            | "md"
            | "yml"
            | "yaml"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "sh"
            | "bash"
            | "zsh"
            | "css"
            | "scss"
            | "html"
            | "vue"
            | "svelte"
            | "sql"
    );
    known_ext && (without_line.contains('/') || without_line.starts_with('.'))
}

pub(super) fn image_reference(source: &allthecodes_types::message::ImageSource) -> String {
    format!(
        "[image: {}, {} chars]",
        source.media_type,
        source.data.len()
    )
}

pub(super) fn attachment_copy_text(attachment: &Attachment) -> String {
    match attachment {
        Attachment::EditedTextFile { path } => path.clone(),
        Attachment::QueuedCommand { prompt, .. } => prompt.clone(),
        Attachment::MaxTurnsReached {
            max_turns,
            turn_count,
        } => format!("Max turns reached: {turn_count}/{max_turns}"),
        Attachment::StructuredOutput { data } => data.to_string(),
        Attachment::HookStoppedContinuation => "Hook stopped continuation".to_string(),
        Attachment::NestedMemory { path, content } => format!("{path}\n{content}"),
        Attachment::SkillDiscovery { skills } => skills.join(", "),
    }
}

pub(super) fn attachment_reference(attachment: &Attachment) -> Option<String> {
    match attachment {
        Attachment::EditedTextFile { path } | Attachment::NestedMemory { path, .. } => {
            Some(format!("path={path}"))
        }
        Attachment::QueuedCommand { prompt, .. } => Some(format!(
            "prompt={}",
            allthecodes_utils::messages::truncate_text(prompt, 48)
        )),
        _ => None,
    }
}

pub(super) fn strip_system_reminders(text: &str) -> String {
    const OPEN: &str = "<system-reminder>";
    const CLOSE: &str = "</system-reminder>";
    let mut rest = text.trim_start();
    while let Some(after_open) = rest.strip_prefix(OPEN) {
        let Some(end) = after_open.find(CLOSE) else {
            break;
        };
        rest = after_open[end + CLOSE.len()..].trim_start();
    }
    rest.to_string()
}

pub(super) fn compact_boundary_summary(
    subtype: &allthecodes_types::message::SystemSubtype,
    prefix: &str,
) -> String {
    use allthecodes_types::message::SystemSubtype;
    match subtype {
        SystemSubtype::CompactBoundary {
            compact_metadata: Some(meta),
        } => format!(
            "--- {prefix}: {} → {} tokens ---",
            meta.pre_compact_token_count, meta.post_compact_token_count
        ),
        SystemSubtype::MicrocompactBoundary {
            microcompact_metadata: Some(meta),
        } => format!(
            "--- {prefix}: saved {} tokens from {} tool results ---",
            meta.tokens_saved,
            meta.compacted_tool_ids.len()
        ),
        _ => format!("--- {prefix} ---"),
    }
}
