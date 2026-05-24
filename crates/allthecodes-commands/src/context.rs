//! `/context` — show the effective, post-compact API view of the conversation.
//!
//! Delegates to [`allthecodes_compact::context_analysis::analyze_context_usage`]
//! which runs the same snip + microcompact transforms as the real send
//! pipeline, categorises the result into tracked buckets (messages,
//! system prompt, skills, file cache, tool schemas, hook results,
//! free budget) and renders it either as a TUI token grid or as JSON.
//!
//! Subcommands (matching the pattern established by `/doctor`):
//!
//! ```text
//! /context            — rendered TUI output (token grid + percentages)
//! /context json       — machine-readable JSON (headless / scripting)
//! /context raw        — alias for `json`
//! ```

use allthecodes_compact::context_analysis::{
    analyze_context_usage_with_window, ContextAnalysis, ContextAnalysisInput,
};
use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct ContextHandler;

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn render_bar(percent: f32, width: usize) -> String {
    let pct = percent.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f32).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    let mut bar = String::with_capacity(width);
    for _ in 0..filled {
        bar.push('\u{2588}');
    }
    for _ in 0..empty {
        bar.push('\u{2591}');
    }
    bar
}

fn render_tui(report: &ContextAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("## Estimated Conversation Context".into());
    lines.push(String::new());
    lines.push(format!("**Model:** {}", report.model));
    lines.push(format!(
        "**Window:** {} tokens",
        format_tokens(report.context_window)
    ));
    lines.push(format!(
        "**Used:**   {} / {} ({:.1}%){}",
        format_tokens(report.total_used),
        format_tokens(report.context_window),
        report.total_percent,
        if report.compacted {
            "  [compacted]"
        } else {
            ""
        },
    ));
    lines.push(format!(
        "**Messages:** {} in -> {} after pre-send pipeline",
        report.messages_in, report.messages_out,
    ));
    lines.push(String::new());
    lines.push("### Breakdown".into());
    lines.push(String::new());
    let bar_width = 24;
    let label_width = report
        .categories
        .iter()
        .map(|c| c.label.len())
        .max()
        .unwrap_or(0);
    for cat in &report.categories {
        lines.push(format!(
            "  {:<lw$}  {}  {:>7}  {:>5.1}%",
            cat.label,
            render_bar(cat.percent, bar_width),
            format_tokens(cat.tokens),
            cat.percent,
            lw = label_width,
        ));
    }
    lines.push(String::new());
    if !report.unavailable_categories.is_empty() {
        lines.push(format!(
            "Unavailable in this command context: {}.",
            report.unavailable_categories.join(", ")
        ));
    }
    lines.push(
        "Note: estimated conversation context only. Snip + microcompact are \
         simulated; the async tool-result-budget pass is skipped."
            .into(),
    );
    for note in &report.estimation_notes {
        lines.push(format!("Note: {note}"));
    }
    lines.join("\n")
}

#[async_trait]
impl CommandHandler for ContextHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let mode = args.trim().to_ascii_lowercase();
        let hook_results_str = if ctx.app_state.hooks.is_empty() {
            Some(String::new())
        } else {
            serde_json::to_string(&ctx.app_state.hooks).ok()
        };
        let input = ContextAnalysisInput {
            messages: &ctx.messages,
            system_prompt: None,
            skills_manifest: None,
            cached_files_chars: 0,
            tools_schema: None,
            hook_results: hook_results_str.as_deref(),
            model: &ctx.app_state.main_loop_model,
        };
        let context_window = allthecodes_utils::tokens::get_context_window_size_with_settings(
            &ctx.app_state.main_loop_model,
            Some(&ctx.app_state.settings),
        );
        let report = analyze_context_usage_with_window(input, Some(context_window));
        match mode.as_str() {
            "json" | "raw" => {
                let json = serde_json::to_string_pretty(&report)
                    .unwrap_or_else(|e| format!("(serialisation error: {})", e));
                Ok(CommandResult::Output(json))
            }
            "" | "tui" | "full" => Ok(CommandResult::Output(render_tui(&report))),
            other => Ok(CommandResult::Output(format!(
                "Unknown /context subcommand '{}'.\n\n\
                 Usage:\n  \
                 /context        - rendered TUI output (token grid + percentages)\n  \
                 /context json   - machine-readable JSON (headless / scripting)\n",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_user_msg(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_context_empty_renders_tui() {
        let handler = ContextHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Estimated Conversation Context"));
                assert!(text.contains("Breakdown"));
                assert!(text.contains("free"));
                assert!(text.contains("Unavailable"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_context_with_messages_tui() {
        let handler = ContextHandler;
        let mut ctx = test_ctx();
        ctx.messages = vec![
            make_user_msg("Hello, world!"),
            make_user_msg("Another message here."),
        ];
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Estimated Conversation Context"));
                assert!(text.contains("messages"));
                assert!(text.contains(&ctx.app_state.main_loop_model));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_context_json_output_parses() {
        let handler = ContextHandler;
        let mut ctx = test_ctx();
        ctx.messages = vec![make_user_msg("hello")];
        let result = handler.execute("json", &mut ctx).await.unwrap();
        let text = match result {
            CommandResult::Output(text) => text,
            _ => panic!("Expected Output result"),
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("/context json must emit valid JSON");
        assert!(parsed.get("model").is_some());
        assert!(parsed.get("context_window").is_some());
        assert!(parsed.get("estimation_notes").is_some());
        assert!(parsed.get("unavailable_categories").is_some());
        let total_used = parsed.get("total_used").unwrap().as_u64().unwrap();
        assert!(total_used > 0);
        let total_pct = parsed.get("total_percent").unwrap().as_f64().unwrap();
        assert!((0.0..=100.0).contains(&total_pct));
        let cats = parsed.get("categories").unwrap().as_array().unwrap();
        assert!(!cats.is_empty());
    }

    #[tokio::test]
    async fn test_context_json_alias_raw() {
        let handler = ContextHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("raw", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                let _parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[tokio::test]
    async fn test_context_unknown_subcommand() {
        let handler = ContextHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("wobble", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown /context subcommand"));
                assert!(text.contains("wobble"));
            }
            _ => panic!("Expected Output result"),
        }
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(1500), "1.5K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_render_bar_bounds() {
        let zero = render_bar(0.0, 10);
        assert_eq!(zero.chars().filter(|c| *c == '\u{2588}').count(), 0);
        let full = render_bar(100.0, 10);
        assert_eq!(full.chars().filter(|c| *c == '\u{2588}').count(), 10);
        let half = render_bar(50.0, 10);
        let filled = half.chars().filter(|c| *c == '\u{2588}').count();
        assert!(
            filled == 5,
            "expected 5 filled chars at 50%, got {}",
            filled
        );
        let over = render_bar(999.0, 10);
        assert_eq!(over.chars().filter(|c| *c == '\u{2588}').count(), 10);
    }
}
