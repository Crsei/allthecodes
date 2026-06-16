//! Jobs and cron history REST handlers.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use allthecodes_protocol::ApiError as ProtocolApiError;

static STORE_LOCK: Mutex<()> = Mutex::new(());
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobPayload {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub status: String,
    pub schedule: String,
    pub schedule_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub payload: JobPayload,
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<Value>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobRunSummary {
    pub id: String,
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_name: Option<String>,
    pub status: String,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredJobRun {
    #[serde(flatten)]
    run: JobRunSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobsListResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub jobs: Vec<JobSummary>,
    pub runs: Vec<JobRunSummary>,
}

#[derive(Debug, Serialize)]
pub struct JobMutationResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<Vec<JobSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct JobRunResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<JobSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<JobRunSummary>,
}

#[derive(Debug, Serialize)]
pub struct CronHistoryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub runs: Vec<JobRunSummary>,
}

#[derive(Debug, Deserialize)]
pub struct JobsQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CronHistoryQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JobCreateRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub schedule: String,
    pub schedule_kind: String,
    #[serde(default)]
    pub timezone: Option<String>,
    pub payload: JobPayload,
    #[serde(default)]
    pub paused: Option<bool>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct JobUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub schedule_kind: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub timezone: Option<Option<String>>,
    #[serde(default)]
    pub payload: Option<JobPayload>,
    #[serde(default)]
    pub paused: Option<bool>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub session_id: Option<Option<String>>,
    #[serde(default)]
    pub artifact_links: Option<Vec<Value>>,
    pub revision: u64,
}

#[derive(Debug, Deserialize)]
pub struct JobActionRequest {
    pub revision: u64,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct JobDeleteRequest {
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

pub async fn jobs_list_handler(Query(query): Query<JobsQuery>) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(query.profile_id);
        let jobs = filter_jobs(load_jobs()?, profile_id.as_deref());
        let runs = filter_runs(load_runs()?, profile_id.as_deref(), None);
        Ok(Json(JobsListResponse {
            profile_id,
            jobs,
            runs,
        })
        .into_response())
    })
}

pub async fn jobs_create_handler(Json(req): Json<JobCreateRequest>) -> Response {
    with_store(|| {
        let now = Utc::now();
        let job = build_job(req, now)?;
        let mut jobs = load_jobs()?;
        jobs.push(job.clone());
        sort_jobs(&mut jobs);
        save_jobs(&jobs)?;
        Ok((
            StatusCode::CREATED,
            Json(JobMutationResponse {
                job: Some(job),
                jobs: None,
                ok: Some(true),
            }),
        )
            .into_response())
    })
}

pub async fn jobs_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<JobUpdateRequest>,
) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(req.profile_id.clone());
        let mut jobs = load_jobs()?;
        let idx = find_job_index(&jobs, &id, profile_id.as_deref())?;
        check_revision(jobs[idx].revision, Some(req.revision))?;
        apply_job_update(&mut jobs[idx], req, Utc::now())?;
        sort_jobs(&mut jobs);
        let job = jobs
            .iter()
            .find(|job| job.id == id)
            .cloned()
            .ok_or_else(|| MutationError::NotFound(format!("Job '{id}' not found")))?;
        save_jobs(&jobs)?;
        Ok(Json(JobMutationResponse {
            job: Some(job),
            jobs: None,
            ok: Some(true),
        })
        .into_response())
    })
}

pub async fn jobs_delete_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<JobDeleteRequest>,
) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(req.profile_id);
        let mut jobs = load_jobs()?;
        let idx = find_job_index(&jobs, &id, profile_id.as_deref())?;
        let revision = req
            .revision
            .ok_or_else(|| MutationError::Validation("revision is required".to_string()))?;
        check_revision(jobs[idx].revision, Some(revision))?;
        jobs.remove(idx);
        sort_jobs(&mut jobs);
        save_jobs(&jobs)?;
        Ok(Json(JobMutationResponse {
            job: None,
            jobs: Some(filter_jobs(jobs, profile_id.as_deref())),
            ok: Some(true),
        })
        .into_response())
    })
}

pub async fn jobs_pause_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<JobActionRequest>,
) -> Response {
    set_job_paused(id, req, true).await
}

pub async fn jobs_resume_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<JobActionRequest>,
) -> Response {
    set_job_paused(id, req, false).await
}

pub async fn jobs_run_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<JobActionRequest>,
) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(req.profile_id);
        let mut jobs = load_jobs()?;
        let idx = find_job_index(&jobs, &id, profile_id.as_deref())?;
        check_revision(jobs[idx].revision, Some(req.revision))?;

        let started_at = Utc::now();
        let completed_at = Utc::now();
        let diagnostic = concat!(
            "Job execution backend is not wired; command, prompt, and slash-command ",
            "payloads are not executed by this MVP."
        );
        let run = JobRunSummary {
            id: new_id("run"),
            job_id: jobs[idx].id.clone(),
            job_name: Some(jobs[idx].name.clone()),
            status: "failed".to_string(),
            started_at,
            completed_at: Some(completed_at),
            duration_ms: Some((completed_at - started_at).num_milliseconds().max(0) as u64),
            session_id: jobs[idx].session_id.clone(),
            artifact_links: jobs[idx].artifact_links.clone(),
            log: Some(diagnostic.to_string()),
            output: None,
            error: Some(diagnostic.to_string()),
        };

        jobs[idx].status = "failed".to_string();
        jobs[idx].last_run_at = Some(started_at);
        jobs[idx].updated_at = Some(completed_at);
        jobs[idx].next_run_at = compute_next_run(&jobs[idx], completed_at);
        jobs[idx].revision += 1;
        let job = jobs[idx].clone();
        save_jobs(&jobs)?;
        append_run(&StoredJobRun {
            run: run.clone(),
            profile_id: job.profile_id.clone(),
        })?;

        Ok(Json(JobRunResponse {
            job: Some(job),
            run: Some(run),
        })
        .into_response())
    })
}

pub async fn cron_history_handler(Query(query): Query<CronHistoryQuery>) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(query.profile_id);
        let job_id = normalize_optional(query.job_id);
        let runs = filter_runs(load_runs()?, profile_id.as_deref(), job_id.as_deref());
        Ok(Json(CronHistoryResponse {
            profile_id,
            job_id,
            runs,
        })
        .into_response())
    })
}

async fn set_job_paused(id: String, req: JobActionRequest, paused: bool) -> Response {
    with_store(|| {
        let profile_id = normalize_optional(req.profile_id);
        let mut jobs = load_jobs()?;
        let idx = find_job_index(&jobs, &id, profile_id.as_deref())?;
        check_revision(jobs[idx].revision, Some(req.revision))?;
        jobs[idx].paused = paused;
        jobs[idx].status = if paused { "paused" } else { "idle" }.to_string();
        jobs[idx].updated_at = Some(Utc::now());
        jobs[idx].next_run_at = compute_next_run(&jobs[idx], Utc::now());
        jobs[idx].revision += 1;
        let job = jobs[idx].clone();
        sort_jobs(&mut jobs);
        save_jobs(&jobs)?;
        Ok(Json(JobMutationResponse {
            job: Some(job),
            jobs: None,
            ok: Some(true),
        })
        .into_response())
    })
}

fn with_store<F>(f: F) -> Response
where
    F: FnOnce() -> Result<Response, MutationError>,
{
    let _guard = match STORE_LOCK.lock() {
        Ok(guard) => guard,
        Err(err) => {
            return internal_error(format!("Job store lock poisoned: {err}"));
        }
    };
    match f() {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

fn build_job(req: JobCreateRequest, now: DateTime<Utc>) -> Result<JobSummary, MutationError> {
    let name = required_trimmed("name", req.name)?;
    let schedule = required_trimmed("schedule", req.schedule)?;
    let schedule_kind = normalize_schedule_kind(req.schedule_kind)?;
    let payload = normalize_payload(req.payload)?;
    let paused = req.paused.unwrap_or(false);
    let mut job = JobSummary {
        id: new_id("job"),
        name,
        description: normalize_optional(req.description),
        status: if paused { "paused" } else { "idle" }.to_string(),
        schedule,
        schedule_kind,
        timezone: normalize_optional(req.timezone),
        payload,
        paused,
        profile_id: normalize_optional(req.profile_id),
        session_id: normalize_optional(req.session_id),
        artifact_links: req.artifact_links,
        revision: 1,
        created_at: Some(now),
        updated_at: Some(now),
        last_run_at: None,
        next_run_at: None,
    };
    job.next_run_at = compute_next_run(&job, now);
    Ok(job)
}

fn apply_job_update(
    job: &mut JobSummary,
    req: JobUpdateRequest,
    now: DateTime<Utc>,
) -> Result<(), MutationError> {
    if let Some(name) = req.name {
        job.name = required_trimmed("name", name)?;
    }
    if let Some(description) = req.description {
        job.description = normalize_optional(description);
    }
    if let Some(schedule) = req.schedule {
        job.schedule = required_trimmed("schedule", schedule)?;
    }
    if let Some(schedule_kind) = req.schedule_kind {
        job.schedule_kind = normalize_schedule_kind(schedule_kind)?;
    }
    if let Some(timezone) = req.timezone {
        job.timezone = normalize_optional(timezone);
    }
    if let Some(payload) = req.payload {
        job.payload = normalize_payload(payload)?;
    }
    if let Some(paused) = req.paused {
        job.paused = paused;
        job.status = if paused { "paused" } else { "idle" }.to_string();
    }
    if let Some(session_id) = req.session_id {
        job.session_id = normalize_optional(session_id);
    }
    if let Some(artifact_links) = req.artifact_links {
        job.artifact_links = artifact_links;
    }
    job.updated_at = Some(now);
    job.next_run_at = compute_next_run(job, now);
    job.revision += 1;
    Ok(())
}

fn find_job_index(
    jobs: &[JobSummary],
    id: &str,
    profile_id: Option<&str>,
) -> Result<usize, MutationError> {
    jobs.iter()
        .position(|job| job.id == id && profile_matches(job.profile_id.as_deref(), profile_id))
        .ok_or_else(|| MutationError::NotFound(format!("Job '{id}' not found")))
}

fn check_revision(current: u64, expected: Option<u64>) -> Result<(), MutationError> {
    if let Some(expected) = expected {
        if current != expected {
            return Err(MutationError::RevisionConflict(format!(
                "Job revision conflict: expected {expected}, current {current}"
            )));
        }
    }
    Ok(())
}

fn normalize_schedule_kind(value: String) -> Result<String, MutationError> {
    let value = required_trimmed("schedule_kind", value)?;
    match value.as_str() {
        "manual" | "interval" | "cron" => Ok(value),
        _ => Err(MutationError::Validation(
            "schedule_kind must be manual, interval, or cron".to_string(),
        )),
    }
}

fn normalize_payload(payload: JobPayload) -> Result<JobPayload, MutationError> {
    let kind = required_trimmed("payload.kind", payload.kind)?;
    match kind.as_str() {
        "prompt" | "slash_command" | "command" => {}
        _ => {
            return Err(MutationError::Validation(
                "payload.kind must be prompt, slash_command, or command".to_string(),
            ))
        }
    }
    Ok(JobPayload {
        kind,
        value: required_trimmed("payload.value", payload.value)?,
    })
}

fn required_trimmed(field: &str, value: String) -> Result<String, MutationError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        Err(MutationError::Validation(format!("{field} is required")))
    } else {
        Ok(value)
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn compute_next_run(job: &JobSummary, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if job.paused || job.schedule_kind != "interval" {
        return None;
    }
    parse_interval(&job.schedule).map(|duration| from + duration)
}

fn parse_interval(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let split_at = value
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(idx, _)| idx)
        .unwrap_or(value.len());
    let amount = value[..split_at].parse::<i64>().ok()?;
    if amount <= 0 {
        return None;
    }
    match value[split_at..].trim() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => Some(Duration::seconds(amount)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(amount)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(amount)),
        "d" | "day" | "days" => Some(Duration::days(amount)),
        _ => None,
    }
}

fn filter_jobs(mut jobs: Vec<JobSummary>, profile_id: Option<&str>) -> Vec<JobSummary> {
    jobs.retain(|job| profile_matches(job.profile_id.as_deref(), profile_id));
    sort_jobs(&mut jobs);
    jobs
}

fn filter_runs(
    runs: Vec<StoredJobRun>,
    profile_id: Option<&str>,
    job_id: Option<&str>,
) -> Vec<JobRunSummary> {
    let mut runs: Vec<JobRunSummary> = runs
        .into_iter()
        .filter(|record| profile_matches(record.profile_id.as_deref(), profile_id))
        .filter(|record| job_id.is_none_or(|job_id| record.run.job_id == job_id))
        .map(|record| record.run)
        .collect();
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    runs
}

fn profile_matches(job_profile: Option<&str>, query_profile: Option<&str>) -> bool {
    query_profile.is_none_or(|profile_id| job_profile == Some(profile_id))
}

fn sort_jobs(jobs: &mut [JobSummary]) {
    jobs.sort_by(|a, b| {
        a.paused
            .cmp(&b.paused)
            .then_with(|| a.next_run_at.cmp(&b.next_run_at))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn load_jobs() -> Result<Vec<JobSummary>, MutationError> {
    let path = jobs_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = fs::read(&path).map_err(|err| {
        MutationError::Internal(format!(
            "Failed to read jobs store {}: {err}",
            path.display()
        ))
    })?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&bytes).map_err(|err| {
        MutationError::Internal(format!(
            "Failed to parse jobs store {}: {err}",
            path.display()
        ))
    })
}

fn save_jobs(jobs: &[JobSummary]) -> Result<(), MutationError> {
    let path = jobs_path();
    ensure_store_dir()?;
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(jobs)
        .map_err(|err| MutationError::Internal(format!("Failed to encode jobs store: {err}")))?;
    fs::write(&tmp_path, bytes).map_err(|err| {
        MutationError::Internal(format!(
            "Failed to write jobs store {}: {err}",
            tmp_path.display()
        ))
    })?;
    fs::rename(&tmp_path, &path).map_err(|err| {
        MutationError::Internal(format!(
            "Failed to replace jobs store {}: {err}",
            path.display()
        ))
    })
}

fn load_runs() -> Result<Vec<StoredJobRun>, MutationError> {
    let path = runs_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|err| {
        MutationError::Internal(format!(
            "Failed to read job runs store {}: {err}",
            path.display()
        ))
    })?;
    let mut runs = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let run = serde_json::from_str(line).map_err(|err| {
            MutationError::Internal(format!(
                "Failed to parse job run at line {} in {}: {err}",
                idx + 1,
                path.display()
            ))
        })?;
        runs.push(run);
    }
    Ok(runs)
}

fn append_run(run: &StoredJobRun) -> Result<(), MutationError> {
    ensure_store_dir()?;
    let path = runs_path();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| {
            MutationError::Internal(format!(
                "Failed to open job runs store {}: {err}",
                path.display()
            ))
        })?;
    let line = serde_json::to_string(run)
        .map_err(|err| MutationError::Internal(format!("Failed to encode job run: {err}")))?;
    writeln!(file, "{line}").map_err(|err| {
        MutationError::Internal(format!(
            "Failed to append job runs store {}: {err}",
            path.display()
        ))
    })
}

fn ensure_store_dir() -> Result<(), MutationError> {
    fs::create_dir_all(store_dir()).map_err(|err| {
        MutationError::Internal(format!("Failed to create jobs store directory: {err}"))
    })
}

fn store_dir() -> PathBuf {
    allthecodes_config::paths::data_root().join("web")
}

fn jobs_path() -> PathBuf {
    store_dir().join("jobs.json")
}

fn runs_path() -> PathBuf {
    store_dir().join("job-runs.jsonl")
}

fn new_id(prefix: &str) -> String {
    let now = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{now}-{}-{counter}", std::process::id())
}

#[derive(Debug)]
enum MutationError {
    Validation(String),
    NotFound(String),
    RevisionConflict(String),
    Internal(String),
}

impl MutationError {
    fn into_response(self) -> Response {
        match self {
            MutationError::Validation(error) => validation_error(error),
            MutationError::NotFound(error) => not_found(error),
            MutationError::RevisionConflict(error) => revision_conflict(error),
            MutationError::Internal(error) => internal_error(error),
        }
    }
}

fn validation_error(error: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "validation_error",
        message: error,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "job",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn revision_conflict(error: String) -> Response {
    let body = ProtocolApiError::Conflict { reason: error }.into_body();
    (StatusCode::CONFLICT, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use serde_json::json;
    use serial_test::serial;

    fn create_request() -> JobCreateRequest {
        JobCreateRequest {
            name: "nightly".to_string(),
            description: Some("test job".to_string()),
            schedule: "15m".to_string(),
            schedule_kind: "interval".to_string(),
            timezone: None,
            payload: JobPayload {
                kind: "prompt".to_string(),
                value: "summarize".to_string(),
            },
            paused: Some(false),
            profile_id: Some("profile-a".to_string()),
            session_id: None,
            artifact_links: Vec::new(),
        }
    }

    #[tokio::test]
    #[serial]
    async fn create_and_list_jobs_echoes_profile_id() {
        let (_temp, _guard) = temp_home();

        let response = jobs_create_handler(Json(create_request())).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response_json(response).await;
        assert_eq!(body["job"]["name"], json!("nightly"));
        assert_eq!(body["job"]["profile_id"], json!("profile-a"));
        assert_eq!(body["job"]["revision"], json!(1));

        let response = jobs_list_handler(Query(JobsQuery {
            profile_id: Some("profile-a".to_string()),
        }))
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("profile-a"));
        assert_eq!(body["jobs"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    #[serial]
    async fn stale_revision_returns_conflict() {
        let (_temp, _guard) = temp_home();

        let created = jobs_create_handler(Json(create_request())).await;
        let created = response_json(created).await;
        let id = created["job"]["id"].as_str().unwrap().to_string();

        let response = jobs_pause_handler(
            AxumPath(id),
            Json(JobActionRequest {
                revision: 999,
                profile_id: Some("profile-a".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("conflict"));
    }

    #[tokio::test]
    #[serial]
    async fn run_records_failed_backend_diagnostic() {
        let (_temp, _guard) = temp_home();

        let created = jobs_create_handler(Json(create_request())).await;
        let created = response_json(created).await;
        let id = created["job"]["id"].as_str().unwrap().to_string();
        let revision = created["job"]["revision"].as_u64().unwrap();

        let response = jobs_run_handler(
            AxumPath(id.clone()),
            Json(JobActionRequest {
                revision,
                profile_id: Some("profile-a".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["job"]["status"], json!("failed"));
        assert_eq!(body["run"]["status"], json!("failed"));
        assert!(body["run"]["error"]
            .as_str()
            .unwrap()
            .contains("execution backend is not wired"));

        let response = cron_history_handler(Query(CronHistoryQuery {
            profile_id: Some("profile-a".to_string()),
            job_id: Some(id),
        }))
        .await;
        let body = response_json(response).await;
        assert_eq!(body["runs"].as_array().unwrap().len(), 1);
    }
}
