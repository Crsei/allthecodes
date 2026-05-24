//! `/team-onboarding` — generate a teammate-facing onboarding guide
//! (issue #63).
//!
//! This is a greenfield feature: the Bun reference does not expose a
//! public `/team-onboarding`, so the Rust implementation is grounded in
//! existing local/project/team state rather than a 1:1 port.
//!
//! The generated guide is Markdown. It walks a new teammate through:
//!
//!   1. Welcome — honoring the display name from onboarding state when
//!      available so the guide is personalized.
//!   2. Project overview — derived from `AGENTS.md` / `CLAUDE.md` / `README.md`
//!      the git `origin` URL when present.
//!   3. Common commands — filtered list of slash commands that belong on
//!      a new teammate's first page.
//!   4. Skills — whatever is registered in the skills registry.
//!   5. Active teams — names + member counts from `{data_root}/teams/`.
//!   6. Risk areas — a short list keyed off what the project state
//!      actually exposes (auth set-up status, pending team tasks, etc.).
//!
//! Subcommands:
//!
//!   /team-onboarding                  print the guide
//!   /team-onboarding save [path]      write the guide to a file
//!                                     (default: ONBOARDING_TEAM.md in cwd)

use std::path::{Path, PathBuf};

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_services::onboarding::{OnboardingState, OnboardingStore};

pub struct TeamOnboardingHandler;

#[async_trait]
impl CommandHandler for TeamOnboardingHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let store = OnboardingStore::open_default();
        let onboarding = store.load().unwrap_or_default();
        let guide = build_guide(&ctx.cwd, &onboarding);

        let (sub, rest) = split_sub(args);
        match sub.as_str() {
            "" | "show" | "print" => Ok(CommandResult::Output(guide)),
            "save" | "write" => {
                let target = resolve_save_path(&ctx.cwd, rest);
                match std::fs::write(&target, guide.as_bytes()) {
                    Ok(_) => Ok(CommandResult::Output(format!(
                        "Wrote teammate onboarding guide to {} ({} bytes).",
                        target.display(),
                        guide.len()
                    ))),
                    Err(e) => Ok(CommandResult::Output(format!(
                        "Could not write onboarding guide to {}: {}",
                        target.display(),
                        e
                    ))),
                }
            }
            "help" | "--help" | "-h" => Ok(CommandResult::Output(help_text())),
            other => Ok(CommandResult::Output(format!(
                "Unknown /team-onboarding subcommand '{}'. Run '/team-onboarding help'.",
                other
            ))),
        }
    }
}

fn help_text() -> String {
    [
        "/team-onboarding — generate a teammate onboarding guide.",
        "",
        "  /team-onboarding                 print the guide to the REPL",
        "  /team-onboarding save [path]     write to a file (default: ONBOARDING_TEAM.md)",
        "",
        "The guide pulls from real local state: AGENTS.md, README.md,",
        "the skills registry, teams on disk, and onboarding status. It's",
        "not a template — sections are empty-suppressed when they have",
        "nothing to say.",
    ]
    .join("\n")
}

// ---------------------------------------------------------------------------
// Guide construction
// ---------------------------------------------------------------------------

fn build_guide(cwd: &Path, onboarding: &OnboardingState) -> String {
    let project_name = derive_project_name(cwd);

    let mut out = String::new();
    out.push_str(&format!("# {} — Teammate Onboarding\n\n", project_name));
    append_welcome(&mut out, onboarding);
    append_project_overview(&mut out, cwd);
    append_common_commands(&mut out);
    append_skills(&mut out);
    append_teams_section(&mut out);
    append_scheduling_section(&mut out);
    append_risk_areas(&mut out, cwd, onboarding);
    append_footer(&mut out);
    out
}

fn append_welcome(out: &mut String, onboarding: &OnboardingState) {
    out.push_str("## Welcome\n\n");
    let greet = match &onboarding.display_name {
        Some(name) if !name.trim().is_empty() => format!("Hi — {} here's the short tour.", name),
        _ => "Hi — here's the short tour.".to_string(),
    };
    out.push_str(&greet);
    out.push_str(
        "\n\nThis guide was generated from the state on this machine right now. \
         If you see something stale, regenerate it with `/team-onboarding`.\n\n",
    );
}

fn append_project_overview(out: &mut String, cwd: &Path) {
    out.push_str("## Project overview\n\n");

    // Prefer AGENTS.md over CLAUDE.md.
    let agents_md = cwd.join("AGENTS.md");
    let claude_md = cwd.join("CLAUDE.md");
    let (md_path, md_label) = if agents_md.exists() {
        (agents_md, "AGENTS.md")
    } else if claude_md.exists() {
        (claude_md, "CLAUDE.md")
    } else {
        (agents_md, "AGENTS.md") // default for error case
    };
    if let Ok(contents) = std::fs::read_to_string(&md_path) {
        let summary = first_sentences(&strip_yaml_frontmatter(&contents), 5);
        if !summary.trim().is_empty() {
            out.push_str(&format!("From `{}`:\n\n", md_label));
            out.push_str(&quote_block(&summary));
            out.push_str("\n\n");
        }
    }

    let readme = ["README.md", "readme.md", "README.MD"]
        .iter()
        .map(|n| cwd.join(n))
        .find(|p| p.exists());
    if let Some(path) = readme {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let summary = first_sentences(&contents, 4);
            if !summary.trim().is_empty() {
                out.push_str(&format!(
                    "From `{}`:\n\n",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                out.push_str(&quote_block(&summary));
                out.push_str("\n\n");
            }
        }
    }

    if let Some(origin) = detect_git_origin(cwd) {
        out.push_str(&format!("- Git origin: `{}`\n", origin));
    }
    if let Some(branch) = detect_git_branch(cwd) {
        out.push_str(&format!("- Current branch: `{}`\n", branch));
    }
    out.push('\n');
}

fn append_common_commands(out: &mut String) {
    out.push_str("## Common slash commands\n\n");
    for (name, purpose) in COMMON_COMMANDS {
        out.push_str(&format!("- `{}` — {}\n", name, purpose));
    }
    out.push_str(
        "\nFor the full list, run `/help`. A teammate's first week usually \
         needs `/plan`, `/commit`, `/review`, and `/schedule`.\n\n",
    );
}

fn append_skills(out: &mut String) {
    let skills = allthecodes_skills::get_user_invocable_skills();
    out.push_str("## Skills\n\n");
    if skills.is_empty() {
        out.push_str(
            "No user-invocable skills are currently registered. Skills live \
             under `{data_root}/skills/` or `.allthecodes/skills/`; drop a \
             `SKILL.md` in either to make one available here.\n\n",
        );
        return;
    }
    for s in skills {
        let desc = if s.frontmatter.description.is_empty() {
            "no description"
        } else {
            s.frontmatter.description.trim()
        };
        out.push_str(&format!("- `/{}` — {}\n", s.name, one_line(desc)));
    }
    out.push('\n');
}

fn append_teams_section(out: &mut String) {
    out.push_str("## Active teams\n\n");
    let teams_root = allthecodes_config::paths::teams_dir();
    if !teams_root.exists() {
        out.push_str(
            "No team data directory yet — run `/team create <name>` to \
             bootstrap one when you need agent teams.\n\n",
        );
        return;
    }

    let names: Vec<String> = match std::fs::read_dir(&teams_root) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| team_exists(n))
            .collect(),
        Err(_) => Vec::new(),
    };

    if names.is_empty() {
        out.push_str(
            "No teams on disk yet. When you need one, `/team create <name>` \
             sets one up. `/team list` shows the current roster.\n\n",
        );
        return;
    }

    for name in names {
        let member_count = read_team_member_count(&name).unwrap_or(0);
        out.push_str(&format!("- `{}` — {} member(s)\n", name, member_count));
    }
    out.push_str("\nInteract via `/team status`, `/team spawn`, `/team send`.\n\n");
}

fn append_scheduling_section(out: &mut String) {
    out.push_str("## Scheduled work\n\n");
    let store = allthecodes_services::scheduler::SchedulerStore::open_default();
    match store.load() {
        Ok(tasks) if tasks.is_empty() => {
            out.push_str(
                "No scheduled tasks yet. Use `/loop <interval> <prompt>` for \
                 recurring prompts or `/schedule add` for a bare cron entry.\n\n",
            );
        }
        Ok(tasks) => {
            out.push_str(&format!("Currently {} scheduled task(s):\n\n", tasks.len()));
            for t in tasks.iter().take(8) {
                out.push_str(&format!(
                    "- `{}` — every {}s, next at {}\n",
                    t.name,
                    t.interval_seconds,
                    t.next_run_at.to_rfc3339()
                ));
            }
            if tasks.len() > 8 {
                out.push_str(&format!(
                    "- … and {} more — see `/schedule list`.\n",
                    tasks.len() - 8
                ));
            }
            out.push('\n');
        }
        Err(_) => {
            out.push_str(
                "Scheduled tasks are stored under `{data_root}/scheduled_tasks.json` \
                 but the file could not be read — likely a first-run state.\n\n",
            );
        }
    }
}

fn append_risk_areas(out: &mut String, cwd: &Path, onboarding: &OnboardingState) {
    out.push_str("## Risk areas to watch\n\n");
    let mut bullets: Vec<String> = Vec::new();

    if onboarding.is_first_run() {
        bullets.push(
            "The machine looks like it has never finished first-run onboarding. \
             Run `/login` (or set `ANTHROPIC_API_KEY`) before your first session."
                .into(),
        );
    } else if !onboarding.auth_onboarding_done {
        bullets.push(
            "Auth onboarding is incomplete — some commands may fail until \
             `/login` succeeds."
                .into(),
        );
    }

    if cwd.join(".allthecodes").is_dir() {
        bullets.push(
            "This project has a `.allthecodes/` directory — prefer project-level \
             settings over global ones when they conflict."
                .into(),
        );
    } else {
        bullets.push(
            "No `.allthecodes/` directory in this cwd. `/init` creates one when \
             you're ready to pin project-level settings."
                .into(),
        );
    }

    if !cwd.join("AGENTS.md").exists() && !cwd.join("CLAUDE.md").exists() {
        bullets.push(
            "No `AGENTS.md` in this project — expectations about tooling \
             may be implicit. Adding one helps teammates and the assistant \
             stay aligned."
                .into(),
        );
    }

    if bullets.is_empty() {
        out.push_str("Nothing obvious stands out right now.\n\n");
        return;
    }

    for b in bullets {
        out.push_str(&format!("- {}\n", b));
    }
    out.push('\n');
}

fn append_footer(out: &mut String) {
    out.push_str("---\n");
    out.push_str(&format!(
        "_Generated by `/team-onboarding` on {}._\n",
        chrono::Utc::now().to_rfc3339()
    ));
}

// ---------------------------------------------------------------------------
// Data inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct TeamFileSummary {
    #[serde(default)]
    members: Vec<serde_json::Value>,
}

fn team_exists(team_name: &str) -> bool {
    team_config_path(team_name).exists()
}

fn read_team_member_count(team_name: &str) -> Result<usize> {
    let raw = std::fs::read_to_string(team_config_path(team_name))?;
    let summary: TeamFileSummary = serde_json::from_str(&raw)?;
    Ok(summary.members.len())
}

fn team_config_path(team_name: &str) -> PathBuf {
    allthecodes_config::paths::teams_dir()
        .join(sanitize_team_name(team_name))
        .join("config.json")
}

fn sanitize_team_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Curated first-week command list. Intentionally short so the guide reads
/// well — the exhaustive list lives in `/help`.
const COMMON_COMMANDS: &[(&str, &str)] = &[
    ("/help", "list every slash command"),
    ("/context", "see how much of the context window is in use"),
    (
        "/plan",
        "enter plan mode — drafts an implementation plan before coding",
    ),
    (
        "/commit",
        "build a conventional commit from the current diff",
    ),
    ("/review", "review a PR through the `gh` CLI"),
    ("/recap", "summarize the current session"),
    ("/schedule", "manage local cron tasks"),
    (
        "/loop",
        "register a recurring prompt (with immediate first run)",
    ),
    ("/team", "manage Agent Teams (create, spawn, kill)"),
    (
        "/logout",
        "clear credentials + onboarding state for a clean hand-off",
    ),
];

fn derive_project_name(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Project".to_string())
}

fn detect_git_origin(cwd: &Path) -> Option<String> {
    let config = cwd.join(".git").join("config");
    let contents = std::fs::read_to_string(config).ok()?;
    let mut in_origin = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_origin = false;
        }
        if in_origin {
            if let Some(url) = trimmed.strip_prefix("url = ") {
                return Some(url.trim().to_string());
            }
        }
    }
    None
}

fn detect_git_branch(cwd: &Path) -> Option<String> {
    let head_path = cwd.join(".git").join("HEAD");
    let contents = std::fs::read_to_string(head_path).ok()?;
    let trimmed = contents.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") {
        Some(rest.to_string())
    } else {
        Some(trimmed.chars().take(7).collect())
    }
}

fn strip_yaml_frontmatter(text: &str) -> String {
    // Only strip if the very first line is `---`. Otherwise return the
    // original text unchanged — consuming the first line "just in case"
    // silently drops content when there's no frontmatter.
    let mut lines = text.lines();
    let first = lines.next();
    if first.map(|l| l.trim() == "---").unwrap_or(false) {
        // Advance past the closing `---`, then join the remainder.
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
        return lines.collect::<Vec<_>>().join("\n");
    }
    text.to_string()
}

fn first_sentences(text: &str, max: usize) -> String {
    // Skip pure markdown headers when computing "first sentences" so the
    // extract isn't just `# Foo`.
    let mut collected = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        collected.push(trimmed.to_string());
        if collected.len() >= max {
            break;
        }
    }
    collected.join("\n")
}

fn quote_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("> {}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

fn one_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

fn split_sub(args: &str) -> (String, &str) {
    match args.trim().split_once(char::is_whitespace) {
        Some((h, rest)) => (h.to_lowercase(), rest.trim()),
        None => (args.trim().to_lowercase(), ""),
    }
}

fn resolve_save_path(cwd: &Path, rest: &str) -> PathBuf {
    if rest.is_empty() {
        cwd.join("ONBOARDING_TEAM.md")
    } else {
        let raw = PathBuf::from(rest);
        if raw.is_absolute() {
            raw
        } else {
            cwd.join(raw)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn default_state() -> OnboardingState {
        OnboardingState::default()
    }

    #[test]
    fn guide_includes_every_section() {
        let dir = tempdir().unwrap();
        let guide = build_guide(dir.path(), &default_state());
        for section in [
            "Welcome",
            "Project overview",
            "Common slash commands",
            "Skills",
            "Active teams",
            "Scheduled work",
            "Risk areas",
        ] {
            assert!(
                guide.contains(&format!("## {}", section)),
                "missing section {} in {}",
                section,
                guide
            );
        }
    }

    #[test]
    fn project_overview_picks_up_agents_md() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "---\ntitle: Test\n---\n# Heading\n\nThis project is interesting.\nIt has rules.\n",
        )
        .unwrap();
        let guide = build_guide(dir.path(), &default_state());
        assert!(guide.contains("From `AGENTS.md`"));
        assert!(guide.contains("This project is interesting"));
    }

    #[test]
    fn project_overview_picks_up_readme() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("README.md"),
            "# README\nThis repo is the canonical source.",
        )
        .unwrap();
        let guide = build_guide(dir.path(), &default_state());
        assert!(guide.contains("From `README.md`"));
        assert!(guide.contains("canonical source"));
    }

    #[test]
    fn personalizes_welcome_when_display_name_present() {
        let dir = tempdir().unwrap();
        let state = OnboardingState {
            display_name: Some("Sam".into()),
            ..OnboardingState::default()
        };
        let guide = build_guide(dir.path(), &state);
        assert!(guide.contains("Sam"));
    }

    #[test]
    fn flags_first_run_in_risk_area() {
        let dir = tempdir().unwrap();
        let guide = build_guide(dir.path(), &default_state());
        assert!(guide.contains("first-run onboarding"));
    }

    #[test]
    fn flags_missing_agents_md() {
        let dir = tempdir().unwrap();
        let guide = build_guide(dir.path(), &default_state());
        assert!(guide.contains("No `AGENTS.md`"));
    }

    #[test]
    fn common_commands_include_the_core_set() {
        let dir = tempdir().unwrap();
        let guide = build_guide(dir.path(), &default_state());
        for cmd in ["/help", "/plan", "/commit", "/review", "/schedule", "/loop"] {
            assert!(guide.contains(cmd), "missing {} in guide", cmd);
        }
    }

    #[test]
    fn save_writes_to_disk() {
        let dir = tempdir().unwrap();
        let guide = build_guide(dir.path(), &default_state());
        let target = dir.path().join("ONBOARDING_TEAM.md");
        std::fs::write(&target, &guide).unwrap();
        assert!(target.exists());
        let reloaded = std::fs::read_to_string(&target).unwrap();
        assert!(reloaded.contains("Teammate Onboarding"));
    }

    #[test]
    fn resolve_save_path_defaults_to_cwd_filename() {
        let dir = tempdir().unwrap();
        let out = resolve_save_path(dir.path(), "");
        assert_eq!(out, dir.path().join("ONBOARDING_TEAM.md"));
    }

    #[test]
    fn resolve_save_path_respects_relative_input() {
        let dir = tempdir().unwrap();
        let out = resolve_save_path(dir.path(), "docs/onboarding.md");
        assert_eq!(out, dir.path().join("docs/onboarding.md"));
    }

    #[test]
    fn strip_yaml_frontmatter_removes_leading_block() {
        let input = "---\nkey: value\n---\nbody line\n";
        assert_eq!(strip_yaml_frontmatter(input), "body line");
    }

    #[test]
    fn strip_yaml_frontmatter_preserves_body_when_no_leading_dash() {
        let input = "no frontmatter\nline two";
        assert_eq!(strip_yaml_frontmatter(input), "no frontmatter\nline two");
    }

    #[test]
    fn first_sentences_skips_headers() {
        let input = "# Heading\n\nOne.\nTwo.\n# Another\nThree.\n";
        let out = first_sentences(input, 2);
        assert!(out.contains("One."));
        assert!(out.contains("Two."));
        assert!(!out.contains("# Heading"));
    }

    #[test]
    fn help_text_describes_save() {
        let help = help_text();
        assert!(help.contains("save"));
        assert!(help.contains("ONBOARDING_TEAM.md"));
    }
}
