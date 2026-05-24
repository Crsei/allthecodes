//! `/dream` command -- distill daily logs into memory (KAIROS).
//!
//! Compresses recent session logs into long-term memory summaries.
//! Accepts an optional `--days N` argument (default 7).
//!
//! Requires `FEATURE_KAIROS=1`.

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

pub struct DreamHandler;

/// Parse `--days N` from the argument string.  Returns `None` on parse failure.
fn parse_days(args: &str) -> Option<u32> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "--days" {
            if let Some(val) = parts.get(i + 1) {
                return val.parse::<u32>().ok();
            }
        }
    }
    None
}

#[async_trait]
impl CommandHandler for DreamHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        if !features::enabled(Feature::Kairos) {
            return Ok(CommandResult::Output(
                "Dream mode requires FEATURE_KAIROS=1".into(),
            ));
        }

        let trimmed = args.trim();

        // Handle help / unknown flags
        if trimmed == "help" || trimmed == "--help" {
            return Ok(CommandResult::Output(
                "Usage: /dream [--days N]  (default: 7 days)".into(),
            ));
        }

        let days = if trimmed.is_empty() {
            7
        } else {
            match parse_days(trimmed) {
                Some(d) if d > 0 => d,
                _ => {
                    return Ok(CommandResult::Output(
                        "Usage: /dream [--days N]  (default: 7 days)".into(),
                    ));
                }
            }
        };

        Ok(CommandResult::Output(format!(
            "Distilling last {} days of logs into memory...",
            days
        )))
    }
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
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_feature_gate() {
        let handler = DreamHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_unknown_argument_gated() {
        let handler = DreamHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("--days 30", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS")),
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn test_parse_days_valid() {
        assert_eq!(parse_days("--days 14"), Some(14));
        assert_eq!(parse_days("--days 1"), Some(1));
    }

    #[test]
    fn test_parse_days_missing_value() {
        assert_eq!(parse_days("--days"), None);
    }

    #[test]
    fn test_parse_days_invalid() {
        assert_eq!(parse_days("--days abc"), None);
        assert_eq!(parse_days("random text"), None);
    }

    #[test]
    fn test_parse_days_absent() {
        assert_eq!(parse_days(""), None);
    }
}
