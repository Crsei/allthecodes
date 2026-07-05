use chrono::{DateTime, Local, Utc};

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
        decide_tick(
            &snapshot,
            streaming,
            pending_permission,
            pending_question,
            now,
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn tick_decision_requires_active_idle_and_due() {
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
}
