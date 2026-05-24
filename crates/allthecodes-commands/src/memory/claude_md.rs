use super::*;

// ---------------------------------------------------------------------------

pub(super) fn show_memory(cwd: &Path) -> Result<CommandResult> {
    let context = config_claude_md::build_agents_md_context(cwd)?;

    if context.is_empty() {
        Ok(CommandResult::Output(
            "No AGENTS.md (or CLAUDE.md fallback) found in project hierarchy.\n\n\
             Use `/memory edit` to create one."
                .to_string(),
        ))
    } else {
        Ok(CommandResult::Output(format!(
            "**Project Instructions (AGENTS.md)**\n\n{}",
            context
        )))
    }
}

pub(super) fn show_paths(cwd: &Path) -> Result<CommandResult> {
    let files = config_claude_md::find_agents_md_files(cwd);

    if files.is_empty() {
        Ok(CommandResult::Output(
            "No AGENTS.md (or CLAUDE.md fallback) files found.".to_string(),
        ))
    } else {
        let mut lines = vec!["AGENTS.md files found:".to_string()];
        for path in &files {
            let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            lines.push(format!("  {} ({} bytes)", path.display(), size));
        }
        Ok(CommandResult::Output(lines.join("\n")))
    }
}

pub(super) fn edit_memory(cwd: &Path) -> Result<CommandResult> {
    let agents_md_path = cwd.join("AGENTS.md");
    if !agents_md_path.exists() {
        let template = "# AGENTS.md\n\n\
            This file provides guidance to allthecodes when working with code in this repository.\n\n\
            ## Project Overview\n\n\
            <!-- Describe your project here -->\n";
        fs::write(&agents_md_path, template)?;
    }
    Ok(CommandResult::Output(format!(
        "AGENTS.md location: {}\n\nEdit this file to add project instructions.",
        agents_md_path.display()
    )))
}

// ---------------------------------------------------------------------------
// Selector (default entry point: issue #45)
