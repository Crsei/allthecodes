//! Durable lifecycle records for manual and scheduled job dispatches.
//!
//! The run repository is deliberately separate from the daemon command queue:
//! it records scheduler intent and stable command/session references, while the
//! daemon remains the only owner allowed to enqueue or execute commands.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use super::{ScheduledTask, TaskId};

const RUN_STATE_VERSION: u32 = 1;
const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 200;
const MAX_ARTIFACT_LINKS: usize = 64;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024;
const MAX_OUTPUT_SUMMARY_BYTES: usize = 16 * 1024;
const MAX_FAILURE_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_REFERENCE_BYTES: usize = 256;
const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;

/// Stable identifier for one canonical scheduler run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchedulerRunId(String);

impl SchedulerRunId {
    pub fn new() -> Self {
        Self(format!("run_{}", Uuid::new_v4().simple()))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, SchedulerRunStoreError> {
        let value = value.into();
        if is_valid_reference(&value) && value.starts_with("run_") {
            Ok(Self(value))
        } else {
            Err(SchedulerRunStoreError::Validation(
                "run id must start with 'run_' and contain only safe identifier characters"
                    .to_string(),
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SchedulerRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SchedulerRunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why a run was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerRunTriggerSource {
    Manual,
    Scheduled,
}

impl SchedulerRunTriggerSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
        }
    }
}

/// Canonical dispatch/execution lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerRunStatus {
    Enqueueing,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Rejected,
    FailedToEnqueue,
}

impl SchedulerRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enqueueing => "enqueueing",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::FailedToEnqueue => "failed_to_enqueue",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
                | Self::Cancelled
                | Self::Rejected
                | Self::FailedToEnqueue
        )
    }

    pub const fn was_accepted(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running | Self::Completed | Self::Failed | Self::Cancelled
        )
    }
}

/// Stable, typed reason for an unsuccessful run transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerRunFailureCode {
    DispatcherUnavailable,
    EnqueueFailed,
    DispatchRejected,
    ExecutionFailed,
    Cancelled,
}

/// Bounded failure detail safe to expose through history adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerRunFailure {
    pub code: SchedulerRunFailureCode,
    pub message: String,
}

impl SchedulerRunFailure {
    pub fn new(code: SchedulerRunFailureCode, message: impl AsRef<str>) -> Self {
        Self {
            code,
            message: truncate_utf8(message.as_ref(), MAX_FAILURE_MESSAGE_BYTES),
        }
    }
}

/// Durable receipt returned only after the daemon command queue accepted work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerDispatchReceipt {
    pub run_id: SchedulerRunId,
    pub command_id: String,
    pub accepted_at: DateTime<Utc>,
    pub idempotency_key: String,
}

/// One canonical run record. The full definition snapshot is retained so an
/// interrupted enqueue can be reconciled without reading a newer definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerRunRecord {
    pub id: SchedulerRunId,
    pub task_id: TaskId,
    pub task_snapshot: ScheduledTask,
    pub trigger_source: SchedulerRunTriggerSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduled_for: Option<DateTime<Utc>>,
    pub idempotency_key: String,
    pub status: SchedulerRunStatus,
    pub revision: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enqueued_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SchedulerRunFailure>,
    #[serde(default)]
    pub legacy: bool,
}

impl SchedulerRunRecord {
    pub fn dispatch_receipt(&self) -> Option<SchedulerDispatchReceipt> {
        Some(SchedulerDispatchReceipt {
            run_id: self.id.clone(),
            command_id: self.command_id.clone()?,
            accepted_at: self.enqueued_at?,
            idempotency_key: self.idempotency_key.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerRunBegin {
    Created(SchedulerRunRecord),
    Existing(SchedulerRunRecord),
}

impl SchedulerRunBegin {
    pub fn record(&self) -> &SchedulerRunRecord {
        match self {
            Self::Created(record) | Self::Existing(record) => record,
        }
    }
}

/// Bounded history query. Cursor values are bound to every filter other than
/// the page limit, so a cursor cannot silently be reused for a different view.
#[derive(Debug, Clone, Default)]
pub struct SchedulerRunQuery {
    pub task_id: Option<TaskId>,
    pub statuses: Vec<SchedulerRunStatus>,
    pub trigger_source: Option<SchedulerRunTriggerSource>,
    pub profile_id: Option<String>,
    pub requested_after: Option<DateTime<Utc>>,
    pub requested_before: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerRunPage {
    pub runs: Vec<SchedulerRunRecord>,
    pub next_cursor: Option<String>,
}

/// Execution-terminal information supplied by the daemon worker/event bridge.
#[derive(Debug, Clone, Default)]
pub struct SchedulerRunCompletion {
    pub completed_at: Option<DateTime<Utc>>,
    pub execution_task_id: Option<String>,
    pub session_id: Option<String>,
    pub artifact_links: Vec<Value>,
    pub output_summary: Option<String>,
}

/// One terminal record imported from the retired Web-only history log.
#[derive(Debug, Clone)]
pub struct LegacySchedulerRunInput {
    pub source_id: String,
    pub task: ScheduledTask,
    pub status: SchedulerRunStatus,
    pub requested_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub session_id: Option<String>,
    pub artifact_links: Vec<Value>,
    pub output_summary: Option<String>,
    pub failure: Option<SchedulerRunFailure>,
}

#[derive(Debug, Error)]
pub enum SchedulerRunStoreError {
    #[error("I/O error touching {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode scheduler run store {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode scheduler run store: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("scheduler run '{0}' not found")]
    NotFound(String),
    #[error("idempotency key is already bound to scheduler run '{run_id}'")]
    IdempotencyConflict { run_id: SchedulerRunId },
    #[error("scheduler run revision conflict: expected {expected}, current {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("invalid scheduler run transition for '{run_id}': {from} -> {to}")]
    InvalidTransition {
        run_id: SchedulerRunId,
        from: &'static str,
        to: &'static str,
    },
    #[error("invalid scheduler run cursor")]
    InvalidCursor,
    #[error("invalid scheduler run data: {0}")]
    Validation(String),
    #[error("scheduler run state exceeds the {MAX_STATE_BYTES}-byte safety limit")]
    TooLarge,
    #[error("could not acquire scheduler run lock at {path} within {}ms", timeout.as_millis())]
    LockTimeout { path: PathBuf, timeout: Duration },
    #[error("scheduler run store version {actual} is newer than supported version {supported}")]
    UnsupportedVersion { actual: u32, supported: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunStateFile {
    version: u32,
    #[serde(default)]
    runs: Vec<SchedulerRunRecord>,
}

impl Default for RunStateFile {
    fn default() -> Self {
        Self {
            version: RUN_STATE_VERSION,
            runs: Vec::new(),
        }
    }
}

/// Cross-process-safe JSON repository for canonical scheduler lifecycle rows.
pub struct SchedulerRunStore {
    state_path: PathBuf,
    lock_path: PathBuf,
    inner: Mutex<()>,
}

impl SchedulerRunStore {
    pub fn default_path() -> PathBuf {
        allthecodes_config::paths::data_root().join("scheduler_runs.json")
    }

    pub fn new(state_path: impl Into<PathBuf>) -> Self {
        let state_path = state_path.into();
        let lock_path = state_path.with_extension("json.lock");
        Self {
            state_path,
            lock_path,
            inner: Mutex::new(()),
        }
    }

    pub fn open_default() -> Self {
        Self::new(Self::default_path())
    }

    pub fn path(&self) -> &Path {
        &self.state_path
    }

    pub fn begin(
        &self,
        task: &ScheduledTask,
        trigger_source: SchedulerRunTriggerSource,
        scheduled_for: Option<DateTime<Utc>>,
        idempotency_key: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<SchedulerRunBegin, SchedulerRunStoreError> {
        let idempotency_key = idempotency_key.into();
        validate_idempotency_key(&idempotency_key)?;
        validate_task_snapshot(task)?;

        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        if let Some(existing) = state
            .runs
            .iter()
            .find(|run| run.idempotency_key == idempotency_key)
        {
            if existing.task_id == task.id
                && existing.trigger_source == trigger_source
                && existing.scheduled_for == scheduled_for
            {
                return Ok(SchedulerRunBegin::Existing(existing.clone()));
            }
            return Err(SchedulerRunStoreError::IdempotencyConflict {
                run_id: existing.id.clone(),
            });
        }

        let record = SchedulerRunRecord {
            id: SchedulerRunId::new(),
            task_id: task.id.clone(),
            task_snapshot: task.clone(),
            trigger_source,
            scheduled_for,
            idempotency_key,
            status: SchedulerRunStatus::Enqueueing,
            revision: 1,
            requested_at: now,
            updated_at: now,
            enqueued_at: None,
            started_at: None,
            completed_at: None,
            command_id: None,
            execution_task_id: None,
            session_id: task.metadata.session_id.clone(),
            artifact_links: task.metadata.artifact_links.clone(),
            output_summary: None,
            failure: None,
            legacy: false,
        };
        validate_record(&record)?;
        state.runs.push(record.clone());
        self.write_state(&state)?;
        Ok(SchedulerRunBegin::Created(record))
    }

    pub fn get(&self, id: &SchedulerRunId) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        self.read_state()?
            .runs
            .into_iter()
            .find(|run| run.id == *id)
            .ok_or_else(|| SchedulerRunStoreError::NotFound(id.to_string()))
    }

    pub fn get_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<SchedulerRunRecord>, SchedulerRunStoreError> {
        validate_idempotency_key(idempotency_key)?;
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        Ok(self
            .read_state()?
            .runs
            .into_iter()
            .find(|run| run.idempotency_key == idempotency_key))
    }

    pub fn occurrence_runs(
        &self,
        task_id: &TaskId,
        scheduled_for: DateTime<Utc>,
    ) -> Result<Vec<SchedulerRunRecord>, SchedulerRunStoreError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut runs: Vec<_> = self
            .read_state()?
            .runs
            .into_iter()
            .filter(|run| {
                run.task_id == *task_id
                    && run.trigger_source == SchedulerRunTriggerSource::Scheduled
                    && run.scheduled_for == Some(scheduled_for)
            })
            .collect();
        runs.sort_by(|left, right| {
            left.requested_at
                .cmp(&right.requested_at)
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        });
        Ok(runs)
    }

    /// Import one terminal legacy row with a deterministic idempotency key.
    /// Re-running migration returns the already imported canonical record.
    pub fn import_legacy(
        &self,
        input: LegacySchedulerRunInput,
    ) -> Result<SchedulerRunBegin, SchedulerRunStoreError> {
        if !input.status.is_terminal() {
            return Err(SchedulerRunStoreError::Validation(
                "legacy scheduler run must be terminal".to_string(),
            ));
        }
        validate_task_snapshot(&input.task)?;
        validate_optional_reference("session id", input.session_id.as_deref())?;
        validate_artifacts(&input.artifact_links)?;
        let idempotency_key = format!(
            "legacy:web-run:{:016x}",
            stable_hash(input.source_id.as_bytes())
        );

        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        if let Some(existing) = state
            .runs
            .iter()
            .find(|run| run.idempotency_key == idempotency_key)
        {
            return Ok(SchedulerRunBegin::Existing(existing.clone()));
        }
        let completed_at = input.completed_at.or(Some(input.requested_at));
        let record = SchedulerRunRecord {
            id: SchedulerRunId::new(),
            task_id: input.task.id.clone(),
            task_snapshot: input.task,
            trigger_source: SchedulerRunTriggerSource::Manual,
            scheduled_for: None,
            idempotency_key,
            status: input.status,
            revision: 1,
            requested_at: input.requested_at,
            updated_at: completed_at.unwrap_or(input.requested_at),
            enqueued_at: None,
            started_at: Some(input.requested_at),
            completed_at,
            command_id: None,
            execution_task_id: None,
            session_id: input.session_id,
            artifact_links: input.artifact_links,
            output_summary: input
                .output_summary
                .as_deref()
                .map(|value| truncate_utf8(value, MAX_OUTPUT_SUMMARY_BYTES)),
            failure: input.failure,
            legacy: true,
        };
        validate_record(&record)?;
        state.runs.push(record.clone());
        self.write_state(&state)?;
        Ok(SchedulerRunBegin::Created(record))
    }

    pub fn mark_queued(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        receipt: &SchedulerDispatchReceipt,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        if receipt.run_id != *id {
            return Err(SchedulerRunStoreError::Validation(
                "dispatch receipt run id does not match the updated run".to_string(),
            ));
        }
        validate_reference("command id", &receipt.command_id)?;
        validate_idempotency_key(&receipt.idempotency_key)?;
        self.update(id, expected_revision, SchedulerRunStatus::Queued, |run| {
            if run.idempotency_key != receipt.idempotency_key {
                return Err(SchedulerRunStoreError::Validation(
                    "dispatch receipt idempotency key does not match the run".to_string(),
                ));
            }
            if run.status == SchedulerRunStatus::Queued
                && run.command_id.as_deref() == Some(receipt.command_id.as_str())
            {
                return Ok(false);
            }
            require_status(
                run,
                &[SchedulerRunStatus::Enqueueing],
                SchedulerRunStatus::Queued,
            )?;
            run.status = SchedulerRunStatus::Queued;
            run.command_id = Some(receipt.command_id.clone());
            run.enqueued_at = Some(receipt.accepted_at);
            run.failure = None;
            Ok(true)
        })
    }

    pub fn mark_dispatch_failed(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        status: SchedulerRunStatus,
        failure: SchedulerRunFailure,
        at: DateTime<Utc>,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        if !matches!(
            status,
            SchedulerRunStatus::Rejected | SchedulerRunStatus::FailedToEnqueue
        ) {
            return Err(SchedulerRunStoreError::Validation(
                "dispatch failure must use rejected or failed_to_enqueue status".to_string(),
            ));
        }
        self.update(id, expected_revision, status, |run| {
            require_status(run, &[SchedulerRunStatus::Enqueueing], status)?;
            run.status = status;
            run.completed_at = Some(at);
            run.failure = Some(failure);
            Ok(true)
        })
    }

    pub fn mark_running(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        started_at: DateTime<Utc>,
        execution_task_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        validate_optional_reference("execution task id", execution_task_id.as_deref())?;
        validate_optional_reference("session id", session_id.as_deref())?;
        self.update(id, expected_revision, SchedulerRunStatus::Running, |run| {
            if run.status == SchedulerRunStatus::Running {
                return Ok(false);
            }
            require_status(
                run,
                &[SchedulerRunStatus::Queued],
                SchedulerRunStatus::Running,
            )?;
            run.status = SchedulerRunStatus::Running;
            run.started_at = Some(started_at);
            if execution_task_id.is_some() {
                run.execution_task_id = execution_task_id;
            }
            if session_id.is_some() {
                run.session_id = session_id;
            }
            Ok(true)
        })
    }

    pub fn mark_completed(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        completion: SchedulerRunCompletion,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        self.mark_execution_terminal(
            id,
            expected_revision,
            SchedulerRunStatus::Completed,
            completion,
            None,
        )
    }

    pub fn mark_failed(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        completion: SchedulerRunCompletion,
        message: impl AsRef<str>,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        self.mark_execution_terminal(
            id,
            expected_revision,
            SchedulerRunStatus::Failed,
            completion,
            Some(SchedulerRunFailure::new(
                SchedulerRunFailureCode::ExecutionFailed,
                message,
            )),
        )
    }

    pub fn mark_cancelled(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        completion: SchedulerRunCompletion,
        message: impl AsRef<str>,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        self.mark_execution_terminal(
            id,
            expected_revision,
            SchedulerRunStatus::Cancelled,
            completion,
            Some(SchedulerRunFailure::new(
                SchedulerRunFailureCode::Cancelled,
                message,
            )),
        )
    }

    pub fn query(
        &self,
        query: &SchedulerRunQuery,
    ) -> Result<SchedulerRunPage, SchedulerRunStoreError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let state = self.read_state()?;
        let mut runs: Vec<_> = state
            .runs
            .into_iter()
            .filter(|run| query.task_id.as_ref().is_none_or(|id| run.task_id == *id))
            .filter(|run| query.statuses.is_empty() || query.statuses.contains(&run.status))
            .filter(|run| {
                query
                    .trigger_source
                    .is_none_or(|source| run.trigger_source == source)
            })
            .filter(|run| {
                query.profile_id.as_deref().is_none_or(|profile_id| {
                    run.task_snapshot.metadata.profile_id.as_deref() == Some(profile_id)
                })
            })
            .filter(|run| {
                query
                    .requested_after
                    .is_none_or(|after| run.requested_at >= after)
            })
            .filter(|run| {
                query
                    .requested_before
                    .is_none_or(|before| run.requested_at <= before)
            })
            .collect();
        runs.sort_by(|left, right| {
            right
                .requested_at
                .cmp(&left.requested_at)
                .then_with(|| right.id.as_str().cmp(left.id.as_str()))
        });

        let fingerprint = query_fingerprint(query);
        let start = match query.cursor.as_deref() {
            Some(cursor) => {
                let decoded = decode_cursor(cursor, fingerprint)?;
                runs.iter()
                    .position(|run| {
                        run.id == decoded.id
                            && run.requested_at.timestamp_micros() == decoded.micros
                    })
                    .map(|position| position + 1)
                    .ok_or(SchedulerRunStoreError::InvalidCursor)?
            }
            None => 0,
        };
        let limit = query
            .limit
            .unwrap_or(DEFAULT_PAGE_LIMIT)
            .clamp(1, MAX_PAGE_LIMIT);
        let end = start.saturating_add(limit).min(runs.len());
        let page_runs = runs[start..end].to_vec();
        let next_cursor = if end < runs.len() {
            page_runs.last().map(|run| encode_cursor(run, fingerprint))
        } else {
            None
        };
        Ok(SchedulerRunPage {
            runs: page_runs,
            next_cursor,
        })
    }

    fn mark_execution_terminal(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        status: SchedulerRunStatus,
        completion: SchedulerRunCompletion,
        failure: Option<SchedulerRunFailure>,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError> {
        validate_optional_reference("execution task id", completion.execution_task_id.as_deref())?;
        validate_optional_reference("session id", completion.session_id.as_deref())?;
        validate_artifacts(&completion.artifact_links)?;
        let output_summary = completion
            .output_summary
            .as_deref()
            .map(|value| truncate_utf8(value, MAX_OUTPUT_SUMMARY_BYTES));
        self.update(id, expected_revision, status, |run| {
            require_status(
                run,
                &[SchedulerRunStatus::Queued, SchedulerRunStatus::Running],
                status,
            )?;
            run.status = status;
            run.completed_at = Some(completion.completed_at.unwrap_or_else(Utc::now));
            if completion.execution_task_id.is_some() {
                run.execution_task_id = completion.execution_task_id;
            }
            if completion.session_id.is_some() {
                run.session_id = completion.session_id;
            }
            if !completion.artifact_links.is_empty() {
                run.artifact_links = completion.artifact_links;
            }
            run.output_summary = output_summary;
            run.failure = failure;
            Ok(true)
        })
    }

    fn update<F>(
        &self,
        id: &SchedulerRunId,
        expected_revision: Option<u64>,
        target_status: SchedulerRunStatus,
        update: F,
    ) -> Result<SchedulerRunRecord, SchedulerRunStoreError>
    where
        F: FnOnce(&mut SchedulerRunRecord) -> Result<bool, SchedulerRunStoreError>,
    {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let run = state
            .runs
            .iter_mut()
            .find(|run| run.id == *id)
            .ok_or_else(|| SchedulerRunStoreError::NotFound(id.to_string()))?;
        if let Some(expected) = expected_revision {
            if run.revision != expected {
                return Err(SchedulerRunStoreError::RevisionConflict {
                    expected,
                    actual: run.revision,
                });
            }
        }
        let changed = update(run).map_err(|error| match error {
            SchedulerRunStoreError::InvalidTransition { .. } => error,
            other => other,
        })?;
        if changed {
            run.revision = run.revision.saturating_add(1);
            run.updated_at = Utc::now();
            validate_record(run)?;
            let snapshot = run.clone();
            self.write_state(&state)?;
            Ok(snapshot)
        } else {
            if run.status != target_status {
                return Err(SchedulerRunStoreError::InvalidTransition {
                    run_id: run.id.clone(),
                    from: run.status.as_str(),
                    to: target_status.as_str(),
                });
            }
            Ok(run.clone())
        }
    }

    fn read_state(&self) -> Result<RunStateFile, SchedulerRunStoreError> {
        if !self.state_path.exists() {
            return Ok(RunStateFile::default());
        }
        let mut file =
            File::open(&self.state_path).map_err(|source| SchedulerRunStoreError::Io {
                path: self.state_path.clone(),
                source,
            })?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|source| SchedulerRunStoreError::Io {
                path: self.state_path.clone(),
                source,
            })?;
        if bytes.is_empty() || bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(RunStateFile::default());
        }
        if bytes.len() > MAX_STATE_BYTES {
            return Err(SchedulerRunStoreError::TooLarge);
        }
        let mut state: RunStateFile =
            serde_json::from_slice(&bytes).map_err(|source| SchedulerRunStoreError::Decode {
                path: self.state_path.clone(),
                source,
            })?;
        if state.version > RUN_STATE_VERSION {
            return Err(SchedulerRunStoreError::UnsupportedVersion {
                actual: state.version,
                supported: RUN_STATE_VERSION,
            });
        }
        state.version = RUN_STATE_VERSION;
        for run in &state.runs {
            validate_record(run)?;
        }
        Ok(state)
    }

    fn write_state(&self, state: &RunStateFile) -> Result<(), SchedulerRunStoreError> {
        let persisted = RunStateFile {
            version: RUN_STATE_VERSION,
            runs: state.runs.clone(),
        };
        let bytes =
            serde_json::to_vec_pretty(&persisted).map_err(SchedulerRunStoreError::Encode)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(SchedulerRunStoreError::TooLarge);
        }
        let parent = self.state_path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| SchedulerRunStoreError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let tmp_path = self.state_path.with_extension("json.tmp");
        {
            let mut tmp = File::create(&tmp_path).map_err(|source| SchedulerRunStoreError::Io {
                path: tmp_path.clone(),
                source,
            })?;
            tmp.write_all(&bytes)
                .map_err(|source| SchedulerRunStoreError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
            tmp.sync_all()
                .map_err(|source| SchedulerRunStoreError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
        }
        fs::rename(&tmp_path, &self.state_path).map_err(|source| SchedulerRunStoreError::Io {
            path: self.state_path.clone(),
            source,
        })?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn acquire_lock(&self) -> Result<RunFileLockGuard<'_>, SchedulerRunStoreError> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent).map_err(|source| SchedulerRunStoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let timeout = Duration::from_secs(2);
        let started = Instant::now();
        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.lock_path)
            {
                Ok(file) => {
                    return Ok(RunFileLockGuard {
                        file: Some(file),
                        path: &self.lock_path,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if started.elapsed() >= timeout {
                        return Err(SchedulerRunStoreError::LockTimeout {
                            path: self.lock_path.clone(),
                            timeout,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(source) => {
                    return Err(SchedulerRunStoreError::Io {
                        path: self.lock_path.clone(),
                        source,
                    });
                }
            }
        }
    }
}

fn require_status(
    run: &SchedulerRunRecord,
    allowed: &[SchedulerRunStatus],
    target: SchedulerRunStatus,
) -> Result<(), SchedulerRunStoreError> {
    if allowed.contains(&run.status) {
        Ok(())
    } else {
        Err(SchedulerRunStoreError::InvalidTransition {
            run_id: run.id.clone(),
            from: run.status.as_str(),
            to: target.as_str(),
        })
    }
}

fn validate_record(run: &SchedulerRunRecord) -> Result<(), SchedulerRunStoreError> {
    SchedulerRunId::parse(run.id.to_string())?;
    validate_idempotency_key(&run.idempotency_key)?;
    validate_task_snapshot(&run.task_snapshot)?;
    if run.task_id != run.task_snapshot.id {
        return Err(SchedulerRunStoreError::Validation(
            "run task id does not match its definition snapshot".to_string(),
        ));
    }
    validate_optional_reference("command id", run.command_id.as_deref())?;
    validate_optional_reference("execution task id", run.execution_task_id.as_deref())?;
    validate_optional_reference("session id", run.session_id.as_deref())?;
    validate_artifacts(&run.artifact_links)?;
    if run.revision == 0 {
        return Err(SchedulerRunStoreError::Validation(
            "run revision must be at least 1".to_string(),
        ));
    }
    Ok(())
}

fn validate_task_snapshot(task: &ScheduledTask) -> Result<(), SchedulerRunStoreError> {
    validate_artifacts(&task.metadata.artifact_links)?;
    validate_optional_reference("profile id", task.metadata.profile_id.as_deref())?;
    validate_optional_reference("session id", task.metadata.session_id.as_deref())?;
    if task
        .metadata
        .working_directory
        .as_ref()
        .is_some_and(|value| {
            value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
        })
    {
        return Err(SchedulerRunStoreError::Validation(
            "working directory must be 1..=4096 bytes without control characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_idempotency_key(value: &str) -> Result<(), SchedulerRunStoreError> {
    if value.is_empty()
        || value.len() > MAX_IDEMPOTENCY_KEY_BYTES
        || value.chars().any(char::is_control)
    {
        Err(SchedulerRunStoreError::Validation(format!(
            "idempotency key must be 1..={MAX_IDEMPOTENCY_KEY_BYTES} bytes without control characters"
        )))
    } else {
        Ok(())
    }
}

fn validate_optional_reference(
    field: &str,
    value: Option<&str>,
) -> Result<(), SchedulerRunStoreError> {
    match value {
        Some(value) => validate_reference(field, value),
        None => Ok(()),
    }
}

fn validate_reference(field: &str, value: &str) -> Result<(), SchedulerRunStoreError> {
    if is_valid_reference(value) {
        Ok(())
    } else {
        Err(SchedulerRunStoreError::Validation(format!(
            "{field} must be 1..={MAX_REFERENCE_BYTES} bytes and contain only safe identifier characters"
        )))
    }
}

fn is_valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REFERENCE_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validate_artifacts(artifacts: &[Value]) -> Result<(), SchedulerRunStoreError> {
    if artifacts.len() > MAX_ARTIFACT_LINKS {
        return Err(SchedulerRunStoreError::Validation(format!(
            "artifact links exceed the {MAX_ARTIFACT_LINKS}-item limit"
        )));
    }
    let bytes = serde_json::to_vec(artifacts).map_err(SchedulerRunStoreError::Encode)?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(SchedulerRunStoreError::Validation(format!(
            "artifact links exceed the {MAX_ARTIFACT_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let suffix = "…";
    let mut end = max_bytes.saturating_sub(suffix.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], suffix)
}

#[derive(Debug)]
struct DecodedCursor {
    micros: i64,
    id: SchedulerRunId,
}

fn encode_cursor(run: &SchedulerRunRecord, fingerprint: u64) -> String {
    format!(
        "v1.{}.{}.{fingerprint:016x}",
        run.requested_at.timestamp_micros(),
        run.id.as_str()
    )
}

fn decode_cursor(
    cursor: &str,
    expected_fingerprint: u64,
) -> Result<DecodedCursor, SchedulerRunStoreError> {
    let mut parts = cursor.split('.');
    if parts.next() != Some("v1") {
        return Err(SchedulerRunStoreError::InvalidCursor);
    }
    let micros = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or(SchedulerRunStoreError::InvalidCursor)?;
    let id = parts
        .next()
        .ok_or(SchedulerRunStoreError::InvalidCursor)
        .and_then(|value| {
            SchedulerRunId::parse(value.to_string())
                .map_err(|_| SchedulerRunStoreError::InvalidCursor)
        })?;
    let fingerprint = parts
        .next()
        .and_then(|value| u64::from_str_radix(value, 16).ok())
        .ok_or(SchedulerRunStoreError::InvalidCursor)?;
    if parts.next().is_some() || fingerprint != expected_fingerprint {
        return Err(SchedulerRunStoreError::InvalidCursor);
    }
    Ok(DecodedCursor { micros, id })
}

fn query_fingerprint(query: &SchedulerRunQuery) -> u64 {
    let mut statuses: Vec<_> = query
        .statuses
        .iter()
        .map(|status| status.as_str())
        .collect();
    statuses.sort_unstable();
    let canonical = format!(
        "task={}|statuses={}|trigger={}|profile={}|after={}|before={}",
        query.task_id.as_ref().map(TaskId::as_str).unwrap_or(""),
        statuses.join(","),
        query
            .trigger_source
            .map(SchedulerRunTriggerSource::as_str)
            .unwrap_or(""),
        query.profile_id.as_deref().unwrap_or(""),
        query
            .requested_after
            .map(|value| value.timestamp_micros().to_string())
            .unwrap_or_default(),
        query
            .requested_before
            .map(|value| value.timestamp_micros().to_string())
            .unwrap_or_default(),
    );
    stable_hash(canonical.as_bytes())
}

fn stable_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

struct RunFileLockGuard<'a> {
    file: Option<File>,
    path: &'a Path,
}

impl Drop for RunFileLockGuard<'_> {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{Interval, SchedulerKind, TaskPayload};

    fn task(now: DateTime<Utc>) -> ScheduledTask {
        ScheduledTask::new(
            SchedulerKind::LocalCron,
            "daily report",
            "60s",
            Interval::from_seconds(60),
            TaskPayload::Prompt("prepare report".to_string()),
            now,
        )
    }

    fn store() -> (tempfile::TempDir, SchedulerRunStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SchedulerRunStore::new(dir.path().join("runs.json"));
        (dir, store)
    }

    #[test]
    fn begin_is_idempotent_for_the_same_occurrence() {
        let (_dir, store) = store();
        let now = Utc::now();
        let task = task(now);
        let created = store
            .begin(
                &task,
                SchedulerRunTriggerSource::Manual,
                None,
                "manual-1",
                now,
            )
            .unwrap();
        let existing = store
            .begin(
                &task,
                SchedulerRunTriggerSource::Manual,
                None,
                "manual-1",
                now,
            )
            .unwrap();
        assert!(matches!(created, SchedulerRunBegin::Created(_)));
        assert!(matches!(existing, SchedulerRunBegin::Existing(_)));
        assert_eq!(created.record().id, existing.record().id);
    }

    #[test]
    fn accepted_run_has_strict_lifecycle_and_revision_cas() {
        let (_dir, store) = store();
        let now = Utc::now();
        let task = task(now);
        let run = store
            .begin(
                &task,
                SchedulerRunTriggerSource::Manual,
                None,
                "manual-2",
                now,
            )
            .unwrap()
            .record()
            .clone();
        let receipt = SchedulerDispatchReceipt {
            run_id: run.id.clone(),
            command_id: "cmd-1".to_string(),
            accepted_at: now,
            idempotency_key: run.idempotency_key.clone(),
        };
        let queued = store
            .mark_queued(&run.id, Some(run.revision), &receipt)
            .unwrap();
        assert_eq!(queued.status, SchedulerRunStatus::Queued);
        let running = store
            .mark_running(
                &run.id,
                Some(queued.revision),
                now,
                Some("task-1".to_string()),
                Some("session-1".to_string()),
            )
            .unwrap();
        let completed = store
            .mark_completed(
                &run.id,
                Some(running.revision),
                SchedulerRunCompletion {
                    completed_at: Some(now),
                    output_summary: Some("done".to_string()),
                    ..SchedulerRunCompletion::default()
                },
            )
            .unwrap();
        assert_eq!(completed.status, SchedulerRunStatus::Completed);
        assert_eq!(completed.command_id.as_deref(), Some("cmd-1"));
        assert_eq!(completed.output_summary.as_deref(), Some("done"));

        let stale = store
            .mark_cancelled(
                &run.id,
                Some(queued.revision),
                SchedulerRunCompletion::default(),
                "stale",
            )
            .unwrap_err();
        assert!(matches!(
            stale,
            SchedulerRunStoreError::RevisionConflict { .. }
        ));
    }

    #[test]
    fn dispatch_failure_cannot_be_reported_as_execution_failure() {
        let (_dir, store) = store();
        let now = Utc::now();
        let task = task(now);
        let run = store
            .begin(
                &task,
                SchedulerRunTriggerSource::Scheduled,
                Some(task.next_run_at),
                "scheduled-1",
                now,
            )
            .unwrap()
            .record()
            .clone();
        let failed = store
            .mark_dispatch_failed(
                &run.id,
                Some(run.revision),
                SchedulerRunStatus::FailedToEnqueue,
                SchedulerRunFailure::new(
                    SchedulerRunFailureCode::DispatcherUnavailable,
                    "daemon unavailable",
                ),
                now,
            )
            .unwrap();
        assert_eq!(failed.status, SchedulerRunStatus::FailedToEnqueue);
        assert!(failed.command_id.is_none());

        let invalid = store
            .mark_running(&run.id, None, now, None, None)
            .unwrap_err();
        assert!(matches!(
            invalid,
            SchedulerRunStoreError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn history_is_bounded_and_cursor_is_filter_bound() {
        let (_dir, store) = store();
        let now = Utc::now();
        let mut first_task = task(now);
        first_task.metadata.profile_id = Some("profile-a".to_string());
        for offset in 0..3 {
            store
                .begin(
                    &first_task,
                    SchedulerRunTriggerSource::Manual,
                    None,
                    format!("manual-{offset}"),
                    now + chrono::Duration::seconds(offset),
                )
                .unwrap();
        }

        let query = SchedulerRunQuery {
            profile_id: Some("profile-a".to_string()),
            limit: Some(2),
            ..SchedulerRunQuery::default()
        };
        let first = store.query(&query).unwrap();
        assert_eq!(first.runs.len(), 2);
        assert!(first.next_cursor.is_some());

        let second = store
            .query(&SchedulerRunQuery {
                cursor: first.next_cursor.clone(),
                ..query.clone()
            })
            .unwrap();
        assert_eq!(second.runs.len(), 1);
        assert!(second.next_cursor.is_none());

        let mismatched = store.query(&SchedulerRunQuery {
            profile_id: Some("profile-b".to_string()),
            cursor: first.next_cursor,
            ..SchedulerRunQuery::default()
        });
        assert!(matches!(
            mismatched,
            Err(SchedulerRunStoreError::InvalidCursor)
        ));
    }
}
