//! One-time migration from the retired Web and `allthecodes-tasks` schedulers.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::service::build_task;
use super::{
    LegacySchedulerRunInput, ScheduleKind, ScheduledTask, ScheduledTaskMetadata, SchedulerError,
    SchedulerRunBegin, SchedulerRunFailure, SchedulerRunFailureCode, SchedulerRunStatus,
    SchedulerRunStore, SchedulerRunStoreError, SchedulerService, SchedulerStore, TaskId,
    TaskPayload,
};

const MIGRATION_VERSION: u32 = 1;
const MAX_JOBS_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RUNS_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TASKS_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LEGACY_ROWS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerMigrationReport {
    pub version: u32,
    pub completed_at: DateTime<Utc>,
    pub imported_web_jobs: usize,
    pub imported_task_definitions: usize,
    pub imported_web_runs: usize,
    pub skipped_existing_definitions: usize,
    pub skipped_existing_runs: usize,
    pub mappings: Vec<SchedulerMigrationMapping>,
    pub quarantined: Vec<SchedulerMigrationQuarantine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerMigrationMapping {
    pub source: String,
    pub legacy_id: String,
    pub canonical_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerMigrationQuarantine {
    pub source: String,
    pub legacy_id: String,
    pub reason: String,
    pub record: Value,
}

#[derive(Debug, Error)]
pub enum SchedulerMigrationError {
    #[error("I/O error touching {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode legacy scheduler data {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("legacy scheduler source {path} exceeds its {limit}-byte safety limit")]
    SourceTooLarge { path: PathBuf, limit: u64 },
    #[error("legacy scheduler source {path} exceeds the {limit}-row safety limit")]
    TooManyRows { path: PathBuf, limit: usize },
    #[error("could not acquire scheduler migration lock at {path} within {}ms", timeout.as_millis())]
    LockTimeout { path: PathBuf, timeout: Duration },
    #[error(transparent)]
    Definitions(#[from] SchedulerError),
    #[error(transparent)]
    Runs(#[from] SchedulerRunStoreError),
    #[error("failed to encode scheduler migration report: {0}")]
    Encode(#[source] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyWebJob {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    schedule: String,
    schedule_kind: String,
    #[serde(default)]
    timezone: Option<String>,
    payload: LegacyWebPayload,
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    artifact_links: Vec<Value>,
    #[serde(default = "initial_revision")]
    revision: u64,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    last_run_at: Option<DateTime<Utc>>,
    #[serde(default)]
    next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyWebPayload {
    kind: String,
    value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyStoredRun {
    #[serde(flatten)]
    run: LegacyWebRun,
    #[serde(default)]
    profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyWebRun {
    id: String,
    job_id: String,
    #[serde(default)]
    job_name: Option<String>,
    status: String,
    started_at: DateTime<Utc>,
    #[serde(default)]
    completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    artifact_links: Vec<Value>,
    #[serde(default)]
    log: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTasksFile {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(default)]
    tasks: Vec<Value>,
}

// Keep the retired `allthecodes-tasks` file shape local to the migration. The
// scheduler service only needs to decode this one legacy document and should
// not pull the old task store (or its storage backend features) into the
// canonical scheduler domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScheduledAgentTask {
    id: String,
    prompt: String,
    cwd: String,
    schedule: ScheduleSpec,
    enabled: bool,
    #[serde(default)]
    last_run_at: Option<String>,
    #[serde(default)]
    next_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ScheduleSpec {
    Interval { every_seconds: u64 },
    Once { run_at: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TypedArtifactLink {
    #[serde(default)]
    id: Option<String>,
    label: String,
    href: String,
    #[serde(default)]
    kind: Option<String>,
}

const fn initial_revision() -> u64 {
    1
}

pub fn migrate_default_scheduler_data(
    service: &SchedulerService,
) -> Result<SchedulerMigrationReport, SchedulerMigrationError> {
    migrate_legacy_scheduler_data(
        &allthecodes_config::paths::data_root(),
        service.definitions(),
        service.runs(),
    )
}

pub fn migrate_legacy_scheduler_data(
    data_root: &Path,
    definitions: &SchedulerStore,
    runs: &SchedulerRunStore,
) -> Result<SchedulerMigrationReport, SchedulerMigrationError> {
    let migration_dir = data_root.join("migration");
    let report_path = migration_dir.join("scheduler-v1-report.json");
    if report_path.is_file() {
        return read_report(&report_path);
    }
    fs::create_dir_all(&migration_dir).map_err(|source| SchedulerMigrationError::Io {
        path: migration_dir.clone(),
        source,
    })?;
    let lock_path = migration_dir.join("scheduler-v1.lock");
    let _guard = acquire_lock(&lock_path)?;
    if report_path.is_file() {
        return read_report(&report_path);
    }

    let now = Utc::now();
    let mut report = SchedulerMigrationReport {
        version: MIGRATION_VERSION,
        completed_at: now,
        imported_web_jobs: 0,
        imported_task_definitions: 0,
        imported_web_runs: 0,
        skipped_existing_definitions: 0,
        skipped_existing_runs: 0,
        mappings: Vec::new(),
        quarantined: Vec::new(),
    };

    migrate_web_jobs(data_root, definitions, now, &mut report)?;
    migrate_task_definitions(data_root, definitions, now, &mut report)?;
    migrate_web_runs(data_root, definitions, runs, &mut report)?;
    report.completed_at = Utc::now();
    write_report(&report_path, &report)?;
    Ok(report)
}

fn migrate_web_jobs(
    data_root: &Path,
    definitions: &SchedulerStore,
    now: DateTime<Utc>,
    report: &mut SchedulerMigrationReport,
) -> Result<(), SchedulerMigrationError> {
    let path = data_root.join("web").join("jobs.json");
    let Some(value) = read_optional_json(&path, MAX_JOBS_FILE_BYTES)? else {
        return Ok(());
    };
    let jobs: Vec<Value> =
        serde_json::from_value(value).map_err(|source| SchedulerMigrationError::Decode {
            path: path.clone(),
            source,
        })?;
    ensure_row_limit(&path, jobs.len())?;

    for (index, raw) in jobs.into_iter().enumerate() {
        let legacy_id = legacy_row_id(&raw, index);
        let job: LegacyWebJob = match serde_json::from_value(raw.clone()) {
            Ok(job) => job,
            Err(error) => {
                quarantine(
                    report,
                    "web_jobs",
                    legacy_id,
                    format!("failed to decode legacy Web job: {error}"),
                    raw,
                );
                continue;
            }
        };
        let legacy_id = job.id.clone();
        let task = match web_job_to_task(job, now) {
            Ok(task) => task,
            Err(reason) => {
                quarantine(report, "web_jobs", legacy_id, reason, raw);
                continue;
            }
        };
        import_definition(definitions, task, "web_jobs", legacy_id, report, |report| {
            report.imported_web_jobs += 1
        })?;
    }
    Ok(())
}

fn migrate_task_definitions(
    data_root: &Path,
    definitions: &SchedulerStore,
    now: DateTime<Utc>,
    report: &mut SchedulerMigrationReport,
) -> Result<(), SchedulerMigrationError> {
    let path = data_root.join("scheduled_tasks").join("tasks.json");
    let Some(value) = read_optional_json(&path, MAX_TASKS_FILE_BYTES)? else {
        return Ok(());
    };
    let file: LegacyTasksFile =
        serde_json::from_value(value).map_err(|source| SchedulerMigrationError::Decode {
            path: path.clone(),
            source,
        })?;
    ensure_row_limit(&path, file.tasks.len())?;

    for (index, raw) in file.tasks.into_iter().enumerate() {
        let legacy_id = legacy_row_id(&raw, index);
        let legacy: ScheduledAgentTask = match serde_json::from_value(raw.clone()) {
            Ok(legacy) => legacy,
            Err(error) => {
                quarantine(
                    report,
                    "allthecodes_tasks",
                    legacy_id,
                    format!("failed to decode legacy scheduled task: {error}"),
                    raw,
                );
                continue;
            }
        };
        let legacy_id = legacy.id.clone();
        let task = match old_task_to_task(legacy, now) {
            Ok(task) => task,
            Err(reason) => {
                quarantine(report, "allthecodes_tasks", legacy_id, reason, raw);
                continue;
            }
        };
        import_definition(
            definitions,
            task,
            "allthecodes_tasks",
            legacy_id,
            report,
            |report| report.imported_task_definitions += 1,
        )?;
    }
    Ok(())
}

fn migrate_web_runs(
    data_root: &Path,
    definitions: &SchedulerStore,
    runs: &SchedulerRunStore,
    report: &mut SchedulerMigrationReport,
) -> Result<(), SchedulerMigrationError> {
    let path = data_root.join("web").join("job-runs.jsonl");
    let Some(content) = read_optional_text(&path, MAX_RUNS_FILE_BYTES)? else {
        return Ok(());
    };
    let lines: Vec<_> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    ensure_row_limit(&path, lines.len())?;
    for (index, line) in lines.into_iter().enumerate() {
        let raw: Value =
            serde_json::from_str(line).map_err(|source| SchedulerMigrationError::Decode {
                path: path.clone(),
                source,
            })?;
        let legacy: LegacyStoredRun = match serde_json::from_value(raw.clone()) {
            Ok(legacy) => legacy,
            Err(error) => {
                quarantine(
                    report,
                    "web_job_runs",
                    format!("line-{}", index + 1),
                    error.to_string(),
                    raw,
                );
                continue;
            }
        };
        let legacy_id = legacy.run.id.clone();
        let task = match definitions.get(&TaskId(legacy.run.job_id.clone())) {
            Ok(task) => task,
            Err(SchedulerError::NotFound(_)) => {
                quarantine(
                    report,
                    "web_job_runs",
                    legacy_id,
                    "referenced job was not imported".to_string(),
                    raw,
                );
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let input = match legacy_run_input(legacy, task) {
            Ok(input) => input,
            Err(reason) => {
                quarantine(report, "web_job_runs", legacy_id, reason, raw);
                continue;
            }
        };
        match runs.import_legacy(input)? {
            SchedulerRunBegin::Created(run) => {
                report.imported_web_runs += 1;
                report.mappings.push(SchedulerMigrationMapping {
                    source: "web_job_runs".to_string(),
                    legacy_id,
                    canonical_id: run.id.to_string(),
                });
            }
            SchedulerRunBegin::Existing(run) => {
                report.skipped_existing_runs += 1;
                report.mappings.push(SchedulerMigrationMapping {
                    source: "web_job_runs".to_string(),
                    legacy_id,
                    canonical_id: run.id.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn import_definition<F>(
    definitions: &SchedulerStore,
    task: ScheduledTask,
    source: &str,
    legacy_id: String,
    report: &mut SchedulerMigrationReport,
    count_import: F,
) -> Result<(), SchedulerMigrationError>
where
    F: FnOnce(&mut SchedulerMigrationReport),
{
    let canonical_id = task.id.to_string();
    match definitions.get(&task.id) {
        Ok(_) => report.skipped_existing_definitions += 1,
        Err(SchedulerError::NotFound(_)) => match definitions.add(task) {
            Ok(_) => count_import(report),
            Err(SchedulerError::AlreadyExists(_)) => report.skipped_existing_definitions += 1,
            Err(error) => return Err(error.into()),
        },
        Err(error) => return Err(error.into()),
    }
    report.mappings.push(SchedulerMigrationMapping {
        source: source.to_string(),
        legacy_id,
        canonical_id,
    });
    Ok(())
}

fn web_job_to_task(job: LegacyWebJob, now: DateTime<Utc>) -> Result<ScheduledTask, String> {
    validate_legacy_id(&job.id)?;
    let schedule_kind = match job.schedule_kind.as_str() {
        "interval" => ScheduleKind::Interval,
        "cron" => ScheduleKind::Cron,
        "manual" => return Err("manual-only jobs have no canonical recurring schedule".to_string()),
        other => return Err(format!("unsupported schedule kind '{other}'")),
    };
    let payload = match job.payload.kind.as_str() {
        "prompt" => TaskPayload::Prompt(job.payload.value),
        "slash_command" => TaskPayload::SlashCommand(job.payload.value),
        "command" => return Err("raw command jobs are quarantined and never enabled".to_string()),
        other => return Err(format!("unsupported payload kind '{other}'")),
    };
    validate_legacy_artifacts(&job.artifact_links)?;
    let created_at = job.created_at.unwrap_or(now);
    let mut task = build_task(
        super::SchedulerDefinitionInput {
            name: job.name,
            schedule: job.schedule,
            schedule_kind,
            timezone: job.timezone,
            payload,
            paused: job.paused,
            metadata: ScheduledTaskMetadata {
                description: job.description,
                profile_id: job.profile_id,
                session_id: job.session_id,
                working_directory: None,
                artifact_links: job.artifact_links,
            },
        },
        created_at,
    )
    .map_err(|error| error.to_string())?;
    task.id = TaskId(job.id);
    task.revision = job.revision.max(1);
    task.created_at = created_at;
    task.updated_at = job.updated_at;
    task.last_run_at = job.last_run_at;
    if let Some(next_run_at) = job.next_run_at {
        task.next_run_at = next_run_at;
    }
    Ok(task)
}

fn old_task_to_task(
    legacy: ScheduledAgentTask,
    now: DateTime<Utc>,
) -> Result<ScheduledTask, String> {
    validate_legacy_id(&legacy.id)?;
    let every_seconds = match legacy.schedule {
        ScheduleSpec::Interval { every_seconds } => every_seconds,
        ScheduleSpec::Once { .. } => {
            return Err("one-shot scheduled tasks require an explicit future API".to_string())
        }
    };
    let name = legacy
        .prompt
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(legacy.id.as_str())
        .chars()
        .take(120)
        .collect::<String>();
    let mut task = build_task(
        super::SchedulerDefinitionInput {
            name,
            schedule: format!("{every_seconds}s"),
            schedule_kind: ScheduleKind::Interval,
            timezone: None,
            payload: TaskPayload::from_user_input(&legacy.prompt),
            paused: !legacy.enabled,
            metadata: ScheduledTaskMetadata {
                working_directory: Some(legacy.cwd),
                ..ScheduledTaskMetadata::default()
            },
        },
        now,
    )
    .map_err(|error| error.to_string())?;
    task.id = TaskId(legacy.id);
    task.last_run_at = legacy
        .last_run_at
        .as_deref()
        .map(parse_datetime)
        .transpose()?;
    if let Some(next_run_at) = legacy.next_run_at.as_deref() {
        task.next_run_at = parse_datetime(next_run_at)?;
    }
    Ok(task)
}

fn legacy_run_input(
    legacy: LegacyStoredRun,
    mut task: ScheduledTask,
) -> Result<LegacySchedulerRunInput, String> {
    validate_legacy_id(&legacy.run.id)?;
    validate_legacy_artifacts(&legacy.run.artifact_links)?;
    if task.metadata.profile_id.is_none() {
        task.metadata.profile_id = legacy.profile_id;
    }
    let diagnostic = legacy.run.error.clone().or(legacy.run.log.clone());
    let (status, failure) = match legacy.run.status.as_str() {
        "completed" | "succeeded" => (SchedulerRunStatus::Completed, None),
        "cancelled" | "canceled" => (
            SchedulerRunStatus::Cancelled,
            Some(SchedulerRunFailure::new(
                SchedulerRunFailureCode::Cancelled,
                diagnostic.as_deref().unwrap_or("legacy run was cancelled"),
            )),
        ),
        "rejected" => (
            SchedulerRunStatus::Rejected,
            Some(SchedulerRunFailure::new(
                SchedulerRunFailureCode::DispatchRejected,
                diagnostic.as_deref().unwrap_or("legacy run was rejected"),
            )),
        ),
        "failed"
            if diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("backend is not wired")) =>
        {
            (
                SchedulerRunStatus::FailedToEnqueue,
                Some(SchedulerRunFailure::new(
                    SchedulerRunFailureCode::EnqueueFailed,
                    diagnostic
                        .as_deref()
                        .unwrap_or("legacy Web handler did not enqueue work"),
                )),
            )
        }
        "failed" => (
            SchedulerRunStatus::Failed,
            Some(SchedulerRunFailure::new(
                SchedulerRunFailureCode::ExecutionFailed,
                diagnostic.as_deref().unwrap_or("legacy run failed"),
            )),
        ),
        other => return Err(format!("unsupported legacy run status '{other}'")),
    };
    Ok(LegacySchedulerRunInput {
        source_id: legacy.run.id,
        task,
        status,
        requested_at: legacy.run.started_at,
        completed_at: legacy.run.completed_at,
        session_id: legacy.run.session_id,
        artifact_links: legacy.run.artifact_links,
        output_summary: legacy.run.output,
        failure,
    })
}

fn validate_legacy_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Err("legacy id is empty, too long, or contains unsafe characters".to_string())
    } else {
        Ok(())
    }
}

fn legacy_row_id(record: &Value, index: usize) -> String {
    record
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("row-{}", index + 1))
}

fn validate_legacy_artifacts(artifacts: &[Value]) -> Result<(), String> {
    for artifact in artifacts {
        let typed: TypedArtifactLink =
            serde_json::from_value(artifact.clone()).map_err(|error| error.to_string())?;
        if typed.label.trim().is_empty() || typed.href.trim().is_empty() {
            return Err("artifact links require non-empty label and href".to_string());
        }
        let _ = (&typed.id, &typed.kind);
    }
    Ok(())
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| error.to_string())
}

fn quarantine(
    report: &mut SchedulerMigrationReport,
    source: &str,
    legacy_id: String,
    reason: String,
    record: Value,
) {
    report.quarantined.push(SchedulerMigrationQuarantine {
        source: source.to_string(),
        legacy_id,
        reason,
        record,
    });
}

fn ensure_row_limit(path: &Path, rows: usize) -> Result<(), SchedulerMigrationError> {
    if rows > MAX_LEGACY_ROWS {
        Err(SchedulerMigrationError::TooManyRows {
            path: path.to_path_buf(),
            limit: MAX_LEGACY_ROWS,
        })
    } else {
        Ok(())
    }
}

fn read_optional_json(path: &Path, limit: u64) -> Result<Option<Value>, SchedulerMigrationError> {
    let Some(content) = read_optional_text(path, limit)? else {
        return Ok(None);
    };
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|source| SchedulerMigrationError::Decode {
            path: path.to_path_buf(),
            source,
        })
}

fn read_optional_text(path: &Path, limit: u64) -> Result<Option<String>, SchedulerMigrationError> {
    if !path.is_file() {
        return Ok(None);
    }
    let metadata = fs::metadata(path).map_err(|source| SchedulerMigrationError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > limit {
        return Err(SchedulerMigrationError::SourceTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }
    let mut file = File::open(path).map_err(|source| SchedulerMigrationError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|source| SchedulerMigrationError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if content.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

fn read_report(path: &Path) -> Result<SchedulerMigrationReport, SchedulerMigrationError> {
    let content = read_optional_text(path, MAX_JOBS_FILE_BYTES)?.ok_or_else(|| {
        SchedulerMigrationError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::UnexpectedEof, "migration report is empty"),
        }
    })?;
    serde_json::from_str(&content).map_err(|source| SchedulerMigrationError::Decode {
        path: path.to_path_buf(),
        source,
    })
}

fn write_report(
    path: &Path,
    report: &SchedulerMigrationReport,
) -> Result<(), SchedulerMigrationError> {
    let bytes = serde_json::to_vec_pretty(report).map_err(SchedulerMigrationError::Encode)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = File::create(&tmp).map_err(|source| SchedulerMigrationError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.write_all(&bytes)
            .map_err(|source| SchedulerMigrationError::Io {
                path: tmp.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| SchedulerMigrationError::Io {
                path: tmp.clone(),
                source,
            })?;
    }
    fs::rename(&tmp, path).map_err(|source| SchedulerMigrationError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(parent) = path.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn acquire_lock(path: &Path) -> Result<MigrationLockGuard<'_>, SchedulerMigrationError> {
    let timeout = Duration::from_secs(5);
    let started = Instant::now();
    loop {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => {
                return Ok(MigrationLockGuard {
                    file: Some(file),
                    path,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if started.elapsed() >= timeout {
                    return Err(SchedulerMigrationError::LockTimeout {
                        path: path.to_path_buf(),
                        timeout,
                    });
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(source) => {
                return Err(SchedulerMigrationError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

struct MigrationLockGuard<'a> {
    file: Option<File>,
    path: &'a Path,
}

impl Drop for MigrationLockGuard<'_> {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn migrates_supported_rows_quarantines_unsafe_rows_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let web = dir.path().join("web");
        let old_tasks = dir.path().join("scheduled_tasks");
        fs::create_dir_all(&web).unwrap();
        fs::create_dir_all(&old_tasks).unwrap();
        fs::write(
            web.join("jobs.json"),
            serde_json::to_vec_pretty(&serde_json::json!([
                {
                    "id": "legacy-web",
                    "name": "legacy web",
                    "status": "idle",
                    "schedule": "5m",
                    "schedule_kind": "interval",
                    "payload": {"kind": "prompt", "value": "review"},
                    "paused": false,
                    "revision": 3
                },
                {
                    "id": "unsafe-command",
                    "name": "unsafe",
                    "status": "idle",
                    "schedule": "5m",
                    "schedule_kind": "interval",
                    "payload": {"kind": "command", "value": "echo unsafe"},
                    "paused": false,
                    "revision": 1
                },
                {
                    "id": "malformed-web"
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            old_tasks.join("tasks.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "tasks": [{
                    "id": "legacy-task",
                    "prompt": "summarize workspace",
                    "cwd": "/repo",
                    "schedule": {"type": "interval", "every_seconds": 60},
                    "enabled": true
                }, {
                    "id": "legacy-once",
                    "prompt": "one shot",
                    "cwd": "/repo",
                    "schedule": {"type": "once", "run_at": "2026-07-20T00:00:00Z"},
                    "enabled": true
                }, {
                    "id": "malformed-task",
                    "prompt": 42
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            web.join("job-runs.jsonl"),
            serde_json::to_string(&serde_json::json!({
                "id": "legacy-run",
                "job_id": "legacy-web",
                "job_name": "legacy web",
                "status": "failed",
                "started_at": "2026-07-16T00:00:00Z",
                "completed_at": "2026-07-16T00:00:01Z",
                "artifact_links": [],
                "error": "Job execution backend is not wired"
            }))
            .unwrap(),
        )
        .unwrap();

        let definitions = Arc::new(SchedulerStore::new(dir.path().join("definitions.json")));
        let runs = Arc::new(SchedulerRunStore::new(dir.path().join("runs.json")));
        let first = migrate_legacy_scheduler_data(dir.path(), &definitions, &runs).unwrap();
        assert_eq!(first.imported_web_jobs, 1);
        assert_eq!(first.imported_task_definitions, 1);
        assert_eq!(first.imported_web_runs, 1);
        assert_eq!(first.quarantined.len(), 4);
        assert_eq!(
            definitions
                .get(&TaskId("legacy-web".to_string()))
                .unwrap()
                .revision,
            3
        );
        assert_eq!(
            definitions
                .get(&TaskId("legacy-task".to_string()))
                .unwrap()
                .metadata
                .working_directory
                .as_deref(),
            Some("/repo")
        );
        let history = runs
            .query(&super::super::SchedulerRunQuery::default())
            .unwrap();
        assert_eq!(history.runs[0].status, SchedulerRunStatus::FailedToEnqueue);
        assert!(history.runs[0].legacy);

        let second = migrate_legacy_scheduler_data(dir.path(), &definitions, &runs).unwrap();
        assert_eq!(second, first);
        assert_eq!(definitions.load().unwrap().len(), 2);
        assert_eq!(
            runs.query(&super::super::SchedulerRunQuery::default())
                .unwrap()
                .runs
                .len(),
            1
        );
    }
}
