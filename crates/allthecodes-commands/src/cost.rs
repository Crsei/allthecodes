//! /cost command -- show token usage and cost for the current session.
//!
//! Aggregates usage data from all assistant messages in the conversation
//! to display total input/output tokens and estimated cost.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::Message;

/// Handler for the `/cost` slash command.
pub struct CostHandler;

/// Accumulated usage statistics.
struct UsageStats {
    input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    total_cost_usd: f64,
    api_calls: usize,
}

/// Gather usage statistics from the conversation messages.
fn gather_usage(messages: &[Message]) -> UsageStats {
    let mut stats = UsageStats {
        input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_cost_usd: 0.0,
        api_calls: 0,
    };

    for msg in messages {
        if let Message::Assistant(a) = msg {
            stats.api_calls += 1;
            stats.total_cost_usd += a.cost_usd;

            if let Some(ref usage) = a.usage {
                stats.input_tokens += usage.input_tokens;
                stats.output_tokens += usage.output_tokens;
                stats.reasoning_output_tokens += usage.reasoning_output_tokens;
                stats.cache_read_tokens += usage.cache_read_input_tokens;
                stats.cache_creation_tokens += usage.cache_creation_input_tokens;
            }
        }
    }

    stats
}

/// Format a token count with thousands separators.
fn format_tokens(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

/// Format a USD cost.
fn format_cost(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

#[async_trait]
impl CommandHandler for CostHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let stats = gather_usage(&ctx.messages);

        if stats.api_calls == 0 {
            return Ok(CommandResult::Output(
                "No API calls made in this session yet.".into(),
            ));
        }

        let total_tokens = stats.input_tokens + stats.output_tokens;

        let cached_input = stats.cache_read_tokens + stats.cache_creation_tokens;

        let mut lines = Vec::new();
        lines.push(format!(
            "Token usage: total={} input={} (+ {} cached) output={}{}",
            format_tokens(total_tokens),
            format_tokens(stats.input_tokens),
            format_tokens(cached_input),
            format_tokens(stats.output_tokens),
            if stats.reasoning_output_tokens > 0 {
                format!(
                    " (reasoning {})",
                    format_tokens(stats.reasoning_output_tokens)
                )
            } else {
                String::new()
            },
        ));
        lines.push(String::new());
        lines.push(format!("  API calls:       {}", stats.api_calls));
        lines.push(format!(
            "  Input tokens:    {}",
            format_tokens(stats.input_tokens)
        ));
        lines.push(format!(
            "  Output tokens:   {}{}",
            format_tokens(stats.output_tokens),
            if stats.reasoning_output_tokens > 0 {
                format!(
                    " (reasoning {})",
                    format_tokens(stats.reasoning_output_tokens)
                )
            } else {
                String::new()
            },
        ));

        if stats.cache_read_tokens > 0 || stats.cache_creation_tokens > 0 {
            lines.push(format!(
                "  Cache read:      {}",
                format_tokens(stats.cache_read_tokens)
            ));
            lines.push(format!(
                "  Cache creation:  {}",
                format_tokens(stats.cache_creation_tokens)
            ));
        }

        lines.push(format!(
            "  Estimated cost:  {}",
            format_cost(stats.total_cost_usd)
        ));

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{AssistantMessage, Usage};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_assistant_msg(input_tokens: u64, output_tokens: u64, cost: f64) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: Some(Usage {
                input_tokens,
                output_tokens,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: cost,
        })
    }

    #[tokio::test]
    async fn test_cost_no_messages() {
        let handler = CostHandler;
        let mut ctx = CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("No API calls"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_cost_with_messages() {
        let handler = CostHandler;
        let mut ctx = CommandContext {
            messages: vec![
                make_assistant_msg(100, 50, 0.001),
                make_assistant_msg(200, 100, 0.002),
            ],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("API calls:"));
                assert!(text.contains("2"));
                assert!(text.contains("Input tokens:"));
                assert!(text.contains("Output tokens:"));
                assert!(text.contains("Estimated cost:"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1,000");
        assert_eq!(format_tokens(1234567), "1,234,567");
    }
}
