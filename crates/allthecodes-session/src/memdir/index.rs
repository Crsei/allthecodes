use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::types::{
    MemoryEntry, MemoryScope, MEMORY_ENTRYPOINT_MAX_BYTES, MEMORY_ENTRYPOINT_MAX_LINES,
    MEMORY_ENTRYPOINT_NAME,
};

const MEMORY_INDEX_HOOK_MAX_CHARS: usize = 150;

/// Get the memory directory for a given scope.
pub fn memory_dir(scope: MemoryScope, cwd: &Path) -> Result<PathBuf> {
    match scope {
        MemoryScope::Global => Ok(allthecodes_config::paths::memory_dir_global()),
        MemoryScope::Project => {
            Ok(allthecodes_config::paths::project_allthecodes_dir(cwd).join("memory"))
        }
        MemoryScope::Team => Ok(allthecodes_config::paths::team_memory_dir(cwd)),
        MemoryScope::Auto => Ok(allthecodes_config::paths::auto_memory_dir()),
    }
}

/// Ensure the memory directory exists.
pub(super) fn ensure_memory_dir(scope: MemoryScope, cwd: &Path) -> Result<PathBuf> {
    let dir = memory_dir(scope, cwd)?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create memory directory: {}", dir.display()))?;
    }
    Ok(dir)
}

/// Sanitize a key for use as a filename.
pub(super) fn key_to_filename(key: &str) -> String {
    let sanitized: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{}.json", sanitized)
}

pub(super) fn memory_entrypoint_path(scope: MemoryScope, cwd: &Path) -> Result<PathBuf> {
    Ok(memory_dir(scope, cwd)?.join(MEMORY_ENTRYPOINT_NAME))
}

fn one_line_hook(value: &str) -> String {
    let hook = value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    truncate_chars(hook, MEMORY_INDEX_HOOK_MAX_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut truncated = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            truncated.push_str("...");
            return truncated;
        }
        truncated.push(ch);
    }
    truncated
}

fn escape_markdown_link_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn memory_index_line(entry: &MemoryEntry) -> String {
    let title = escape_markdown_link_text(&entry.key);
    let filename = key_to_filename(&entry.key);
    let hook = one_line_hook(&entry.value);

    if let Some(label) = entry.display_label() {
        format!("- [{title}]({filename}) - {label}: {hook}")
    } else {
        format!("- [{title}]({filename}) - {hook}")
    }
}

fn truncate_memory_index_content(content: &str) -> String {
    let mut output = String::new();
    let mut byte_count = 0usize;
    let mut truncated = false;

    for (line_count, line) in content.lines().enumerate() {
        let separator_len = usize::from(!output.is_empty());
        let next_len = separator_len + line.len();

        if line_count >= MEMORY_ENTRYPOINT_MAX_LINES
            || byte_count + next_len > MEMORY_ENTRYPOINT_MAX_BYTES
        {
            truncated = true;
            break;
        }

        if separator_len == 1 {
            output.push('\n');
            byte_count += 1;
        }
        output.push_str(line);
        byte_count += line.len();
    }

    if truncated {
        append_memory_index_warning(output)
    } else {
        output
    }
}

fn append_memory_index_warning(mut output: String) -> String {
    let warning = "- [truncated] MEMORY.md exceeded allthecodes index limits.";
    let separator_len = usize::from(!output.is_empty());
    let line_count = output.lines().count();

    if line_count < MEMORY_ENTRYPOINT_MAX_LINES
        && output.len() + separator_len + warning.len() <= MEMORY_ENTRYPOINT_MAX_BYTES
    {
        if separator_len == 1 {
            output.push('\n');
        }
        output.push_str(warning);
        return output;
    }

    if let Some(last_break) = output.rfind('\n') {
        let prefix = &output[..last_break];
        let candidate = if prefix.is_empty() {
            warning.to_string()
        } else {
            format!("{prefix}\n{warning}")
        };
        if candidate.lines().count() <= MEMORY_ENTRYPOINT_MAX_LINES
            && candidate.len() <= MEMORY_ENTRYPOINT_MAX_BYTES
        {
            return candidate;
        }
    }

    if warning.len() <= MEMORY_ENTRYPOINT_MAX_BYTES {
        warning.to_string()
    } else {
        output
    }
}

pub(super) fn build_memory_index_from_entries(entries: &[MemoryEntry]) -> String {
    let lines = entries
        .iter()
        .map(memory_index_line)
        .collect::<Vec<_>>()
        .join("\n");
    truncate_memory_index_content(&lines)
}

pub fn build_memory_index(scope: MemoryScope, cwd: &Path) -> Result<String> {
    let entries = list_memories(scope, cwd)?;
    Ok(build_memory_index_from_entries(&entries))
}

pub fn read_memory_index(scope: MemoryScope, cwd: &Path) -> Result<Option<String>> {
    let path = memory_entrypoint_path(scope, cwd)?;
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read memory index: {}", path.display()))?;
    Ok(Some(truncate_memory_index_content(&content)))
}

pub fn refresh_memory_index(scope: MemoryScope, cwd: &Path) -> Result<Option<PathBuf>> {
    let dir = ensure_memory_dir(scope, cwd)?;
    let path = dir.join(MEMORY_ENTRYPOINT_NAME);
    let entries = list_memories(scope, cwd)?;

    if entries.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove memory index: {}", path.display()))?;
        }
        return Ok(None);
    }

    let index = build_memory_index_from_entries(&entries);
    std::fs::write(&path, index)
        .with_context(|| format!("Failed to write memory index: {}", path.display()))?;
    Ok(Some(path))
}

pub(super) fn format_memory_context_section(
    title: &str,
    scope: MemoryScope,
    cwd: &Path,
    memories: &[MemoryEntry],
) -> String {
    let mut section = format!("## {title}\n");

    let index = read_memory_index(scope, cwd)
        .ok()
        .flatten()
        .unwrap_or_else(|| build_memory_index_from_entries(memories));

    if !index.trim().is_empty() {
        section.push_str("### MEMORY.md Index\n");
        section.push_str(index.trim_end());
        section.push('\n');
    }

    for mem in memories {
        section.push_str(&format!(
            "- **{}**{}: {}\n",
            mem.key,
            mem.bracketed_label(),
            mem.value
        ));
    }

    section
}

pub(super) fn memory_identity(scope: MemoryScope, entry: &MemoryEntry) -> String {
    format!("{}:{}", scope.as_str(), entry.key)
}

pub fn query_requests_memory_ignore(query: &str) -> bool {
    let normalized = query.to_ascii_lowercase();
    normalized.contains("ignore memory")
        || normalized.contains("ignore memories")
        || normalized.contains("do not use memory")
        || normalized.contains("don't use memory")
        || normalized.contains("dont use memory")
        || normalized.contains("without memory")
}

/// List all memory entries in a scope.
pub fn list_memories(scope: MemoryScope, cwd: &Path) -> Result<Vec<MemoryEntry>> {
    let dir = memory_dir(scope, cwd)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read memory directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Ok(mem) = serde_json::from_str::<MemoryEntry>(&content) {
                    entries.push(mem);
                }
            }
            Err(_) => continue,
        }
    }

    // Sort by updated_at descending (most recent first)
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(entries)
}
