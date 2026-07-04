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
use allthecodes_services::dream;

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

        let written = dream::run_recent_days(days, chrono::Local::now().date_naive())?;
        if written.is_empty() {
            return Ok(CommandResult::Output(format!(
                "No dream memory written for the last {} days; logs were missing or already distilled.",
                days
            )));
        }

        let paths = written
            .iter()
            .map(|path| format!("- {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(CommandResult::Output(format!(
            "Dream memory written for {} day(s):\n{}",
            written.len(),
            paths
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
    use allthecodes_config::features::{self, FeatureFlags};
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    struct FeatureGuard(Option<FeatureFlags>);

    impl FeatureGuard {
        fn set(flags: FeatureFlags) -> Self {
            let previous = features::runtime_override();
            features::set_runtime_override(flags);
            Self(previous)
        }

        fn disable_all() -> Self {
            Self::set(FeatureFlags::all_disabled())
        }

        fn enable_kairos() -> Self {
            Self::set(FeatureFlags {
                kairos: true,
                proactive: true,
                ..FeatureFlags::all_disabled()
            })
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => features::set_runtime_override(previous),
                None => features::clear_runtime_override(),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_feature_gate() {
        let _features = FeatureGuard::disable_all();
        let handler = DreamHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("FEATURE_KAIROS")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_unknown_argument_gated() {
        let _features = FeatureGuard::disable_all();
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

    #[tokio::test]
    #[serial_test::serial]
    async fn kairos_dream_writes_today_memory_file() {
        use chrono::Local;

        struct EnvGuard {
            key: &'static str,
            previous: Option<String>,
        }

        impl EnvGuard {
            fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
                let previous = std::env::var(key).ok();
                std::env::set_var(key, value.as_ref());
                Self { key, previous }
            }
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }

        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureGuard::enable_kairos();

        let today = Local::now();
        let log_path = allthecodes_config::paths::daily_log_path(today);
        std::fs::create_dir_all(log_path.parent().expect("daily log has parent")).unwrap();
        std::fs::write(&log_path, "- Task: command dream distillation.\n").unwrap();

        let handler = DreamHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("--days 1", &mut ctx).await.unwrap();
        let output = match result {
            CommandResult::Output(text) => text,
            _ => panic!("Expected Output"),
        };

        assert!(output.contains("Dream memory written"));
        assert!(output.contains("memory/dream"));
        let expected = home
            .path()
            .join("memory")
            .join("dream")
            .join(format!("{}.md", today.format("%Y-%m-%d")));
        assert!(
            expected.exists(),
            "expected dream memory at {}",
            expected.display()
        );
    }
}
