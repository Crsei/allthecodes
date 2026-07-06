use chrono::{DateTime, Local, Utc};

use crate::ui::app::ProactiveUiStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProactiveTickDecision {
    Submit,
    Inactive,
    NotDue,
    BlockedByRunningTurn,
    BlockedByPendingInput,
    Sleeping,
}

pub(super) struct ProactiveTickDriver;

impl ProactiveTickDriver {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) fn decide(
        &self,
        streaming: bool,
        pending_permission: bool,
        pending_question: bool,
        now: DateTime<Utc>,
    ) -> ProactiveTickDecision {
        let snapshot = allthecodes_services::proactive::global_controller().snapshot();
        self.decide_with_snapshot(
            &snapshot,
            streaming,
            pending_permission,
            pending_question,
            now,
        )
    }

    fn decide_with_snapshot(
        &self,
        snapshot: &allthecodes_services::proactive::ProactiveSnapshot,
        streaming: bool,
        pending_permission: bool,
        pending_question: bool,
        now: DateTime<Utc>,
    ) -> ProactiveTickDecision {
        let decision = decide_tick(
            snapshot,
            streaming,
            pending_permission,
            pending_question,
            now,
        );
        if suppressed_due_tick_should_reschedule(snapshot, decision, now) {
            self.mark_tick_submitted();
        }
        decision
    }

    pub(super) fn build_prompt(&self, now: DateTime<Local>, terminal_focus: bool) -> String {
        let payload =
            allthecodes_services::proactive::build_tick_payload(now, terminal_focus, None);
        payload["text"].as_str().unwrap_or_default().to_string()
    }

    pub(super) fn mark_tick_submitted(&self) {
        let controller = allthecodes_services::proactive::global_controller();
        let source = controller
            .snapshot()
            .source
            .unwrap_or_else(|| "tui_tick".to_string());
        controller.resume(&source);
    }
}

fn suppressed_due_tick_should_reschedule(
    snapshot: &allthecodes_services::proactive::ProactiveSnapshot,
    decision: ProactiveTickDecision,
    now: DateTime<Utc>,
) -> bool {
    matches!(
        decision,
        ProactiveTickDecision::BlockedByRunningTurn | ProactiveTickDecision::BlockedByPendingInput
    ) && matches!(snapshot.next_tick_at, Some(next) if next <= now)
}

pub(super) fn decide_tick(
    snapshot: &allthecodes_services::proactive::ProactiveSnapshot,
    streaming: bool,
    pending_permission: bool,
    pending_question: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> ProactiveTickDecision {
    use allthecodes_services::proactive::ProactiveStatus;
    if snapshot.status != ProactiveStatus::Active {
        return ProactiveTickDecision::Inactive;
    }
    if streaming {
        return ProactiveTickDecision::BlockedByRunningTurn;
    }
    if pending_permission || pending_question {
        return ProactiveTickDecision::BlockedByPendingInput;
    }
    if allthecodes_services::proactive::active_sleep_state()
        .ok()
        .flatten()
        .is_some()
    {
        return ProactiveTickDecision::Sleeping;
    }
    match snapshot.next_tick_at {
        Some(next) if next <= now => ProactiveTickDecision::Submit,
        _ => ProactiveTickDecision::NotDue,
    }
}

pub(super) fn ui_status_from_snapshot(
    snapshot: &allthecodes_services::proactive::ProactiveSnapshot,
    sleep: Option<&allthecodes_services::proactive::SleepState>,
) -> Option<ProactiveUiStatus> {
    use allthecodes_services::proactive::ProactiveStatus;

    if snapshot.status == ProactiveStatus::Inactive {
        return None;
    }
    if let Some(sleep) = sleep {
        return Some(ProactiveUiStatus {
            label: "proactive sleeping".to_string(),
            next_tick_text: Some(format!(
                "wake in {}",
                format_countdown(sleep.sleeping_until)
            )),
        });
    }
    if snapshot.context_blocked {
        return Some(ProactiveUiStatus {
            label: "proactive blocked".to_string(),
            next_tick_text: snapshot.paused_reason.clone(),
        });
    }

    match snapshot.status {
        ProactiveStatus::Active => Some(ProactiveUiStatus {
            label: "proactive standby".to_string(),
            next_tick_text: snapshot
                .next_tick_at
                .map(|next_tick_at| format!("next tick in {}", format_countdown(next_tick_at))),
        }),
        ProactiveStatus::Paused => Some(ProactiveUiStatus {
            label: "proactive paused".to_string(),
            next_tick_text: snapshot.paused_reason.clone(),
        }),
        ProactiveStatus::ContextBlocked => Some(ProactiveUiStatus {
            label: "proactive blocked".to_string(),
            next_tick_text: snapshot.paused_reason.clone(),
        }),
        ProactiveStatus::Inactive => None,
    }
}

fn format_countdown(deadline: DateTime<Utc>) -> String {
    let remaining_millis = deadline
        .signed_duration_since(Utc::now())
        .num_milliseconds()
        .max(0);
    let remaining = (remaining_millis + 999) / 1000;
    if remaining >= 3600 {
        let hours = remaining / 3600;
        let minutes = (remaining % 3600) / 60;
        if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h{minutes}m")
        }
    } else if remaining >= 60 {
        let minutes = remaining / 60;
        let seconds = remaining % 60;
        if seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m{seconds}s")
        }
    } else {
        format!("{remaining}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
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

    #[test]
    #[serial]
    fn tick_decision_requires_active_idle_and_due() {
        let home = tempfile::tempdir().expect("allthecodes home");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let due = Utc::now() - Duration::seconds(1);
        let snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::Active,
            source: Some("test".into()),
            next_tick_at: Some(due),
            paused_reason: None,
            context_blocked: false,
        };

        assert_eq!(
            decide_tick(&snapshot, false, false, false, Utc::now()),
            ProactiveTickDecision::Submit
        );
        assert_eq!(
            decide_tick(&snapshot, true, false, false, Utc::now()),
            ProactiveTickDecision::BlockedByRunningTurn
        );
        assert_eq!(
            decide_tick(&snapshot, false, true, false, Utc::now()),
            ProactiveTickDecision::BlockedByPendingInput
        );
    }

    #[test]
    #[serial]
    fn due_streaming_tick_reschedules_when_suppressed() {
        let home = tempfile::tempdir().expect("allthecodes home");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test");
        let before = controller
            .snapshot()
            .next_tick_at
            .expect("active controller has next tick");
        let now = Utc::now();
        let due_snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::Active,
            source: Some("test".into()),
            next_tick_at: Some(now - Duration::seconds(1)),
            paused_reason: None,
            context_blocked: false,
        };

        std::thread::sleep(std::time::Duration::from_millis(2));
        let decision =
            ProactiveTickDriver::new().decide_with_snapshot(&due_snapshot, true, false, false, now);

        let after = controller
            .snapshot()
            .next_tick_at
            .expect("suppressed due tick reschedules");
        controller.deactivate("test_cleanup");

        assert_eq!(decision, ProactiveTickDecision::BlockedByRunningTurn);
        assert!(
            after > before,
            "blocked due tick should advance next_tick_at"
        );
    }

    #[test]
    #[serial]
    fn due_pending_input_tick_reschedules_when_suppressed() {
        let home = tempfile::tempdir().expect("allthecodes home");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test");
        let before = controller
            .snapshot()
            .next_tick_at
            .expect("active controller has next tick");
        let now = Utc::now();
        let due_snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::Active,
            source: Some("test".into()),
            next_tick_at: Some(now - Duration::seconds(1)),
            paused_reason: None,
            context_blocked: false,
        };

        std::thread::sleep(std::time::Duration::from_millis(2));
        let decision =
            ProactiveTickDriver::new().decide_with_snapshot(&due_snapshot, false, true, false, now);

        let after = controller
            .snapshot()
            .next_tick_at
            .expect("suppressed due tick reschedules");
        controller.deactivate("test_cleanup");

        assert_eq!(decision, ProactiveTickDecision::BlockedByPendingInput);
        assert!(
            after > before,
            "blocked due tick should advance next_tick_at"
        );
    }

    #[test]
    #[serial]
    fn blocked_tick_preserves_schedule_when_not_due() {
        let home = tempfile::tempdir().expect("allthecodes home");
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test");
        let before = controller
            .snapshot()
            .next_tick_at
            .expect("active controller has next tick");
        let now = Utc::now();
        let not_due_snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::Active,
            source: Some("test".into()),
            next_tick_at: Some(now + Duration::seconds(60)),
            paused_reason: None,
            context_blocked: false,
        };

        let decision = ProactiveTickDriver::new().decide_with_snapshot(
            &not_due_snapshot,
            true,
            false,
            false,
            now,
        );

        let after = controller
            .snapshot()
            .next_tick_at
            .expect("blocked not-due tick preserves schedule");
        controller.deactivate("test_cleanup");

        assert_eq!(decision, ProactiveTickDecision::BlockedByRunningTurn);
        assert_eq!(after, before);
    }

    #[test]
    #[serial]
    fn mark_tick_submitted_reschedules_next_tick() {
        let controller = allthecodes_services::proactive::global_controller();
        controller.activate("test");
        let before = controller
            .snapshot()
            .next_tick_at
            .expect("active controller has next tick");

        std::thread::sleep(std::time::Duration::from_millis(2));
        ProactiveTickDriver::new().mark_tick_submitted();

        let after_snapshot = controller.snapshot();
        let after = after_snapshot.next_tick_at.expect("rescheduled next tick");
        controller.deactivate("test_cleanup");

        assert_eq!(
            after_snapshot.status,
            allthecodes_services::proactive::ProactiveStatus::Active
        );
        assert_eq!(after_snapshot.source.as_deref(), Some("test"));
        assert!(after > before, "next_tick_at should advance after submit");
    }

    #[test]
    fn ui_status_prefers_sleep_over_context_blocked() {
        let snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::ContextBlocked,
            source: Some("test".into()),
            next_tick_at: None,
            paused_reason: Some("context_limit".into()),
            context_blocked: true,
        };
        let sleep = allthecodes_services::proactive::SleepState {
            schema_version: allthecodes_services::proactive::SLEEP_STATE_SCHEMA_VERSION,
            sleeping_until: Utc::now() + Duration::seconds(60),
            reason: Some("waiting".into()),
            updated_at: Utc::now(),
        };

        let status = ui_status_from_snapshot(&snapshot, Some(&sleep)).expect("ui status");

        assert_eq!(status.label, "proactive sleeping");
        assert_eq!(status.next_tick_text.as_deref(), Some("wake in 1m"));
    }
}
