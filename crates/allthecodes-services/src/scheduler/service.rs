//! Canonical scheduler domain service shared by Web adapters and the daemon.
//!
//! The service owns definition validation and run lifecycle ordering. Actual
//! command enqueue remains behind [`SchedulerCommandDispatcher`], which the
//! daemon composition root must provide.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;

use super::{
    parse_cron, parse_interval, Interval, ScheduleKind, ScheduledTask, ScheduledTaskMetadata,
    SchedulerDispatchReceipt, SchedulerError, SchedulerKind, SchedulerRunBegin,
    SchedulerRunFailure, SchedulerRunFailureCode, SchedulerRunId, SchedulerRunRecord,
    SchedulerRunStatus, SchedulerRunStore, SchedulerRunStoreError, SchedulerRunTriggerSource,
    SchedulerStore, TaskId, TaskPayload,
};

const MAX_NAME_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 4 * 1024;
const MAX_PAYLOAD_BYTES: usize = 128 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;
const MAX_DISPATCH_ERROR_BYTES: usize = 4 * 1024;

/// Complete definition input. PATCH adapters should merge partial DTOs with a
/// current snapshot, then submit the merged input with its expected revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDefinitionInput {
    pub name: String,
    pub schedule: String,
    pub schedule_kind: ScheduleKind,
    pub timezone: Option<String>,
    pub payload: TaskPayload,
    pub paused: bool,
    pub metadata: ScheduledTaskMetadata,
}

impl SchedulerDefinitionInput {
    pub fn from_task(task: &ScheduledTask) -> Self {
        Self {
            name: task.name.clone(),
            schedule: task.schedule.clone(),
            schedule_kind: task.schedule_kind,
            timezone: task.timezone.clone(),
            payload: task.payload.clone(),
            paused: task.paused,
            metadata: task.metadata.clone(),
        }
    }
}

/// Snapshot passed across the daemon-owned enqueue boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDispatchRequest {
    pub run_id: SchedulerRunId,
    pub task: ScheduledTask,
    pub trigger_source: SchedulerRunTriggerSource,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub idempotency_key: String,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulerDispatcherError {
    #[error("scheduler dispatcher unavailable: {0}")]
    Unavailable(String),
    #[error("scheduler dispatch rejected: {0}")]
    Rejected(String),
    #[error("scheduler command enqueue failed: {0}")]
    EnqueueFailed(String),
}

/// The only scheduler-to-daemon enqueue capability. Implementations must make
/// the idempotency key durable before returning a receipt.
pub trait SchedulerCommandDispatcher: Send + Sync {
    fn dispatch(
        &self,
        request: SchedulerDispatchRequest,
    ) -> Result<SchedulerDispatchReceipt, SchedulerDispatcherError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedSchedulerRun {
    pub run: SchedulerRunRecord,
    pub receipt: SchedulerDispatchReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDueDispatch {
    pub accepted: AcceptedSchedulerRun,
    pub task: ScheduledTask,
}

#[derive(Debug, Error)]
pub enum SchedulerServiceError {
    #[error(transparent)]
    Definitions(#[from] SchedulerError),
    #[error(transparent)]
    Runs(#[from] SchedulerRunStoreError),
    #[error("invalid scheduler definition: {0}")]
    Validation(String),
    #[error("scheduler dispatch for run '{run_id}' is already in progress")]
    DispatchInProgress { run_id: SchedulerRunId },
    #[error("idempotency key already completed run '{run_id}' without an acceptance receipt")]
    DuplicateIdempotency { run_id: SchedulerRunId },
    #[error("scheduler dispatcher unavailable for run '{run_id}': {message}")]
    DispatcherUnavailable {
        run_id: SchedulerRunId,
        message: String,
    },
    #[error("scheduler dispatch rejected for run '{run_id}': {message}")]
    DispatchRejected {
        run_id: SchedulerRunId,
        message: String,
    },
    #[error("scheduler enqueue failed for run '{run_id}': {message}")]
    EnqueueFailed {
        run_id: SchedulerRunId,
        message: String,
    },
    #[error("run '{run_id}' was accepted, but its recurring schedule could not advance: {source}")]
    ScheduleAdvanceFailed {
        run_id: SchedulerRunId,
        #[source]
        source: SchedulerError,
    },
}

/// Coordinated definition, lifecycle, and dispatch boundary.
pub struct SchedulerService {
    definitions: Arc<SchedulerStore>,
    runs: Arc<SchedulerRunStore>,
    dispatcher: Option<Arc<dyn SchedulerCommandDispatcher>>,
}

impl SchedulerService {
    pub fn new(definitions: Arc<SchedulerStore>, runs: Arc<SchedulerRunStore>) -> Self {
        Self {
            definitions,
            runs,
            dispatcher: None,
        }
    }

    pub fn open_default() -> Self {
        Self::new(
            Arc::new(SchedulerStore::open_default()),
            Arc::new(SchedulerRunStore::open_default()),
        )
    }

    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn SchedulerCommandDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    pub fn definitions(&self) -> &Arc<SchedulerStore> {
        &self.definitions
    }

    pub fn runs(&self) -> &Arc<SchedulerRunStore> {
        &self.runs
    }

    pub fn list_definitions(&self) -> Result<Vec<ScheduledTask>, SchedulerServiceError> {
        Ok(self.definitions.load()?)
    }

    pub fn get_definition(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerServiceError> {
        Ok(self.definitions.get(id)?)
    }

    pub fn create_definition(
        &self,
        input: SchedulerDefinitionInput,
        now: DateTime<Utc>,
    ) -> Result<ScheduledTask, SchedulerServiceError> {
        let task = build_task(input, now)?;
        Ok(self.definitions.add(task)?)
    }

    pub fn replace_definition(
        &self,
        id: &TaskId,
        expected_revision: u64,
        input: SchedulerDefinitionInput,
        now: DateTime<Utc>,
    ) -> Result<ScheduledTask, SchedulerServiceError> {
        let task = build_task(input, now)?;
        Ok(self.definitions.replace(id, expected_revision, task)?)
    }

    pub fn remove_definition(
        &self,
        id: &TaskId,
        expected_revision: u64,
    ) -> Result<ScheduledTask, SchedulerServiceError> {
        Ok(self
            .definitions
            .remove_if_revision(id, Some(expected_revision))?)
    }

    pub fn set_paused(
        &self,
        id: &TaskId,
        paused: bool,
        expected_revision: u64,
    ) -> Result<ScheduledTask, SchedulerServiceError> {
        Ok(self
            .definitions
            .set_paused_if_revision(id, paused, Some(expected_revision))?)
    }

    /// Create and enqueue a manual run. This intentionally does not call
    /// `record_fired`, so recurring state is unchanged.
    pub fn trigger_manual(
        &self,
        id: &TaskId,
        expected_revision: u64,
        idempotency_key: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
        let task = self.definitions.get(id)?;
        check_definition_revision(&task, expected_revision)?;
        let begin = self.runs.begin(
            &task,
            SchedulerRunTriggerSource::Manual,
            None,
            idempotency_key,
            now,
        )?;
        self.dispatch_begin(begin)
    }

    /// Reconcile an `enqueueing` record after a process restart. The daemon
    /// dispatcher must deduplicate the persisted idempotency key.
    pub fn reconcile_enqueueing(
        &self,
        run_id: &SchedulerRunId,
    ) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
        let run = self.runs.get(run_id)?;
        match run.status {
            SchedulerRunStatus::Enqueueing => self.dispatch_new(run),
            status if status.was_accepted() => accepted_from_record(run),
            _ => Err(SchedulerServiceError::DuplicateIdempotency { run_id: run.id }),
        }
    }

    /// Dispatch at most one due occurrence. An accepted occurrence is detected
    /// from canonical history before dispatch, closing the accepted-command /
    /// definition-update crash window without submitting duplicate work.
    pub fn dispatch_due_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<SchedulerDueDispatch>, SchedulerServiceError> {
        let Some(task) = self.definitions.due_tasks()?.into_iter().next() else {
            return Ok(None);
        };
        let scheduled_for = task.next_run_at;
        let attempts = self.runs.occurrence_runs(&task.id, scheduled_for)?;

        let accepted = if let Some(existing) = attempts
            .iter()
            .rev()
            .find(|run| run.status.was_accepted())
            .cloned()
        {
            accepted_from_record(existing)?
        } else if let Some(enqueueing) = attempts
            .iter()
            .rev()
            .find(|run| run.status == SchedulerRunStatus::Enqueueing)
            .cloned()
        {
            self.dispatch_new(enqueueing)?
        } else {
            let base_key = format!(
                "scheduled_task:{}:{}",
                task.id.as_str(),
                scheduled_for.timestamp_millis()
            );
            let idempotency_key = if attempts.is_empty() {
                base_key
            } else {
                format!("{base_key}:attempt:{}", attempts.len() + 1)
            };
            let begin = self.runs.begin(
                &task,
                SchedulerRunTriggerSource::Scheduled,
                Some(scheduled_for),
                idempotency_key,
                now,
            )?;
            self.dispatch_begin(begin)?
        };

        let task = self
            .definitions
            .record_fired_if_revision(&task.id, Some(task.revision))
            .map_err(|source| SchedulerServiceError::ScheduleAdvanceFailed {
                run_id: accepted.run.id.clone(),
                source,
            })?;
        Ok(Some(SchedulerDueDispatch { accepted, task }))
    }

    fn dispatch_begin(
        &self,
        begin: SchedulerRunBegin,
    ) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
        match begin {
            SchedulerRunBegin::Created(run) => self.dispatch_new(run),
            SchedulerRunBegin::Existing(run) if run.status == SchedulerRunStatus::Enqueueing => {
                Err(SchedulerServiceError::DispatchInProgress { run_id: run.id })
            }
            SchedulerRunBegin::Existing(run) if run.status.was_accepted() => {
                accepted_from_record(run)
            }
            SchedulerRunBegin::Existing(run) => {
                Err(SchedulerServiceError::DuplicateIdempotency { run_id: run.id })
            }
        }
    }

    fn dispatch_new(
        &self,
        run: SchedulerRunRecord,
    ) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
        let Some(dispatcher) = &self.dispatcher else {
            let message = "daemon scheduler dispatcher is not registered".to_string();
            let failed = self.runs.mark_dispatch_failed(
                &run.id,
                Some(run.revision),
                SchedulerRunStatus::FailedToEnqueue,
                SchedulerRunFailure::new(SchedulerRunFailureCode::DispatcherUnavailable, &message),
                Utc::now(),
            )?;
            return Err(SchedulerServiceError::DispatcherUnavailable {
                run_id: failed.id,
                message,
            });
        };

        let request = SchedulerDispatchRequest {
            run_id: run.id.clone(),
            task: run.task_snapshot.clone(),
            trigger_source: run.trigger_source,
            scheduled_for: run.scheduled_for,
            idempotency_key: run.idempotency_key.clone(),
            requested_at: run.requested_at,
        };
        match dispatcher.dispatch(request) {
            Ok(receipt) => {
                let queued = self
                    .runs
                    .mark_queued(&run.id, Some(run.revision), &receipt)
                    .map_err(|error| match error {
                        SchedulerRunStoreError::Validation(message) => {
                            SchedulerServiceError::EnqueueFailed {
                                run_id: run.id.clone(),
                                message,
                            }
                        }
                        other => SchedulerServiceError::Runs(other),
                    })?;
                Ok(AcceptedSchedulerRun {
                    run: queued,
                    receipt,
                })
            }
            Err(error) => self.persist_dispatch_error(run, error),
        }
    }

    fn persist_dispatch_error(
        &self,
        run: SchedulerRunRecord,
        error: SchedulerDispatcherError,
    ) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
        let (status, code, message) = match error {
            SchedulerDispatcherError::Unavailable(message) => (
                SchedulerRunStatus::FailedToEnqueue,
                SchedulerRunFailureCode::DispatcherUnavailable,
                bounded_error(&message),
            ),
            SchedulerDispatcherError::Rejected(message) => (
                SchedulerRunStatus::Rejected,
                SchedulerRunFailureCode::DispatchRejected,
                bounded_error(&message),
            ),
            SchedulerDispatcherError::EnqueueFailed(message) => (
                SchedulerRunStatus::FailedToEnqueue,
                SchedulerRunFailureCode::EnqueueFailed,
                bounded_error(&message),
            ),
        };
        let failed = self.runs.mark_dispatch_failed(
            &run.id,
            Some(run.revision),
            status,
            SchedulerRunFailure::new(code, &message),
            Utc::now(),
        )?;
        match code {
            SchedulerRunFailureCode::DispatcherUnavailable => {
                Err(SchedulerServiceError::DispatcherUnavailable {
                    run_id: failed.id,
                    message,
                })
            }
            SchedulerRunFailureCode::DispatchRejected => {
                Err(SchedulerServiceError::DispatchRejected {
                    run_id: failed.id,
                    message,
                })
            }
            _ => Err(SchedulerServiceError::EnqueueFailed {
                run_id: failed.id,
                message,
            }),
        }
    }
}

pub(super) fn build_task(
    mut input: SchedulerDefinitionInput,
    now: DateTime<Utc>,
) -> Result<ScheduledTask, SchedulerServiceError> {
    input.name = required_bounded("name", &input.name, MAX_NAME_BYTES)?;
    input.schedule = required_bounded("schedule", &input.schedule, MAX_NAME_BYTES)?;
    validate_payload(&input.payload)?;
    validate_metadata(&input.metadata)?;
    input.timezone = validate_timezone(input.timezone)?;

    let mut task = match input.schedule_kind {
        ScheduleKind::Interval => {
            if input.timezone.is_some() {
                return Err(SchedulerServiceError::Validation(
                    "timezone is only valid for cron schedules".to_string(),
                ));
            }
            let interval = parse_interval(&input.schedule).map_err(|error| {
                SchedulerServiceError::Validation(format!("invalid interval schedule: {error}"))
            })?;
            ScheduledTask::new(
                SchedulerKind::LocalCron,
                input.name,
                input.schedule,
                interval,
                input.payload,
                now,
            )
        }
        ScheduleKind::Cron => {
            let cron = parse_cron(&input.schedule).map_err(|error| {
                SchedulerServiceError::Validation(format!("invalid cron schedule: {error}"))
            })?;
            let next_run_at = cron.next_after(now).ok_or_else(|| {
                SchedulerServiceError::Validation(
                    "cron schedule has no representable next occurrence".to_string(),
                )
            })?;
            ScheduledTask::new(
                SchedulerKind::LocalCron,
                input.name,
                input.schedule.clone(),
                Interval::from_seconds(60),
                input.payload,
                now,
            )
            .with_cron_schedule(input.schedule, next_run_at, input.timezone)
        }
    };
    task.paused = input.paused;
    task.metadata = input.metadata;
    Ok(task)
}

fn validate_timezone(value: Option<String>) -> Result<Option<String>, SchedulerServiceError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if matches!(value, "UTC" | "Etc/UTC" | "Z") {
        Ok(Some("UTC".to_string()))
    } else {
        Err(SchedulerServiceError::Validation(
            "only UTC cron evaluation is currently supported".to_string(),
        ))
    }
}

fn validate_payload(payload: &TaskPayload) -> Result<(), SchedulerServiceError> {
    let value = payload.display().trim();
    if value.is_empty() {
        return Err(SchedulerServiceError::Validation(
            "payload value is required".to_string(),
        ));
    }
    if value.len() > MAX_PAYLOAD_BYTES {
        return Err(SchedulerServiceError::Validation(format!(
            "payload exceeds the {MAX_PAYLOAD_BYTES}-byte limit"
        )));
    }
    if matches!(payload, TaskPayload::SlashCommand(_)) && !value.starts_with('/') {
        return Err(SchedulerServiceError::Validation(
            "slash-command payload must start with '/'".to_string(),
        ));
    }
    Ok(())
}

fn validate_metadata(metadata: &ScheduledTaskMetadata) -> Result<(), SchedulerServiceError> {
    if metadata
        .description
        .as_ref()
        .is_some_and(|value| value.len() > MAX_DESCRIPTION_BYTES)
    {
        return Err(SchedulerServiceError::Validation(format!(
            "description exceeds the {MAX_DESCRIPTION_BYTES}-byte limit"
        )));
    }
    let bytes = serde_json::to_vec(metadata).map_err(|error| {
        SchedulerServiceError::Validation(format!("metadata is not serializable: {error}"))
    })?;
    if bytes.len() > MAX_METADATA_BYTES {
        return Err(SchedulerServiceError::Validation(format!(
            "metadata exceeds the {MAX_METADATA_BYTES}-byte limit"
        )));
    }
    validate_optional_identifier("profile id", metadata.profile_id.as_deref())?;
    validate_optional_identifier("session id", metadata.session_id.as_deref())?;
    if metadata.working_directory.as_ref().is_some_and(|value| {
        value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
    }) {
        return Err(SchedulerServiceError::Validation(
            "working directory must be 1..=4096 bytes without control characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &str,
    value: Option<&str>,
) -> Result<(), SchedulerServiceError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || value.chars().any(|character| character.is_control())
    {
        return Err(SchedulerServiceError::Validation(format!(
            "{field} must be 1..={MAX_NAME_BYTES} bytes without control characters"
        )));
    }
    Ok(())
}

fn required_bounded(
    field: &str,
    value: &str,
    max_bytes: usize,
) -> Result<String, SchedulerServiceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SchedulerServiceError::Validation(format!(
            "{field} is required"
        )));
    }
    if value.len() > max_bytes {
        return Err(SchedulerServiceError::Validation(format!(
            "{field} exceeds the {max_bytes}-byte limit"
        )));
    }
    Ok(value.to_string())
}

fn check_definition_revision(
    task: &ScheduledTask,
    expected: u64,
) -> Result<(), SchedulerServiceError> {
    if task.revision == expected {
        Ok(())
    } else {
        Err(SchedulerServiceError::Definitions(
            SchedulerError::RevisionConflict {
                expected,
                actual: task.revision,
            },
        ))
    }
}

fn accepted_from_record(
    run: SchedulerRunRecord,
) -> Result<AcceptedSchedulerRun, SchedulerServiceError> {
    let receipt =
        run.dispatch_receipt()
            .ok_or_else(|| SchedulerServiceError::DuplicateIdempotency {
                run_id: run.id.clone(),
            })?;
    Ok(AcceptedSchedulerRun { run, receipt })
}

fn bounded_error(value: &str) -> String {
    if value.len() <= MAX_DISPATCH_ERROR_BYTES {
        return value.to_string();
    }
    let suffix = "…";
    let mut end = MAX_DISPATCH_ERROR_BYTES.saturating_sub(suffix.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], suffix)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingDispatcher {
        requests: Mutex<Vec<SchedulerDispatchRequest>>,
        error: Mutex<Option<SchedulerDispatcherError>>,
    }

    impl SchedulerCommandDispatcher for RecordingDispatcher {
        fn dispatch(
            &self,
            request: SchedulerDispatchRequest,
        ) -> Result<SchedulerDispatchReceipt, SchedulerDispatcherError> {
            if let Some(error) = self.error.lock().unwrap().take() {
                return Err(error);
            }
            self.requests.lock().unwrap().push(request.clone());
            Ok(SchedulerDispatchReceipt {
                run_id: request.run_id,
                command_id: format!("cmd-{}", self.requests.lock().unwrap().len()),
                accepted_at: Utc::now(),
                idempotency_key: request.idempotency_key,
            })
        }
    }

    fn input(schedule: &str) -> SchedulerDefinitionInput {
        SchedulerDefinitionInput {
            name: "nightly review".to_string(),
            schedule: schedule.to_string(),
            schedule_kind: ScheduleKind::Interval,
            timezone: None,
            payload: TaskPayload::Prompt("review the workspace".to_string()),
            paused: false,
            metadata: ScheduledTaskMetadata::default(),
        }
    }

    fn service(
        dispatcher: Option<Arc<dyn SchedulerCommandDispatcher>>,
    ) -> (tempfile::TempDir, SchedulerService) {
        let dir = tempfile::tempdir().unwrap();
        let definitions = Arc::new(SchedulerStore::new(dir.path().join("definitions.json")));
        let runs = Arc::new(SchedulerRunStore::new(dir.path().join("runs.json")));
        let mut service = SchedulerService::new(definitions, runs);
        if let Some(dispatcher) = dispatcher {
            service = service.with_dispatcher(dispatcher);
        }
        (dir, service)
    }

    #[test]
    fn validates_interval_cron_timezone_and_payload() {
        let (_dir, service) = service(None);
        let now = Utc::now();
        let interval = service.create_definition(input("5m"), now).unwrap();
        assert_eq!(interval.next_run_at, now + chrono::Duration::minutes(5));

        let mut cron = input("*/5 * * * *");
        cron.schedule_kind = ScheduleKind::Cron;
        cron.timezone = Some("UTC".to_string());
        assert_eq!(
            service.create_definition(cron, now).unwrap().schedule_kind,
            ScheduleKind::Cron
        );

        let mut unsupported_timezone = input("0 0 * * *");
        unsupported_timezone.schedule_kind = ScheduleKind::Cron;
        unsupported_timezone.timezone = Some("Asia/Shanghai".to_string());
        assert!(matches!(
            service.create_definition(unsupported_timezone, now),
            Err(SchedulerServiceError::Validation(_))
        ));

        let mut invalid_command = input("1m");
        invalid_command.payload = TaskPayload::SlashCommand("not-a-command".to_string());
        assert!(matches!(
            service.create_definition(invalid_command, now),
            Err(SchedulerServiceError::Validation(_))
        ));
    }

    #[test]
    fn manual_dispatch_is_real_idempotent_and_does_not_advance_schedule() {
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let (_dir, service) = service(Some(dispatcher.clone()));
        let now = Utc::now();
        let task = service.create_definition(input("1m"), now).unwrap();

        let first = service
            .trigger_manual(&task.id, task.revision, "manual-stable", now)
            .unwrap();
        let second = service
            .trigger_manual(&task.id, task.revision, "manual-stable", now)
            .unwrap();
        assert_eq!(first.run.id, second.run.id);
        assert_eq!(first.receipt.command_id, second.receipt.command_id);
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);

        let unchanged = service.get_definition(&task.id).unwrap();
        assert_eq!(unchanged.revision, task.revision);
        assert_eq!(unchanged.last_run_at, None);
        assert_eq!(unchanged.next_run_at, task.next_run_at);
    }

    #[test]
    fn enqueueing_run_reconciliation_reuses_the_original_run_and_receipt() {
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let (_dir, service) = service(Some(dispatcher.clone()));
        let now = Utc::now();
        let task = service.create_definition(input("1m"), now).unwrap();
        let run = service
            .runs()
            .begin(
                &task,
                SchedulerRunTriggerSource::Manual,
                None,
                "manual-crash-window",
                now,
            )
            .unwrap()
            .record()
            .clone();

        let first = service.reconcile_enqueueing(&run.id).unwrap();
        let second = service.reconcile_enqueueing(&run.id).unwrap();

        assert_eq!(first.run.id, run.id);
        assert_eq!(second.run.id, run.id);
        assert_eq!(first.receipt, second.receipt);
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn missing_dispatcher_records_truthful_enqueue_failure() {
        let (_dir, service) = service(None);
        let now = Utc::now();
        let task = service.create_definition(input("1m"), now).unwrap();
        let error = service
            .trigger_manual(&task.id, task.revision, "manual-unavailable", now)
            .unwrap_err();
        let run_id = match error {
            SchedulerServiceError::DispatcherUnavailable { run_id, .. } => run_id,
            other => panic!("unexpected error: {other}"),
        };
        let run = service.runs().get(&run_id).unwrap();
        assert_eq!(run.status, SchedulerRunStatus::FailedToEnqueue);
        assert!(run.command_id.is_none());
        assert_eq!(
            run.failure.as_ref().unwrap().code,
            SchedulerRunFailureCode::DispatcherUnavailable
        );
    }

    #[test]
    fn due_dispatch_uses_same_boundary_and_advances_only_after_acceptance() {
        let dispatcher = Arc::new(RecordingDispatcher::default());
        let (_dir, service) = service(Some(dispatcher.clone()));
        let now = Utc::now();
        let mut task = service.create_definition(input("1s"), now).unwrap();
        task.next_run_at = now - chrono::Duration::seconds(1);
        let task = service
            .definitions()
            .replace(&task.id, task.revision, task.clone())
            .unwrap();

        let dispatched = service
            .dispatch_due_once(now)
            .unwrap()
            .expect("task should be due");
        assert_eq!(
            dispatched.accepted.run.trigger_source,
            SchedulerRunTriggerSource::Scheduled
        );
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);
        assert!(dispatched.task.last_run_at.is_some());
        assert!(dispatched.task.revision > task.revision);
    }

    #[test]
    fn failed_due_enqueue_does_not_advance_and_next_tick_gets_a_new_attempt() {
        let dispatcher = Arc::new(RecordingDispatcher::default());
        *dispatcher.error.lock().unwrap() = Some(SchedulerDispatcherError::EnqueueFailed(
            "queue is read-only".to_string(),
        ));
        let (_dir, service) = service(Some(dispatcher.clone()));
        let now = Utc::now();
        let mut task = service.create_definition(input("1s"), now).unwrap();
        task.next_run_at = now - chrono::Duration::seconds(1);
        let task = service
            .definitions()
            .replace(&task.id, task.revision, task.clone())
            .unwrap();

        assert!(matches!(
            service.dispatch_due_once(now),
            Err(SchedulerServiceError::EnqueueFailed { .. })
        ));
        let unchanged = service.get_definition(&task.id).unwrap();
        assert_eq!(unchanged.last_run_at, None);
        assert_eq!(unchanged.revision, task.revision);

        let retry = service
            .dispatch_due_once(now)
            .unwrap()
            .expect("failed occurrence should be retried");
        let attempts = service
            .runs()
            .occurrence_runs(&task.id, task.next_run_at)
            .unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(retry.accepted.run.status, SchedulerRunStatus::Queued);
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);
    }
}
