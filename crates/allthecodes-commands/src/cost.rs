//! /cost command -- show token usage and cost for the current session.
//!
//! Aggregates usage data from all assistant messages in the conversation
//! to display total input/output tokens and estimated cost.
//!
//! This handler uses the shared `cost_ledger` from `allthecodes-services` as its
//! primary aggregation path.  When cost_ledger events are not available (e.g. older
//! sessions), it falls back to the original inline aggregation from assistant
//! messages.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_services::cost_ledger;
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

/// Gather usage statistics from the conversation messages (fallback path).
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

/// Render `UsageStats` into the output string.
fn format_usage_output(stats: &UsageStats) -> String {
    if stats.api_calls == 0 {
        return "No API calls made in this session yet.".into();
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

    lines.join("\n")
}

#[async_trait]
impl CommandHandler for CostHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let session_id = ctx.session_id.as_str();

        // Primary path: use the shared cost_ledger.
        let ledger_summary = cost_ledger::get_session_cost_summary(session_id, &ctx.messages);

        // Prefer cost_ledger events when they produced some data.  The ledger
        // backfills from assistant messages with usage, so if there are any API
        // calls we trust the ledger.
        let stats = if ledger_summary.api_calls > 0 {
            UsageStats {
                input_tokens: ledger_summary.total_input_tokens,
                output_tokens: ledger_summary.total_output_tokens,
                reasoning_output_tokens: ledger_summary.total_reasoning_output_tokens,
                cache_read_tokens: ledger_summary.total_cache_read_tokens,
                cache_creation_tokens: ledger_summary.total_cache_creation_tokens,
                total_cost_usd: ledger_summary.total_cost_usd,
                api_calls: ledger_summary.api_calls as usize,
            }
        } else {
            // Fallback to inline aggregation (pre-cost_ledger sessions or edge
            // cases where the ledger returns zero events).
            gather_usage(&ctx.messages)
        };

        Ok(CommandResult::Output(format_usage_output(&stats)))
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
        make_assistant_msg_full(input_tokens, output_tokens, cost, 0, 0, 0)
    }

    fn make_assistant_msg_full(
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
        reasoning_output_tokens: u64,
        cache_read_input_tokens: u64,
        cache_creation_input_tokens: u64,
    ) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: Some(Usage {
                input_tokens,
                output_tokens,
                reasoning_output_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
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

    #[tokio::test]
    async fn test_cost_with_cache_tokens() {
        let handler = CostHandler;
        let mut ctx = CommandContext {
            messages: vec![make_assistant_msg_full(100, 50, 0.0015, 0, 500, 200)],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Cache read:      500"));
                assert!(text.contains("Cache creation:  200"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_cost_with_reasoning_tokens() {
        let handler = CostHandler;
        let mut ctx = CommandContext {
            messages: vec![make_assistant_msg_full(100, 50, 0.002, 300, 0, 0)],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("reasoning"));
                assert!(text.contains("300"));
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

    #[test]
    fn test_gather_usage_fallback_ignores_user_messages() {
        let messages: Vec<Message> = vec![Message::User(allthecodes_types::message::UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: allthecodes_types::message::MessageContent::Blocks(Vec::new()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })];

        let stats = gather_usage(&messages);
        assert_eq!(stats.api_calls, 0);
    }

    #[test]
    fn test_format_usage_output_no_calls() {
        let stats = UsageStats {
            input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_cost_usd: 0.0,
            api_calls: 0,
        };
        let output = format_usage_output(&stats);
        assert_eq!(output, "No API calls made in this session yet.");
    }

    #[test]
    fn test_format_usage_output_full() {
        let stats = UsageStats {
            input_tokens: 1000,
            output_tokens: 500,
            reasoning_output_tokens: 50,
            cache_read_tokens: 200,
            cache_creation_tokens: 100,
            total_cost_usd: 0.025,
            api_calls: 3,
        };
        let output = format_usage_output(&stats);
        assert!(output.contains("API calls:"));
        assert!(output.contains("3"));
        assert!(output.contains("Input tokens:"));
        assert!(output.contains("1,000"));
        assert!(output.contains("Cache read:"));
        assert!(output.contains("200"));
        assert!(output.contains("Estimated cost:"));
    }
}
