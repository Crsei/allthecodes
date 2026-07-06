//! `/sleep` command -- set proactive sleep duration.
//!
//! Schedules a sleep period (in seconds) during which the proactive
//! tick loop pauses autonomous actions.
//!
//! Requires proactive mode to be enabled.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::{OnceLock, RwLock};

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::features::{self, Feature};

fn proactive_sleep_enabled() -> bool {
    features::enabled(Feature::Proactive)
        || allthecodes_types::proactive_context::is_proactive_active()
}

/// Minimum sleep duration in seconds.
const MIN_SLEEP_SECS: u64 = 1;
/// Maximum sleep duration in seconds (1 hour).
const MAX_SLEEP_SECS: u64 = 3600;

#[derive(Debug, Clone)]
pub struct DaemonSleepState {
    pub sleeping_until: DateTime<Utc>,
}

#[derive(Clone, Copy)]
pub struct SleepCommandRuntime {
    pub write_sleep_state: fn(u64, &str) -> Result<DaemonSleepState>,
}

static SLEEP_RUNTIME: OnceLock<RwLock<Option<SleepCommandRuntime>>> = OnceLock::new();

pub fn set_sleep_command_runtime(runtime: SleepCommandRuntime) {
    let slot = SLEEP_RUNTIME.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(runtime);
    }
}

fn sleep_runtime() -> Result<SleepCommandRuntime> {
    let Some(slot) = SLEEP_RUNTIME.get() else {
        anyhow::bail!("Sleep command runtime is unavailable; install SleepCommandRuntime adapter");
    };
    let Ok(guard) = slot.read() else {
        anyhow::bail!("Sleep command runtime lock is poisoned");
    };
    guard.as_ref().copied().ok_or_else(|| {
        anyhow::anyhow!("Sleep command runtime is unavailable; install SleepCommandRuntime adapter")
    })
}

pub struct SleepCmdHandler;

#[async_trait]
impl CommandHandler for SleepCmdHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        if !proactive_sleep_enabled() {
            return Ok(CommandResult::Output(
                "Sleep command requires proactive mode".into(),
            ));
        }

        let trimmed = args.trim();
        if trimmed.is_empty() {
            return Ok(CommandResult::Output(format!(
                "Usage: /sleep <seconds> (range: {}-{})\n\
                 Current tick interval: {}",
                MIN_SLEEP_SECS,
                MAX_SLEEP_SECS,
                match ctx.app_state.autonomous_tick_ms {
                    Some(ms) => format!("{}ms", ms),
                    None => "disabled".to_string(),
                }
            )));
        }

        let secs: u64 = match trimmed.parse() {
            Ok(v) => v,
            Err(_) => {
                return Ok(CommandResult::Output(format!(
                    "Invalid number: '{}'. Usage: /sleep <seconds> ({}-{})",
                    trimmed, MIN_SLEEP_SECS, MAX_SLEEP_SECS
                )));
            }
        };

        if !(MIN_SLEEP_SECS..=MAX_SLEEP_SECS).contains(&secs) {
            return Ok(CommandResult::Output(format!(
                "Sleep duration must be between {} and {} seconds.",
                MIN_SLEEP_SECS, MAX_SLEEP_SECS
            )));
        }

        let sleep_state = (sleep_runtime()?.write_sleep_state)(secs, "slash command /sleep")?;

        Ok(CommandResult::Output(format!(
            "Sleep scheduled until {} ({} second{}). Proactive actions paused.",
            sleep_state.sleeping_until.to_rfc3339(),
            secs,
            if secs == 1 { "" } else { "s" }
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
    use allthecodes_config::features::FeatureFlags;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
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

    struct FeatureOverrideGuard {
        previous: Option<FeatureFlags>,
    }

    impl FeatureOverrideGuard {
        fn proactive_enabled() -> Self {
            let previous = features::runtime_override();
            let mut flags = FeatureFlags::all_disabled();
            flags.proactive = true;
            features::set_runtime_override(flags);
            Self { previous }
        }

        fn all_disabled() -> Self {
            let previous = features::runtime_override();
            features::set_runtime_override(FeatureFlags::all_disabled());
            Self { previous }
        }
    }

    impl Drop for FeatureOverrideGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(flags) => features::set_runtime_override(flags),
                None => features::clear_runtime_override(),
            }
        }
    }

    struct ProactiveActiveGuard;

    impl ProactiveActiveGuard {
        fn inactive() -> Self {
            allthecodes_types::proactive_context::set_proactive_active(false);
            Self
        }

        fn active() -> Self {
            allthecodes_types::proactive_context::set_proactive_active(true);
            Self
        }
    }

    impl Drop for ProactiveActiveGuard {
        fn drop(&mut self) {
            allthecodes_types::proactive_context::set_proactive_active(false);
        }
    }

    fn write_shared_sleep_for_test(secs: u64, reason: &str) -> Result<DaemonSleepState> {
        let state = allthecodes_config::proactive_sleep::write_sleep_state(secs, reason)?;
        Ok(DaemonSleepState {
            sleeping_until: state.sleeping_until,
        })
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_feature_gate() {
        let _features = FeatureOverrideGuard::all_disabled();
        let _active = ProactiveActiveGuard::inactive();
        let handler = SleepCmdHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("10", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("proactive mode")),
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_all_args_gated() {
        let _features = FeatureOverrideGuard::all_disabled();
        let _active = ProactiveActiveGuard::inactive();
        let handler = SleepCmdHandler;
        let mut ctx = test_ctx();

        for input in &["", "abc", "0", "9999", "60"] {
            let result = handler.execute(input, &mut ctx).await.unwrap();
            match result {
                CommandResult::Output(text) => assert!(
                    text.contains("proactive mode"),
                    "expected gate message for input '{}'",
                    input
                ),
                _ => panic!("Expected Output for input '{}'", input),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn sleep_command_uses_runtime_writer_for_shared_sleep_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureOverrideGuard::proactive_enabled();
        set_sleep_command_runtime(SleepCommandRuntime {
            write_sleep_state: write_shared_sleep_for_test,
        });
        let handler = SleepCmdHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("5", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => assert!(text.contains("Sleep scheduled until")),
            _ => panic!("Expected Output"),
        }
        let shared = allthecodes_config::proactive_sleep::active_sleep_state()
            .unwrap()
            .expect("shared sleep state");
        assert_eq!(shared.reason.as_deref(), Some("slash command /sleep"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn sleep_command_allows_active_proactive_controller_without_feature_env() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureOverrideGuard::all_disabled();
        let _active = ProactiveActiveGuard::active();
        set_sleep_command_runtime(SleepCommandRuntime {
            write_sleep_state: write_shared_sleep_for_test,
        });
        let handler = SleepCmdHandler;
        let mut ctx = test_ctx();

        let result = handler.execute("5", &mut ctx).await.unwrap();

        match result {
            CommandResult::Output(text) => assert!(text.contains("Sleep scheduled until")),
            _ => panic!("Expected Output"),
        }
    }
}
