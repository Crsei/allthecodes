//! `/memory` command - default entry point is a memory selector that
//! surfaces auto-memory state and exposes every scope (global, project,
//! team, auto) together with the nearest AGENTS.md (or CLAUDE.md) files.
//!
//! Subcommands (issue #45):
//!   (no args)  - Print the selector (grouped listing of every scope +
//!                 auto-memory header + directory shortcuts)
//!   show       - Display AGENTS.md content
//!   path       - Show AGENTS.md file locations
//!   edit       - Create/locate AGENTS.md for editing
//!   list       - List memdir entries across every scope
//!   get <key>  - Read a memdir entry
//!   set <key> <value> [--global|--team|--auto] [--category=<cat>]
//!   rm <key>   [--global|--team|--auto]
//!   search <query>
//!   auto on|off|status  - Toggle auto-memory capture/injection
//!   open  <auto|team|global|project>  - Print/ensure-and-open a dir
//!
//! # TODO
//! - The selector is currently a formatted listing; a real interactive TUI
//!   picker is a future improvement that belongs in the terminal frontend,
//!   not here.
//! - The `auto` toggle only persists `auto_memory_enabled`; the actual
//!   capture hook is a separate change.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::claude_md as config_claude_md;
use allthecodes_config::features::{self, Feature};
use allthecodes_config::paths as cfg_paths;
use allthecodes_config::settings;
use allthecodes_session::memdir::{self, MemoryEntry, MemoryScope};

mod claude_md;
mod selector;

pub struct MemoryHandler;

#[async_trait]
impl CommandHandler for MemoryHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.trim().splitn(3, char::is_whitespace).collect();
        let subcommand = parts.first().copied().unwrap_or("");

        match subcommand {
            // Default entry point: selector view (issue #45).
            "" => selector::selector(ctx),
            "show" => claude_md::show_memory(&ctx.cwd),
            "path" => claude_md::show_paths(&ctx.cwd),
            "edit" => claude_md::edit_memory(&ctx.cwd),
            "list" | "ls" => list_entries(ctx),
            "get" => {
                let key = parts.get(1).unwrap_or(&"");
                if key.is_empty() {
                    return Ok(CommandResult::Output(
                        "Usage: /memory get <key>".to_string(),
                    ));
                }
                get_entry(key, &ctx.cwd)
            }
            "set" => {
                let key = parts.get(1).copied().unwrap_or("");
                let rest = parts.get(2).copied().unwrap_or("");
                if key.is_empty() || rest.is_empty() {
                    return Ok(CommandResult::Output(
                        "Usage: /memory set <key> <value> [--global|--team|--auto] [--category=<cat>]"
                            .to_string(),
                    ));
                }
                set_entry(key, rest, &ctx.cwd)
            }
            "rm" | "delete" | "del" => {
                let key = parts.get(1).unwrap_or(&"");
                let flag = parts.get(2).copied().unwrap_or("");
                if key.is_empty() {
                    return Ok(CommandResult::Output(
                        "Usage: /memory rm <key> [--global|--team|--auto]".to_string(),
                    ));
                }
                let scope = parse_scope_flag(flag);
                rm_entry(key, scope, &ctx.cwd)
            }
            "search" | "find" => {
                let query = parts.get(1).unwrap_or(&"");
                if query.is_empty() {
                    return Ok(CommandResult::Output(
                        "Usage: /memory search <query>".to_string(),
                    ));
                }
                search_entries(query, &ctx.cwd)
            }
            "auto" => {
                let action = parts.get(1).copied().unwrap_or("status");
                auto_toggle(action, ctx)
            }
            "open" => {
                let which = parts.get(1).copied().unwrap_or("");
                open_dir(which, &ctx.cwd)
            }
            _ => Ok(CommandResult::Output(
                "Usage: /memory [show|path|edit|list|get|set|rm|search|auto|open]\n\n\
                 (no args)           - Interactive memory selector (default)\n\n\
                 AGENTS.md (project instructions):\n\
                 \x20 show           - Display current AGENTS.md content\n\
                 \x20 path           - Show AGENTS.md file locations\n\
                 \x20 edit           - Create/locate AGENTS.md for editing\n\n\
                 Memory entries:\n\
                 \x20 list                       - List entries across all scopes\n\
                 \x20 get <key>                  - Read an entry (searches all scopes)\n\
                 \x20 set <key> <val> [--global|--team|--auto] [--category=<cat>]\n\
                 \x20 rm <key> [--global|--team|--auto]\n\
                 \x20 search <query>             - Substring match across entries\n\n\
                 Auto-memory (issue #45):\n\
                 \x20 auto on|off|status        - Toggle auto-capture\n\
                 \x20 open <auto|team|global|project>\n\
                 \x20                             - Print/open a scope directory"
                    .to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Scope-flag parsing
// ---------------------------------------------------------------------------

fn parse_scope_flag(flag: &str) -> MemoryScope {
    if flag.contains("--global") {
        MemoryScope::Global
    } else if flag.contains("--team") {
        MemoryScope::Team
    } else if flag.contains("--auto") {
        MemoryScope::Auto
    } else {
        MemoryScope::Project
    }
}

// ---------------------------------------------------------------------------
// AGENTS.md subcommands
// ---------------------------------------------------------------------------

fn list_entries(ctx: &CommandContext) -> Result<CommandResult> {
    let cwd = &ctx.cwd;
    let project = memdir::list_memories(MemoryScope::Project, cwd).unwrap_or_default();
    let global = memdir::list_memories(MemoryScope::Global, cwd).unwrap_or_default();
    let team = memdir::list_memories(MemoryScope::Team, cwd).unwrap_or_default();
    let auto = memdir::list_memories(MemoryScope::Auto, cwd).unwrap_or_default();

    if project.is_empty() && global.is_empty() && team.is_empty() && auto.is_empty() {
        return Ok(CommandResult::Output(
            "No memory entries found.\n\nUse `/memory set <key> <value>` to create one."
                .to_string(),
        ));
    }

    let mut lines = Vec::new();
    append_group(&mut lines, "Project memories", &project);
    append_group(&mut lines, "Global memories", &global);
    if features::enabled(Feature::TeamMemory) || !team.is_empty() {
        append_group(&mut lines, "Team memories", &team);
    }
    let auto_on = ctx.app_state.settings.auto_memory_enabled.unwrap_or(false);
    if auto_on || !auto.is_empty() {
        append_group(&mut lines, "Auto memories", &auto);
    }

    Ok(CommandResult::Output(lines.join("\n")))
}

fn append_group(lines: &mut Vec<String>, header: &str, entries: &[MemoryEntry]) {
    if entries.is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(format!("**{}** ({})", header, entries.len()));
    for e in entries {
        let cat = if e.category.is_empty() {
            String::new()
        } else {
            format!(" [{}]", e.category)
        };
        lines.push(format!("  {} - {}{}", e.key, truncate(&e.value, 60), cat));
    }
}

fn get_entry(key: &str, cwd: &Path) -> Result<CommandResult> {
    // Search project -> global -> team -> auto. First hit wins.
    for scope in [
        MemoryScope::Project,
        MemoryScope::Global,
        MemoryScope::Team,
        MemoryScope::Auto,
    ] {
        if let Ok(entry) = memdir::read_memory(key, scope, cwd) {
            return Ok(CommandResult::Output(format_entry(&entry, scope.as_str())));
        }
    }
    Ok(CommandResult::Output(format!(
        "Memory '{}' not found.",
        key
    )))
}

fn set_entry(key: &str, rest: &str, cwd: &Path) -> Result<CommandResult> {
    // Parse flags from the value string
    let mut value_parts = Vec::new();
    let mut scope = MemoryScope::Project;
    let mut category = String::new();

    for token in rest.split_whitespace() {
        match token {
            "--global" => scope = MemoryScope::Global,
            "--team" => scope = MemoryScope::Team,
            "--auto" => scope = MemoryScope::Auto,
            _ => {
                if let Some(cat) = token.strip_prefix("--category=") {
                    category = cat.to_string();
                } else {
                    value_parts.push(token);
                }
            }
        }
    }

    let value = value_parts.join(" ");
    if value.is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /memory set <key> <value> [--global|--team|--auto] [--category=<cat>]"
                .to_string(),
        ));
    }

    let entry = memdir::write_memory(key, &value, &category, scope, cwd)?;
    Ok(CommandResult::Output(format!(
        "Saved {} memory '{}': {}",
        scope.as_str(),
        entry.key,
        entry.value
    )))
}

fn rm_entry(key: &str, scope: MemoryScope, cwd: &Path) -> Result<CommandResult> {
    let deleted = memdir::delete_memory(key, scope, cwd)?;
    if deleted {
        Ok(CommandResult::Output(format!(
            "Deleted {} memory '{}'.",
            scope.as_str(),
            key
        )))
    } else {
        Ok(CommandResult::Output(format!(
            "Memory '{}' not found in {} scope.",
            key,
            scope.as_str()
        )))
    }
}

fn search_entries(query: &str, cwd: &Path) -> Result<CommandResult> {
    let mut results = Vec::new();
    for scope in [
        MemoryScope::Project,
        MemoryScope::Global,
        MemoryScope::Team,
        MemoryScope::Auto,
    ] {
        let hits = memdir::search_memories(query, scope, cwd).unwrap_or_default();
        for entry in hits {
            results.push((scope, entry));
        }
    }

    if results.is_empty() {
        return Ok(CommandResult::Output(format!(
            "No memories matching '{}'.",
            query
        )));
    }

    let mut lines = vec![format!(
        "Found {} result(s) for '{}':",
        results.len(),
        query
    )];
    for (scope, e) in &results {
        let cat = if e.category.is_empty() {
            String::new()
        } else {
            format!(" [{}]", e.category)
        };
        lines.push(format!(
            "  [{}] {} - {}{}",
            scope.as_str(),
            e.key,
            truncate(&e.value, 60),
            cat
        ));
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

// ---------------------------------------------------------------------------
// Auto-memory toggle (issue #45)
// ---------------------------------------------------------------------------

fn auto_toggle(action: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    match action {
        "on" | "off" => {
            let on = action == "on";
            let persist_msg = persist_auto_memory(on, ctx)?;
            ctx.app_state.settings.auto_memory_enabled = Some(on);

            Ok(CommandResult::Output(format!(
                "Auto-memory: {}\n{}\n\nNote: the auto-capture hook is not yet wired; \
                 this toggle persists the setting only.",
                if on { "ON" } else { "OFF" },
                persist_msg
            )))
        }
        "status" | "" => {
            let on = ctx.app_state.settings.auto_memory_enabled.unwrap_or(false);
            Ok(CommandResult::Output(format!(
                "Auto-memory: {} (setting key: autoMemoryEnabled)",
                if on { "ON" } else { "OFF" }
            )))
        }
        _ => Ok(CommandResult::Output(
            "Usage: /memory auto [on|off|status]".into(),
        )),
    }
}

fn persist_auto_memory(on: bool, ctx: &CommandContext) -> Result<String> {
    let path = auto_memory_settings_path(ctx);
    persist_auto_memory_to_path(on, &path)
}

#[cfg(not(test))]
fn auto_memory_settings_path(_ctx: &CommandContext) -> PathBuf {
    settings::user_settings_path()
}

#[cfg(test)]
fn auto_memory_settings_path(ctx: &CommandContext) -> PathBuf {
    ctx.cwd.join("settings.json")
}

fn persist_auto_memory_to_path(on: bool, path: &Path) -> Result<String> {
    // Missing settings still default, but an existing unreadable or invalid
    // file must abort so we don't replace it with a partial rewrite.
    let mut raw: settings::RawSettings = if path.exists() {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to load existing settings {}", path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to load existing settings {}", path.display()))?
    } else {
        settings::RawSettings::default()
    };
    raw.auto_memory_enabled = Some(on);
    settings::write_settings_file(path, &raw)
        .with_context(|| format!("Failed to persist setting to {}", path.display()))?;
    Ok(format!("Persisted to {}", path.display()))
}

// ---------------------------------------------------------------------------
// /memory open <scope>
// ---------------------------------------------------------------------------

fn open_dir(which: &str, cwd: &Path) -> Result<CommandResult> {
    let (label, dir): (&str, PathBuf) = match which {
        "auto" => ("auto-memory", cfg_paths::auto_memory_dir()),
        "team" => ("team-memory", cfg_paths::team_memory_dir(cwd)),
        "global" => ("global memory", cfg_paths::memory_dir_global()),
        "project" => ("project memory", cwd.join(".allthecodes").join("memory")),
        "" => {
            return Ok(CommandResult::Output(
                "Usage: /memory open <auto|team|global|project>".into(),
            ));
        }
        other => {
            return Ok(CommandResult::Output(format!(
                "Unknown scope: '{}'. Use auto|team|global|project.",
                other
            )));
        }
    };

    // Ensure the directory exists so the path resolves to something
    // openable. We intentionally don't spawn an external editor; the UI
    // layer (or the user) picks the right opener.
    if let Err(e) = fs::create_dir_all(&dir) {
        return Ok(CommandResult::Output(format!(
            "Could not create {} dir {}: {}",
            label,
            dir.display(),
            e
        )));
    }

    Ok(CommandResult::Output(format!(
        "{} directory:\n  {}",
        label,
        dir.display()
    )))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

fn format_entry(entry: &memdir::MemoryEntry, scope: &str) -> String {
    let mut lines = vec![
        format!("**{}** ({})", entry.key, scope),
        entry.value.clone(),
    ];
    if !entry.category.is_empty() {
        lines.push(format!("Category: {}", entry.category));
    }
    lines.push(format!("Created:  {}", entry.created_at));
    lines.push(format!("Updated:  {}", entry.updated_at));
    lines.join("\n")
}

#[cfg(test)]
mod tests;
