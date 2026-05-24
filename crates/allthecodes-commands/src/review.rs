//! `/review` command — local PR review prompt wrapper.
//!
//! Thin wrapper that injects a structured code-review prompt guiding the model
//! through a local `gh pr` inspection workflow. Optional argument `[pr]` is a
//! pull request number/branch/URL passed to `gh pr view`.
//!
//! Mirrors the Bun reference `src/commands/review.ts` behavior.

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::{Message, MessageContent, UserMessage};

pub struct ReviewHandler;

#[async_trait]
impl CommandHandler for ReviewHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let target = args.trim();
        let prompt = build_review_prompt(target);

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

/// Build the review prompt. When `target` is empty, the model will pick the
/// current branch's PR via `gh pr list`. Otherwise the target is forwarded to
/// `gh pr view`.
fn build_review_prompt(target: &str) -> String {
    let header = if target.is_empty() {
        "You are an expert code reviewer. Review the pull request for the current branch.\n\n\
         Use the `gh` CLI to gather context:\n\
           1. Run `gh pr list --head $(git branch --show-current) --state open --json number,title,headRefName` to find the PR.\n\
           2. If no PR is associated with the branch, say so and stop.\n\
           3. Otherwise use `gh pr view <number>` and `gh pr diff <number>` to read the PR.\n".to_string()
    } else {
        format!(
            "You are an expert code reviewer. Review pull request `{}`.\n\n\
             Use the `gh` CLI to gather context:\n\
               1. Run `gh pr view {}` to read the description and metadata.\n\
               2. Run `gh pr diff {}` to read the full diff.\n",
            target, target, target
        )
    };

    format!(
        "{}\n\
         When you review:\n\
         - Focus on correctness, security, performance, and clarity.\n\
         - Quote specific files and lines when giving feedback.\n\
         - Separate must-fix issues from suggestions.\n\
         - Keep feedback concise and actionable — no filler.\n\
         - If tests are missing for a change, call it out.\n\n\
         Output format:\n\
         ## Summary\n\
         <1-2 sentence overview>\n\n\
         ## Must fix\n\
         <list, or 'None'>\n\n\
         ## Suggestions\n\
         <list, or 'None'>\n\n\
         ## Questions\n\
         <list, or 'None'>\n",
        header
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn review_without_target_mentions_current_branch() {
        let handler = ReviewHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Query(msgs) => {
                assert_eq!(msgs.len(), 1);
                if let Message::User(UserMessage {
                    content: MessageContent::Text(body),
                    ..
                }) = &msgs[0]
                {
                    assert!(body.contains("current branch"));
                    assert!(body.contains("gh pr list"));
                    assert!(body.contains("## Summary"));
                } else {
                    panic!("Expected User(Text) message");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[tokio::test]
    async fn review_with_target_threads_identifier_into_prompt() {
        let handler = ReviewHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("123", &mut ctx).await.unwrap();
        match result {
            CommandResult::Query(msgs) => {
                if let Message::User(UserMessage {
                    content: MessageContent::Text(body),
                    ..
                }) = &msgs[0]
                {
                    assert!(body.contains("gh pr view 123"));
                    assert!(body.contains("gh pr diff 123"));
                } else {
                    panic!("Expected User(Text) message");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn build_review_prompt_branch_fallback_has_output_sections() {
        let p = build_review_prompt("");
        assert!(p.contains("## Must fix"));
        assert!(p.contains("## Suggestions"));
        assert!(p.contains("## Questions"));
    }
}
