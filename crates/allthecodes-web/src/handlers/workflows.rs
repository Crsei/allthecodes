//! Project-local FileWorkflow definition and runtime handlers.

use std::path::PathBuf;

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Json;

use allthecodes_protocol::v1::workflows::{
    WorkflowAdvanceRequest, WorkflowAdvanceStatus, WorkflowCancelRequest, WorkflowDefinitionDetail,
    WorkflowDefinitionParams, WorkflowDefinitionSummary, WorkflowDefinitionsQuery,
    WorkflowDefinitionsResponse, WorkflowMutationMetadata, WorkflowRun, WorkflowRunAdvanceParams,
    WorkflowRunCancelParams, WorkflowRunListQuery, WorkflowRunPage, WorkflowRunParams,
    WorkflowRunResponse, WorkflowRunStartParams, WorkflowRunStatus, WorkflowRunStep,
    WorkflowRunStepStatus, WorkflowRunSummary, WorkflowStartRequest, WorkflowStepDefinition,
};
use allthecodes_protocol::{ApiError as ProtocolApiError, ApiMethod};
use allthecodes_tools::workflow::file_workflow::{
    classify_workflow_action, FileWorkflowRunRecord, FileWorkflowService, WorkflowAdvanceOptions,
    WorkflowAuthorizationDecision, WorkflowCancelOptions, WorkflowMutationOutcome,
    WorkflowRunListOptions, WorkflowServiceError, WorkflowServiceErrorKind, WorkflowStartOptions,
};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::state::WebState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowMutationAuthorizationContext {
    pub action: &'static str,
    pub workspace_id: String,
    pub workflow: Option<String>,
    pub run_id: Option<String>,
    pub request_id: String,
    pub expected_revision: Option<u64>,
}

pub trait WorkflowMutationPolicy: Send + Sync {
    fn authorize(
        &self,
        context: &WorkflowMutationAuthorizationContext,
    ) -> WorkflowAuthorizationDecision;
}

/// Production keeps exact tool parity: all FileWorkflow mutations require an
/// interactive approval. REST and API-RPC currently have no challenge/resume
/// transport, so `Ask` is returned as a fail-closed conflict.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductionWorkflowMutationPolicy;

impl WorkflowMutationPolicy for ProductionWorkflowMutationPolicy {
    fn authorize(
        &self,
        context: &WorkflowMutationAuthorizationContext,
    ) -> WorkflowAuthorizationDecision {
        classify_workflow_action(context.action)
    }
}

/// Explicit fixture for adapter/state-machine tests. Production processors do
/// not construct this policy.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowWorkflowMutationsForTest;

impl WorkflowMutationPolicy for AllowWorkflowMutationsForTest {
    fn authorize(
        &self,
        _context: &WorkflowMutationAuthorizationContext,
    ) -> WorkflowAuthorizationDecision {
        WorkflowAuthorizationDecision::Allow
    }
}

#[derive(Clone)]
pub struct WorkflowDefinitionsListProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowDefinitionsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowDefinitionsListProcessor {
    type Request = WorkflowDefinitionsQuery;
    type Response = WorkflowDefinitionsResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.definitions.list"
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let workspace = active_workspace(&self.state);
        blocking(move || {
            let definitions = FileWorkflowService::new(&workspace)?
                .list_definitions()?
                .into_iter()
                .map(|definition| WorkflowDefinitionSummary {
                    workflow: definition.workflow,
                    workflow_file: definition.workflow_file,
                    step_count: definition.step_count,
                    parse_error: definition.parse_error,
                })
                .collect();
            Ok(WorkflowDefinitionsResponse { definitions })
        })
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowDefinitionDetailProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowDefinitionDetailProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowDefinitionDetailProcessor {
    type Request = WorkflowDefinitionParams;
    type Response = WorkflowDefinitionDetail;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.definitions.detail"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let workspace = active_workspace(&self.state);
        blocking(move || {
            let definition = FileWorkflowService::new(&workspace)?.definition(&params.workflow)?;
            Ok(WorkflowDefinitionDetail {
                workflow: definition.workflow,
                workflow_file: definition.workflow_file,
                steps: definition
                    .steps
                    .into_iter()
                    .enumerate()
                    .map(|(index, step)| WorkflowStepDefinition {
                        index,
                        name: step.name,
                        prompt: step.prompt,
                        run: step.run,
                    })
                    .collect(),
            })
        })
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowRunsListProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowRunsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowRunsListProcessor {
    type Request = WorkflowRunListQuery;
    type Response = WorkflowRunPage;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.runs.list"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let workspace = active_workspace(&self.state);
        blocking(move || {
            let page = FileWorkflowService::new(&workspace)?.list_runs(WorkflowRunListOptions {
                status: params.status.map(service_run_status),
                cursor: params.cursor,
                limit: params.limit,
            })?;
            let runs = page
                .runs
                .into_iter()
                .map(run_summary)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(WorkflowRunPage {
                runs,
                next_cursor: page.next_cursor,
                truncated: page.truncated,
                corrupt_entry_count: page.corrupt_entry_count,
            })
        })
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowRunStatusProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowRunStatusProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowRunStatusProcessor {
    type Request = WorkflowRunParams;
    type Response = WorkflowRunResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.runs.status"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let workspace = active_workspace(&self.state);
        blocking(move || {
            let record = FileWorkflowService::new(&workspace)?.load_run(&params.run_id)?;
            Ok(WorkflowRunResponse {
                run: run_detail(record)?,
                mutation: None,
            })
        })
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowRunStartProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowRunStartProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowRunStartProcessor {
    type Request = WorkflowRunStartParams;
    type Response = WorkflowRunResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.runs.start"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        start_in_workspace(
            active_workspace(&self.state),
            params,
            &ProductionWorkflowMutationPolicy,
        )
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowRunAdvanceProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowRunAdvanceProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowRunAdvanceProcessor {
    type Request = WorkflowRunAdvanceParams;
    type Response = WorkflowRunResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.runs.advance"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        advance_in_workspace(
            active_workspace(&self.state),
            params,
            &ProductionWorkflowMutationPolicy,
        )
        .await
    }
}

#[derive(Clone)]
pub struct WorkflowRunCancelProcessor {
    state: WebState,
}

impl From<WebState> for WorkflowRunCancelProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorkflowRunCancelProcessor {
    type Request = WorkflowRunCancelParams;
    type Response = WorkflowRunResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "workflows.runs.cancel"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        cancel_in_workspace(
            active_workspace(&self.state),
            params,
            &ProductionWorkflowMutationPolicy,
        )
        .await
    }
}

pub async fn start_in_workspace(
    workspace: PathBuf,
    params: WorkflowRunStartParams,
    policy: &dyn WorkflowMutationPolicy,
) -> Result<WorkflowRunResponse, ProtocolApiError> {
    let service = FileWorkflowService::new(&workspace).map_err(protocol_service_error)?;
    authorize_mutation(
        policy,
        WorkflowMutationAuthorizationContext {
            action: "start",
            workspace_id: service.workspace_id().to_string(),
            workflow: Some(params.workflow.clone()),
            run_id: None,
            request_id: params.request_id.clone(),
            expected_revision: None,
        },
    )?;
    blocking(move || {
        let outcome = FileWorkflowService::new(&workspace)?.start(WorkflowStartOptions {
            workflow: params.workflow,
            args: params.args,
            request_id: params.request_id.clone(),
        })?;
        mutation_response(outcome, params.request_id)
    })
    .await
}

pub async fn advance_in_workspace(
    workspace: PathBuf,
    params: WorkflowRunAdvanceParams,
    policy: &dyn WorkflowMutationPolicy,
) -> Result<WorkflowRunResponse, ProtocolApiError> {
    let service = FileWorkflowService::new(&workspace).map_err(protocol_service_error)?;
    authorize_mutation(
        policy,
        WorkflowMutationAuthorizationContext {
            action: "advance",
            workspace_id: service.workspace_id().to_string(),
            workflow: None,
            run_id: Some(params.run_id.clone()),
            request_id: params.request_id.clone(),
            expected_revision: Some(params.expected_revision),
        },
    )?;
    blocking(move || {
        let outcome = FileWorkflowService::new(&workspace)?.advance(WorkflowAdvanceOptions {
            run_id: params.run_id,
            applied_status: service_advance_status(params.applied_status),
            expected_revision: Some(params.expected_revision),
            request_id: params.request_id.clone(),
        })?;
        mutation_response(outcome, params.request_id)
    })
    .await
}

pub async fn cancel_in_workspace(
    workspace: PathBuf,
    params: WorkflowRunCancelParams,
    policy: &dyn WorkflowMutationPolicy,
) -> Result<WorkflowRunResponse, ProtocolApiError> {
    let service = FileWorkflowService::new(&workspace).map_err(protocol_service_error)?;
    authorize_mutation(
        policy,
        WorkflowMutationAuthorizationContext {
            action: "cancel",
            workspace_id: service.workspace_id().to_string(),
            workflow: None,
            run_id: Some(params.run_id.clone()),
            request_id: params.request_id.clone(),
            expected_revision: Some(params.expected_revision),
        },
    )?;
    blocking(move || {
        let outcome = FileWorkflowService::new(&workspace)?.cancel(WorkflowCancelOptions {
            run_id: params.run_id,
            expected_revision: Some(params.expected_revision),
            request_id: params.request_id.clone(),
        })?;
        mutation_response(outcome, params.request_id)
    })
    .await
}

fn authorize_mutation(
    policy: &dyn WorkflowMutationPolicy,
    context: WorkflowMutationAuthorizationContext,
) -> Result<(), ProtocolApiError> {
    match policy.authorize(&context) {
        WorkflowAuthorizationDecision::Allow => Ok(()),
        WorkflowAuthorizationDecision::Ask => Err(ProtocolApiError::Conflict {
            reason: "interactive_approval_required: this workflow mutation requires an exact interactive approval"
                .to_string(),
        }),
        WorkflowAuthorizationDecision::Deny => Err(ProtocolApiError::Forbidden {
            code: "workflow_permission_denied",
            message: "workflow mutation was denied by policy".to_string(),
        }),
    }
}

fn mutation_response(
    outcome: WorkflowMutationOutcome,
    request_id: String,
) -> Result<WorkflowRunResponse, WorkflowServiceError> {
    Ok(WorkflowRunResponse {
        run: run_detail(outcome.record)?,
        mutation: Some(WorkflowMutationMetadata {
            request_id,
            replayed: outcome.replayed,
            projection_warning: outcome.projection_warning,
        }),
    })
}

fn run_summary(record: FileWorkflowRunRecord) -> Result<WorkflowRunSummary, WorkflowServiceError> {
    let status = protocol_run_status(&record.status)?;
    let completed_steps = completed_steps(&record);
    let total_steps = record.steps.len();
    Ok(WorkflowRunSummary {
        run_id: record.run_id,
        workflow: record.workflow,
        workflow_file: record.workflow_file,
        status,
        current_step_index: record.current_step_index,
        completed_steps,
        total_steps,
        revision: record.revision,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn run_detail(record: FileWorkflowRunRecord) -> Result<WorkflowRun, WorkflowServiceError> {
    let status = protocol_run_status(&record.status)?;
    let completed_steps = completed_steps(&record);
    let total_steps = record.steps.len();
    let steps = record
        .steps
        .into_iter()
        .enumerate()
        .map(|(index, step)| {
            Ok(WorkflowRunStep {
                index,
                name: step.name,
                prompt: step.prompt,
                run: step.run,
                status: protocol_step_status(&step.status)?,
                started_at: step.started_at,
                completed_at: step.completed_at,
            })
        })
        .collect::<Result<Vec<_>, WorkflowServiceError>>()?;
    Ok(WorkflowRun {
        run_id: record.run_id,
        workflow: record.workflow,
        workflow_file: record.workflow_file,
        status,
        current_step_index: record.current_step_index,
        steps,
        completed_steps,
        total_steps,
        has_args: record.args.is_some(),
        revision: record.revision,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

fn completed_steps(record: &FileWorkflowRunRecord) -> usize {
    record
        .steps
        .iter()
        .filter(|step| step.status == "completed")
        .count()
}

fn protocol_run_status(status: &str) -> Result<WorkflowRunStatus, WorkflowServiceError> {
    match status {
        "running" => Ok(WorkflowRunStatus::Running),
        "completed" => Ok(WorkflowRunStatus::Completed),
        "failed" => Ok(WorkflowRunStatus::Failed),
        "cancelled" => Ok(WorkflowRunStatus::Cancelled),
        _ => Err(WorkflowServiceError {
            kind: WorkflowServiceErrorKind::InvalidInput,
            code: "invalid_workflow_status",
            message: "stored workflow status is invalid".to_string(),
        }),
    }
}

fn protocol_step_status(status: &str) -> Result<WorkflowRunStepStatus, WorkflowServiceError> {
    match status {
        "pending" => Ok(WorkflowRunStepStatus::Pending),
        "running" => Ok(WorkflowRunStepStatus::Running),
        "ready" => Ok(WorkflowRunStepStatus::Ready),
        "completed" => Ok(WorkflowRunStepStatus::Completed),
        "failed" => Ok(WorkflowRunStepStatus::Failed),
        "cancelled" => Ok(WorkflowRunStepStatus::Cancelled),
        _ => Err(WorkflowServiceError {
            kind: WorkflowServiceErrorKind::InvalidInput,
            code: "invalid_workflow_step_status",
            message: "stored workflow step status is invalid".to_string(),
        }),
    }
}

fn service_run_status(
    status: WorkflowRunStatus,
) -> allthecodes_tools::workflow::file_workflow::WorkflowRunStatus {
    match status {
        WorkflowRunStatus::Running => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Running
        }
        WorkflowRunStatus::Completed => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Completed
        }
        WorkflowRunStatus::Failed => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Failed
        }
        WorkflowRunStatus::Cancelled => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Cancelled
        }
    }
}

fn service_advance_status(
    status: WorkflowAdvanceStatus,
) -> allthecodes_tools::workflow::file_workflow::WorkflowRunStatus {
    match status {
        WorkflowAdvanceStatus::Completed => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Completed
        }
        WorkflowAdvanceStatus::Failed => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Failed
        }
        WorkflowAdvanceStatus::Cancelled => {
            allthecodes_tools::workflow::file_workflow::WorkflowRunStatus::Cancelled
        }
    }
}

fn protocol_service_error(error: WorkflowServiceError) -> ProtocolApiError {
    match error.kind {
        WorkflowServiceErrorKind::InvalidInput => ProtocolApiError::BadRequest {
            code: error.code,
            message: error.message,
        },
        WorkflowServiceErrorKind::Forbidden => ProtocolApiError::Forbidden {
            code: error.code,
            message: error.message,
        },
        WorkflowServiceErrorKind::NotFound => ProtocolApiError::NotFound {
            entity: "workflow",
            id: error.message,
        },
        WorkflowServiceErrorKind::Conflict => ProtocolApiError::Conflict {
            reason: format!("{}: {}", error.code, error.message),
        },
        WorkflowServiceErrorKind::TooLarge => ProtocolApiError::PayloadTooLarge {
            code: error.code,
            message: error.message,
        },
        WorkflowServiceErrorKind::Store => ProtocolApiError::Internal {
            message: error.message,
        },
    }
}

async fn blocking<T, F>(operation: F) -> Result<T, ProtocolApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, WorkflowServiceError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| ProtocolApiError::Internal {
            message: format!("workflow filesystem task failed: {error}"),
        })?
        .map_err(protocol_service_error)
}

fn active_workspace(state: &WebState) -> PathBuf {
    PathBuf::from(state.engine().cwd())
}

async fn definitions_list_handler(
    State(state): State<WebState>,
    Query(query): Query<WorkflowDefinitionsQuery>,
) -> Response {
    rest_processor_response::<WorkflowDefinitionsListProcessor>(
        state,
        ApiMethod::WorkflowDefinitionsList,
        query,
    )
    .await
}

async fn definition_detail_handler(
    State(state): State<WebState>,
    AxumPath(workflow): AxumPath<String>,
) -> Response {
    rest_processor_response::<WorkflowDefinitionDetailProcessor>(
        state,
        ApiMethod::WorkflowDefinitionDetail,
        WorkflowDefinitionParams { workflow },
    )
    .await
}

async fn runs_list_handler(
    State(state): State<WebState>,
    Query(query): Query<WorkflowRunListQuery>,
) -> Response {
    rest_processor_response::<WorkflowRunsListProcessor>(state, ApiMethod::WorkflowRunsList, query)
        .await
}

async fn run_status_handler(
    State(state): State<WebState>,
    AxumPath(run_id): AxumPath<String>,
) -> Response {
    rest_processor_response::<WorkflowRunStatusProcessor>(
        state,
        ApiMethod::WorkflowRunStatus,
        WorkflowRunParams { run_id },
    )
    .await
}

async fn run_start_handler(
    State(state): State<WebState>,
    AxumPath(workflow): AxumPath<String>,
    Json(request): Json<WorkflowStartRequest>,
) -> Response {
    rest_processor_response::<WorkflowRunStartProcessor>(
        state,
        ApiMethod::WorkflowRunStart,
        WorkflowRunStartParams {
            workflow,
            request_id: request.request_id,
            args: request.args,
        },
    )
    .await
}

async fn run_advance_handler(
    State(state): State<WebState>,
    AxumPath(run_id): AxumPath<String>,
    Json(request): Json<WorkflowAdvanceRequest>,
) -> Response {
    rest_processor_response::<WorkflowRunAdvanceProcessor>(
        state,
        ApiMethod::WorkflowRunAdvance,
        WorkflowRunAdvanceParams {
            run_id,
            request_id: request.request_id,
            expected_revision: request.expected_revision,
            applied_status: request.applied_status,
        },
    )
    .await
}

async fn run_cancel_handler(
    State(state): State<WebState>,
    AxumPath(run_id): AxumPath<String>,
    Json(request): Json<WorkflowCancelRequest>,
) -> Response {
    rest_processor_response::<WorkflowRunCancelProcessor>(
        state,
        ApiMethod::WorkflowRunCancel,
        WorkflowRunCancelParams {
            run_id,
            request_id: request.request_id,
            expected_revision: request.expected_revision,
        },
    )
    .await
}

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(
            ApiMethod::WorkflowDefinitionsList,
            get(definitions_list_handler),
        )
        .handle(
            ApiMethod::WorkflowDefinitionDetail,
            get(definition_detail_handler),
        )
        .handle(ApiMethod::WorkflowRunsList, get(runs_list_handler))
        .handle(ApiMethod::WorkflowRunStart, post(run_start_handler))
        .handle(ApiMethod::WorkflowRunStatus, get(run_status_handler))
        .handle(ApiMethod::WorkflowRunAdvance, post(run_advance_handler))
        .handle(ApiMethod::WorkflowRunCancel, post(run_cancel_handler))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::Path;

    use serde_json::json;

    use super::*;

    struct EnvGuard {
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn isolated(home: &Path) -> Self {
            let previous = std::env::var_os("ALLTHECODES_HOME");
            std::env::set_var("ALLTHECODES_HOME", home);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var("ALLTHECODES_HOME", previous);
            } else {
                std::env::remove_var("ALLTHECODES_HOME");
            }
        }
    }

    fn write_definition(workspace: &Path) {
        let definitions = workspace.join(".allthecodes/workflows");
        fs::create_dir_all(&definitions).expect("definitions directory");
        fs::write(
            definitions.join("release.md"),
            "- Plan release\n- Ship release\n",
        )
        .expect("definition");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn production_policy_fails_closed_without_mutating() {
        let workspace = tempfile::tempdir().expect("workspace");
        let home = tempfile::tempdir().expect("home");
        let _guard = EnvGuard::isolated(home.path());
        write_definition(workspace.path());
        let result = start_in_workspace(
            workspace.path().to_path_buf(),
            WorkflowRunStartParams {
                workflow: "release".to_string(),
                request_id: "request-1".to_string(),
                args: Some(json!({"version": "1.0.0"})),
            },
            &ProductionWorkflowMutationPolicy,
        )
        .await;
        assert!(matches!(result, Err(ProtocolApiError::Conflict { .. })));
        assert!(!workspace.path().join(".allthecodes/workflow-runs").exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn explicit_allow_fixture_exercises_idempotent_lifecycle() {
        let workspace = tempfile::tempdir().expect("workspace");
        let home = tempfile::tempdir().expect("home");
        let _guard = EnvGuard::isolated(home.path());
        write_definition(workspace.path());
        let params = WorkflowRunStartParams {
            workflow: "release".to_string(),
            request_id: "request-1".to_string(),
            args: Some(json!({"version": "1.0.0"})),
        };
        let first = start_in_workspace(
            workspace.path().to_path_buf(),
            params.clone(),
            &AllowWorkflowMutationsForTest,
        )
        .await
        .expect("start");
        let replay = start_in_workspace(
            workspace.path().to_path_buf(),
            params,
            &AllowWorkflowMutationsForTest,
        )
        .await
        .expect("replay");
        assert_eq!(first.run.run_id, replay.run.run_id);
        assert_eq!(first.run.revision, 1);
        assert!(replay.mutation.expect("metadata").replayed);
    }
}
