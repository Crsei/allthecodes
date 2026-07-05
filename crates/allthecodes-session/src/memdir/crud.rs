use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use super::index::{
    ensure_memory_dir, format_memory_context_section, key_to_filename, list_memories, memory_dir,
    refresh_memory_index,
};
use super::types::{
    CuratedMemorySnapshot, CuratedMemoryTarget, CuratedMemoryWrite, MemoryEntry, MemoryScope,
    MemoryType, CURATED_MEMORY_PROFILE_MAX_BYTES,
};

/// Write a memory entry.
pub fn write_memory(
    key: &str,
    value: &str,
    category: &str,
    scope: MemoryScope,
    cwd: &Path,
) -> Result<MemoryEntry> {
    let dir = ensure_memory_dir(scope, cwd)?;
    let filename = key_to_filename(key);
    let file_path = dir.join(&filename);

    let now = Utc::now().to_rfc3339();

    // Check if entry exists to preserve created_at
    let created_at = if file_path.exists() {
        read_memory(key, scope, cwd)
            .ok()
            .map(|e| e.created_at)
            .unwrap_or_else(|| now.clone())
    } else {
        now.clone()
    };

    let entry = MemoryEntry {
        key: key.to_string(),
        value: value.to_string(),
        category: category.to_string(),
        memory_type: MemoryType::parse(category),
        description: None,
        search_terms: Vec::new(),
        source_session_id: None,
        approval_id: None,
        created_at,
        updated_at: now,
    };

    let json = serde_json::to_string_pretty(&entry).context("Failed to serialize memory entry")?;
    std::fs::write(&file_path, json)
        .with_context(|| format!("Failed to write memory file: {}", file_path.display()))?;

    refresh_memory_index(scope, cwd)?;

    Ok(entry)
}

/// Write a bounded curated memory entry into the target's default scope.
///
/// This is the common write path for approved self-improvement outputs. Direct
/// user commands may still use [`write_memory`], but proposal reviewers should
/// pass the proposal/approval id here so the durable entry records provenance.
pub fn write_curated_memory(write: CuratedMemoryWrite, cwd: &Path) -> Result<MemoryEntry> {
    let target = write.target;
    let scope = target.default_scope();
    let dir = ensure_memory_dir(scope, cwd)?;
    let filename = key_to_filename(&write.key);
    let file_path = dir.join(&filename);
    let now = Utc::now().to_rfc3339();

    let created_at = if file_path.exists() {
        read_memory(&write.key, scope, cwd)
            .ok()
            .map(|entry| entry.created_at)
            .unwrap_or_else(|| now.clone())
    } else {
        now.clone()
    };

    let entry = MemoryEntry {
        key: write.key,
        value: write.value,
        category: target.as_str().to_string(),
        memory_type: Some(target.memory_type()),
        description: None,
        search_terms: Vec::new(),
        source_session_id: write.source_session_id,
        approval_id: write.approval_id,
        created_at,
        updated_at: now,
    };

    let json = serde_json::to_string_pretty(&entry).context("Failed to serialize memory entry")?;
    std::fs::write(&file_path, json)
        .with_context(|| format!("Failed to write memory file: {}", file_path.display()))?;

    refresh_memory_index(scope, cwd)?;
    refresh_curated_memory_profile(target, cwd, CURATED_MEMORY_PROFILE_MAX_BYTES)?;

    Ok(entry)
}

pub fn curated_memory_profile_path(
    target: CuratedMemoryTarget,
    cwd: &Path,
) -> Result<std::path::PathBuf> {
    Ok(memory_dir(target.default_scope(), cwd)?.join(target.profile_name()))
}

pub fn build_curated_memory_profile(
    target: CuratedMemoryTarget,
    cwd: &Path,
    max_bytes: usize,
) -> Result<String> {
    let mut entries = list_memories(target.default_scope(), cwd)?;
    entries.retain(|entry| entry.effective_memory_type() == Some(target.memory_type()));
    if entries.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec![format!("# {}", target.profile_name())];
    for entry in entries {
        let mut line = format!("- **{}**: {}", entry.key, one_line(&entry.value));
        if let Some(session_id) = entry.source_session_id.as_deref() {
            line.push_str(&format!(" (source session: {session_id})"));
        }
        if let Some(approval_id) = entry.approval_id.as_deref() {
            line.push_str(&format!(" (approval: {approval_id})"));
        }
        lines.push(line);
    }
    Ok(truncate_to_limit(&lines.join("\n"), max_bytes))
}

pub fn refresh_curated_memory_profile(
    target: CuratedMemoryTarget,
    cwd: &Path,
    max_bytes: usize,
) -> Result<Option<std::path::PathBuf>> {
    let dir = ensure_memory_dir(target.default_scope(), cwd)?;
    let path = dir.join(target.profile_name());
    let profile = build_curated_memory_profile(target, cwd, max_bytes)?;

    if profile.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("Failed to remove memory profile: {}", path.display()))?;
        }
        return Ok(None);
    }

    std::fs::write(&path, profile)
        .with_context(|| format!("Failed to write memory profile: {}", path.display()))?;
    Ok(Some(path))
}

pub fn capture_curated_memory_snapshot(
    cwd: &Path,
    include_auto: bool,
    max_bytes: usize,
) -> Result<CuratedMemorySnapshot> {
    let context = build_memory_context_with(cwd, include_auto)?;
    Ok(CuratedMemorySnapshot {
        context: truncate_to_limit(&context, max_bytes),
        captured_at: Utc::now().to_rfc3339(),
    })
}

/// Read a memory entry by key.
pub fn read_memory(key: &str, scope: MemoryScope, cwd: &Path) -> Result<MemoryEntry> {
    let dir = memory_dir(scope, cwd)?;
    let filename = key_to_filename(key);
    let file_path = dir.join(&filename);

    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("Memory '{}' not found", key))?;

    serde_json::from_str(&content).context("Failed to parse memory entry")
}

/// Delete a memory entry.
pub fn delete_memory(key: &str, scope: MemoryScope, cwd: &Path) -> Result<bool> {
    let dir = memory_dir(scope, cwd)?;
    let filename = key_to_filename(key);
    let file_path = dir.join(&filename);

    if file_path.exists() {
        std::fs::remove_file(&file_path)
            .with_context(|| format!("Failed to delete memory: {}", file_path.display()))?;
        refresh_memory_index(scope, cwd)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Search memories by keyword across keys, values, and categories.
pub fn search_memories(query: &str, scope: MemoryScope, cwd: &Path) -> Result<Vec<MemoryEntry>> {
    let all = list_memories(scope, cwd)?;
    let query_lower = query.to_lowercase();

    Ok(all
        .into_iter()
        .filter(|e| {
            e.key.to_lowercase().contains(&query_lower)
                || e.value.to_lowercase().contains(&query_lower)
                || e.category.to_lowercase().contains(&query_lower)
                || e.effective_memory_type()
                    .map(|memory_type| memory_type.as_str().contains(&query_lower))
                    .unwrap_or(false)
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Context injection
// ---------------------------------------------------------------------------

/// Build a memory context string for injection into system prompts.
///
/// Collects relevant memories and formats them for the model's context.
/// Equivalent to `build_memory_context_with(cwd, false)` — the auto
/// scope is skipped unless the caller opts in.
pub fn build_memory_context(cwd: &Path) -> Result<String> {
    build_memory_context_with(cwd, false)
}

/// See [`build_memory_context`]. Extra `include_auto` flag lets the root
/// crate wire in the per-session `auto_memory_enabled` toggle without
/// dragging settings types into this crate.
///
/// Scopes included:
/// - `Project` and `Global` are always considered.
/// - `Team` is included when `FEATURE_TEAMMEM` is enabled.
/// - `Auto` is included when `include_auto` is true.
pub fn build_memory_context_with(cwd: &Path, include_auto: bool) -> Result<String> {
    let mut sections = Vec::new();

    // Collect project memories
    if let Ok(project_mems) = list_memories(MemoryScope::Project, cwd) {
        if !project_mems.is_empty() {
            sections.push(format_memory_context_section(
                "Project Memories",
                MemoryScope::Project,
                cwd,
                &project_mems,
            ));
        }
    }

    // Collect global memories
    if let Ok(global_mems) = list_memories(MemoryScope::Global, cwd) {
        if !global_mems.is_empty() {
            sections.push(format_memory_context_section(
                "Global Memories",
                MemoryScope::Global,
                cwd,
                &global_mems,
            ));
        }
    }

    // Team memories — gated on FEATURE_TEAMMEM at the context-injection
    // layer. The dir is readable regardless so the selector can still show
    // legacy entries even when the feature is off.
    if allthecodes_config::features::enabled(allthecodes_config::features::Feature::TeamMemory) {
        if let Ok(team_mems) = list_memories(MemoryScope::Team, cwd) {
            if !team_mems.is_empty() {
                sections.push(format_memory_context_section(
                    "Team Memories",
                    MemoryScope::Team,
                    cwd,
                    &team_mems,
                ));
            }
        }
    }

    // Auto memories — injected only when the caller opts in via toggle.
    if include_auto {
        if let Ok(auto_mems) = list_memories(MemoryScope::Auto, cwd) {
            if !auto_mems.is_empty() {
                sections.push(format_memory_context_section(
                    "Auto Memories",
                    MemoryScope::Auto,
                    cwd,
                    &auto_mems,
                ));
            }
        }
    }

    if sections.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!(
            "<memory-context>\n{}</memory-context>",
            sections.join("\n")
        ))
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_to_limit(content: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || content.is_empty() {
        return String::new();
    }
    if content.len() <= max_bytes {
        return content.to_string();
    }

    const WARNING: &str = "- [truncated] curated memory profile exceeded allthecodes limits.";
    if WARNING.len() >= max_bytes {
        return truncate_at_boundary(WARNING, max_bytes);
    }

    let keep = max_bytes.saturating_sub(WARNING.len() + 1);
    let mut output = truncate_at_boundary(content, keep)
        .trim_end_matches(char::is_whitespace)
        .to_string();
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(WARNING);
    output
}

fn truncate_at_boundary(value: &str, max_bytes: usize) -> String {
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
