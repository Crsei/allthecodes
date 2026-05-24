//! `/btw` — side-question command (issue #37).
//!
//! Ask a quick side question without interrupting the main task. The question
//! is routed to a forked, tool-free, single-turn child engine via
//! the injected fork runtime. The main conversation history
//! is NOT modified — the answer is returned as `CommandResult::Output` so
//! it appears in the UI as a one-off system message.
//!
//! Usage:
//!   /btw <question>
//!
//! Examples:
//!   /btw What does the `fold` pattern do in Rust?
//!   /btw Is serde_json::Value Send + Sync?
//!
//! The forked agent sees the parent conversation as cache-safe context
//! (no mutation); its own reply is discarded after it is delivered here.

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::CommandForkParams;
use crate::{CommandContext, CommandHandler, CommandResult};

/// Shared system-prompt fragment that pins the forked agent to its side-
/// question role. Kept short so cache reuse with the parent prompt is high.
const BTW_SYSTEM_APPEND: &str = concat!(
    "You are answering a one-off side question alongside a main task. ",
    "Reply with a focused, self-contained answer — no tools, no file reads, ",
    "no follow-ups. Prefer 1-3 sentences unless a longer answer is truly ",
    "required. Do NOT continue the main task; assume the user will return ",
    "to it after reading your reply."
);

pub struct BtwHandler;

#[async_trait]
impl CommandHandler for BtwHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let question = args.trim();
        if question.is_empty() {
            return Ok(CommandResult::Output(
                "Usage: /btw <question>\n\nAsk a side question without \
                 interrupting the current task. The question runs as a \
                 tool-free, single-turn fork."
                    .to_string(),
            ));
        }

        let cwd = ctx.cwd.to_string_lossy().to_string();
        let model = ctx.app_state.main_loop_model.clone();
        let parent_messages = if ctx.messages.is_empty() {
            None
        } else {
            Some(ctx.messages.clone())
        };

        let params = CommandForkParams {
            prompt: question.to_string(),
            cwd,
            model: model.clone(),
            fallback_model: Some(model),
            // Tool-free: side questions are answered from conversation + training.
            tools: vec![],
            max_turns: Some(1),
            parent_messages,
            append_system_prompt: Some(BTW_SYSTEM_APPEND.to_string()),
            custom_system_prompt: None,
        };

        match crate::runtime::run_fork(params).await {
            Ok(outcome) => {
                let header = format!("/btw (forked agent, {} ms)\n", outcome.duration_ms);
                if outcome.had_error {
                    Ok(CommandResult::Output(format!(
                        "{}error: {}",
                        header, outcome.text
                    )))
                } else {
                    Ok(CommandResult::Output(format!(
                        "{}\n{}",
                        header.trim_end(),
                        outcome.text
                    )))
                }
            }
            Err(e) => Ok(CommandResult::Output(format!(
                "/btw error: failed to run fork: {}",
                e
            ))),
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

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("btw-test-session"),
        }
    }

    #[tokio::test]
    async fn empty_args_shows_usage() {
        let handler = BtwHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage: /btw"));
                assert!(text.contains("side question"));
            }
            _ => panic!("expected Output result"),
        }
    }

    #[tokio::test]
    async fn whitespace_only_args_shows_usage() {
        let handler = BtwHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("   \t  ", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Usage: /btw")),
            _ => panic!("expected Output result"),
        }
    }

    #[test]
    fn btw_system_append_is_nonempty_and_role_restricting() {
        assert!(BTW_SYSTEM_APPEND.contains("side question"));
        assert!(BTW_SYSTEM_APPEND.contains("no tools"));
    }
}
