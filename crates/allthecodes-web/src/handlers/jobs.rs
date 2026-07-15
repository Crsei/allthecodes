//! Jobs and cron history REST handlers backed by the canonical scheduler.

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use allthecodes_protocol::v1::jobs::{
    CronHistoryQuery, CronHistoryResponse, JobActionParams, JobActionRequest, JobArtifactLink,
    JobCreateRequest, JobDefinitionStatus, JobDeleteParams, JobDeleteRequest, JobDispatchReceipt,
    JobMutationResponse, JobPayload, JobPayloadKind, JobRunFailure, JobRunFailureCode,
    JobRunParams, JobRunRequest, JobRunResponse, JobRunStatus, JobRunSummary, JobRunTriggerSource,
    JobScheduleKind, JobSummary, JobUpdateParams, JobUpdateRequest, JobsListResponse, JobsQuery,
};
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod, SerializationScope};
use allthecodes_services::scheduler::{
    migrate_default_scheduler_data, AcceptedSchedulerRun, ScheduleKind, ScheduledTask,
    ScheduledTaskMetadata, SchedulerError, SchedulerRunFailureCode, SchedulerRunQuery,
    SchedulerRunRecord, SchedulerRunStatus, SchedulerRunStoreError, SchedulerRunTriggerSource,
    SchedulerService, SchedulerServiceError, TaskId, TaskPayload,
};

use crate::api_dispatcher::rest_processor_response;
use crate::processors::Processor;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processor implementations shared by REST and API-RPC.
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct JobsListProcessor {
    state: WebState,
}

impl From<WebState> for JobsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsListProcessor {
    type Request = JobsQuery;
    type Response = JobsListResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.list"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_list_with_service(&state_service(&self.state)?, query)
    }
}

#[derive(Clone)]
pub struct JobsCreateProcessor {
    state: WebState,
}

impl From<WebState> for JobsCreateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsCreateProcessor {
    type Request = JobCreateRequest;
    type Response = JobMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.create"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(_params: &Self::Request) -> SerializationScope {
        SerializationScope::PerProcess
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_create_with_service(&state_service(&self.state)?, request)
    }
}

#[derive(Clone)]
pub struct JobsUpdateProcessor {
    state: WebState,
}

impl From<WebState> for JobsUpdateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsUpdateProcessor {
    type Request = JobUpdateParams;
    type Response = JobMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.update"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_update_with_service(&state_service(&self.state)?, params.id, params.request)
    }
}

#[derive(Clone)]
pub struct JobsDeleteProcessor {
    state: WebState,
}

impl From<WebState> for JobsDeleteProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsDeleteProcessor {
    type Request = JobDeleteParams;
    type Response = JobMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.delete"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_delete_with_service(&state_service(&self.state)?, params.id, params.request)
    }
}

#[derive(Clone)]
pub struct JobsPauseProcessor {
    state: WebState,
}

impl From<WebState> for JobsPauseProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsPauseProcessor {
    type Request = JobActionParams;
    type Response = JobMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.pause"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_set_paused_with_service(
            &state_service(&self.state)?,
            params.id,
            params.request,
            true,
        )
    }
}

#[derive(Clone)]
pub struct JobsResumeProcessor {
    state: WebState,
}

impl From<WebState> for JobsResumeProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsResumeProcessor {
    type Request = JobActionParams;
    type Response = JobMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.resume"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_set_paused_with_service(
            &state_service(&self.state)?,
            params.id,
            params.request,
            false,
        )
    }
}

#[derive(Clone)]
pub struct JobsRunProcessor {
    state: WebState,
}

impl From<WebState> for JobsRunProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for JobsRunProcessor {
    type Request = JobRunParams;
    type Response = JobRunResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "jobs.run"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    fn serialization_scope(params: &Self::Request) -> SerializationScope {
        SerializationScope::per_key(params, "id")
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        jobs_run_with_service(&state_service(&self.state)?, params.id, params.request)
    }
}

#[derive(Clone)]
pub struct CronHistoryProcessor {
    state: WebState,
}

impl From<WebState> for CronHistoryProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for CronHistoryProcessor {
    type Request = CronHistoryQuery;
    type Response = CronHistoryResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "cron.history"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        cron_history_with_service(&state_service(&self.state)?, query)
    }
}

// ---------------------------------------------------------------------------
// Axum bridges.
// ---------------------------------------------------------------------------

pub async fn jobs_list_handler(
    State(state): State<WebState>,
    Query(query): Query<JobsQuery>,
) -> Response {
    rest_processor_response::<JobsListProcessor>(state, ApiMethod::JobsList, query).await
}

pub async fn jobs_create_handler(
    State(state): State<WebState>,
    Json(request): Json<JobCreateRequest>,
) -> Response {
    let response =
        rest_processor_response::<JobsCreateProcessor>(state, ApiMethod::JobsCreate, request).await;
    with_success_status(response, StatusCode::CREATED)
}

pub async fn jobs_update_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<JobUpdateRequest>,
) -> Response {
    rest_processor_response::<JobsUpdateProcessor>(
        state,
        ApiMethod::JobsUpdate,
        JobUpdateParams { id, request },
    )
    .await
}

pub async fn jobs_delete_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<JobDeleteRequest>,
) -> Response {
    rest_processor_response::<JobsDeleteProcessor>(
        state,
        ApiMethod::JobsDelete,
        JobDeleteParams { id, request },
    )
    .await
}

pub async fn jobs_pause_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<JobActionRequest>,
) -> Response {
    rest_processor_response::<JobsPauseProcessor>(
        state,
        ApiMethod::JobsPause,
        JobActionParams { id, request },
    )
    .await
}

pub async fn jobs_resume_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<JobActionRequest>,
) -> Response {
    rest_processor_response::<JobsResumeProcessor>(
        state,
        ApiMethod::JobsResume,
        JobActionParams { id, request },
    )
    .await
}

pub async fn jobs_run_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<JobRunRequest>,
) -> Response {
    let response = rest_processor_response::<JobsRunProcessor>(
        state,
        ApiMethod::JobsRun,
        JobRunParams { id, request },
    )
    .await;
    with_success_status(response, StatusCode::ACCEPTED)
}

pub async fn cron_history_handler(
    State(state): State<WebState>,
    Query(query): Query<CronHistoryQuery>,
) -> Response {
    rest_processor_response::<CronHistoryProcessor>(state, ApiMethod::CronHistory, query).await
}

fn with_success_status(mut response: Response, status: StatusCode) -> Response {
    if response.status().is_success() {
        *response.status_mut() = status;
    }
    response
}

fn state_service(state: &WebState) -> Result<SchedulerService, ProtocolApiError> {
    let service = state.scheduler_service();
    migrate_default_scheduler_data(&service).map_err(|error| ProtocolApiError::Internal {
        message: error.to_string(),
    })?;
    Ok(service)
}

pub fn jobs_list_with_service(
    service: &SchedulerService,
    query: JobsQuery,
) -> Result<JobsListResponse, ProtocolApiError> {
    let profile_id = normalize_optional(query.profile_id);
    let mut jobs: Vec<_> = service
        .list_definitions()
        .map_err(map_service_error)?
        .into_iter()
        .filter(|task| profile_matches(task, profile_id.as_deref()))
        .map(|task| job_summary(&task))
        .collect::<Result<_, _>>()?;
    sort_jobs(&mut jobs);
    let page = service
        .runs()
        .query(&SchedulerRunQuery {
            profile_id: profile_id.clone(),
            limit: Some(50),
            ..SchedulerRunQuery::default()
        })
        .map_err(map_run_store_error)?;
    let runs = page
        .runs
        .iter()
        .map(job_run_summary)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JobsListResponse {
        profile_id,
        jobs,
        runs,
        runs_next_cursor: page.next_cursor,
    })
}

pub fn jobs_create_with_service(
    service: &SchedulerService,
    request: JobCreateRequest,
) -> Result<JobMutationResponse, ProtocolApiError> {
    let task = service
        .create_definition(definition_input(request)?, Utc::now())
        .map_err(map_service_error)?;
    Ok(JobMutationResponse {
        job: Some(job_summary(&task)?),
        jobs: None,
        ok: true,
    })
}

pub fn jobs_update_with_service(
    service: &SchedulerService,
    id: String,
    request: JobUpdateRequest,
) -> Result<JobMutationResponse, ProtocolApiError> {
    let id = TaskId(id);
    let current = service.get_definition(&id).map_err(map_service_error)?;
    let mut input = allthecodes_services::scheduler::SchedulerDefinitionInput::from_task(&current);
    if let Some(name) = request.name {
        input.name = name;
    }
    if let Some(description) = request.description {
        input.metadata.description = normalize_optional(description);
    }
    if let Some(schedule) = request.schedule {
        input.schedule = schedule;
    }
    if let Some(schedule_kind) = request.schedule_kind {
        input.schedule_kind = schedule_kind_from_api(schedule_kind);
    }
    if let Some(timezone) = request.timezone {
        input.timezone = normalize_optional(timezone);
    }
    if let Some(payload) = request.payload {
        input.payload = payload_from_api(payload)?;
    }
    if let Some(paused) = request.paused {
        input.paused = paused;
    }
    if let Some(profile_id) = request.profile_id {
        input.metadata.profile_id = normalize_optional(profile_id);
    }
    if let Some(session_id) = request.session_id {
        input.metadata.session_id = normalize_optional(session_id);
    }
    if let Some(artifact_links) = request.artifact_links {
        input.metadata.artifact_links = artifacts_to_values(artifact_links)?;
    }
    let task = service
        .replace_definition(&id, request.revision, input, Utc::now())
        .map_err(map_service_error)?;
    Ok(JobMutationResponse {
        job: Some(job_summary(&task)?),
        jobs: None,
        ok: true,
    })
}

pub fn jobs_delete_with_service(
    service: &SchedulerService,
    id: String,
    request: JobDeleteRequest,
) -> Result<JobMutationResponse, ProtocolApiError> {
    let id = TaskId(id);
    let current = service.get_definition(&id).map_err(map_service_error)?;
    require_profile(&current, request.profile_id.as_deref())?;
    service
        .remove_definition(&id, request.revision)
        .map_err(map_service_error)?;
    let profile_id = normalize_optional(request.profile_id);
    let mut jobs = service
        .list_definitions()
        .map_err(map_service_error)?
        .into_iter()
        .filter(|task| profile_matches(task, profile_id.as_deref()))
        .map(|task| job_summary(&task))
        .collect::<Result<Vec<_>, _>>()?;
    sort_jobs(&mut jobs);
    Ok(JobMutationResponse {
        job: None,
        jobs: Some(jobs),
        ok: true,
    })
}

pub fn jobs_set_paused_with_service(
    service: &SchedulerService,
    id: String,
    request: JobActionRequest,
    paused: bool,
) -> Result<JobMutationResponse, ProtocolApiError> {
    let id = TaskId(id);
    let current = service.get_definition(&id).map_err(map_service_error)?;
    require_profile(&current, request.profile_id.as_deref())?;
    let task = service
        .set_paused(&id, paused, request.revision)
        .map_err(map_service_error)?;
    Ok(JobMutationResponse {
        job: Some(job_summary(&task)?),
        jobs: None,
        ok: true,
    })
}

pub fn jobs_run_with_service(
    service: &SchedulerService,
    id: String,
    request: JobRunRequest,
) -> Result<JobRunResponse, ProtocolApiError> {
    let id = TaskId(id);
    let current = service.get_definition(&id).map_err(map_service_error)?;
    require_profile(&current, request.profile_id.as_deref())?;
    let idempotency_key = normalize_optional(request.idempotency_key)
        .unwrap_or_else(|| format!("manual_job:{}:{}", id.as_str(), Uuid::new_v4().simple()));
    let accepted = service
        .trigger_manual(&id, request.revision, idempotency_key, Utc::now())
        .map_err(map_service_error)?;
    let job = service.get_definition(&id).map_err(map_service_error)?;
    accepted_response(job, accepted)
}

pub fn cron_history_with_service(
    service: &SchedulerService,
    query: CronHistoryQuery,
) -> Result<CronHistoryResponse, ProtocolApiError> {
    let profile_id = normalize_optional(query.profile_id);
    let job_id = normalize_optional(query.job_id);
    let requested_after = query
        .requested_after
        .as_deref()
        .map(parse_datetime)
        .transpose()?;
    let requested_before = query
        .requested_before
        .as_deref()
        .map(parse_datetime)
        .transpose()?;
    let page = service
        .runs()
        .query(&SchedulerRunQuery {
            task_id: job_id.clone().map(TaskId),
            statuses: query.status.map(status_from_api).into_iter().collect(),
            trigger_source: query.trigger_source.map(trigger_from_api),
            profile_id: profile_id.clone(),
            requested_after,
            requested_before,
            limit: query.limit,
            cursor: query.cursor,
        })
        .map_err(map_run_store_error)?;
    Ok(CronHistoryResponse {
        profile_id,
        job_id,
        runs: page
            .runs
            .iter()
            .map(job_run_summary)
            .collect::<Result<_, _>>()?,
        next_cursor: page.next_cursor,
    })
}

fn definition_input(
    request: JobCreateRequest,
) -> Result<allthecodes_services::scheduler::SchedulerDefinitionInput, ProtocolApiError> {
    Ok(allthecodes_services::scheduler::SchedulerDefinitionInput {
        name: request.name,
        schedule: request.schedule,
        schedule_kind: schedule_kind_from_api(request.schedule_kind),
        timezone: normalize_optional(request.timezone),
        payload: payload_from_api(request.payload)?,
        paused: request.paused.unwrap_or(false),
        metadata: ScheduledTaskMetadata {
            description: normalize_optional(request.description),
            profile_id: normalize_optional(request.profile_id),
            session_id: normalize_optional(request.session_id),
            working_directory: None,
            artifact_links: artifacts_to_values(request.artifact_links)?,
        },
    })
}

fn payload_from_api(payload: JobPayload) -> Result<TaskPayload, ProtocolApiError> {
    let value = payload.value.trim().to_string();
    if value.is_empty() {
        return Err(ProtocolApiError::BadRequest {
            code: "validation_error",
            message: "payload.value is required".to_string(),
        });
    }
    Ok(match payload.kind {
        JobPayloadKind::Prompt => TaskPayload::Prompt(value),
        JobPayloadKind::SlashCommand => TaskPayload::SlashCommand(value),
    })
}

fn payload_to_api(payload: &TaskPayload) -> JobPayload {
    match payload {
        TaskPayload::Prompt(value) => JobPayload {
            kind: JobPayloadKind::Prompt,
            value: value.clone(),
        },
        TaskPayload::SlashCommand(value) => JobPayload {
            kind: JobPayloadKind::SlashCommand,
            value: value.clone(),
        },
    }
}

fn schedule_kind_from_api(kind: JobScheduleKind) -> ScheduleKind {
    match kind {
        JobScheduleKind::Interval => ScheduleKind::Interval,
        JobScheduleKind::Cron => ScheduleKind::Cron,
    }
}

fn schedule_kind_to_api(kind: ScheduleKind) -> JobScheduleKind {
    match kind {
        ScheduleKind::Interval => JobScheduleKind::Interval,
        ScheduleKind::Cron => JobScheduleKind::Cron,
    }
}

fn job_summary(task: &ScheduledTask) -> Result<JobSummary, ProtocolApiError> {
    Ok(JobSummary {
        id: task.id.to_string(),
        name: task.name.clone(),
        description: task.metadata.description.clone(),
        status: if task.paused {
            JobDefinitionStatus::Paused
        } else {
            JobDefinitionStatus::Idle
        },
        schedule: task.schedule.clone(),
        schedule_kind: schedule_kind_to_api(task.schedule_kind),
        timezone: task.timezone.clone(),
        payload: payload_to_api(&task.payload),
        paused: task.paused,
        profile_id: task.metadata.profile_id.clone(),
        session_id: task.metadata.session_id.clone(),
        artifact_links: artifacts_from_values(&task.metadata.artifact_links)?,
        revision: task.revision,
        created_at: task.created_at.to_rfc3339(),
        updated_at: task.updated_at.unwrap_or(task.created_at).to_rfc3339(),
        last_run_at: task.last_run_at.map(|value| value.to_rfc3339()),
        next_run_at: Some(task.next_run_at.to_rfc3339()),
    })
}

fn job_run_summary(run: &SchedulerRunRecord) -> Result<JobRunSummary, ProtocolApiError> {
    let duration_start = run
        .started_at
        .or(run.enqueued_at)
        .unwrap_or(run.requested_at);
    let duration_ms = run
        .completed_at
        .map(|completed_at| (completed_at - duration_start).num_milliseconds().max(0) as u64);
    Ok(JobRunSummary {
        id: run.id.to_string(),
        job_id: run.task_id.to_string(),
        job_name: run.task_snapshot.name.clone(),
        status: status_to_api(run.status),
        trigger_source: trigger_to_api(run.trigger_source),
        idempotency_key: run.idempotency_key.clone(),
        requested_at: run.requested_at.to_rfc3339(),
        updated_at: run.updated_at.to_rfc3339(),
        scheduled_for: run.scheduled_for.map(|value| value.to_rfc3339()),
        enqueued_at: run.enqueued_at.map(|value| value.to_rfc3339()),
        started_at: run.started_at.map(|value| value.to_rfc3339()),
        completed_at: run.completed_at.map(|value| value.to_rfc3339()),
        duration_ms,
        command_id: run.command_id.clone(),
        task_id: run.execution_task_id.clone(),
        session_id: run.session_id.clone(),
        artifact_links: artifacts_from_values(&run.artifact_links)?,
        output: run.output_summary.clone(),
        failure: run.failure.as_ref().map(|failure| JobRunFailure {
            code: failure_code_to_api(failure.code),
            message: failure.message.clone(),
        }),
        revision: run.revision,
    })
}

fn accepted_response(
    task: ScheduledTask,
    accepted: AcceptedSchedulerRun,
) -> Result<JobRunResponse, ProtocolApiError> {
    Ok(JobRunResponse {
        job: job_summary(&task)?,
        run: job_run_summary(&accepted.run)?,
        receipt: JobDispatchReceipt {
            run_id: accepted.receipt.run_id.to_string(),
            command_id: accepted.receipt.command_id,
            accepted_at: accepted.receipt.accepted_at.to_rfc3339(),
            idempotency_key: accepted.receipt.idempotency_key,
        },
    })
}

fn status_to_api(status: SchedulerRunStatus) -> JobRunStatus {
    match status {
        SchedulerRunStatus::Enqueueing => JobRunStatus::Enqueueing,
        SchedulerRunStatus::Queued => JobRunStatus::Queued,
        SchedulerRunStatus::Running => JobRunStatus::Running,
        SchedulerRunStatus::Completed => JobRunStatus::Completed,
        SchedulerRunStatus::Failed => JobRunStatus::Failed,
        SchedulerRunStatus::Cancelled => JobRunStatus::Cancelled,
        SchedulerRunStatus::Rejected => JobRunStatus::Rejected,
        SchedulerRunStatus::FailedToEnqueue => JobRunStatus::FailedToEnqueue,
    }
}

fn status_from_api(status: JobRunStatus) -> SchedulerRunStatus {
    match status {
        JobRunStatus::Enqueueing => SchedulerRunStatus::Enqueueing,
        JobRunStatus::Queued => SchedulerRunStatus::Queued,
        JobRunStatus::Running => SchedulerRunStatus::Running,
        JobRunStatus::Completed => SchedulerRunStatus::Completed,
        JobRunStatus::Failed => SchedulerRunStatus::Failed,
        JobRunStatus::Cancelled => SchedulerRunStatus::Cancelled,
        JobRunStatus::Rejected => SchedulerRunStatus::Rejected,
        JobRunStatus::FailedToEnqueue => SchedulerRunStatus::FailedToEnqueue,
    }
}

fn trigger_to_api(source: SchedulerRunTriggerSource) -> JobRunTriggerSource {
    match source {
        SchedulerRunTriggerSource::Manual => JobRunTriggerSource::Manual,
        SchedulerRunTriggerSource::Scheduled => JobRunTriggerSource::Scheduled,
    }
}

fn trigger_from_api(source: JobRunTriggerSource) -> SchedulerRunTriggerSource {
    match source {
        JobRunTriggerSource::Manual => SchedulerRunTriggerSource::Manual,
        JobRunTriggerSource::Scheduled => SchedulerRunTriggerSource::Scheduled,
    }
}

fn failure_code_to_api(code: SchedulerRunFailureCode) -> JobRunFailureCode {
    match code {
        SchedulerRunFailureCode::DispatcherUnavailable => JobRunFailureCode::DispatcherUnavailable,
        SchedulerRunFailureCode::EnqueueFailed => JobRunFailureCode::EnqueueFailed,
        SchedulerRunFailureCode::DispatchRejected => JobRunFailureCode::DispatchRejected,
        SchedulerRunFailureCode::ExecutionFailed => JobRunFailureCode::ExecutionFailed,
        SchedulerRunFailureCode::Cancelled => JobRunFailureCode::Cancelled,
    }
}

fn artifacts_to_values(artifacts: Vec<JobArtifactLink>) -> Result<Vec<Value>, ProtocolApiError> {
    artifacts
        .into_iter()
        .map(|artifact| {
            serde_json::to_value(artifact).map_err(|error| ProtocolApiError::BadRequest {
                code: "invalid_artifact_link",
                message: error.to_string(),
            })
        })
        .collect()
}

fn artifacts_from_values(artifacts: &[Value]) -> Result<Vec<JobArtifactLink>, ProtocolApiError> {
    artifacts
        .iter()
        .cloned()
        .map(|artifact| {
            serde_json::from_value(artifact).map_err(|error| ProtocolApiError::Internal {
                message: format!("canonical scheduler artifact metadata is invalid: {error}"),
            })
        })
        .collect()
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_string();
        (!value.is_empty()).then_some(value)
    })
}

fn profile_matches(task: &ScheduledTask, profile_id: Option<&str>) -> bool {
    profile_id.is_none_or(|expected| task.metadata.profile_id.as_deref() == Some(expected))
}

fn require_profile(task: &ScheduledTask, profile_id: Option<&str>) -> Result<(), ProtocolApiError> {
    if profile_matches(task, profile_id) {
        Ok(())
    } else {
        Err(ProtocolApiError::NotFound {
            entity: "job",
            id: task.id.to_string(),
        })
    }
}

fn sort_jobs(jobs: &mut [JobSummary]) {
    jobs.sort_by(|left, right| {
        left.paused
            .cmp(&right.paused)
            .then_with(|| left.next_run_at.cmp(&right.next_run_at))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, ProtocolApiError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| ProtocolApiError::BadRequest {
            code: "invalid_datetime",
            message: error.to_string(),
        })
}

fn map_service_error(error: SchedulerServiceError) -> ProtocolApiError {
    match error {
        SchedulerServiceError::Definitions(error) => map_scheduler_error(error),
        SchedulerServiceError::Runs(error) => map_run_store_error(error),
        SchedulerServiceError::Validation(message) => ProtocolApiError::BadRequest {
            code: "validation_error",
            message,
        },
        SchedulerServiceError::DispatchInProgress { run_id } => ProtocolApiError::Conflict {
            reason: format!("scheduler dispatch for run '{run_id}' is already in progress"),
        },
        SchedulerServiceError::DuplicateIdempotency { run_id } => ProtocolApiError::Conflict {
            reason: format!("idempotency key is already bound to run '{run_id}'"),
        },
        SchedulerServiceError::DispatcherUnavailable { run_id, message } => {
            ProtocolApiError::ServiceUnavailable {
                code: "dispatcher_unavailable",
                message: format!("run_id={run_id}: {message}"),
            }
        }
        SchedulerServiceError::DispatchRejected { run_id, message } => {
            ProtocolApiError::ServiceUnavailable {
                code: "dispatch_rejected",
                message: format!("run_id={run_id}: {message}"),
            }
        }
        SchedulerServiceError::EnqueueFailed { run_id, message } => {
            ProtocolApiError::ServiceUnavailable {
                code: "enqueue_failed",
                message: format!("run_id={run_id}: {message}"),
            }
        }
        SchedulerServiceError::ScheduleAdvanceFailed { run_id, source } => {
            ProtocolApiError::ServiceUnavailable {
                code: "schedule_advance_failed",
                message: format!("run_id={run_id}: {source}"),
            }
        }
    }
}

fn map_scheduler_error(error: SchedulerError) -> ProtocolApiError {
    match error {
        SchedulerError::NotFound(id) => ProtocolApiError::NotFound { entity: "job", id },
        SchedulerError::AlreadyExists(id) => ProtocolApiError::Conflict {
            reason: format!("job '{id}' already exists"),
        },
        SchedulerError::RevisionConflict { expected, actual } => ProtocolApiError::Conflict {
            reason: format!("job revision conflict: expected {expected}, current {actual}"),
        },
        SchedulerError::RemoteTriggerUnsupported => ProtocolApiError::BadRequest {
            code: "unsupported_execution_kind",
            message: error.to_string(),
        },
        other => ProtocolApiError::Internal {
            message: other.to_string(),
        },
    }
}

fn map_run_store_error(error: SchedulerRunStoreError) -> ProtocolApiError {
    match error {
        SchedulerRunStoreError::NotFound(id) => ProtocolApiError::NotFound {
            entity: "job_run",
            id,
        },
        SchedulerRunStoreError::IdempotencyConflict { run_id } => ProtocolApiError::Conflict {
            reason: format!("idempotency key is already bound to run '{run_id}'"),
        },
        SchedulerRunStoreError::RevisionConflict { expected, actual } => {
            ProtocolApiError::Conflict {
                reason: format!("run revision conflict: expected {expected}, current {actual}"),
            }
        }
        SchedulerRunStoreError::InvalidTransition { run_id, from, to } => {
            ProtocolApiError::Conflict {
                reason: format!("invalid run transition for '{run_id}': {from} -> {to}"),
            }
        }
        SchedulerRunStoreError::InvalidCursor => ProtocolApiError::BadRequest {
            code: "invalid_cursor",
            message: "cron history cursor is invalid for these filters".to_string(),
        },
        SchedulerRunStoreError::Validation(message) => ProtocolApiError::BadRequest {
            code: "validation_error",
            message,
        },
        SchedulerRunStoreError::TooLarge => ProtocolApiError::PayloadTooLarge {
            code: "scheduler_history_too_large",
            message: error.to_string(),
        },
        other => ProtocolApiError::Internal {
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::handlers::test_support::{make_web_state, response_json, temp_home};
    use allthecodes_daemon::protocol::DaemonProtocolStore;
    use allthecodes_daemon::scheduler_loop::DaemonSchedulerDispatcher;
    use allthecodes_daemon::supervisor::ASSISTANT_WORKER_ID;
    use allthecodes_services::scheduler::{
        SchedulerCommandDispatcher, SchedulerDispatchReceipt, SchedulerDispatchRequest,
        SchedulerDispatcherError, SchedulerRunFailureCode, SchedulerRunId, SchedulerRunStore,
        SchedulerStore,
    };

    #[derive(Default)]
    struct TestDispatcher {
        requests: Mutex<Vec<SchedulerDispatchRequest>>,
    }

    impl SchedulerCommandDispatcher for TestDispatcher {
        fn dispatch(
            &self,
            request: SchedulerDispatchRequest,
        ) -> Result<SchedulerDispatchReceipt, SchedulerDispatcherError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(SchedulerDispatchReceipt {
                run_id: request.run_id,
                command_id: "cmd-web-1".to_string(),
                accepted_at: Utc::now(),
                idempotency_key: request.idempotency_key,
            })
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

    fn create_request() -> JobCreateRequest {
        JobCreateRequest {
            name: "nightly review".to_string(),
            description: Some("review workspace".to_string()),
            schedule: "5m".to_string(),
            schedule_kind: JobScheduleKind::Interval,
            timezone: None,
            payload: JobPayload {
                kind: JobPayloadKind::Prompt,
                value: "summarize changes".to_string(),
            },
            paused: None,
            profile_id: Some("profile-1".to_string()),
            session_id: None,
            artifact_links: Vec::new(),
        }
    }

    #[test]
    fn web_crud_reads_and_writes_canonical_scheduler_store() {
        let (_dir, service) = service(None);
        let created = jobs_create_with_service(&service, create_request()).unwrap();
        let created = created.job.unwrap();
        assert_eq!(created.revision, 1);
        assert_eq!(service.definitions().load().unwrap().len(), 1);

        let listed = jobs_list_with_service(
            &service,
            JobsQuery {
                profile_id: Some("profile-1".to_string()),
            },
        )
        .unwrap();
        assert_eq!(listed.jobs[0].id, created.id);

        let updated = jobs_update_with_service(
            &service,
            created.id.clone(),
            JobUpdateRequest {
                name: Some("renamed".to_string()),
                description: None,
                schedule: None,
                schedule_kind: None,
                timezone: None,
                payload: None,
                paused: None,
                profile_id: None,
                session_id: None,
                artifact_links: None,
                revision: created.revision,
            },
        )
        .unwrap()
        .job
        .unwrap();
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.revision, 2);

        let stale = jobs_set_paused_with_service(
            &service,
            created.id,
            JobActionRequest {
                revision: 1,
                profile_id: Some("profile-1".to_string()),
            },
            true,
        )
        .unwrap_err();
        assert_eq!(stale.status_code(), 409);
    }

    #[test]
    fn manual_run_returns_real_acceptance_and_history() {
        let dispatcher = Arc::new(TestDispatcher::default());
        let (_dir, service) = service(Some(dispatcher.clone()));
        let job = jobs_create_with_service(&service, create_request())
            .unwrap()
            .job
            .unwrap();
        let response = jobs_run_with_service(
            &service,
            job.id.clone(),
            JobRunRequest {
                revision: job.revision,
                profile_id: Some("profile-1".to_string()),
                idempotency_key: Some("web-manual-1".to_string()),
            },
        )
        .unwrap();
        assert_eq!(response.run.status, JobRunStatus::Queued);
        assert_eq!(response.receipt.command_id, "cmd-web-1");
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);

        let history = cron_history_with_service(
            &service,
            CronHistoryQuery {
                job_id: Some(job.id),
                ..CronHistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(history.runs.len(), 1);
        assert_eq!(history.runs[0].command_id.as_deref(), Some("cmd-web-1"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn all_processors_share_the_canonical_scheduler_state() {
        let (_home, _guard) = temp_home();
        let dispatcher = Arc::new(TestDispatcher::default());
        let state = make_web_state().with_scheduler_dispatcher(dispatcher.clone());

        let created = crate::processors::dispatch_processor(
            JobsCreateProcessor::from(state.clone()),
            create_request(),
        )
        .await
        .unwrap()
        .job
        .unwrap();

        let listed = crate::processors::dispatch_processor(
            JobsListProcessor::from(state.clone()),
            JobsQuery {
                profile_id: Some("profile-1".to_string()),
            },
        )
        .await
        .unwrap();
        assert_eq!(listed.jobs.len(), 1);
        assert_eq!(listed.jobs[0].id, created.id);

        let updated = crate::processors::dispatch_processor(
            JobsUpdateProcessor::from(state.clone()),
            JobUpdateParams {
                id: created.id.clone(),
                request: JobUpdateRequest {
                    name: Some("processor-renamed".to_string()),
                    description: None,
                    schedule: None,
                    schedule_kind: None,
                    timezone: None,
                    payload: None,
                    paused: None,
                    profile_id: None,
                    session_id: None,
                    artifact_links: None,
                    revision: created.revision,
                },
            },
        )
        .await
        .unwrap()
        .job
        .unwrap();
        assert_eq!(updated.name, "processor-renamed");

        let paused = crate::processors::dispatch_processor(
            JobsPauseProcessor::from(state.clone()),
            JobActionParams {
                id: updated.id.clone(),
                request: JobActionRequest {
                    revision: updated.revision,
                    profile_id: Some("profile-1".to_string()),
                },
            },
        )
        .await
        .unwrap()
        .job
        .unwrap();
        assert!(paused.paused);

        let resumed = crate::processors::dispatch_processor(
            JobsResumeProcessor::from(state.clone()),
            JobActionParams {
                id: paused.id.clone(),
                request: JobActionRequest {
                    revision: paused.revision,
                    profile_id: Some("profile-1".to_string()),
                },
            },
        )
        .await
        .unwrap()
        .job
        .unwrap();
        assert!(!resumed.paused);

        let accepted = crate::processors::dispatch_processor(
            JobsRunProcessor::from(state.clone()),
            JobRunParams {
                id: resumed.id.clone(),
                request: JobRunRequest {
                    revision: resumed.revision,
                    profile_id: Some("profile-1".to_string()),
                    idempotency_key: Some("processor-manual-1".to_string()),
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(accepted.run.status, JobRunStatus::Queued);
        assert_eq!(dispatcher.requests.lock().unwrap().len(), 1);

        let history = crate::processors::dispatch_processor(
            CronHistoryProcessor::from(state.clone()),
            CronHistoryQuery {
                job_id: Some(resumed.id.clone()),
                ..CronHistoryQuery::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(history.runs.len(), 1);
        assert_eq!(history.runs[0].id, accepted.run.id);

        let deleted = crate::processors::dispatch_processor(
            JobsDeleteProcessor::from(state),
            JobDeleteParams {
                id: resumed.id,
                request: JobDeleteRequest {
                    revision: accepted.job.revision,
                    profile_id: Some("profile-1".to_string()),
                },
            },
        )
        .await
        .unwrap();
        assert!(deleted.ok);
        assert!(deleted.jobs.unwrap().is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn production_dispatcher_accepts_manual_run_and_records_terminal_state() {
        let (_home, _guard) = temp_home();
        let state = make_web_state().with_scheduler_dispatcher(Arc::new(DaemonSchedulerDispatcher));

        let create_response =
            jobs_create_handler(State(state.clone()), Json(create_request())).await;
        assert_eq!(create_response.status(), StatusCode::CREATED);
        let created: JobMutationResponse =
            serde_json::from_value(response_json(create_response).await).unwrap();
        let job = created.job.expect("created response includes job");

        let run_response = jobs_run_handler(
            State(state.clone()),
            AxumPath(job.id.clone()),
            Json(JobRunRequest {
                revision: job.revision,
                profile_id: Some("profile-1".to_string()),
                idempotency_key: Some("web-production-manual".to_string()),
            }),
        )
        .await;
        assert_eq!(run_response.status(), StatusCode::ACCEPTED);
        let accepted: JobRunResponse =
            serde_json::from_value(response_json(run_response).await).unwrap();
        assert_eq!(accepted.run.status, JobRunStatus::Queued);

        let protocol = DaemonProtocolStore::new(allthecodes_daemon::process_state::daemon_dir());
        let claimed = protocol
            .claim_next_pending_command(ASSISTANT_WORKER_ID, "assistant")
            .unwrap()
            .expect("accepted manual run should enqueue a daemon command");
        assert_eq!(claimed.command_id, accepted.receipt.command_id);
        let run_id = SchedulerRunId::parse(accepted.receipt.run_id.clone()).unwrap();
        assert_eq!(
            SchedulerRunStore::open_default()
                .get(&run_id)
                .unwrap()
                .status,
            SchedulerRunStatus::Running
        );
        protocol.mark_command_handled(claimed).unwrap();

        let history_response = cron_history_handler(
            State(state),
            Query(CronHistoryQuery {
                job_id: Some(job.id),
                ..CronHistoryQuery::default()
            }),
        )
        .await;
        assert_eq!(history_response.status(), StatusCode::OK);
        let history: CronHistoryResponse =
            serde_json::from_value(response_json(history_response).await).unwrap();
        assert_eq!(history.runs.len(), 1);
        assert_eq!(history.runs[0].status, JobRunStatus::Completed);
        assert_eq!(
            history.runs[0].command_id.as_deref(),
            Some(accepted.receipt.command_id.as_str())
        );
    }

    #[test]
    fn missing_dispatcher_is_503_and_history_is_truthful() {
        let (_dir, service) = service(None);
        let job = jobs_create_with_service(&service, create_request())
            .unwrap()
            .job
            .unwrap();
        let error = jobs_run_with_service(
            &service,
            job.id.clone(),
            JobRunRequest {
                revision: job.revision,
                profile_id: None,
                idempotency_key: Some("web-manual-unavailable".to_string()),
            },
        )
        .unwrap_err();
        assert_eq!(error.status_code(), 503);

        let page = service.runs().query(&SchedulerRunQuery::default()).unwrap();
        assert_eq!(page.runs.len(), 1);
        assert_eq!(page.runs[0].status, SchedulerRunStatus::FailedToEnqueue);
        assert_eq!(
            page.runs[0].failure.as_ref().unwrap().code,
            SchedulerRunFailureCode::DispatcherUnavailable
        );
        assert!(page.runs[0].command_id.is_none());
    }
}
