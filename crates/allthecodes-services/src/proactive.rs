use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use allthecodes_config::proactive_sleep::{
    active_sleep_state, clear_sleep_state, read_sleep_state, write_sleep_state,
    write_sleep_state_until, SleepState, SLEEP_STATE_SCHEMA_VERSION,
};

pub const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;
pub const DURABLE_PROACTIVE_STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProactiveStatus {
    Inactive,
    Active,
    Paused,
    ContextBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProactiveSnapshot {
    pub status: ProactiveStatus,
    pub source: Option<String>,
    pub next_tick_at: Option<DateTime<Utc>>,
    pub paused_reason: Option<String>,
    pub context_blocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableProactiveState {
    pub schema_version: u32,
    pub active: bool,
    pub next_tick_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ProactiveController {
    state: RwLock<ProactiveSnapshot>,
    publish_global_state: bool,
}

impl ProactiveController {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(inactive_snapshot(None)),
            publish_global_state: false,
        }
    }

    fn with_global_state_publisher() -> Self {
        Self {
            state: RwLock::new(inactive_snapshot(None)),
            publish_global_state: true,
        }
    }

    pub fn snapshot(&self) -> ProactiveSnapshot {
        self.state.read().clone()
    }

    pub fn activate(&self, source: &str) {
        let mut state = self.state.write();
        *state = ProactiveSnapshot {
            status: ProactiveStatus::Active,
            source: normalized_text(source),
            next_tick_at: Some(next_tick_at()),
            paused_reason: None,
            context_blocked: false,
        };
        self.publish_active_state(&state);
    }

    pub fn pause(&self, reason: &str) {
        let mut state = self.state.write();
        state.status = ProactiveStatus::Paused;
        state.next_tick_at = None;
        state.paused_reason = normalized_text(reason);
        state.context_blocked = false;
        self.publish_active_state(&state);
    }

    pub fn resume(&self, source: &str) {
        let mut state = self.state.write();
        state.status = ProactiveStatus::Active;
        state.source = normalized_text(source);
        state.next_tick_at = Some(next_tick_at());
        state.paused_reason = None;
        state.context_blocked = false;
        self.publish_active_state(&state);
    }

    pub fn deactivate(&self, source: &str) {
        let mut state = self.state.write();
        *state = inactive_snapshot(normalized_text(source));
        self.publish_active_state(&state);
    }

    pub fn set_context_blocked(&self, blocked: bool, reason: &str) {
        if blocked {
            self.mark_context_blocked(reason);
        } else {
            self.clear_context_blocked(reason);
        }
    }

    pub fn mark_context_blocked(&self, reason: &str) {
        let mut state = self.state.write();
        if matches!(
            state.status,
            ProactiveStatus::Active | ProactiveStatus::ContextBlocked
        ) {
            state.status = ProactiveStatus::ContextBlocked;
            state.paused_reason = normalized_text(reason);
        }
        state.next_tick_at = None;
        state.context_blocked = true;
        self.publish_active_state(&state);
    }

    pub fn clear_context_blocked(&self, source: &str) {
        let mut state = self.state.write();
        if state.status == ProactiveStatus::ContextBlocked {
            state.status = ProactiveStatus::Active;
            state.source = normalized_text(source);
            state.next_tick_at = Some(next_tick_at());
            state.paused_reason = None;
        }
        state.context_blocked = false;
        self.publish_active_state(&state);
    }

    fn publish_active_state(&self, state: &ProactiveSnapshot) {
        if self.publish_global_state {
            allthecodes_types::proactive_context::set_proactive_active(
                state.status == ProactiveStatus::Active,
            );
        }
    }
}

impl Default for ProactiveController {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global_controller() -> &'static ProactiveController {
    static CONTROLLER: LazyLock<Arc<ProactiveController>> = LazyLock::new(|| {
        let controller = Arc::new(ProactiveController::with_global_state_publisher());
        let callback_controller = Arc::clone(&controller);
        allthecodes_types::proactive_context::register_context_blocked_callback(Arc::new(
            move |blocked, reason| {
                callback_controller.set_context_blocked(blocked, reason);
            },
        ));
        controller
    });
    CONTROLLER.as_ref()
}

pub fn write_durable_state(
    active: bool,
    next_tick_at: Option<DateTime<Utc>>,
) -> Result<DurableProactiveState> {
    let state = DurableProactiveState {
        schema_version: DURABLE_PROACTIVE_STATE_SCHEMA_VERSION,
        active,
        next_tick_at,
        updated_at: Utc::now(),
    };
    write_durable_state_file(&state)?;
    Ok(state)
}

pub fn read_durable_state() -> Result<Option<DurableProactiveState>> {
    let path = durable_state_path();
    read_durable_state_from_path(&path, |path| fs::read_to_string(path))
}

pub fn clear_durable_state(reason: &str) -> Result<bool> {
    let path = durable_state_path();
    match fs::remove_file(&path) {
        Ok(()) => {
            tracing::debug!(
                reason = %reason,
                path = %path.display(),
                "cleared durable proactive state"
            );
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to clear daemon proactive state {}", path.display())),
    }
}

pub fn build_tick_payload(
    now: DateTime<Local>,
    terminal_focus: bool,
    daily_log: Option<&str>,
) -> Value {
    let daily_log = daily_log.filter(|log| !log.is_empty());
    let tick_prompt = format!(
        "<tick_tag>\nLocal time: {}\nTerminal focus: {}\n</tick_tag>{}",
        now.format("%Y-%m-%d %H:%M:%S"),
        terminal_focus,
        daily_log
            .map(|log| format!("\n<daily_log>\n{log}</daily_log>"))
            .unwrap_or_default(),
    );

    json!({
        "text": tick_prompt,
        "message_id": format!("proactive-tick-{}", now.timestamp_millis()),
        "source": "proactive_tick",
        "terminal_focus": terminal_focus,
        "proactive": {
            "time": now.to_rfc3339(),
            "terminal_focus": terminal_focus
        }
    })
}

fn inactive_snapshot(source: Option<String>) -> ProactiveSnapshot {
    ProactiveSnapshot {
        status: ProactiveStatus::Inactive,
        source,
        next_tick_at: None,
        paused_reason: None,
        context_blocked: false,
    }
}

fn next_tick_at() -> DateTime<Utc> {
    Utc::now() + Duration::milliseconds(DEFAULT_TICK_INTERVAL_MS as i64)
}

fn durable_state_path() -> PathBuf {
    allthecodes_config::paths::daemon_dir().join("proactive-state.json")
}

fn write_durable_state_file(state: &DurableProactiveState) -> Result<()> {
    let path = durable_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create daemon directory {}", parent.display()))?;
    }

    let tmp = path.with_file_name(format!(
        "proactive-state.json.{}.{}.tmp",
        std::process::id(),
        Utc::now().timestamp_micros()
    ));
    fs::write(&tmp, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("failed to write daemon proactive state {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "failed to replace daemon proactive state {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

fn read_durable_state_from_path<F>(
    path: &Path,
    read_to_string: F,
) -> Result<Option<DurableProactiveState>>
where
    F: FnOnce(&Path) -> std::io::Result<String>,
{
    let text = match read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read daemon proactive state {}", path.display())
            })
        }
    };
    let state = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon proactive state {}", path.display()))?;
    Ok(Some(state))
}

fn normalized_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serial_test::serial;
    use std::fs;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var_os(key);
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

    #[test]
    fn controller_activate_pause_resume_and_deactivate_updates_snapshot() {
        let controller = ProactiveController::new();

        assert_eq!(controller.snapshot().status, ProactiveStatus::Inactive);

        controller.activate("test");
        let active = controller.snapshot();
        assert_eq!(active.status, ProactiveStatus::Active);
        assert_eq!(active.source.as_deref(), Some("test"));
        assert!(active.next_tick_at.is_some());

        controller.pause("user_input");
        let paused = controller.snapshot();
        assert_eq!(paused.status, ProactiveStatus::Paused);
        assert!(paused.next_tick_at.is_none());

        controller.resume("turn_complete");
        let resumed = controller.snapshot();
        assert_eq!(resumed.status, ProactiveStatus::Active);
        assert!(resumed.next_tick_at.is_some());

        controller.deactivate("slash_command");
        let inactive = controller.snapshot();
        assert_eq!(inactive.status, ProactiveStatus::Inactive);
        assert!(inactive.next_tick_at.is_none());
    }

    #[test]
    fn context_blocked_clears_next_tick_and_resume_reschedules() {
        let controller = ProactiveController::new();
        controller.activate("test");

        controller.set_context_blocked(true, "compact_required");
        let blocked = controller.snapshot();
        assert_eq!(blocked.status, ProactiveStatus::ContextBlocked);
        assert!(blocked.next_tick_at.is_none());
        assert_eq!(blocked.paused_reason.as_deref(), Some("compact_required"));
        assert!(blocked.context_blocked);

        controller.set_context_blocked(false, "compact_complete");
        let active = controller.snapshot();
        assert_eq!(active.status, ProactiveStatus::Active);
        assert!(active.next_tick_at.is_some());
        assert_eq!(active.source.as_deref(), Some("compact_complete"));
        assert!(active.paused_reason.is_none());
        assert!(!active.context_blocked);
    }

    #[test]
    fn context_blocked_ready_does_not_activate_inactive_or_paused_controller() {
        let inactive = ProactiveController::new();
        inactive.set_context_blocked(false, "context_ready");
        let inactive_snapshot = inactive.snapshot();
        assert_eq!(inactive_snapshot.status, ProactiveStatus::Inactive);
        assert!(inactive_snapshot.next_tick_at.is_none());
        assert!(!inactive_snapshot.context_blocked);

        let paused = ProactiveController::new();
        paused.activate("test");
        paused.pause("user_wait");
        paused.set_context_blocked(false, "context_ready");
        let paused_snapshot = paused.snapshot();
        assert_eq!(paused_snapshot.status, ProactiveStatus::Paused);
        assert!(paused_snapshot.next_tick_at.is_none());
        assert_eq!(paused_snapshot.paused_reason.as_deref(), Some("user_wait"));
        assert!(!paused_snapshot.context_blocked);
    }

    #[test]
    #[serial]
    fn context_blocked_shared_signal_updates_global_controller() {
        struct ControllerGuard;
        impl Drop for ControllerGuard {
            fn drop(&mut self) {
                allthecodes_types::proactive_context::set_context_blocked(false, "test_cleanup");
                global_controller().deactivate("test_cleanup");
            }
        }
        let _guard = ControllerGuard;

        global_controller().activate("test");
        allthecodes_types::proactive_context::set_context_blocked(true, "context_limit");

        let blocked = global_controller().snapshot();
        assert_eq!(blocked.status, ProactiveStatus::ContextBlocked);
        assert!(blocked.next_tick_at.is_none());

        allthecodes_types::proactive_context::set_context_blocked(false, "context_ready");
        let active = global_controller().snapshot();
        assert_eq!(active.status, ProactiveStatus::Active);
        assert!(!active.context_blocked);
        assert!(active.next_tick_at.is_some());
    }

    #[test]
    fn tick_payload_contains_upstream_contract_fields() {
        let now = Utc
            .with_ymd_and_hms(2026, 7, 6, 12, 30, 0)
            .unwrap()
            .with_timezone(&chrono::Local);
        let payload = build_tick_payload(now, false, Some("previous work item"));

        assert_eq!(payload["source"], "proactive_tick");
        assert_eq!(payload["terminal_focus"], false);
        assert!(payload["text"].as_str().unwrap().contains("<tick_tag>"));
        assert!(payload["text"].as_str().unwrap().contains("Local time:"));
        assert!(payload["text"]
            .as_str()
            .unwrap()
            .contains("Terminal focus: false"));
        assert!(payload["text"].as_str().unwrap().contains("<daily_log>"));
    }

    #[test]
    #[serial]
    fn sleep_state_helpers_write_read_and_clear_daemon_file() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let path = home.path().join("daemon").join("sleep-state.json");

        let state = write_sleep_state(60, " waiting for ci ").unwrap();
        assert_eq!(state.schema_version, SLEEP_STATE_SCHEMA_VERSION);
        assert_eq!(state.reason.as_deref(), Some("waiting for ci"));
        assert!(path.exists());

        let stored: SleepState = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored.schema_version, SLEEP_STATE_SCHEMA_VERSION);
        assert_eq!(stored.reason.as_deref(), Some("waiting for ci"));

        let active = active_sleep_state().unwrap().unwrap();
        assert_eq!(active.reason.as_deref(), Some("waiting for ci"));
        assert!(active.sleeping_until > Utc::now());

        assert!(clear_sleep_state("test cleanup").unwrap());
        assert!(!path.exists());
        assert!(!clear_sleep_state("second cleanup").unwrap());
    }
}
