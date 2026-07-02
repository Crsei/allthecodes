//! /extra-usage command -- show extended token usage and cost analysis.
//!
//! Goes beyond `/cost` by providing per-message token breakdowns,
//! top-5 most expensive API calls, token efficiency metrics,
//! and estimated cost breakdown by token type.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_types::message::Message;

/// Handler for the `/extra-usage` slash command.
pub struct ExtraUsageHandler;

/// Per-message usage snapshot (one per assistant message).
struct MsgStats {
    index: usize,
    input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    cost_usd: f64,
}

use allthecodes_types::models::pricing;

/// Format a token count with thousands separators.
fn fmt_tok(n: u64) -> String {
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
fn fmt_cost(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

/// Collect per-message stats from the conversation.
fn collect_stats(messages: &[Message]) -> Vec<MsgStats> {
    let mut idx: usize = 0;
    let mut stats = Vec::new();

    for msg in messages {
        if let Message::Assistant(a) = msg {
            idx += 1;
            let (inp, out, reas, cr, cc) = a
                .usage
                .as_ref()
                .map(|u| {
                    (
                        u.input_tokens,
                        u.output_tokens,
                        u.reasoning_output_tokens,
                        u.cache_read_input_tokens,
                        u.cache_creation_input_tokens,
                    )
                })
                .unwrap_or((0, 0, 0, 0, 0));
            stats.push(MsgStats {
                index: idx,
                input_tokens: inp,
                output_tokens: out,
                reasoning_output_tokens: reas,
                cache_read_tokens: cr,
                cache_creation_tokens: cc,
                cost_usd: a.cost_usd,
            });
        }
    }

    stats
}

#[async_trait]
impl CommandHandler for ExtraUsageHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let stats = collect_stats(&ctx.messages);

        if stats.is_empty() {
            return Ok(CommandResult::Output(
                "No API calls made in this session yet.".into(),
            ));
        }

        let mut lines: Vec<String> = Vec::new();

        // --- Section 1: Per-message token breakdown ---
        lines.push("Extended Usage Analysis".into());
        lines.push("=".repeat(50));
        lines.push(String::new());
        lines.push("Per-message token breakdown:".into());
        lines.push(format!(
            "  {:<5} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Call", "Input", "Output", "Cache-R", "Reas.", "Cost"
        ));
        lines.push(format!("  {}", "-".repeat(60)));

        for s in &stats {
            lines.push(format!(
                "  {:<5} {:>10} {:>10} {:>10} {:>10} {:>10}",
                format!("#{}", s.index),
                fmt_tok(s.input_tokens),
                fmt_tok(s.output_tokens),
                fmt_tok(s.cache_read_tokens),
                fmt_tok(s.reasoning_output_tokens),
                fmt_cost(s.cost_usd),
            ));
        }

        // --- Section 2: Top 5 most expensive API calls ---
        lines.push(String::new());
        lines.push("Top 5 most expensive API calls:".into());

        let mut sorted: Vec<&MsgStats> = stats.iter().collect();
        sorted.sort_by(|a, b| {
            b.cost_usd
                .partial_cmp(&a.cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let top5: Vec<&&MsgStats> = sorted.iter().take(5).collect();

        for (rank, s) in top5.iter().enumerate() {
            let total_tok =
                s.input_tokens + s.output_tokens + s.cache_read_tokens + s.cache_creation_tokens;
            lines.push(format!(
                "  {}. Call #{}: {} ({} tokens)",
                rank + 1,
                s.index,
                fmt_cost(s.cost_usd),
                fmt_tok(total_tok),
            ));
        }

        // --- Section 3: Token efficiency metrics ---
        let total_input: u64 = stats.iter().map(|s| s.input_tokens).sum();
        let total_output: u64 = stats.iter().map(|s| s.output_tokens).sum();
        let _total_reasoning: u64 = stats.iter().map(|s| s.reasoning_output_tokens).sum();
        let total_cache_read: u64 = stats.iter().map(|s| s.cache_read_tokens).sum();
        let total_cache_create: u64 = stats.iter().map(|s| s.cache_creation_tokens).sum();
        let total_cost: f64 = stats.iter().map(|s| s.cost_usd).sum();

        let output_input_ratio = if total_input > 0 {
            total_output as f64 / total_input as f64
        } else {
            0.0
        };

        let total_input_side = total_input + total_cache_read + total_cache_create;
        let cache_hit_rate = if total_input_side > 0 {
            total_cache_read as f64 / total_input_side as f64 * 100.0
        } else {
            0.0
        };

        lines.push(String::new());
        lines.push("Token efficiency metrics:".into());
        lines.push(format!("  Output/Input ratio:  {:.2}", output_input_ratio));
        lines.push(format!("  Cache hit rate:      {:.1}%", cache_hit_rate));
        lines.push(format!("  Total API calls:     {}", stats.len()));

        // --- Section 4: Estimated cost breakdown by token type ---
        let model = &ctx.app_state.main_loop_model;
        let p = pricing::get_pricing(model);
        let est_input_cost = total_input as f64 * p.input_per_1m / 1_000_000.0;
        let est_output_cost = total_output as f64 * p.output_per_1m / 1_000_000.0;
        let est_cache_read_cost = total_cache_read as f64 * p.input_per_1m * 0.1 / 1_000_000.0;
        let est_cache_write_cost = total_cache_create as f64 * p.input_per_1m * 1.25 / 1_000_000.0;
        let est_total =
            est_input_cost + est_output_cost + est_cache_read_cost + est_cache_write_cost;

        lines.push(String::new());
        lines.push("Estimated cost breakdown by token type:".into());
        lines.push(format!(
            "  Input tokens:        {} ({} tokens)",
            fmt_cost(est_input_cost),
            fmt_tok(total_input)
        ));
        lines.push(format!(
            "  Output tokens:       {} ({} tokens)",
            fmt_cost(est_output_cost),
            fmt_tok(total_output)
        ));
        if total_cache_read > 0 {
            lines.push(format!(
                "  Cache read tokens:   {} ({} tokens)",
                fmt_cost(est_cache_read_cost),
                fmt_tok(total_cache_read)
            ));
        }
        if total_cache_create > 0 {
            lines.push(format!(
                "  Cache write tokens:  {} ({} tokens)",
                fmt_cost(est_cache_write_cost),
                fmt_tok(total_cache_create)
            ));
        }
        lines.push(format!("  Estimated total:     {}", fmt_cost(est_total)));
        lines.push(format!("  Actual total:        {}", fmt_cost(total_cost)));

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

    fn make_assistant_msg(
        input_tokens: u64,
        output_tokens: u64,
        cache_read: u64,
        cache_create: u64,
        cost: f64,
    ) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: Some(Usage {
                input_tokens,
                output_tokens,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: cache_read,
                cache_creation_input_tokens: cache_create,
            }),
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: cost,
        })
    }

    fn make_assistant_msg_full(
        input_tokens: u64,
        output_tokens: u64,
        reasoning_output_tokens: u64,
        cache_read: u64,
        cache_create: u64,
        cost: f64,
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
                cache_read_input_tokens: cache_read,
                cache_creation_input_tokens: cache_create,
            }),
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: cost,
        })
    }

    fn make_assistant_msg_no_usage() -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::new(),
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[tokio::test]
    async fn test_extra_usage_no_messages() {
        let handler = ExtraUsageHandler;
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
    async fn test_extra_usage_with_messages() {
        let handler = ExtraUsageHandler;
        let mut ctx = CommandContext {
            messages: vec![
                make_assistant_msg(1000, 200, 500, 100, 0.005),
                make_assistant_msg(2000, 400, 1500, 0, 0.012),
                make_assistant_msg(500, 100, 0, 0, 0.002),
            ],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Extended Usage Analysis"));
                assert!(text.contains("Per-message token breakdown"));
                assert!(text.contains("Top 5 most expensive"));
                assert!(text.contains("Token efficiency metrics"));
                assert!(text.contains("Output/Input ratio"));
                assert!(text.contains("Cache hit rate"));
                assert!(text.contains("Estimated cost breakdown"));
                assert!(text.contains("Actual total"));
                // Verify the top expensive call is #2 (cost 0.012)
                assert!(text.contains("1. Call #2"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_extra_usage_single_message_no_usage() {
        let handler = ExtraUsageHandler;
        let mut ctx = CommandContext {
            messages: vec![make_assistant_msg_no_usage()],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Extended Usage Analysis"));
                // Should still render even with zero tokens
                assert!(text.contains("#1"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_extra_usage_cache_scenarios() {
        // Messages with varying cache_read and cache_creation values,
        // verify that cache cost breakdown sections appear only when > 0.
        let handler = ExtraUsageHandler;
        let mut ctx = CommandContext {
            messages: vec![
                // Message 1: high cache read, no cache write
                make_assistant_msg(1000, 200, 5000, 0, 0.008),
                // Message 2: high cache write (creation), no cache read
                make_assistant_msg(500, 100, 0, 3000, 0.006),
                // Message 3: both cache read and write
                make_assistant_msg(2000, 400, 1000, 500, 0.015),
                // Message 4: no cache at all
                make_assistant_msg(800, 300, 0, 0, 0.004),
            ],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                // Core sections present
                assert!(text.contains("Extended Usage Analysis"));
                assert!(text.contains("Per-message token breakdown"));
                assert!(text.contains("Cache hit rate"));
                // Cache cost breakdown lines should appear because we have
                // both cache_read (6000 total) and cache_create (3500 total)
                assert!(
                    text.contains("Cache read tokens"),
                    "expected Cache read tokens line"
                );
                assert!(
                    text.contains("Cache write tokens"),
                    "expected Cache write tokens line"
                );
                // Verify specific cache token counts appear in per-message breakdown.
                // Message 1: 5,000 cache-read
                assert!(text.contains("5,000"), "expected 5,000 in output");
                // Message 3: 1,000 cache-read, 500 cache-write
                assert!(text.contains("1,000"), "expected 1,000 in output");
                // Verify top-5 ranking: message #3 (cost 0.015) should be first
                assert!(text.contains("1. Call #3"), "expected Call #3 as most expensive");
                // Verify cache hit rate is present and non-zero
                assert!(text.contains("Cache hit rate"));
                // Should NOT be 0.0% because we have cache reads
                assert!(!text.contains("Cache hit rate:      0.0%"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_extra_usage_reasoning_tokens() {
        // Message with reasoning_output_tokens, verify they appear
        // in the per-message breakdown.
        let handler = ExtraUsageHandler;
        let mut ctx = CommandContext {
            messages: vec![
                // Message with no reasoning
                make_assistant_msg(500, 100, 0, 0, 0.002),
                // Message with substantial reasoning tokens
                make_assistant_msg_full(1000, 300, 500, 200, 50, 0.01),
                // Message with reasoning but zero everything else (unusual)
                make_assistant_msg_full(0, 0, 100, 0, 0, 0.0),
            ],
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        };

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Per-message token breakdown"));
                // The Reas. column header should now be present
                assert!(text.contains("Reas."));
                // Message #2 has 500 reasoning tokens
                assert!(text.contains("500"), "expected 500 reasoning tokens");
                // Message #3 has 100 reasoning tokens
                assert!(text.contains("100"), "expected 100 reasoning tokens");
                // Header row has "Reas." label
                assert!(text.contains("Reas."));
                // Verify token efficiency still works with reasoning present
                assert!(text.contains("Token efficiency metrics"));
                assert!(text.contains("Output/Input ratio"));
                // Since message #2 has input=1000, output=300, ratio should be < 1
                assert!(text.contains("Cache hit rate"));
            }
            _ => panic!("Expected Output result"),
        }
    }
}
