use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use super::index::{
    ensure_memory_dir, format_memory_context_section, key_to_filename, list_memories, memory_dir,
    refresh_memory_index,
};
use super::types::{MemoryEntry, MemoryScope, MemoryType};

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
        created_at,
        updated_at: now,
    };

    let json = serde_json::to_string_pretty(&entry).context("Failed to serialize memory entry")?;
    std::fs::write(&file_path, json)
        .with_context(|| format!("Failed to write memory file: {}", file_path.display()))?;

    refresh_memory_index(scope, cwd)?;

    Ok(entry)
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
