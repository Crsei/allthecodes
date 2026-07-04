use allthecodes_tasks::{ScheduleSpec, ScheduledAgentTask};

use crate::{BusyPolicy, RemoteSource, RemoteTransport, RunPolicy, RunRequest};

pub fn scheduled_task_run_request(task: &ScheduledAgentTask) -> RunRequest {
    RunRequest {
        prompt: task.prompt.clone(),
        source: scheduled_task_source(task),
        policy: RunPolicy {
            busy: BusyPolicy::Queue,
            permission_mode: "ask".to_string(),
            delivery: vec!["local".to_string()],
        },
        idempotency_key: Some(scheduled_task_idempotency_key(task)),
    }
}

pub fn scheduled_task_source(task: &ScheduledAgentTask) -> RemoteSource {
    let mut source = RemoteSource::new(
        RemoteTransport::Scheduled,
        "local",
        task.cwd.clone(),
        "scheduled_agent",
        task.id.clone(),
        task.id.clone(),
    )
    .with_metadata("scheduled_task_id", task.id.clone())
    .with_metadata("schedule_kind", schedule_kind(&task.schedule));

    if let Some(next_run_at) = &task.next_run_at {
        source = source.with_metadata("next_run_at", next_run_at.clone());
    }
    if let Some(last_run_at) = &task.last_run_at {
        source = source.with_metadata("last_run_at", last_run_at.clone());
    }
    source
}

fn scheduled_task_idempotency_key(task: &ScheduledAgentTask) -> String {
    match task.next_run_at.as_deref() {
        Some(next_run_at) => format!("scheduled:{}:{next_run_at}", task.id),
        None => format!("scheduled:{}:manual", task.id),
    }
}

fn schedule_kind(schedule: &ScheduleSpec) -> &'static str {
    match schedule {
        ScheduleSpec::Interval { .. } => "interval",
        ScheduleSpec::Once { .. } => "once",
    }
}
