//! Scheduled task types — the on-disk format for `scheduled_tasks.json`.
//!
//! The task struct is the primary public vocabulary; `/loop` and `/schedule`
//! both operate over `ScheduledTask` instances and the [`SchedulerStore`] is
//! only thin wrapping around a `Vec<ScheduledTask>`.
//!
//! [`SchedulerStore`]: super::store::SchedulerStore

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a scheduled task. Wraps a short UUID-derived string
/// so the CLI can round-trip IDs without quoting hassles.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub String);

impl TaskId {
    /// Generate a fresh ID. Uses the first 12 hex chars of a UUID v4 so IDs
    /// stay short enough to copy-paste but keep collision probability low
    /// enough for per-user persistence.
    pub fn new() -> Self {
        let uuid = Uuid::new_v4().simple().to_string();
        Self(uuid.chars().take(12).collect())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which capability line a task belongs to. Kept explicit so `/schedule`
/// (issue #60) can, in the future, surface a second section for remote
/// triggers without mixing the semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerKind {
    /// Local cron-style task run by the host process / daemon.
    LocalCron,
    /// Placeholder for a future remote-trigger capability. Creating a task
    /// with this kind today is rejected with a clear message; keeping the
    /// enum variant now means on-disk state survives when the feature lands.
    RemoteTrigger,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    #[default]
    Interval,
    Cron,
}

impl SchedulerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SchedulerKind::LocalCron => "local",
            SchedulerKind::RemoteTrigger => "remote",
        }
    }
}

/// What the scheduler should submit when a task fires.
///
/// `/loop` supports both a slash-command payload (`/foo bar`) and a plain
/// prompt; we distinguish them so the host can route command payloads
/// through its command dispatcher instead of always going to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TaskPayload {
    /// Slash command, including the leading `/` (e.g. `/simplify`).
    SlashCommand(String),
    /// Plain user prompt forwarded to the model.
    Prompt(String),
}

impl TaskPayload {
    /// Build a payload from raw user input; strings starting with `/` become
    /// slash-command payloads, everything else is a plain prompt.
    pub fn from_user_input(input: &str) -> Self {
        let trimmed = input.trim();
        if trimmed.starts_with('/') {
            TaskPayload::SlashCommand(trimmed.to_string())
        } else {
            TaskPayload::Prompt(trimmed.to_string())
        }
    }

    pub fn display(&self) -> &str {
        match self {
            TaskPayload::SlashCommand(s) => s,
            TaskPayload::Prompt(s) => s,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            TaskPayload::SlashCommand(_) => "command",
            TaskPayload::Prompt(_) => "prompt",
        }
    }
}

/// One persisted scheduled task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: TaskId,
    pub kind: SchedulerKind,
    /// Human-friendly label used in `/schedule list` output.
    pub name: String,
    /// Cron-like expression (e.g. `*/5 * * * *`) or interval spec
    /// (e.g. `5m`) — stored verbatim and re-parsed at tick time.
    pub schedule: String,
    /// Whether `schedule` is an interval shorthand or a real 5-field cron
    /// expression. Defaults to interval for backward compatibility with older
    /// `scheduled_tasks.json` files.
    #[serde(default)]
    pub schedule_kind: ScheduleKind,
    /// IANA timezone name reserved for future local-time cron evaluation.
    /// Phase 2 evaluates cron in UTC and records the field for compatibility.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Interval in seconds, derived from `schedule` at creation time.
    /// Stored so polling doesn't have to re-parse on every tick.
    pub interval_seconds: u64,
    pub payload: TaskPayload,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: DateTime<Utc>,
    /// If true, the task is temporarily suspended (skipped by `due_tasks`)
    /// without being deleted.
    #[serde(default)]
    pub paused: bool,
}

impl ScheduledTask {
    /// Construct a new task with its next-run pre-computed from `now`.
    pub fn new(
        kind: SchedulerKind,
        name: impl Into<String>,
        schedule: impl Into<String>,
        interval: super::Interval,
        payload: TaskPayload,
        now: DateTime<Utc>,
    ) -> Self {
        let interval_seconds = interval.seconds();
        Self {
            id: TaskId::new(),
            kind,
            name: name.into(),
            schedule: schedule.into(),
            schedule_kind: ScheduleKind::Interval,
            timezone: None,
            interval_seconds,
            payload,
            created_at: now,
            last_run_at: None,
            next_run_at: now + chrono::Duration::seconds(interval_seconds as i64),
            paused: false,
        }
    }

    /// Mark the task as having just fired and roll `next_run_at` forward
    /// by one interval.
    pub fn mark_fired(&mut self, now: DateTime<Utc>) {
        self.last_run_at = Some(now);
        self.next_run_at = self.compute_next_run(now);
    }

    /// Is this task due to fire relative to `now`?
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        !self.paused && self.next_run_at <= now
    }

    pub fn with_cron_schedule(
        mut self,
        schedule: impl Into<String>,
        next_run_at: DateTime<Utc>,
        timezone: Option<String>,
    ) -> Self {
        self.schedule = schedule.into();
        self.schedule_kind = ScheduleKind::Cron;
        self.timezone = timezone;
        self.interval_seconds = 60;
        self.next_run_at = next_run_at;
        self
    }

    pub fn compute_next_run(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        if self.schedule_kind == ScheduleKind::Cron {
            if let Ok(cron) = super::parse_cron(&self.schedule) {
                if let Some(next) = cron.next_after(now) {
                    return next;
                }
            }
        }
        now + chrono::Duration::seconds(self.interval_seconds as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::Interval;

    #[test]
    fn task_id_is_short_and_unique() {
        let a = TaskId::new();
        let b = TaskId::new();
        assert_eq!(a.as_str().len(), 12);
        assert_ne!(a, b);
    }

    #[test]
    fn payload_from_user_input_detects_slash() {
        assert!(matches!(
            TaskPayload::from_user_input("/simplify"),
            TaskPayload::SlashCommand(_)
        ));
        assert!(matches!(
            TaskPayload::from_user_input("run the tests"),
            TaskPayload::Prompt(_)
        ));
    }

    #[test]
    fn mark_fired_advances_next_run() {
        let now = Utc::now();
        let mut task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "t",
            "5m",
            Interval::from_seconds(300),
            TaskPayload::Prompt("hi".into()),
            now,
        );
        assert_eq!(task.last_run_at, None);

        let fired_at = now + chrono::Duration::seconds(600);
        task.mark_fired(fired_at);

        assert_eq!(task.last_run_at, Some(fired_at));
        assert_eq!(task.next_run_at, fired_at + chrono::Duration::seconds(300));
    }

    #[test]
    fn paused_tasks_are_never_due() {
        let now = Utc::now();
        let mut task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "t",
            "1s",
            Interval::from_seconds(1),
            TaskPayload::Prompt("p".into()),
            now - chrono::Duration::seconds(1000),
        );
        assert!(task.is_due(now));
        task.paused = true;
        assert!(!task.is_due(now));
    }

    #[test]
    fn is_due_respects_next_run_threshold() {
        let now = Utc::now();
        let task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            "t",
            "5m",
            Interval::from_seconds(300),
            TaskPayload::Prompt("p".into()),
            now,
        );
        assert!(!task.is_due(now));
        assert!(task.is_due(now + chrono::Duration::seconds(301)));
    }
}
