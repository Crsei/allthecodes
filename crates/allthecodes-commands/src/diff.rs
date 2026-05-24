//! /diff command -- shows git diff of current changes.
//!
//! Uses the `git2` crate to read the repository status and produce a unified
//! diff of all modified (staged and unstaged) files relative to HEAD.

use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

/// Handler for the `/diff` slash command.
pub struct DiffHandler;

#[async_trait]
impl CommandHandler for DiffHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let cwd = &ctx.cwd;

        // Open the repository.
        let repo = match git2::Repository::discover(cwd) {
            Ok(r) => r,
            Err(e) => {
                return Ok(CommandResult::Output(format!(
                    "Not a git repository (or any parent): {}",
                    e
                )));
            }
        };

        let staged = args.trim() == "--staged" || args.trim() == "--cached";

        let diff_text = if staged {
            get_staged_diff(&repo)?
        } else {
            let unstaged = get_unstaged_diff(&repo)?;
            let staged_diff = get_staged_diff(&repo)?;

            let mut combined = String::new();
            if !staged_diff.is_empty() {
                combined.push_str("=== Staged changes ===\n\n");
                combined.push_str(&staged_diff);
            }
            if !unstaged.is_empty() {
                if !combined.is_empty() {
                    combined.push_str("\n\n");
                }
                combined.push_str("=== Unstaged changes ===\n\n");
                combined.push_str(&unstaged);
            }
            combined
        };

        if diff_text.is_empty() {
            return Ok(CommandResult::Output("No changes detected.".into()));
        }

        Ok(CommandResult::Output(diff_text))
    }
}

/// Get the diff of staged changes (index vs HEAD).
fn get_staged_diff(repo: &git2::Repository) -> Result<String> {
    let head_tree = match repo.head() {
        Ok(head) => {
            let commit = head
                .peel_to_commit()
                .context("Failed to peel HEAD to commit")?;
            Some(commit.tree().context("Failed to get HEAD tree")?)
        }
        Err(_) => None, // Initial commit -- no HEAD yet
    };

    let diff = repo
        .diff_tree_to_index(
            head_tree.as_ref(),
            None, // default index
            None, // default options
        )
        .context("Failed to compute staged diff")?;

    diff_to_string(&diff)
}

/// Get the diff of unstaged changes (workdir vs index).
fn get_unstaged_diff(repo: &git2::Repository) -> Result<String> {
    let diff = repo
        .diff_index_to_workdir(
            None, // default index
            None, // default options
        )
        .context("Failed to compute unstaged diff")?;

    diff_to_string(&diff)
}

/// Convert a `git2::Diff` to a unified diff string.
fn diff_to_string(diff: &git2::Diff) -> Result<String> {
    let mut output = String::new();

    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        match origin {
            '+' | '-' | ' ' => output.push(origin),
            _ => {}
        }
        if let Ok(content) = std::str::from_utf8(line.content()) {
            output.push_str(content);
        }
        true
    })
    .context("Failed to print diff")?;

    Ok(output)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_handler_exists() {
        // Verify the handler can be constructed.
        let _handler = DiffHandler;
    }
}
