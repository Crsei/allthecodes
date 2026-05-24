//! `/skills` command: list, inspect, diagnose, and reload skill packages.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct SkillsHandler;

pub type PluginSkillsProvider = fn() -> Vec<allthecodes_skills::SkillDefinition>;

static PLUGIN_SKILLS_PROVIDER: OnceLock<RwLock<Option<PluginSkillsProvider>>> = OnceLock::new();

/// Install the runtime-owned plugin skill discovery hook used by `/skills reload`.
///
/// Plugin registry/cache state is owned by the root/plugin runtime today; this
/// adapter keeps cc-commands from depending on root-private plugin modules.
pub fn set_plugin_skills_provider(provider: PluginSkillsProvider) {
    let slot = PLUGIN_SKILLS_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

#[async_trait]
impl CommandHandler for SkillsHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let arg = args.trim();

        if arg == "reload" {
            let plugin_skills = match plugin_skills_for_reload() {
                Ok(skills) => skills,
                Err(error) => {
                    return Ok(CommandResult::Output(format!(
                        "Cannot reload skills with full plugin support: {}",
                        error
                    )));
                }
            };
            let report = allthecodes_skills::reload_skills_with_extra(
                &allthecodes_config::paths::skills_dir_global(),
                Some(&ctx.cwd),
                plugin_skills,
                allthecodes_skills::SkillLoadOptions::for_app_version(env!("CARGO_PKG_VERSION")),
            );
            return Ok(CommandResult::Output(format_reload_report(&report)));
        }

        if arg == "diagnostics" {
            return Ok(CommandResult::Output(format_diagnostics()));
        }

        let all = allthecodes_skills::get_all_skills();

        if !arg.is_empty() && arg != "list" && !arg.starts_with("--sort") {
            if let Some(skill) = all
                .iter()
                .find(|s| s.name == arg || s.display_name() == arg)
            {
                return Ok(CommandResult::Output(format_skill_detail(skill)));
            }
            return Ok(CommandResult::Output(format!(
                "Skill '{}' not found. Use /skills to list all available skills.",
                arg
            )));
        }

        if all.is_empty() {
            return Ok(CommandResult::Output(
                "No skills loaded.\n\n\
                 Bundled skills: simplify, remember, debug, stuck, update-config\n\
                 Place custom skills in ~/.allthecodes/skills/<name>/SKILL.md"
                    .to_string(),
            ));
        }

        // Determine sort mode
        let sort_mode = parse_sort_arg(arg);

        let usage_data = allthecodes_skills::ranked_skill_usage();
        let usage_by_name: std::collections::HashMap<&str, f64> = usage_data
            .iter()
            .map(|d| (d.name.as_str(), d.rolling_score))
            .collect();

        // Collect and sort skills
        let mut sorted_skills: Vec<&allthecodes_skills::SkillDefinition> = all.iter().collect();
        match sort_mode {
            SortMode::Name => {
                sorted_skills.sort_by(|a, b| a.display_name().cmp(b.display_name()));
            }
            SortMode::Source => {
                sorted_skills.sort_by(|a, b| {
                    source_sort_key(&a.source)
                        .cmp(&source_sort_key(&b.source))
                        .then_with(|| a.display_name().cmp(b.display_name()))
                });
            }
            SortMode::Usage => {
                sorted_skills.sort_by(|a, b| {
                    let a_score = usage_by_name.get(a.name.as_str()).copied().unwrap_or(0.0);
                    let b_score = usage_by_name.get(b.name.as_str()).copied().unwrap_or(0.0);
                    b_score
                        .partial_cmp(&a_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.display_name().cmp(b.display_name()))
                });
            }
        }

        let mut lines = Vec::new();
        lines.push(format!("Available Skills ({} total)", all.len()));
        lines.push(format!(
            "Registry revision: {}",
            allthecodes_skills::registry_revision()
        ));

        // Show sort mode indicator
        let sort_hint = match sort_mode {
            SortMode::Name => "sorted by name",
            SortMode::Source => "sorted by source",
            SortMode::Usage => "sorted by usage",
        };
        lines.push(format!("({})", sort_hint));
        lines.push("-".repeat(60));

        for skill in &sorted_skills {
            let usage_score = usage_by_name
                .get(skill.name.as_str())
                .copied()
                .unwrap_or(0.0);
            let invocability = invocability_tag(skill);
            let score_str = format_score(usage_score);

            lines.push(format!(
                "  {} {}{} {}@{} -- {}",
                source_tag(&skill.source),
                invocability,
                skill.display_name(),
                skill.effective_version(),
                score_str,
                skill.frontmatter.description
            ));
        }

        lines.push(String::new());
        lines.push("Use /skills <name> for details on a specific skill.".to_string());
        lines.push("Use /skills reload to hot-reload skill packages.".to_string());
        lines.push("Use /skills diagnostics to show validation diagnostics.".to_string());
        lines.push("Use /skills --sort <name|source|usage> to change sort order.".to_string());

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

fn plugin_skills_for_reload(
) -> std::result::Result<Vec<allthecodes_skills::SkillDefinition>, String> {
    crate::runtime::ensure_runtime_installed();
    let Some(slot) = PLUGIN_SKILLS_PROVIDER.get() else {
        return Err(
            "plugin skills runtime adapter is not installed; root must inject plugin discovery"
                .to_string(),
        );
    };
    match slot.read() {
        Ok(guard) => match *guard {
            Some(provider) => Ok(provider()),
            None => Err(
                "plugin skills runtime adapter is empty; root must inject plugin discovery"
                    .to_string(),
            ),
        },
        Err(_) => Err("plugin skills runtime adapter lock is poisoned".to_string()),
    }
}

fn source_tag(source: &allthecodes_skills::SkillSource) -> &'static str {
    match source {
        allthecodes_skills::SkillSource::Bundled => "[bundled]",
        allthecodes_skills::SkillSource::User => "[user]",
        allthecodes_skills::SkillSource::Project => "[project]",
        allthecodes_skills::SkillSource::Plugin(_) => "[plugin]",
        allthecodes_skills::SkillSource::Mcp(_) => "[mcp]",
    }
}

fn source_sort_key(source: &allthecodes_skills::SkillSource) -> u8 {
    match source {
        allthecodes_skills::SkillSource::Bundled => 0,
        allthecodes_skills::SkillSource::User => 1,
        allthecodes_skills::SkillSource::Project => 2,
        allthecodes_skills::SkillSource::Plugin(_) => 3,
        allthecodes_skills::SkillSource::Mcp(_) => 4,
    }
}

fn invocability_tag(skill: &allthecodes_skills::SkillDefinition) -> &'static str {
    match (skill.is_user_invocable(), skill.is_model_invocable()) {
        (true, false) => "(user) ",
        (false, true) => "(model) ",
        (true, true) => "(both) ",
        (false, false) => "",
    }
}

fn format_score(score: f64) -> String {
    if score < 0.01 {
        String::new()
    } else if score < 0.1 {
        format!("score={:.3}", score)
    } else if score < 1.0 {
        format!("score={:.2}", score)
    } else {
        "score=1.0".to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    Name,
    Source,
    Usage,
}

fn parse_sort_arg(args: &str) -> SortMode {
    let trimmed = args.trim();
    if let Some(sort_val) = trimmed.strip_prefix("--sort ") {
        match sort_val.trim() {
            "source" => return SortMode::Source,
            "usage" => return SortMode::Usage,
            _ => return SortMode::Name,
        }
    }
    if trimmed == "list" || trimmed.is_empty() {
        SortMode::Name
    } else {
        // If it's a specific skill name, the caller already handled it above
        SortMode::Name
    }
}

fn format_skill_detail(skill: &allthecodes_skills::SkillDefinition) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Skill: {}", skill.display_name()));
    lines.push(format!("Canonical name: {}", skill.name));
    lines.push(format!("Source: {:?}", skill.source));
    lines.push(format!("Version: {}", skill.effective_version()));
    lines.push(format!("Description: {}", skill.frontmatter.description));
    if let Some(ref when) = skill.frontmatter.when_to_use {
        lines.push(format!("When to use: {}", when));
    }
    if !skill.frontmatter.allowed_tools.is_empty() {
        lines.push(format!(
            "Allowed tools: {}",
            skill.frontmatter.allowed_tools.join(", ")
        ));
    }
    lines.push(format!(
        "User invocable: {}",
        skill.frontmatter.user_invocable
    ));
    lines.push(format!("Model invocable: {}", skill.is_model_invocable()));
    if let Some(ref req) = skill.frontmatter.compatible_app_version {
        lines.push(format!("Compatible app version: {}", req));
    }
    if !skill.frontmatter.dependencies.is_empty() {
        let deps = skill
            .frontmatter
            .dependencies
            .iter()
            .map(|d| d.label())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Dependencies: {}", deps));
    }
    if !skill.frontmatter.paths.is_empty() {
        lines.push(format!(
            "Path filters: {}",
            skill.frontmatter.paths.join(", ")
        ));
    }
    if !skill.frontmatter.assets.is_empty() {
        lines.push(format!("Assets: {}", skill.frontmatter.assets.join(", ")));
    }
    if !skill.frontmatter.entry_docs.is_empty() {
        lines.push(format!(
            "Entry docs: {}",
            skill.frontmatter.entry_docs.join(", ")
        ));
    }
    if let Some(ref dir) = skill.base_dir {
        lines.push(format!("Base dir: {}", dir.display()));
    }
    lines.join("\n")
}

fn format_reload_report(report: &allthecodes_skills::SkillLoadReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Reloaded {} skill(s) at revision {}.",
        report.loaded, report.revision
    ));
    if report.skipped > 0 {
        lines.push(format!(
            "Skipped {} invalid or duplicate candidate(s).",
            report.skipped
        ));
    }
    lines.push(format!(
        "Diagnostics: {} warning(s), {} error(s).",
        report.warning_count(),
        report.error_count()
    ));

    for diagnostic in report.diagnostics.iter().take(10) {
        lines.push(format!(
            "  - {:?} {}{}: {}",
            diagnostic.severity,
            diagnostic.code,
            diagnostic
                .skill
                .as_deref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default(),
            diagnostic.message
        ));
    }

    if report.diagnostics.len() > 10 {
        lines.push(format!(
            "  ... {} more diagnostic(s). Use /skills diagnostics for the full list.",
            report.diagnostics.len() - 10
        ));
    }

    lines.join("\n")
}

fn format_diagnostics() -> String {
    let diagnostics = allthecodes_skills::get_skill_diagnostics();
    if diagnostics.is_empty() {
        return "No skill diagnostics recorded.".to_string();
    }

    let mut lines = vec![format!("Skill Diagnostics ({} total)", diagnostics.len())];
    for diagnostic in diagnostics {
        let mut line = format!(
            "{:?} {}: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        );
        if let Some(skill) = diagnostic.skill {
            line.push_str(&format!(" [skill: {}]", skill));
        }
        if let Some(path) = diagnostic.path {
            line.push_str(&format!(" [path: {}]", path.display()));
        }
        lines.push(line);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_skills_list() {
        let handler = SkillsHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Available Skills") || text.contains("No skills loaded"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_skill() {
        let handler = SkillsHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("nonexistent", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("not found")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_skills_diagnostics() {
        let handler = SkillsHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("diagnostics", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("Skill Diagnostics") || text.contains("No skill diagnostics")
                );
            }
            _ => panic!("Expected Output"),
        }
    }
}
