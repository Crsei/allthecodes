//! Proactive tick worker -- periodically queues autonomous assistant work.

use std::time::Duration;

use allthecodes_config::features::{self, Feature};
use allthecodes_services::skill_search_prefetch::{
    collect_skill_discovery_prefetch, start_skill_discovery_prefetch, SkillPrefetchContext,
};
use anyhow::Result;
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::memory_log::append_log_entry;
use super::state::DaemonState;
use crate::protocol::{DaemonCommand, DaemonCommandKind};
use crate::supervisor::ASSISTANT_WORKER_ID;

pub const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;

pub async fn tick_loop(state: DaemonState) {
    let mut interval = tokio::time::interval(Duration::from_millis(DEFAULT_TICK_INTERVAL_MS));
    info!(
        "proactive tick loop started (interval: {}ms)",
        DEFAULT_TICK_INTERVAL_MS
    );

    interval.tick().await;
    loop {
        interval.tick().await;
        match enqueue_proactive_tick_once(Local::now(), state.terminal_focus()) {
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
    if !proactive_ticks_enabled() {
        return Ok(None);
    }
    if super::automation_state::autonomous_worker_blocked() {
        return Ok(None);
    }

    write_proactive_schedule(now);
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
    let mut payload = allthecodes_services::proactive::build_tick_payload(
        now,
        terminal_focus,
        (!today_log.is_empty()).then_some(today_log.as_str()),
    );
    if let Some(skill_discovery) = skill_discovery_tick_summary() {
        if let Some(proactive) = payload["proactive"].as_object_mut() {
            proactive.insert("skill_discovery".to_string(), skill_discovery);
        }
    }

    Ok(payload)
}

fn proactive_ticks_enabled() -> bool {
    match crate::process_state::read_proactive_state() {
        Ok(Some(state)) => state.active,
        Ok(None) => {
            features::enabled(Feature::Proactive)
                || allthecodes_services::proactive::global_controller()
                    .snapshot()
                    .status
                    == allthecodes_services::proactive::ProactiveStatus::Active
        }
        Err(error) => {
            warn!(error = %error, "failed to read daemon proactive state; using feature gate");
            features::enabled(Feature::Proactive)
        }
    }
}

fn write_proactive_schedule(now: DateTime<Local>) {
    let next_tick_at = now.with_timezone(&chrono::Utc)
        + chrono::Duration::milliseconds(DEFAULT_TICK_INTERVAL_MS as i64);
    if let Err(error) = crate::process_state::write_proactive_state(true, Some(next_tick_at)) {
        warn!(error = %error, "failed to write daemon proactive state");
    }
}

fn skill_discovery_tick_summary() -> Option<Value> {
    if !features::enabled(Feature::ExperimentalSkillSearch) {
        return None;
    }

    let result =
        collect_skill_discovery_prefetch(start_skill_discovery_prefetch(SkillPrefetchContext {
            session_id: ASSISTANT_WORKER_ID.to_string(),
            query: String::new(),
        }));
    Some(json!({
        "count": result.skills.len(),
        "remote_state": result.remote_state,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_state::DaemonSleepState;
    use crate::protocol::DaemonCommandKind;
    use crate::supervisor::ASSISTANT_WORKER_ID;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_commands::{CommandContext, CommandHandler, CommandResult};
    use allthecodes_config::features::{self, FeatureFlags};
    use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
    use chrono::{TimeZone, Utc};
    use serial_test::serial;
    use std::path::PathBuf;

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

    struct FeatureGuard;

    impl FeatureGuard {
        fn set(flags: FeatureFlags) -> Self {
            features::set_runtime_override(flags);
            Self
        }
    }

    impl Drop for FeatureGuard {
        fn drop(&mut self) {
            features::clear_runtime_override();
        }
    }

    struct SkillRegistryGuard;

    impl SkillRegistryGuard {
        fn new(skills: Vec<SkillDefinition>) -> Self {
            allthecodes_skills::clear_skills();
            for skill in skills {
                allthecodes_skills::register_skill(skill);
            }
            Self
        }
    }

    impl Drop for SkillRegistryGuard {
        fn drop(&mut self) {
            allthecodes_skills::clear_skills();
        }
    }

    fn make_skill(name: &str, description: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source: SkillSource::User,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                name: Some(name.to_string()),
                description: description.to_string(),
                when_to_use: Some(format!("Use {name} locally")),
                ..Default::default()
            },
            prompt_body: "local prompt body".to_string(),
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

    fn test_command_context() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/tmp/proactive-daemon-test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("proactive-daemon-test"),
        }
    }

    #[test]
    #[serial]
    fn proactive_tick_enqueues_assistant_submit_command() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });
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
        let _features = FeatureGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });
        write_sleep_state();

        let result = enqueue_proactive_tick_once(Local::now(), false).unwrap();

        assert!(result.is_none());
        assert!(crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[serial]
    fn proactive_tick_skips_while_context_blocked_by_another_process() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let _features = FeatureGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test-active");
        allthecodes_types::proactive_context::set_context_blocked(true, "plan_mode");

        let result = enqueue_proactive_tick_once(Local::now(), false).unwrap();

        allthecodes_types::proactive_context::set_context_blocked(false, "test-cleanup");
        controller.deactivate("test-cleanup");
        assert!(result.is_none());
        assert!(crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn proactive_slash_disable_blocks_future_daemon_ticks() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test-active");
        let _features = FeatureGuard::set(FeatureFlags {
            proactive: true,
            ..FeatureFlags::all_disabled()
        });
        let handler = allthecodes_commands::proactive_cmd::ProactiveCmdHandler;
        let mut ctx = test_command_context();

        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("disabled")),
            _ => panic!("expected output"),
        }
        let durable = allthecodes_services::proactive::read_durable_state()
            .unwrap()
            .expect("durable proactive state after disable");
        assert!(!durable.active);

        let tick = enqueue_proactive_tick_once(Local::now(), false).unwrap();

        controller.deactivate("test-cleanup");
        assert!(tick.is_none());
        assert!(crate::protocol_store()
            .read_worker_commands(ASSISTANT_WORKER_ID)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[serial]
    fn proactive_tick_payload_includes_skill_discovery_state_when_enabled() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let mut flags = FeatureFlags::all_disabled();
        flags.experimental_skill_search = true;
        let _features = FeatureGuard::set(flags);
        let _skills = SkillRegistryGuard::new(vec![make_skill(
            "rust-review",
            "Review Rust code without remote fetch",
        )]);
        let now = Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();

        let payload = build_tick_payload(now, true).expect("payload");

        assert_eq!(
            payload["proactive"]["skill_discovery"]["remote_state"],
            "deferred"
        );
        assert_eq!(payload["proactive"]["skill_discovery"]["count"], 1);
        let body = serde_json::to_string(&payload).unwrap();
        assert!(!body.contains("http://"));
        assert!(!body.contains("https://"));
    }

    #[test]
    #[serial]
    fn proactive_tick_payload_matches_shared_builder_fields() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        append_log_entry("shared payload daily log");
        let now = Local.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();

        let payload = build_tick_payload(now, true).expect("payload");
        let today_log = crate::memory_log::read_today_log();
        let expected = allthecodes_services::proactive::build_tick_payload(
            now,
            true,
            (!today_log.is_empty()).then_some(today_log.as_str()),
        );

        assert_eq!(payload["source"], "proactive_tick");
        assert_eq!(payload["terminal_focus"], expected["terminal_focus"]);
        assert_eq!(payload["proactive"]["time"], expected["proactive"]["time"]);
        assert_eq!(
            payload["proactive"]["terminal_focus"],
            expected["proactive"]["terminal_focus"]
        );
        assert_eq!(payload["text"], expected["text"]);
    }
}
