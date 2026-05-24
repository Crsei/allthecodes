//! `/simplify` — multi-agent code simplification command (issue #62).
//!
//! Exposes the bundled `simplify` skill as a user-facing slash command.
//! When the fork runtime adapter is wired, it runs a **multi-agent review**
//! by spawning parallel reviewer forks —
//! one each for the three canonical concerns:
//!
//!   1. **reuse**      — duplicated logic, missed shared helpers
//!   2. **quality**    — naming, readability, cyclomatic complexity
//!   3. **efficiency** — avoidable allocations / copies / hot paths
//!
//! The three reviewer outputs are joined into a single action-oriented
//! summary and returned via [`CommandResult::Output`]. Unlike a naive
//! `/recap`-style `CommandResult::Query`, this does NOT pollute the main
//! transcript.
//!
//! Fallback: if the bundled `simplify` skill is missing (e.g. the registry
//! was cleared in tests), we return a clear error instead of falling back
//! silently.
//!
//! Usage:
//!   /simplify                 — review recently changed code across concerns
//!   /simplify <file-or-dir>   — scope the review to a file/directory
//!   /simplify --single         — run a single-agent pass (no parallel review)

use allthecodes_tools::tool::Tools;
use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::{CommandForkOutcome, CommandForkParams};
use crate::{CommandContext, CommandHandler, CommandResult};

const SIMPLIFY_SKILL: &str = "simplify";

/// Canonical review angles for the parallel multi-agent pass.
const REVIEW_ANGLES: &[(&str, &str)] = &[
    (
        "reuse",
        "Focus on REUSE: look for duplicated logic, near-identical code paths, \
         and places where an existing helper should be used. Propose concrete \
         refactors that consolidate duplication.",
    ),
    (
        "quality",
        "Focus on QUALITY: naming clarity, readability, cyclomatic complexity, \
         and consistency with surrounding style. Propose concrete renamings \
         or small restructurings.",
    ),
    (
        "efficiency",
        "Focus on EFFICIENCY: avoidable allocations, unnecessary clones, \
         redundant iteration, obvious hot-path regressions. Propose concrete \
         edits, not speculative rewrites.",
    ),
];

pub struct SimplifyHandler;

#[async_trait]
impl CommandHandler for SimplifyHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let trimmed = args.trim();
        let (single_agent, scope) = parse_args(trimmed);

        let skill = match allthecodes_skills::find_skill(SIMPLIFY_SKILL) {
            Some(s) => s,
            None => {
                return Ok(CommandResult::Output(
                    "/simplify error: bundled 'simplify' skill is not registered. \
                     Skills may not have been initialized."
                        .to_string(),
                ));
            }
        };

        let cwd = ctx.cwd.to_string_lossy().to_string();
        let model = ctx.app_state.main_loop_model.clone();
        let allowed = &skill.frontmatter.allowed_tools;

        let tools = crate::runtime::available_tools()
            .into_iter()
            .filter(|t| allowed.is_empty() || allowed.iter().any(|a| a == t.name()))
            .collect::<Vec<_>>();

        if single_agent {
            return run_single(&skill, scope.as_deref(), cwd, model, tools).await;
        }

        run_multi_agent(&skill, scope.as_deref(), cwd, model, tools).await
    }
}

/// Parse the argument string into `(single_agent, scope)`.
fn parse_args(raw: &str) -> (bool, Option<String>) {
    let mut single_agent = false;
    let mut scope: Vec<String> = Vec::new();
    for token in raw.split_whitespace() {
        match token {
            "--single" | "-1" => single_agent = true,
            other => scope.push(other.to_string()),
        }
    }
    let scope_str = if scope.is_empty() {
        None
    } else {
        Some(scope.join(" "))
    };
    (single_agent, scope_str)
}

/// Single-pass mode — one forked agent invokes the skill directly.
async fn run_single(
    skill: &allthecodes_skills::SkillDefinition,
    scope: Option<&str>,
    cwd: String,
    model: String,
    tools: Tools,
) -> Result<CommandResult> {
    let prompt = build_single_prompt(skill, scope);
    let params = CommandForkParams {
        prompt,
        cwd,
        model: model.clone(),
        fallback_model: Some(model),
        tools,
        max_turns: Some(20),
        parent_messages: None,
        append_system_prompt: None,
        custom_system_prompt: None,
    };
    match crate::runtime::run_fork(params).await {
        Ok(outcome) => Ok(CommandResult::Output(format_single(outcome))),
        Err(e) => Ok(CommandResult::Output(format!(
            "/simplify error: fork failed: {}",
            e
        ))),
    }
}

/// Multi-agent mode — fan out to 3 parallel reviewer forks and aggregate.
async fn run_multi_agent(
    skill: &allthecodes_skills::SkillDefinition,
    scope: Option<&str>,
    cwd: String,
    model: String,
    tools: Tools,
) -> Result<CommandResult> {
    let base = skill.expand_prompt(scope.unwrap_or(""), None);

    let mut futs = Vec::new();
    for (angle, extra) in REVIEW_ANGLES {
        let prompt = format!("{}\n\n--- Reviewer angle: {} ---\n{}", base, angle, extra);
        let params = CommandForkParams {
            prompt,
            cwd: cwd.clone(),
            model: model.clone(),
            fallback_model: Some(model.clone()),
            tools: tools.clone(),
            max_turns: Some(20),
            parent_messages: None,
            append_system_prompt: None,
            custom_system_prompt: None,
        };
        futs.push(crate::runtime::run_fork(params));
    }

    let outcomes = futures::future::join_all(futs).await;
    Ok(CommandResult::Output(format_multi(&outcomes)))
}

/// Build the single-pass prompt. Injects optional scope into the skill body.
fn build_single_prompt(skill: &allthecodes_skills::SkillDefinition, scope: Option<&str>) -> String {
    let base = skill.expand_prompt(scope.unwrap_or(""), None);
    match scope {
        Some(s) if !s.is_empty() => format!("{}\n\nScope: {}", base, s),
        _ => base,
    }
}

fn format_single(outcome: CommandForkOutcome) -> String {
    let header = format!("/simplify (single-agent fork, {} ms)", outcome.duration_ms);
    if outcome.had_error {
        format!("{}\nerror: {}", header, outcome.text)
    } else {
        format!("{}\n\n{}", header, outcome.text)
    }
}

/// Join the three reviewer outputs into a single action-oriented summary.
fn format_multi(outcomes: &[Result<CommandForkOutcome>]) -> String {
    let mut out = String::from("/simplify (multi-agent review)\n");
    for (i, (angle, _)) in REVIEW_ANGLES.iter().enumerate() {
        out.push('\n');
        out.push_str(&format!("━━━ {} ━━━\n", angle));
        match outcomes.get(i) {
            Some(Ok(outcome)) => {
                if outcome.had_error {
                    out.push_str(&format!("(error) {}", outcome.text));
                } else {
                    out.push_str(&outcome.text);
                }
                out.push_str(&format!(
                    "\n({} ms, agent {})",
                    outcome.duration_ms, outcome.agent_id
                ));
            }
            Some(Err(e)) => {
                out.push_str(&format!("(fork failed) {}", e));
            }
            None => {
                out.push_str("(missing result)");
            }
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_defaults_to_multi_agent() {
        let (single, scope) = parse_args("");
        assert!(!single);
        assert!(scope.is_none());
    }

    #[test]
    fn parse_args_detects_single_flag() {
        let (single, scope) = parse_args("--single");
        assert!(single);
        assert!(scope.is_none());
    }

    #[test]
    fn parse_args_detects_short_flag() {
        let (single, _) = parse_args("-1");
        assert!(single);
    }

    #[test]
    fn parse_args_collects_scope() {
        let (single, scope) = parse_args("src/main.rs src/lib.rs");
        assert!(!single);
        assert_eq!(scope.as_deref(), Some("src/main.rs src/lib.rs"));
    }

    #[test]
    fn parse_args_mixed_flag_and_scope() {
        let (single, scope) = parse_args("--single src/tools/");
        assert!(single);
        assert_eq!(scope.as_deref(), Some("src/tools/"));
    }

    #[test]
    fn review_angles_are_three_canonical_concerns() {
        let names: Vec<&str> = REVIEW_ANGLES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["reuse", "quality", "efficiency"]);
    }

    #[test]
    fn format_multi_includes_all_angles() {
        let outcomes: Vec<Result<CommandForkOutcome>> = REVIEW_ANGLES
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Ok(CommandForkOutcome {
                    text: format!("reviewer output #{i}"),
                    had_error: false,
                    duration_ms: 100 + i as u64,
                    agent_id: format!("agent-{i}"),
                })
            })
            .collect();
        let out = format_multi(&outcomes);
        assert!(out.contains("reuse"));
        assert!(out.contains("quality"));
        assert!(out.contains("efficiency"));
        assert!(out.contains("reviewer output #0"));
        assert!(out.contains("reviewer output #1"));
        assert!(out.contains("reviewer output #2"));
    }

    #[test]
    fn format_multi_surfaces_fork_errors() {
        let outcomes: Vec<Result<CommandForkOutcome>> = vec![
            Err(anyhow::anyhow!("auth failed")),
            Ok(CommandForkOutcome {
                text: "ok".into(),
                had_error: false,
                duration_ms: 10,
                agent_id: "q".into(),
            }),
            Ok(CommandForkOutcome {
                text: "err".into(),
                had_error: true,
                duration_ms: 5,
                agent_id: "e".into(),
            }),
        ];
        let out = format_multi(&outcomes);
        assert!(out.contains("fork failed"));
        assert!(out.contains("auth failed"));
        assert!(out.contains("(error)"));
    }

    #[test]
    fn format_single_sucess_and_error() {
        let ok = format_single(CommandForkOutcome {
            text: "good".into(),
            had_error: false,
            duration_ms: 11,
            agent_id: "a".into(),
        });
        assert!(ok.contains("single-agent fork"));
        assert!(ok.contains("good"));

        let err = format_single(CommandForkOutcome {
            text: "bad".into(),
            had_error: true,
            duration_ms: 22,
            agent_id: "b".into(),
        });
        assert!(err.contains("error:"));
        assert!(err.contains("bad"));
    }

    // Note: a "missing skill" test would need to clear the global skill
    // registry, which races with other tests that register skills. The
    // error branch is trivial (returns a plain-text Output), so we cover
    // only the pure-function surface here.
}
