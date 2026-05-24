use super::*;

// ---------------------------------------------------------------------------

/// Build the default selector listing: auto-memory header + grouped entries
/// from every enabled scope + directory shortcuts.
///
/// Output format:
///   Memory selector
///   Auto-memory: OFF (enable via /memory auto on)
///
///   [1] [global] my_key - 2026-04-20
///   [2] [project] auth_notes - 2026-04-18
///   ...
///   [a] open auto-memory dir   - <path>
///   [t] open team-memory dir   - <path>
///   [g] open global memory dir - <path>
///   [p] open project memory dir - <path>
///
/// The true interactive TUI picker belongs in the terminal frontend;
/// see the module-level TODO.
pub(super) fn selector(ctx: &CommandContext) -> Result<CommandResult> {
    let cwd = &ctx.cwd;
    let auto_on = ctx.app_state.settings.auto_memory_enabled.unwrap_or(false);
    let team_gate = features::enabled(Feature::TeamMemory);

    let mut lines = Vec::new();
    lines.push("**Memory selector**".to_string());
    lines.push(format!(
        "Auto-memory: {} (toggle via /memory auto on|off)",
        if auto_on { "ON" } else { "OFF" }
    ));
    lines.push(String::new());

    // AGENTS.md / CLAUDE.md files at the top, unnumbered because they're content-only.
    let md_files = config_claude_md::find_agents_md_files(cwd);
    if !md_files.is_empty() {
        lines.push(format!("AGENTS.md files ({}):", md_files.len()));
        for p in &md_files {
            lines.push(format!("  {}", p.display()));
        }
        lines.push(String::new());
    }

    // Enumerate memory entries, grouped by scope.
    let mut idx: usize = 0;
    let mut any_entries = false;

    let mut emit_group = |label: &str, entries: &[MemoryEntry], lines: &mut Vec<String>| {
        if entries.is_empty() {
            return;
        }
        any_entries = true;
        for e in entries {
            idx += 1;
            let date = e
                .updated_at
                .split('T')
                .next()
                .unwrap_or(&e.updated_at)
                .to_string();
            lines.push(format!("  [{}] [{}] {} - {}", idx, label, e.key, date));
        }
    };

    let global = memdir::list_memories(MemoryScope::Global, cwd).unwrap_or_default();
    let project = memdir::list_memories(MemoryScope::Project, cwd).unwrap_or_default();
    let team = memdir::list_memories(MemoryScope::Team, cwd).unwrap_or_default();
    let auto = memdir::list_memories(MemoryScope::Auto, cwd).unwrap_or_default();

    emit_group("global", &global, &mut lines);
    emit_group("project", &project, &mut lines);
    if team_gate || !team.is_empty() {
        // Show team entries even when the feature is gated off so legacy
        // data is never stranded; only injection into prompts is gated.
        emit_group("team", &team, &mut lines);
    }
    if auto_on || !auto.is_empty() {
        // Same principle: show auto entries even when the toggle is off
        // so users can inspect/purge past captures.
        emit_group("auto", &auto, &mut lines);
    }

    if !any_entries {
        lines.push("  (no memory entries; use `/memory set <key> <value>` to create one)".into());
    }

    lines.push(String::new());
    lines.push("Directory shortcuts:".into());
    lines.push(format!(
        "  [a] auto-memory dir    - {}",
        cfg_paths::auto_memory_dir().display()
    ));
    lines.push(format!(
        "  [t] team-memory dir    - {}",
        cfg_paths::team_memory_dir(cwd).display()
    ));
    lines.push(format!(
        "  [g] global memory dir  - {}",
        cfg_paths::memory_dir_global().display()
    ));
    lines.push(format!(
        "  [p] project memory dir - {}",
        cwd.join(".allthecodes").join("memory").display()
    ));
    lines.push(String::new());
    lines.push("Open a directory with `/memory open <auto|team|global|project>`.".into());

    Ok(CommandResult::Output(lines.join("\n")))
}

// ---------------------------------------------------------------------------
// Memdir subcommands
