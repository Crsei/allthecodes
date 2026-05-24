//! `/recap` command — one-line or short-paragraph summary of the current session.
//!
//! Uses the current conversation history (already in `CommandContext.messages`)
//! to ask the model for a tight recap. Optional argument: `short` (default) or
//! `long` to pick granularity.

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::{
    ContentBlock, Message, MessageContent, ToolResultContent, UserMessage,
};

pub struct RecapHandler;

#[derive(Copy, Clone)]
enum Granularity {
    Short,
    Long,
}

#[async_trait]
impl CommandHandler for RecapHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let granularity = match args.trim().to_ascii_lowercase().as_str() {
            "" | "short" | "brief" | "line" => Granularity::Short,
            "long" | "full" | "detailed" => Granularity::Long,
            other => {
                return Ok(CommandResult::Output(format!(
                    "Unknown recap style: {:?}. Use `/recap` or `/recap long`.",
                    other
                )));
            }
        };

        let turns = count_user_turns(&ctx.messages);
        if turns == 0 {
            return Ok(CommandResult::Output(
                "Nothing to recap yet — the session has no user messages.".to_string(),
            ));
        }

        let prompt = build_recap_prompt(granularity, turns);

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

/// Count user messages that contribute to the conversation (exclude meta
/// messages and pure tool results).
fn count_user_turns(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| match m {
            Message::User(u) => !u.is_meta && !is_tool_result_only(u),
            _ => false,
        })
        .count()
}

fn is_tool_result_only(u: &UserMessage) -> bool {
    if u.tool_use_result.is_some() {
        return true;
    }
    match &u.content {
        MessageContent::Blocks(blocks) => {
            !blocks.is_empty()
                && blocks.iter().all(|b| {
                    matches!(
                        b,
                        ContentBlock::ToolResult {
                            content: ToolResultContent::Text(_),
                            ..
                        } | ContentBlock::ToolResult {
                            content: ToolResultContent::Blocks(_),
                            ..
                        }
                    )
                })
        }
        MessageContent::Text(_) => false,
    }
}

fn build_recap_prompt(g: Granularity, turns: usize) -> String {
    match g {
        Granularity::Short => format!(
            "Summarize the current session in one short sentence (max ~25 words). \
             The session has {turns} user turn(s). \
             Focus on what the user was trying to accomplish and the current state. \
             No preamble, no trailing notes — just the sentence."
        ),
        Granularity::Long => format!(
            "Summarize the current session as a short paragraph (3-5 sentences). \
             The session has {turns} user turn(s). \
             Cover:\n\
             1. What the user asked for.\n\
             2. What has been done.\n\
             3. What is still open, if anything.\n\
             Do not speculate about future work the user did not request."
        ),
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

    fn test_ctx(messages: Vec<Message>) -> CommandContext {
        CommandContext {
            messages,
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    fn user_text(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            timestamp: 0,
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn meta_user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            timestamp: 0,
            is_meta: true,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn tool_result_user() -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: ToolResultContent::Text("ok".to_string()),
                is_error: false,
            }]),
            timestamp: 0,
            is_meta: false,
            tool_use_result: Some("ok".to_string()),
            source_tool_assistant_uuid: None,
        })
    }

    #[tokio::test]
    async fn recap_empty_session_returns_output() {
        let handler = RecapHandler;
        let mut ctx = test_ctx(Vec::new());
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Nothing to recap"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn recap_short_default_includes_turn_count() {
        let handler = RecapHandler;
        let mut ctx = test_ctx(vec![user_text("hello"), user_text("and again")]);
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Query(msgs) => {
                if let Message::User(UserMessage {
                    content: MessageContent::Text(body),
                    ..
                }) = &msgs[0]
                {
                    assert!(body.contains("2 user turn"));
                    assert!(body.contains("one short sentence"));
                } else {
                    panic!("Expected User(Text)");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[tokio::test]
    async fn recap_long_switches_template() {
        let handler = RecapHandler;
        let mut ctx = test_ctx(vec![user_text("hello")]);
        let result = handler.execute("long", &mut ctx).await.unwrap();
        match result {
            CommandResult::Query(msgs) => {
                if let Message::User(UserMessage {
                    content: MessageContent::Text(body),
                    ..
                }) = &msgs[0]
                {
                    assert!(body.contains("short paragraph"));
                } else {
                    panic!("Expected User(Text)");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[tokio::test]
    async fn recap_unknown_style_returns_error_output() {
        let handler = RecapHandler;
        let mut ctx = test_ctx(vec![user_text("hello")]);
        let result = handler.execute("super-long", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown recap style"));
            }
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn count_user_turns_ignores_meta_and_tool_results() {
        let msgs = vec![
            user_text("one"),
            meta_user("system-injected context"),
            tool_result_user(),
            user_text("two"),
        ];
        assert_eq!(count_user_turns(&msgs), 2);
    }
}
