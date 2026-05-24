//! `/schedule` — raw management surface for the local cron scheduler
//! (issue #60).
//!
//! ## Scope of the first milestone
//!
//! `/schedule` is explicitly split in two capability lines:
//!
//! - **local cron** — persisted under `{data_root}/scheduled_tasks.json`
//!   and served by [`allthecodes_services::scheduler::SchedulerStore`]. All
//!   subcommands below operate on this store.
//! - **remote triggers** — delegated to a cloud-side agent runtime in the
//!   Bun reference. In allthecodes the remote path requires OAuth/API
//!   groundwork we haven't landed yet, so `/schedule remote …` currently
//!   refuses and points the user at the design doc.
//!
//! Keeping the two lines syntactically distinct means the first milestone
//! ships without implying the second works. When the remote path lands,
//! it'll extend the `remote` subcommand without changing the local
//! surface.
//!
//! ## Subcommands
//!
//!   /schedule                    alias for 'list'
//!   /schedule list               list all scheduled tasks
//!   /schedule add <interval> <payload>   add a new local task (no immediate run)
//!   /schedule show <id>          inspect one task
//!   /schedule remove <id>        delete a task
//!   /schedule pause <id>         suspend a task without deleting
//!   /schedule resume <id>        re-enable a paused task
//!   /schedule trigger <id>       mark task as fired and roll next_run_at forward
//!   /schedule remote …           (disabled) surface for remote triggers

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_services::scheduler::{
    parse_interval, Interval, ScheduledTask, SchedulerError, SchedulerKind, SchedulerStore, TaskId,
    TaskPayload,
};

pub struct ScheduleHandler;

#[async_trait]
impl CommandHandler for ScheduleHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let store = SchedulerStore::open_default();
        Ok(CommandResult::Output(dispatch(&store, args)))
    }
}

fn dispatch(store: &SchedulerStore, args: &str) -> String {
    let trimmed = args.trim();
    let (head, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((h, r)) => (h.to_lowercase(), r.trim()),
        None => (trimmed.to_lowercase(), ""),
    };

    match head.as_str() {
        "" | "list" | "ls" => list(store),
        "help" | "--help" | "-h" => help_text(store),
        "add" | "create" | "new" => add(store, rest),
        "show" | "info" => show(store, rest),
        "remove" | "rm" | "delete" => remove(store, rest),
        "pause" => set_paused(store, rest, true),
        "resume" | "unpause" => set_paused(store, rest, false),
        "trigger" | "fire" => trigger(store, rest),
        "due" => due(store),
        "remote" => remote_hint(rest),
        other => format!(
            "Unknown /schedule subcommand '{}'. Run '/schedule help' for the list.",
            other
        ),
    }
}

fn help_text(store: &SchedulerStore) -> String {
    let storage = store.path().display().to_string();
    [
        "/schedule — local cron scheduler.".to_string(),
        String::new(),
        "Scope: this command manages LOCAL cron tasks. Remote triggers".to_string(),
        "  (cloud-side scheduled agents) are tracked as a separate".to_string(),
        "  capability line and are not yet implemented — see".to_string(),
        "  `/schedule remote`.".to_string(),
        format!("  Storage: {}", storage),
        String::new(),
        "  /schedule                    list all scheduled tasks".to_string(),
        "  /schedule add <interval> <payload>   add a new local task".to_string(),
        "  /schedule show <id>          inspect one task".to_string(),
        "  /schedule remove <id>        delete a task".to_string(),
        "  /schedule pause <id>         suspend a task".to_string(),
        "  /schedule resume <id>        re-enable a paused task".to_string(),
        "  /schedule trigger <id>       mark task as fired now".to_string(),
        "  /schedule due                show tasks that are due right now".to_string(),
        "  /schedule remote …           (disabled) remote-trigger surface".to_string(),
        String::new(),
        "Interval examples: 30s, 5m, 1h, 2d, '*/10 * * * *'.".to_string(),
        "Payload is a slash command (/simplify) or a plain prompt.".to_string(),
        String::new(),
        "Tip: /loop is a higher-level wrapper that also runs the payload".to_string(),
        "once immediately.".to_string(),
    ]
    .join("\n")
}

fn due(store: &SchedulerStore) -> String {
    match store.due_tasks() {
        Ok(tasks) => {
            if tasks.is_empty() {
                return "No scheduled tasks are due right now.".into();
            }
            let mut out = format!("{} task(s) due now\n", tasks.len());
            out.push_str(&"─".repeat(24));
            out.push('\n');
            for t in &tasks {
                out.push_str(&render_row(t));
            }
            out.push_str(
                "\nTip: the daemon tick loop (if running) will execute these \
                 on its next poll; use `/schedule trigger <id>` to mark as fired.\n",
            );
            out
        }
        Err(e) => format!("Could not read scheduler state: {}", e),
    }
}

fn list(store: &SchedulerStore) -> String {
    match store.load() {
        Ok(tasks) => {
            if tasks.is_empty() {
                return "No scheduled tasks registered. Try '/schedule add <interval> <payload>'."
                    .into();
            }
            let mut out = String::new();
            out.push_str(&format!("Local scheduled tasks ({})\n", tasks.len()));
            out.push_str(&"─".repeat(26));
            out.push('\n');
            for t in &tasks {
                out.push_str(&render_row(t));
            }
            out.push_str("\nRemote triggers: (not implemented — see `/schedule remote`).\n");
            out
        }
        Err(e) => format!("Could not read scheduler state: {}", e),
    }
}

fn render_row(t: &ScheduledTask) -> String {
    let status = if t.paused { "paused" } else { "active" };
    format!(
        "  {id}  every {interval}  [{kind} · {status}]\n    \
         payload ({ptype}): {payload}\n    next run: {next}\n",
        id = t.id,
        interval = Interval::from_seconds(t.interval_seconds).human(),
        kind = t.kind.as_str(),
        status = status,
        ptype = t.payload.kind_label(),
        payload = t.payload.display(),
        next = t.next_run_at.to_rfc3339(),
    )
}

fn add(store: &SchedulerStore, rest: &str) -> String {
    let Some((interval_raw, payload_raw)) = rest.split_once(char::is_whitespace) else {
        return "Usage: /schedule add <interval> <payload>".into();
    };
    let payload_raw = payload_raw.trim();
    if payload_raw.is_empty() {
        return "Usage: /schedule add <interval> <payload>".into();
    }

    let interval = match parse_interval(interval_raw) {
        Ok(i) => i,
        Err(e) => return format!("Could not parse interval '{}': {}", interval_raw, e),
    };

    let payload = TaskPayload::from_user_input(payload_raw);
    let task = ScheduledTask::new(
        SchedulerKind::LocalCron,
        derive_name(&payload),
        interval_raw,
        interval,
        payload,
        Utc::now(),
    );

    match store.add(task) {
        Ok(saved) => format!(
            "Added scheduled task '{}' (id={}) — runs every {} starting at {}.",
            saved.name,
            saved.id,
            interval.human(),
            saved.next_run_at.to_rfc3339()
        ),
        Err(e) => format!("Could not add task: {}", e),
    }
}

fn show(store: &SchedulerStore, id_raw: &str) -> String {
    if id_raw.is_empty() {
        return "Usage: /schedule show <id>".into();
    }
    let id = TaskId(id_raw.to_string());
    match store.get(&id) {
        Ok(t) => {
            let mut out = format!("Scheduled task {}\n", t.id);
            out.push_str(&"─".repeat(16 + t.id.as_str().len()));
            out.push('\n');
            out.push_str(&format!("  Name:          {}\n", t.name));
            out.push_str(&format!("  Kind:          {}\n", t.kind.as_str()));
            out.push_str(&format!(
                "  Status:        {}\n",
                if t.paused { "paused" } else { "active" }
            ));
            out.push_str(&format!(
                "  Interval:      {} (raw: {})\n",
                Interval::from_seconds(t.interval_seconds).human(),
                t.schedule
            ));
            out.push_str(&format!("  Payload kind:  {}\n", t.payload.kind_label()));
            out.push_str(&format!("  Payload:       {}\n", t.payload.display()));
            out.push_str(&format!("  Created at:    {}\n", t.created_at.to_rfc3339()));
            out.push_str(&format!(
                "  Last run at:   {}\n",
                t.last_run_at
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_else(|| "never".to_string())
            ));
            out.push_str(&format!(
                "  Next run at:   {}\n",
                t.next_run_at.to_rfc3339()
            ));
            out
        }
        Err(SchedulerError::NotFound(_)) => {
            format!("No scheduled task with id '{}'.", id_raw)
        }
        Err(e) => format!("Could not read task: {}", e),
    }
}

fn remove(store: &SchedulerStore, id_raw: &str) -> String {
    if id_raw.is_empty() {
        return "Usage: /schedule remove <id>".into();
    }
    let id = TaskId(id_raw.to_string());
    match store.remove(&id) {
        Ok(removed) => format!(
            "Removed scheduled task '{}' (id={}).",
            removed.name, removed.id
        ),
        Err(SchedulerError::NotFound(_)) => {
            format!("No scheduled task with id '{}'.", id_raw)
        }
        Err(e) => format!("Could not remove task: {}", e),
    }
}

fn set_paused(store: &SchedulerStore, id_raw: &str, paused: bool) -> String {
    if id_raw.is_empty() {
        return if paused {
            "Usage: /schedule pause <id>".into()
        } else {
            "Usage: /schedule resume <id>".into()
        };
    }
    let id = TaskId(id_raw.to_string());
    match store.set_paused(&id, paused) {
        Ok(task) => {
            let verb = if paused { "Paused" } else { "Resumed" };
            format!("{} scheduled task '{}' (id={}).", verb, task.name, task.id)
        }
        Err(SchedulerError::NotFound(_)) => {
            format!("No scheduled task with id '{}'.", id_raw)
        }
        Err(e) => format!("Could not update task: {}", e),
    }
}

fn trigger(store: &SchedulerStore, id_raw: &str) -> String {
    if id_raw.is_empty() {
        return "Usage: /schedule trigger <id>".into();
    }
    let id = TaskId(id_raw.to_string());
    match store.record_fired(&id) {
        Ok(task) => format!(
            "Marked task '{}' (id={}) as fired. Next run at {}.",
            task.name, task.id, task.next_run_at
        ),
        Err(SchedulerError::NotFound(_)) => {
            format!("No scheduled task with id '{}'.", id_raw)
        }
        Err(e) => format!("Could not trigger task: {}", e),
    }
}

fn remote_hint(_rest: &str) -> String {
    [
        "Remote triggers are not implemented yet in allthecodes (issue #60).",
        "",
        "The first /schedule milestone covers LOCAL cron only — tasks persist",
        "to {data_root}/scheduled_tasks.json and are run by the current",
        "process. The remote-trigger capability requires cloud OAuth and",
        "agent APIs that haven't been ported from the Bun reference yet.",
        "",
        "Use '/schedule list' to see local tasks.",
    ]
    .join("\n")
}

fn derive_name(payload: &TaskPayload) -> String {
    let raw = payload.display();
    let first_line = raw.lines().next().unwrap_or(raw).trim();
    let short: String = first_line.chars().take(40).collect();
    if short.is_empty() {
        "task".into()
    } else {
        short
    }
}

#[cfg(any())]
pub(crate) mod scheduler {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    const SCHEMA_VERSION: u32 = 1;
    const ONE_YEAR_SECS: u64 = 365 * 86_400;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Interval {
        seconds: u64,
    }

    impl Interval {
        pub(crate) fn from_seconds(seconds: u64) -> Self {
            Self { seconds }
        }

        pub(crate) fn seconds(self) -> u64 {
            self.seconds
        }

        pub(crate) fn human(self) -> String {
            let s = self.seconds;
            if s % 86_400 == 0 && s >= 86_400 {
                return format!("{}d", s / 86_400);
            }
            if s % 3_600 == 0 && s >= 3_600 {
                return format!("{}h", s / 3_600);
            }
            if s % 60 == 0 && s >= 60 {
                return format!("{}m", s / 60);
            }
            format!("{}s", s)
        }
    }

    impl std::fmt::Display for Interval {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.human())
        }
    }

    #[derive(Debug)]
    pub(crate) enum IntervalParseError {
        Empty,
        Malformed(String),
        NonPositive(String),
        TooLarge(String),
        CronUnsupported(String),
    }

    impl std::fmt::Display for IntervalParseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                IntervalParseError::Empty => f.write_str("interval must not be empty"),
                IntervalParseError::Malformed(raw) => write!(
                    f,
                    "interval '{raw}' is not a recognized form (try 5m, 1h, 30s, 2d, or '*/5 * * * *')"
                ),
                IntervalParseError::NonPositive(raw) => {
                    write!(f, "interval '{raw}' must be positive")
                }
                IntervalParseError::TooLarge(raw) => {
                    write!(f, "interval '{raw}' is larger than the 1-year cap")
                }
                IntervalParseError::CronUnsupported(raw) => write!(
                    f,
                    "cron expression '{raw}' is only partially supported - only the minute stride ('*/N * * * *' or 'N * * * *') is honored right now"
                ),
            }
        }
    }

    impl std::error::Error for IntervalParseError {}

    pub(crate) fn parse_interval(raw: &str) -> Result<Interval, IntervalParseError> {
        let input = raw.trim();
        if input.is_empty() {
            return Err(IntervalParseError::Empty);
        }
        if input.contains(' ') || input.contains('/') {
            return parse_cron_like(input);
        }
        parse_duration(input)
    }

    fn parse_duration(input: &str) -> Result<Interval, IntervalParseError> {
        let (num_part, unit_part) = split_numeric_suffix(input);
        if num_part.is_empty() {
            return Err(IntervalParseError::Malformed(input.to_string()));
        }
        let value: u64 = num_part
            .parse()
            .map_err(|_| IntervalParseError::Malformed(input.to_string()))?;
        if value == 0 {
            return Err(IntervalParseError::NonPositive(input.to_string()));
        }

        let seconds = match unit_part {
            "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
            "m" | "min" | "mins" | "minute" | "minutes" => value
                .checked_mul(60)
                .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
            "h" | "hr" | "hrs" | "hour" | "hours" => value
                .checked_mul(3_600)
                .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
            "d" | "day" | "days" => value
                .checked_mul(86_400)
                .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
            _ => return Err(IntervalParseError::Malformed(input.to_string())),
        };
        if seconds > ONE_YEAR_SECS {
            return Err(IntervalParseError::TooLarge(input.to_string()));
        }
        Ok(Interval::from_seconds(seconds))
    }

    fn parse_cron_like(input: &str) -> Result<Interval, IntervalParseError> {
        let fields: Vec<&str> = input.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(IntervalParseError::Malformed(input.to_string()));
        }
        let (minute_field, rest) = (fields[0], &fields[1..]);
        if !rest.iter().all(|f| *f == "*") {
            return Err(IntervalParseError::CronUnsupported(input.to_string()));
        }

        let minutes: u64 = if let Some(stride) = minute_field.strip_prefix("*/") {
            stride
                .parse()
                .map_err(|_| IntervalParseError::Malformed(input.to_string()))?
        } else if minute_field == "*" {
            1
        } else {
            minute_field
                .parse()
                .map_err(|_| IntervalParseError::CronUnsupported(input.to_string()))?
        };
        if minutes == 0 {
            return Err(IntervalParseError::NonPositive(input.to_string()));
        }
        let seconds = minutes
            .checked_mul(60)
            .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?;
        if seconds > ONE_YEAR_SECS {
            return Err(IntervalParseError::TooLarge(input.to_string()));
        }
        Ok(Interval::from_seconds(seconds))
    }

    fn split_numeric_suffix(input: &str) -> (&str, &str) {
        let split = input
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        input.split_at(split)
    }

    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub(crate) struct TaskId(pub(crate) String);

    impl TaskId {
        pub(crate) fn new() -> Self {
            let raw = allthecodes_bootstrap::SessionId::new().to_string();
            let short: String = raw
                .chars()
                .filter(|c| c.is_ascii_hexdigit())
                .take(12)
                .collect();
            Self(short)
        }

        pub(crate) fn as_str(&self) -> &str {
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum SchedulerKind {
        LocalCron,
        RemoteTrigger,
    }

    impl SchedulerKind {
        pub(crate) fn as_str(self) -> &'static str {
            match self {
                SchedulerKind::LocalCron => "local",
                SchedulerKind::RemoteTrigger => "remote",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
    pub(crate) enum TaskPayload {
        SlashCommand(String),
        Prompt(String),
    }

    impl TaskPayload {
        pub(crate) fn from_user_input(input: &str) -> Self {
            let trimmed = input.trim();
            if trimmed.starts_with('/') {
                TaskPayload::SlashCommand(trimmed.to_string())
            } else {
                TaskPayload::Prompt(trimmed.to_string())
            }
        }

        pub(crate) fn display(&self) -> &str {
            match self {
                TaskPayload::SlashCommand(s) | TaskPayload::Prompt(s) => s,
            }
        }

        pub(crate) fn kind_label(&self) -> &'static str {
            match self {
                TaskPayload::SlashCommand(_) => "command",
                TaskPayload::Prompt(_) => "prompt",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub(crate) struct ScheduledTask {
        pub(crate) id: TaskId,
        pub(crate) kind: SchedulerKind,
        pub(crate) name: String,
        pub(crate) schedule: String,
        pub(crate) interval_seconds: u64,
        pub(crate) payload: TaskPayload,
        pub(crate) created_at: DateTime<Utc>,
        pub(crate) last_run_at: Option<DateTime<Utc>>,
        pub(crate) next_run_at: DateTime<Utc>,
        #[serde(default)]
        pub(crate) paused: bool,
    }

    impl ScheduledTask {
        pub(crate) fn new(
            kind: SchedulerKind,
            name: impl Into<String>,
            schedule: impl Into<String>,
            interval: Interval,
            payload: TaskPayload,
            now: DateTime<Utc>,
        ) -> Self {
            let interval_seconds = interval.seconds();
            Self {
                id: TaskId::new(),
                kind,
                name: name.into(),
                schedule: schedule.into(),
                interval_seconds,
                payload,
                created_at: now,
                last_run_at: None,
                next_run_at: now + chrono::Duration::seconds(interval_seconds as i64),
                paused: false,
            }
        }

        pub(crate) fn mark_fired(&mut self, now: DateTime<Utc>) {
            self.last_run_at = Some(now);
            self.next_run_at = now + chrono::Duration::seconds(self.interval_seconds as i64);
        }

        pub(crate) fn is_due(&self, now: DateTime<Utc>) -> bool {
            !self.paused && self.next_run_at <= now
        }
    }

    #[derive(Debug)]
    pub(crate) enum SchedulerError {
        Io {
            path: PathBuf,
            source: io::Error,
        },
        Decode {
            path: PathBuf,
            source: serde_json::Error,
        },
        Encode(serde_json::Error),
        NotFound(String),
        LockTimeout {
            path: PathBuf,
            timeout_ms: Duration,
        },
        RemoteTriggerUnsupported,
    }

    impl std::fmt::Display for SchedulerError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SchedulerError::Io { path, source } => {
                    write!(f, "I/O error touching {}: {}", path.display(), source)
                }
                SchedulerError::Decode { path, source } => {
                    write!(f, "failed to decode {}: {}", path.display(), source)
                }
                SchedulerError::Encode(source) => {
                    write!(f, "failed to encode scheduler state: {source}")
                }
                SchedulerError::NotFound(id) => write!(f, "task '{id}' not found"),
                SchedulerError::LockTimeout { path, timeout_ms } => write!(
                    f,
                    "could not acquire scheduler lock at {} within {}ms",
                    path.display(),
                    timeout_ms.as_millis()
                ),
                SchedulerError::RemoteTriggerUnsupported => {
                    f.write_str("remote-trigger tasks are not supported yet - see issue #60")
                }
            }
        }
    }

    impl std::error::Error for SchedulerError {}

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct StateFile {
        version: u32,
        #[serde(default)]
        tasks: Vec<ScheduledTask>,
    }

    impl Default for StateFile {
        fn default() -> Self {
            Self {
                version: SCHEMA_VERSION,
                tasks: Vec::new(),
            }
        }
    }

    pub(crate) struct SchedulerStore {
        state_path: PathBuf,
        lock_path: PathBuf,
    }

    impl SchedulerStore {
        pub(crate) fn default_path() -> PathBuf {
            allthecodes_config::paths::data_root().join("scheduled_tasks.json")
        }

        pub(crate) fn new(state_path: impl Into<PathBuf>) -> Self {
            let state_path = state_path.into();
            let lock_path = state_path.with_extension("json.lock");
            Self {
                state_path,
                lock_path,
            }
        }

        pub(crate) fn open_default() -> Self {
            Self::new(Self::default_path())
        }

        pub(crate) fn path(&self) -> &Path {
            &self.state_path
        }

        pub(crate) fn load(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
            let _guard = self.acquire_lock()?;
            self.read_state().map(|s| s.tasks)
        }

        pub(crate) fn add(&self, task: ScheduledTask) -> Result<ScheduledTask, SchedulerError> {
            if matches!(task.kind, SchedulerKind::RemoteTrigger) {
                return Err(SchedulerError::RemoteTriggerUnsupported);
            }
            let _guard = self.acquire_lock()?;
            let mut state = self.read_state()?;
            state.tasks.push(task.clone());
            self.write_state(&state)?;
            Ok(task)
        }

        pub(crate) fn remove(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
            let _guard = self.acquire_lock()?;
            let mut state = self.read_state()?;
            let pos = state
                .tasks
                .iter()
                .position(|t| t.id == *id)
                .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
            let removed = state.tasks.remove(pos);
            self.write_state(&state)?;
            Ok(removed)
        }

        pub(crate) fn get(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
            let tasks = self.load()?;
            tasks
                .into_iter()
                .find(|t| t.id == *id)
                .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
        }

        pub(crate) fn set_paused(
            &self,
            id: &TaskId,
            paused: bool,
        ) -> Result<ScheduledTask, SchedulerError> {
            let _guard = self.acquire_lock()?;
            let mut state = self.read_state()?;
            let task = state
                .tasks
                .iter_mut()
                .find(|t| t.id == *id)
                .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
            task.paused = paused;
            let snapshot = task.clone();
            self.write_state(&state)?;
            Ok(snapshot)
        }

        pub(crate) fn record_fired(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
            let _guard = self.acquire_lock()?;
            let mut state = self.read_state()?;
            let task = state
                .tasks
                .iter_mut()
                .find(|t| t.id == *id)
                .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
            task.mark_fired(Utc::now());
            let snapshot = task.clone();
            self.write_state(&state)?;
            Ok(snapshot)
        }

        pub(crate) fn due_tasks(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
            let now = Utc::now();
            Ok(self.load()?.into_iter().filter(|t| t.is_due(now)).collect())
        }

        fn read_state(&self) -> Result<StateFile, SchedulerError> {
            if !self.state_path.exists() {
                return Ok(StateFile::default());
            }
            let mut file = File::open(&self.state_path).map_err(|e| SchedulerError::Io {
                path: self.state_path.clone(),
                source: e,
            })?;
            let mut buf = String::new();
            file.read_to_string(&mut buf)
                .map_err(|e| SchedulerError::Io {
                    path: self.state_path.clone(),
                    source: e,
                })?;
            if buf.trim().is_empty() {
                return Ok(StateFile::default());
            }
            serde_json::from_str(&buf).map_err(|e| SchedulerError::Decode {
                path: self.state_path.clone(),
                source: e,
            })
        }

        fn write_state(&self, state: &StateFile) -> Result<(), SchedulerError> {
            if let Some(parent) = self.state_path.parent() {
                fs::create_dir_all(parent).map_err(|e| SchedulerError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
            let bytes = serde_json::to_vec_pretty(state).map_err(SchedulerError::Encode)?;
            let tmp_path = self.state_path.with_extension("json.tmp");
            {
                let mut tmp = File::create(&tmp_path).map_err(|e| SchedulerError::Io {
                    path: tmp_path.clone(),
                    source: e,
                })?;
                tmp.write_all(&bytes).map_err(|e| SchedulerError::Io {
                    path: tmp_path.clone(),
                    source: e,
                })?;
                tmp.flush().map_err(|e| SchedulerError::Io {
                    path: tmp_path.clone(),
                    source: e,
                })?;
            }
            fs::rename(&tmp_path, &self.state_path).map_err(|e| SchedulerError::Io {
                path: self.state_path.clone(),
                source: e,
            })?;
            Ok(())
        }

        fn acquire_lock(&self) -> Result<FileLockGuard, SchedulerError> {
            let timeout = Duration::from_millis(2_000);
            let started = Instant::now();
            loop {
                if let Some(parent) = self.lock_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| SchedulerError::Io {
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
                }
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.lock_path)
                {
                    Ok(_) => {
                        return Ok(FileLockGuard {
                            path: self.lock_path.clone(),
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        if started.elapsed() >= timeout {
                            return Err(SchedulerError::LockTimeout {
                                path: self.lock_path.clone(),
                                timeout_ms: timeout,
                            });
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(e) => {
                        return Err(SchedulerError::Io {
                            path: self.lock_path.clone(),
                            source: e,
                        });
                    }
                }
            }
        }
    }

    struct FileLockGuard {
        path: PathBuf,
    }

    impl Drop for FileLockGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fresh_store() -> (tempfile::TempDir, SchedulerStore) {
        let dir = tempdir().unwrap();
        let store = SchedulerStore::new(dir.path().join("scheduled_tasks.json"));
        (dir, store)
    }

    #[test]
    fn empty_args_lists_tasks() {
        let (_dir, store) = fresh_store();
        let out = dispatch(&store, "");
        assert!(out.contains("No scheduled tasks"));
    }

    #[test]
    fn help_documents_scope() {
        let (_dir, store) = fresh_store();
        let help = help_text(&store);
        assert!(help.contains("local cron"));
        assert!(help.contains("remote triggers") || help.contains("Remote triggers"));
        assert!(help.contains("not yet implemented") || help.contains("disabled"));
        assert!(
            help.contains(store.path().display().to_string().as_str()),
            "help output should include the storage path"
        );
    }

    #[test]
    fn due_lists_only_tasks_past_next_run() {
        let (_dir, store) = fresh_store();
        // Task scheduled to run in the future — should NOT be due.
        dispatch(&store, "add 1h future thing");
        assert!(dispatch(&store, "due").contains("No scheduled tasks are due"));

        // Mutate the stored task so its next_run_at is in the past.
        let mut tasks = store.load().unwrap();
        tasks[0].next_run_at = Utc::now() - chrono::Duration::seconds(30);
        store.remove(&tasks[0].id).unwrap();
        store
            .add(ScheduledTask {
                id: tasks[0].id.clone(),
                ..tasks.remove(0)
            })
            .unwrap();

        let out = dispatch(&store, "due");
        assert!(
            out.contains("task(s) due now"),
            "unexpected due output: {}",
            out
        );
        assert!(out.contains("future thing"));
    }

    #[test]
    fn add_then_list_shows_task() {
        let (_dir, store) = fresh_store();
        let out = dispatch(&store, "add 5m run the deploy check");
        assert!(out.contains("Added scheduled task"));
        let list_out = dispatch(&store, "list");
        assert!(list_out.contains("run the deploy check"));
        assert!(list_out.contains("every 5m"));
    }

    #[test]
    fn show_inspects_task() {
        let (_dir, store) = fresh_store();
        dispatch(&store, "add 1h do the thing");
        let id = store.load().unwrap()[0].id.clone();
        let out = dispatch(&store, &format!("show {}", id));
        assert!(out.contains("Payload:"));
        assert!(out.contains("do the thing"));
        assert!(out.contains("Interval:"));
        assert!(out.contains("Next run at:"));
    }

    #[test]
    fn remove_and_pause_and_resume() {
        let (_dir, store) = fresh_store();
        dispatch(&store, "add 30s keep watching");
        let id = store.load().unwrap()[0].id.clone();

        assert!(dispatch(&store, &format!("pause {}", id)).contains("Paused"));
        assert!(store.load().unwrap()[0].paused);
        assert!(dispatch(&store, &format!("resume {}", id)).contains("Resumed"));
        assert!(!store.load().unwrap()[0].paused);
        assert!(dispatch(&store, &format!("remove {}", id)).contains("Removed"));
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn trigger_advances_next_run() {
        let (_dir, store) = fresh_store();
        dispatch(&store, "add 5m keep going");
        let id = store.load().unwrap()[0].id.clone();
        let before = store.get(&id).unwrap().next_run_at;
        let out = dispatch(&store, &format!("trigger {}", id));
        assert!(out.contains("Marked task"));
        let after = store.get(&id).unwrap().next_run_at;
        assert!(after >= before);
    }

    #[test]
    fn remote_subcommand_refuses_with_context() {
        let (_dir, store) = fresh_store();
        let out = dispatch(&store, "remote add 1h foo");
        assert!(out.contains("not implemented"));
        // We want the refusal to explain what IS implemented — case
        // insensitively, since the user copy uses "LOCAL cron" for emphasis.
        assert!(out.to_lowercase().contains("local cron"));
    }

    #[test]
    fn unknown_subcommand_reports_error() {
        let (_dir, store) = fresh_store();
        let out = dispatch(&store, "frobnicate");
        assert!(out.contains("Unknown /schedule subcommand"));
    }

    #[test]
    fn add_without_payload_shows_usage() {
        let (_dir, store) = fresh_store();
        assert!(dispatch(&store, "add 5m").contains("Usage"));
        assert!(dispatch(&store, "add").contains("Usage"));
    }
}
