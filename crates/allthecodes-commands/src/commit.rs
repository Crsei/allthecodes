//! `/commit` command — create a git commit from staged changes.
//!
//! Stages all changes, generates a commit message summary, and commits.
//! Optionally accepts a custom message via arguments.

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_utils::git;

pub struct CommitHandler;

#[async_trait]
impl CommandHandler for CommitHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        // Check if we're in a git repo
        if !git::is_git_repo(&ctx.cwd) {
            return Ok(CommandResult::Output(
                "Error: not in a git repository.".to_string(),
            ));
        }

        let status = git::get_status(&ctx.cwd)?;

        if status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty() {
            return Ok(CommandResult::Output(
                "Nothing to commit — working tree clean.".to_string(),
            ));
        }

        // Build a summary of what would be committed
        let mut lines = Vec::new();
        lines.push("**Current changes:**".to_string());

        if !status.staged.is_empty() {
            lines.push(format!("  Staged: {} file(s)", status.staged.len()));
            for f in &status.staged {
                lines.push(format!("    {:?} {}", f.status, f.path));
            }
        }
        if !status.unstaged.is_empty() {
            lines.push(format!("  Unstaged: {} file(s)", status.unstaged.len()));
            for f in &status.unstaged {
                lines.push(format!("    {:?} {}", f.status, f.path));
            }
        }
        if !status.untracked.is_empty() {
            lines.push(format!("  Untracked: {} file(s)", status.untracked.len()));
        }

        if args.trim().is_empty() {
            // No message provided — ask the model to help draft one
            lines.push(String::new());
            lines.push("Please review the changes and create a git commit. Stage the relevant files and write a clear commit message summarizing the changes.".to_string());

            let prompt = lines.join("\n");
            let msg = allthecodes_types::message::Message::User(
                allthecodes_types::message::UserMessage {
                    uuid: uuid::Uuid::new_v4(),
                    role: "user".to_string(),
                    content: allthecodes_types::message::MessageContent::Text(prompt),
                    timestamp: chrono::Utc::now().timestamp(),
                    is_meta: false,
                    tool_use_result: None,
                    source_tool_assistant_uuid: None,
                },
            );
            Ok(CommandResult::Query(vec![msg]))
        } else {
            // Message provided — commit directly
            let output = std::process::Command::new("git")
                .args(["commit", "-m", args.trim()])
                .current_dir(&ctx.cwd)
                .output()
                .context("Failed to run git commit")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if output.status.success() {
                Ok(CommandResult::Output(format!(
                    "Committed successfully.\n{}",
                    stdout.trim()
                )))
            } else {
                Ok(CommandResult::Output(format!(
                    "Commit failed:\n{}{}",
                    stdout, stderr
                )))
            }
        }
    }
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
    async fn test_commit_not_in_git_repo() {
        let handler = CommitHandler;
        let mut ctx = test_ctx(PathBuf::from("/nonexistent/fake/path"));
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("not in a git repository"),
                    "expected 'not in a git repository', got: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_commit_not_in_git_repo_with_message() {
        let handler = CommitHandler;
        let mut ctx = test_ctx(PathBuf::from("/nonexistent/fake/path"));
        let result = handler
            .execute("my commit message", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("not in a git repository"));
            }
            _ => panic!("Expected Output"),
        }
    }
}
