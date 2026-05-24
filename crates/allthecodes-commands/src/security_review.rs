//! `/security-review` command — local security-review prompt wrapper.
//!
//! Gathers git branch/diff/log context and feeds it into a focused
//! security-review prompt. Mirrors the Bun reference
//! `src/commands/security-review.ts`.

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::{Message, MessageContent, UserMessage};

pub struct SecurityReviewHandler;

#[async_trait]
impl CommandHandler for SecurityReviewHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let cwd = &ctx.cwd;

        if !is_git_repo(cwd) {
            return Ok(CommandResult::Output(
                "Error: /security-review must be run inside a git repository.".to_string(),
            ));
        }

        let context_block = collect_git_context(cwd);
        let focus = args.trim();

        let prompt = build_security_prompt(&context_block, focus);

        let msg = Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            role: "user".to_string(),
            content: MessageContent::Text(prompt),
            timestamp: chrono::Utc::now().timestamp(),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        });

        Ok(CommandResult::Query(vec![msg]))
    }
}

/// Collect a compact summary of the current branch, working-tree status, and
/// recent commits to seed the security-review prompt.
fn collect_git_context(cwd: &std::path::Path) -> String {
    let mut lines = Vec::new();

    if let Ok(branch) = git_current_branch(cwd) {
        lines.push(format!("Branch: {}", branch));
    }

    if let Ok(status) = git_status_counts(cwd) {
        lines.push(format!(
            "Working tree: {} staged, {} unstaged, {} untracked",
            status.staged, status.unstaged, status.untracked
        ));
    }

    if let Ok(log) = git_recent_log(cwd, 5) {
        if !log.is_empty() {
            lines.push("Recent commits:".to_string());
            for entry in log {
                let summary: String = entry.summary.chars().take(100).collect();
                lines.push(format!("  {} {}", entry.short_sha, summary));
            }
        }
    }

    if lines.is_empty() {
        "(no git context available)".to_string()
    } else {
        lines.join("\n")
    }
}

#[derive(Debug, Default)]
struct GitStatusCounts {
    staged: usize,
    unstaged: usize,
    untracked: usize,
}

#[derive(Debug)]
struct GitLogEntry {
    short_sha: String,
    summary: String,
}

fn is_git_repo(cwd: &std::path::Path) -> bool {
    run_git(cwd, &["rev-parse", "--is-inside-work-tree"])
        .map(|out| out.trim() == "true")
        .unwrap_or(false)
}

fn git_current_branch(cwd: &std::path::Path) -> Result<String> {
    let branch = run_git(cwd, &["branch", "--show-current"])?;
    let branch = branch.trim();
    if branch.is_empty() {
        let head = run_git(cwd, &["rev-parse", "--short", "HEAD"])?;
        Ok(head.trim().to_string())
    } else {
        Ok(branch.to_string())
    }
}

fn git_status_counts(cwd: &std::path::Path) -> Result<GitStatusCounts> {
    let output = run_git(cwd, &["status", "--porcelain=v1"])?;
    let mut counts = GitStatusCounts::default();
    for line in output.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        if bytes[0] == b'?' && bytes[1] == b'?' {
            counts.untracked += 1;
            continue;
        }
        if bytes[0] != b' ' {
            counts.staged += 1;
        }
        if bytes[1] != b' ' {
            counts.unstaged += 1;
        }
    }
    Ok(counts)
}

fn git_recent_log(cwd: &std::path::Path, count: usize) -> Result<Vec<GitLogEntry>> {
    let arg = format!("-n{count}");
    let output = run_git(cwd, &["log", "--oneline", &arg])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (short_sha, summary) = line.split_once(' ')?;
            Some(GitLogEntry {
                short_sha: short_sha.to_string(),
                summary: summary.to_string(),
            })
        })
        .collect())
}

fn run_git(cwd: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim())
    }
}

fn build_security_prompt(context: &str, focus: &str) -> String {
    let focus_block = if focus.is_empty() {
        String::new()
    } else {
        format!("\nUser focus area: {}\n", focus)
    };

    format!(
        "You are a senior application security engineer performing a focused \
         security review of the changes on the current branch.\n\n\
         Git context:\n{context}\n{focus_block}\n\
         Review procedure:\n\
         1. Read the diff. Use the `Bash` tool to run `git diff --staged`, \
            `git diff`, and `git log --oneline -n 20` as needed.\n\
         2. For each changed file, inspect the surrounding code with `Read` so \
            findings are grounded in context, not diff hunks alone.\n\
         3. Consider common web-app risks: injection (SQL, shell, XSS), \
            authn/authz bypass, insecure deserialization, SSRF, path traversal, \
            unsafe file handling, secrets in code, weak crypto, logging of \
            sensitive data, dependency risks, and race conditions.\n\
         4. Do NOT suggest stylistic or non-security changes.\n\n\
         Output format (Markdown):\n\
         ## Verdict\n\
         <APPROVE | CHANGES REQUESTED | BLOCKED — one line rationale>\n\n\
         ## Findings\n\
         For each finding:\n\
         - **Severity:** Critical | High | Medium | Low\n\
         - **Location:** path:line\n\
         - **Issue:** one sentence\n\
         - **Impact:** what an attacker can do\n\
         - **Fix:** concrete remediation\n\n\
         If there are no findings, write `None found` and justify briefly.\n"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn rejects_non_git_dir() {
        let handler = SecurityReviewHandler;
        let mut ctx = test_ctx(PathBuf::from("/nonexistent/path/definitely-not-a-repo"));
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("git repository"));
            }
            _ => panic!("Expected Output for non-git dir"),
        }
    }

    #[test]
    fn prompt_contains_output_template() {
        let prompt = build_security_prompt("Branch: main", "");
        assert!(prompt.contains("## Verdict"));
        assert!(prompt.contains("## Findings"));
        assert!(prompt.contains("Severity"));
    }

    #[test]
    fn focus_text_is_threaded_through() {
        let prompt = build_security_prompt("Branch: main", "authentication flow");
        assert!(prompt.contains("User focus area: authentication flow"));
    }

    #[test]
    fn empty_focus_omits_focus_block() {
        let prompt = build_security_prompt("Branch: main", "");
        assert!(!prompt.contains("User focus area"));
    }
}
