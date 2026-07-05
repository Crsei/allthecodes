//! `/skills` command: list, inspect, diagnose, and reload skill packages.

use allthecodes_engine::services::background_review::{
    self, BackgroundReviewProposal, BackgroundReviewProposalKind,
};
use anyhow::Result;
use async_trait::async_trait;
use std::error::Error;
use std::sync::{OnceLock, RwLock};

use crate::{
    search_format::format_discovery_search_results, CommandContext, CommandHandler, CommandResult,
};

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

        if let Some(output) = handle_proposal_command(arg, ctx)? {
            return Ok(CommandResult::Output(output));
        }

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

        if let Some(query) = search_query_arg(arg) {
            return Ok(CommandResult::Output(handle_skill_search(query)));
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

fn search_query_arg(arg: &str) -> Option<&str> {
    let trimmed = arg.trim();
    if trimmed == "search" {
        return Some("");
    }
    trimmed.strip_prefix("search ").map(str::trim)
}

fn handle_skill_search(query: &str) -> String {
    if query.trim().is_empty() {
        return "Usage: /skills search <query>".to_string();
    }

    match allthecodes_tools::skills::run_skill_search(
        allthecodes_tools::discovery_search::DiscoverySearchInput {
            query: query.to_string(),
            source_filter: Some("all".to_string()),
            max_results: 10,
            include_summaries: true,
        },
    ) {
        Ok(output) => {
            let results = serde_json::from_value::<
                Vec<allthecodes_tools::discovery_search::DiscoverySearchResult>,
            >(output.data["results"].clone())
            .unwrap_or_default();
            format_discovery_search_results(
                "Skill search results",
                query,
                &results,
                &[
                    "Use /skills <name> for exact skill details.",
                    "Use SkillSearch for normalized local skill discovery metadata.",
                ],
            )
        }
        Err(error) => format!("Skill search failed: {error}"),
    }
}

fn handle_proposal_command(arg: &str, ctx: &CommandContext) -> Result<Option<String>> {
    let mut parts = arg.split_whitespace();
    let Some(command) = parts.next() else {
        return Ok(None);
    };
    match command {
        "pending" => Ok(Some(format_pending_proposals(&ctx.cwd)?)),
        "diff" => {
            let id = parts.next().unwrap_or_default();
            if id.is_empty() {
                return Ok(Some("Usage: /skills diff <id>".to_string()));
            }
            match allthecodes_skills::load_skill_proposal(id, &ctx.cwd) {
                Ok(proposal) => Ok(Some(format!(
                    "Skill proposal {}\nAction: {:?}\nSkill: {}\nProposed path: {}\n\n{}",
                    proposal.id,
                    proposal.action,
                    proposal.skill_name,
                    proposal.proposed_path.display(),
                    proposal.markdown
                ))),
                Err(_) => match background_review::load_background_review_proposal(id) {
                    Ok(proposal) if is_skill_review_proposal(&proposal) => Ok(Some(format!(
                        "Background review proposal {}\nKind: {:?}\nSource session: {}\nSummary: {}\n\n{}",
                        proposal.id,
                        proposal.kind,
                        proposal.source_session_id,
                        proposal.summary,
                        serde_json::to_string_pretty(&proposal.payload).unwrap_or_default()
                    ))),
                    _ => Ok(Some(format!("Skill proposal '{}' not found.", id))),
                },
            }
        }
        "approve" => {
            let id = parts.next().unwrap_or_default();
            if id.is_empty() {
                return Ok(Some("Usage: /skills approve <id>".to_string()));
            }
            match allthecodes_skills::approve_skill_proposal(id, &ctx.cwd) {
                Ok(path) => Ok(Some(format!(
                    "Approved skill proposal {}.\nWrote {}.\nUse /skills reload if the skill list is already loaded.",
                    id,
                    path.display()
                ))),
                Err(_) => approve_background_skill_proposal(id, &ctx.cwd).map(Some),
            }
        }
        "reject" => {
            let id = parts.next().unwrap_or_default();
            if id.is_empty() {
                return Ok(Some("Usage: /skills reject <id>".to_string()));
            }
            match allthecodes_skills::reject_skill_proposal(id, &ctx.cwd) {
                Ok(proposal) => Ok(Some(format!(
                    "Rejected skill proposal {} for '{}'.",
                    proposal.id, proposal.skill_name
                ))),
                Err(_) => match background_review::load_background_review_proposal(id) {
                    Ok(proposal) if is_skill_review_proposal(&proposal) => {
                        background_review::reject_background_review_proposal(id)?;
                        Ok(Some(format!("Rejected skill proposal {}.", proposal.id)))
                    }
                    _ => Ok(Some(format!("Skill proposal '{}' not found.", id))),
                },
            }
        }
        _ => Ok(None),
    }
}

fn format_pending_proposals(cwd: &std::path::Path) -> Result<String> {
    let proposals = allthecodes_skills::list_skill_proposals(cwd).map_err(skill_error)?;
    let review_proposals = background_review::list_background_review_proposals()?
        .into_iter()
        .filter(is_skill_review_proposal)
        .collect::<Vec<_>>();
    if proposals.is_empty() && review_proposals.is_empty() {
        return Ok("No pending skill proposals.".to_string());
    }
    let mut lines = vec![format!(
        "Pending skill proposals ({})",
        proposals.len() + review_proposals.len()
    )];
    for proposal in proposals {
        lines.push(format!(
            "  {} [{}] {} -> {}",
            proposal.id,
            match proposal.scope {
                allthecodes_skills::SkillProposalScope::User => "user",
                allthecodes_skills::SkillProposalScope::Project => "project",
            },
            proposal.skill_name,
            proposal.proposed_path.display()
        ));
    }
    for proposal in review_proposals {
        lines.push(format!(
            "  {} [background {:?}] {}",
            proposal.id,
            proposal.kind,
            truncate(&proposal.summary, 80)
        ));
    }
    Ok(lines.join("\n"))
}

fn skill_error(error: Box<dyn Error + Send + Sync + 'static>) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

fn is_skill_review_proposal(proposal: &BackgroundReviewProposal) -> bool {
    matches!(
        proposal.kind,
        BackgroundReviewProposalKind::SkillCreate
            | BackgroundReviewProposalKind::SkillPatch
            | BackgroundReviewProposalKind::WorkflowWarning
    )
}

fn approve_background_skill_proposal(id: &str, cwd: &std::path::Path) -> Result<String> {
    let proposal = match background_review::load_background_review_proposal(id) {
        Ok(proposal) if is_skill_review_proposal(&proposal) => proposal,
        _ => return Ok(format!("Skill proposal '{}' not found.", id)),
    };
    if proposal.kind == BackgroundReviewProposalKind::WorkflowWarning {
        background_review::reject_background_review_proposal(id)?;
        return Ok(format!(
            "Acknowledged background review proposal {}. No skill was written.",
            id
        ));
    }

    let Some(draft) = skill_draft_from_review(&proposal) else {
        return Ok(format!(
            "Skill proposal '{}' has no concrete skill payload. No skill was written.",
            id
        ));
    };
    let staged = allthecodes_skills::stage_skill_proposal(draft, cwd).map_err(skill_error)?;
    let path = allthecodes_skills::approve_skill_proposal(&staged.id, cwd).map_err(skill_error)?;
    background_review::reject_background_review_proposal(id)?;
    Ok(format!(
        "Approved skill proposal {}.\nWrote {}.\nUse /skills reload if the skill list is already loaded.",
        id,
        path.display()
    ))
}

fn skill_draft_from_review(
    proposal: &BackgroundReviewProposal,
) -> Option<allthecodes_skills::SkillProposalDraft> {
    let skill = proposal.payload.get("skill").unwrap_or(&proposal.payload);
    let skill_name = skill.get("skill_name")?.as_str()?.to_string();
    let markdown = skill.get("markdown")?.as_str()?.to_string();
    let scope = match skill.get("scope").and_then(|value| value.as_str()) {
        Some("user") => allthecodes_skills::SkillProposalScope::User,
        _ => allthecodes_skills::SkillProposalScope::Project,
    };
    let action = match proposal.kind {
        BackgroundReviewProposalKind::SkillPatch => allthecodes_skills::SkillProposalAction::Patch,
        _ => allthecodes_skills::SkillProposalAction::Create,
    };
    Some(allthecodes_skills::SkillProposalDraft {
        action,
        scope,
        skill_name,
        source_session_id: Some(proposal.source_session_id.clone()),
        markdown,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
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
    use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct SkillRegistryGuard;

    impl SkillRegistryGuard {
        fn new(skills: Vec<SkillDefinition>) -> Self {
            allthecodes_skills::clear_skills();
            for skill in skills {
                allthecodes_skills::register_skill(skill);
            }
            Self
        }
    }

    impl Drop for SkillRegistryGuard {
        fn drop(&mut self) {
            allthecodes_skills::clear_skills();
        }
    }

    fn make_skill(name: &str, description: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source: SkillSource::User,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                name: Some(name.to_string()),
                description: description.to_string(),
                when_to_use: Some("Use this skill for focused review workflows".to_string()),
                user_invocable: true,
                ..Default::default()
            },
            prompt_body: format!("Prompt body for {name}"),
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

    #[tokio::test]
    #[serial_test::serial]
    async fn test_skills_search_outputs_reasons_and_next_action() {
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            "Review Rust code and cargo test failures",
        )]);
        let handler = SkillsHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("search rust", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Skill search results for 'rust'"), "{text}");
                assert!(text.contains("rust-review"), "{text}");
                assert!(text.contains("matches:"), "{text}");
                assert!(text.contains("Next:"), "{text}");
                assert!(text.contains("/skills rust-review"), "{text}");
                assert!(text.contains("SkillSearch"), "{text}");
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_skills_search_empty_query_shows_usage() {
        let handler = SkillsHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("search", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage: /skills search <query>"), "{text}");
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_skills_proposal_commands_approve_and_reject() {
        allthecodes_skills::clear_skills();
        let tmp = tempfile::tempdir().unwrap();
        let mut ctx = test_ctx();
        ctx.cwd = tmp.path().join("project");
        std::fs::create_dir_all(&ctx.cwd).unwrap();

        let proposal = allthecodes_skills::stage_skill_proposal(
            allthecodes_skills::SkillProposalDraft {
                action: allthecodes_skills::SkillProposalAction::Create,
                scope: allthecodes_skills::SkillProposalScope::Project,
                skill_name: "queued-review".to_string(),
                source_session_id: Some("session-skills".to_string()),
                markdown: "---\ndescription: Review queued changes.\n---\nCheck the patch."
                    .to_string(),
            },
            &ctx.cwd,
        )
        .unwrap();

        let handler = SkillsHandler;
        let pending = handler.execute("pending", &mut ctx).await.unwrap();
        match pending {
            CommandResult::Output(text) => assert!(text.contains(&proposal.id)),
            _ => panic!("Expected Output"),
        }

        let diff = handler
            .execute(&format!("diff {}", proposal.id), &mut ctx)
            .await
            .unwrap();
        match diff {
            CommandResult::Output(text) => assert!(text.contains("Review queued changes")),
            _ => panic!("Expected Output"),
        }

        let approve = handler
            .execute(&format!("approve {}", proposal.id), &mut ctx)
            .await
            .unwrap();
        match approve {
            CommandResult::Output(text) => assert!(text.contains("Approved skill proposal")),
            _ => panic!("Expected Output"),
        }
        assert!(ctx
            .cwd
            .join(".allthecodes")
            .join("skills")
            .join("queued-review")
            .join("SKILL.md")
            .exists());

        let rejected = allthecodes_skills::stage_skill_proposal(
            allthecodes_skills::SkillProposalDraft {
                action: allthecodes_skills::SkillProposalAction::Create,
                scope: allthecodes_skills::SkillProposalScope::Project,
                skill_name: "rejected-review".to_string(),
                source_session_id: None,
                markdown: "---\ndescription: Rejected review.\n---\nDo not install.".to_string(),
            },
            &ctx.cwd,
        )
        .unwrap();
        let reject = handler
            .execute(&format!("reject {}", rejected.id), &mut ctx)
            .await
            .unwrap();
        match reject {
            CommandResult::Output(text) => assert!(text.contains("Rejected skill proposal")),
            _ => panic!("Expected Output"),
        }
        assert!(!ctx
            .cwd
            .join(".allthecodes")
            .join("skills")
            .join("rejected-review")
            .join("SKILL.md")
            .exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_skills_pending_lists_background_review_without_writing_skill() {
        allthecodes_skills::clear_skills();
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let mut ctx = test_ctx();
        ctx.cwd = tmp.path().join("project");
        std::fs::create_dir_all(&ctx.cwd).unwrap();

        let proposal =
            allthecodes_engine::services::background_review::stage_background_review_if_due(
                allthecodes_engine::services::background_review::BackgroundReviewInput {
                    source_session_id: "skills-review-session".to_string(),
                    cwd: ctx.cwd.to_string_lossy().to_string(),
                    turn_count: 1,
                    replay_seq_start: None,
                    replay_seq_end: None,
                    recent_summary: "review skill behavior".to_string(),
                    tool_errors: Vec::new(),
                    similar_session_hits: Vec::new(),
                },
                &allthecodes_engine::services::background_review::BackgroundReviewConfig {
                    enabled: true,
                    turn_threshold: 1,
                },
            )
            .unwrap()
            .unwrap();

        let handler = SkillsHandler;
        let pending = handler.execute("pending", &mut ctx).await.unwrap();
        match pending {
            CommandResult::Output(text) => assert!(text.contains(&proposal.id)),
            _ => panic!("Expected Output"),
        }

        let approve = handler
            .execute(&format!("approve {}", proposal.id), &mut ctx)
            .await
            .unwrap();
        match approve {
            CommandResult::Output(text) => assert!(text.contains("No skill was written")),
            _ => panic!("Expected Output"),
        }

        assert!(!ctx.cwd.join(".allthecodes").join("skills").exists());
        assert!(
            allthecodes_engine::services::background_review::list_background_review_proposals()
                .unwrap()
                .is_empty()
        );
    }
}
