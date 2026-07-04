//! Proactive tick worker -- periodically queues autonomous assistant work.

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::memory_log::append_log_entry;
use super::state::DaemonState;
use crate::protocol::{DaemonCommand, DaemonCommandKind};
use crate::supervisor::ASSISTANT_WORKER_ID;

pub const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;

pub async fn tick_loop(_state: DaemonState) {
    let mut interval = tokio::time::interval(Duration::from_millis(DEFAULT_TICK_INTERVAL_MS));
    info!(
        "proactive tick loop started (interval: {}ms)",
        DEFAULT_TICK_INTERVAL_MS
    );

    interval.tick().await;
    loop {
        interval.tick().await;
        match enqueue_proactive_tick_once(Local::now(), false) {
            Ok(Some(command)) => {
                debug!(
                    command_id = %command.command_id,
                    "proactive tick queued assistant command"
                );
            }
            Ok(None) => {
                debug!("proactive tick skipped: automation state is not idle");
            }
            Err(error) => {
                warn!(error = %error, "failed to queue proactive tick");
            }
        }
    }
}

pub fn enqueue_proactive_tick_once(
    now: DateTime<Local>,
    terminal_focus: bool,
) -> Result<Option<DaemonCommand>> {
    if super::automation_state::autonomous_worker_blocked() {
        return Ok(None);
    }

    let payload = build_tick_payload(now, terminal_focus)?;
    let command = crate::protocol_store().enqueue_command(
        ASSISTANT_WORKER_ID,
        DaemonCommandKind::Submit,
        payload,
        Some(format!("proactive_tick:{}", now.timestamp_millis())),
    )?;
    append_log_entry(&format!(
        "proactive tick queued assistant command (focus={})",
        terminal_focus
    ));
    Ok(Some(command))
}

pub fn build_tick_payload(now: DateTime<Local>, terminal_focus: bool) -> Result<Value> {
    let today_log = super::memory_log::read_today_log();
    let tick_prompt = format!(
        "<tick_tag>\nLocal time: {}\nTerminal focus: {}\n</tick_tag>{}",
        now.format("%Y-%m-%d %H:%M:%S"),
        terminal_focus,
        if today_log.is_empty() {
            String::new()
        } else {
            format!("\n<daily_log>\n{}</daily_log>", today_log)
        },
    );

    Ok(json!({
        "text": tick_prompt,
        "message_id": format!("proactive-tick-{}", now.timestamp_millis()),
        "source": "proactive_tick",
        "terminal_focus": terminal_focus,
        "proactive": {
            "time": now.to_rfc3339(),
            "terminal_focus": terminal_focus,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_state::DaemonSleepState;
    use crate::protocol::DaemonCommandKind;
    use crate::supervisor::ASSISTANT_WORKER_ID;
    use chrono::Utc;
    use serial_test::serial;

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

    fn write_sleep_state() {
        let now = Utc::now();
        let state = DaemonSleepState {
            schema_version: 2,
            sleeping_until: now + chrono::Duration::seconds(60),
            reason: Some("test sleep".to_string()),
            updated_at: now,
        };
        let path = allthecodes_config::paths::daemon_dir().join("sleep-state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    }

    #[test]
    #[serial]
    fn proactive_tick_enqueues_assistant_submit_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        append_log_entry("previous work item");

        let command = enqueue_proactive_tick_once(Local::now(), false)
            .unwrap()
            .expect("tick should enqueue");

        assert_eq!(command.target_worker_id, ASSISTANT_WORKER_ID);
        assert_eq!(command.kind, DaemonCommandKind::Submit);
        assert_eq!(command.payload["source"], "proactive_tick");
        assert_eq!(command.payload["terminal_focus"], false);
        assert!(command.payload["text"]
            .as_str()
            .unwrap()
            .contains("<tick_tag>"));
        let commands = crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap();
        assert_eq!(commands.len(), 1);
    }

    #[test]
    #[serial]
    fn proactive_tick_skips_while_sleeping() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        write_sleep_state();

        let result = enqueue_proactive_tick_once(Local::now(), false).unwrap();

        assert!(result.is_none());
        assert!(crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap()
            .is_empty());
    }
}
