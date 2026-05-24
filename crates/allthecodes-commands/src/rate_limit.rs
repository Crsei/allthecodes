//! /rate-limit-options command -- show rate limit information for the current model.
//!
//! Displays known rate limit tiers, counts rate-limit errors in the
//! conversation history, and provides tips for managing rate limits.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::Message;

/// Handler for the `/rate-limit-options` slash command.
pub struct RateLimitHandler;

/// Known rate-limit tier for a model family.
struct RateTier {
    pattern: &'static str,
    rpm: &'static str,
    input_tpm: &'static str,
}

/// Reference table of approximate rate limits for common model families.
const RATE_TIERS: &[RateTier] = &[
    RateTier {
        pattern: "claude-3-opus",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-3-sonnet",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-3-haiku",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-3.5-sonnet",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-3-5-sonnet",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-sonnet-4",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
    RateTier {
        pattern: "claude-opus-4",
        rpm: "~4,000 RPM",
        input_tpm: "~400K input TPM",
    },
];

/// Count messages that have a rate-limit api_error.
fn count_rate_limit_errors(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| {
            if let Message::Assistant(a) = m {
                a.api_error.as_deref() == Some("rate_limit")
            } else {
                false
            }
        })
        .count()
}

/// Find the matching rate tier for a model name, if any.
fn find_tier(model: &str) -> Option<&'static RateTier> {
    let model_lower = model.to_lowercase();
    RATE_TIERS.iter().find(|t| model_lower.contains(t.pattern))
}

#[async_trait]
impl CommandHandler for RateLimitHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let model = &ctx.app_state.main_loop_model;
        let rate_errors = count_rate_limit_errors(&ctx.messages);

        let mut lines: Vec<String> = Vec::new();

        lines.push("Rate Limit Information".into());
        lines.push("=".repeat(40));
        lines.push(String::new());

        // Current model
        lines.push(format!("Current model: {}", model));
        lines.push(String::new());

        // Rate limit tier
        if let Some(tier) = find_tier(model) {
            lines.push("Known rate limits (approximate):".into());
            lines.push(format!("  Requests:     {}", tier.rpm));
            lines.push(format!("  Input tokens: {}", tier.input_tpm));
        } else {
            lines.push("Rate limits: vary by tier and model.".into());
            lines.push("  Check your Anthropic dashboard for exact limits.".into());
        }

        lines.push(String::new());

        // Rate limit errors in conversation
        if rate_errors > 0 {
            lines.push(format!(
                "Rate limit errors in this session: {} occurrence{}",
                rate_errors,
                if rate_errors == 1 { "" } else { "s" }
            ));
        } else {
            lines.push("Rate limit errors in this session: none".into());
        }

        // Reference table
        lines.push(String::new());
        lines.push("Model rate limit reference:".into());
        lines.push(format!(
            "  {:<25} {:>12} {:>18}",
            "Model", "RPM", "Input TPM"
        ));
        lines.push(format!("  {}", "-".repeat(57)));
        for tier in RATE_TIERS {
            lines.push(format!(
                "  {:<25} {:>12} {:>18}",
                tier.pattern, tier.rpm, tier.input_tpm
            ));
        }

        // Tips
        lines.push(String::new());
        lines.push("Tips for managing rate limits:".into());
        lines.push("  1. Use /effort low to reduce token usage per call".into());
        lines.push("  2. Enable /fast mode for shorter responses".into());
        lines.push("  3. Keep conversations concise; use /clear to reset".into());
        lines.push("  4. Batch related questions into a single prompt".into());
        lines.push("  5. Wait briefly between rapid successive queries".into());

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::AssistantMessage;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_rate_limit_error_msg() -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: true,
            api_error: Some("rate_limit".into()),
            cost_usd: 0.0,
        })
    }

    fn make_normal_assistant_msg() -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.001,
        })
    }

    #[tokio::test]
    async fn test_rate_limit_no_messages() {
        let handler = RateLimitHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Rate Limit Information"));
                assert!(text.contains("Current model:"));
                assert!(text.contains("none"));
                assert!(text.contains("Tips for managing rate limits"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_rate_limit_with_errors() {
        let handler = RateLimitHandler;
        let mut ctx = CommandContext {
            messages: vec![
                make_normal_assistant_msg(),
                make_rate_limit_error_msg(),
                make_rate_limit_error_msg(),
                make_normal_assistant_msg(),
            ],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Rate Limit Information"));
                assert!(text.contains("2 occurrences"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_rate_limit_known_model() {
        let handler = RateLimitHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };
        ctx.app_state.main_loop_model = "claude-3-opus-20240229".into();

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Known rate limits"));
                assert!(text.contains("~4,000 RPM"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_rate_limit_unknown_model() {
        let handler = RateLimitHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };
        ctx.app_state.main_loop_model = "some-custom-model".into();

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("vary by tier"));
            }
            _ => panic!("Expected Output result"),
        }
    }
}
