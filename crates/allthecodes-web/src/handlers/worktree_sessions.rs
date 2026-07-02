//! Worktree session handlers.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Query, State};
use axum::response::Response;
use axum::routing::get;
use tracing::warn;

use allthecodes_protocol::v1::worktree_sessions::{
    CurrentWorktreeSessionResponse, WorktreeSessionBySessionParams, WorktreeSessionCreator,
    WorktreeSessionSource, WorktreeSessionStatus, WorktreeSessionSummary, WorktreeSessionsQuery,
    WorktreeSessionsResponse,
};
use allthecodes_protocol::ApiError as ProtocolApiError;
use allthecodes_protocol::ApiMethod;
use allthecodes_session::storage::workspace_key;
use allthecodes_session::worktree_sessions::{
    get_active_worktree_session, list_worktree_sessions_for_goal, list_worktree_sessions_for_repo,
    list_worktree_sessions_for_session, reconcile_worktree_session_records, WorktreeSessionRecord,
};
use allthecodes_utils::git::find_git_root;

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;
use crate::state::WebState;

#[derive(Clone)]
pub struct WorktreeSessionsListProcessor {
    state: WebState,
}

impl From<WebState> for WorktreeSessionsListProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorktreeSessionsListProcessor {
    type Request = WorktreeSessionsQuery;
    type Response = WorktreeSessionsResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "worktree_sessions.list"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let sessions = query_worktree_sessions(&self.state, &params)?;
        Ok(WorktreeSessionsResponse {
            ok: true,
            sessions: sessions.into_iter().map(worktree_session_summary).collect(),
        })
    }
}

#[derive(Clone)]
pub struct WorktreeSessionsCurrentProcessor {
    state: WebState,
}

impl From<WebState> for WorktreeSessionsCurrentProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorktreeSessionsCurrentProcessor {
    type Request = allthecodes_protocol::NoParams;
    type Response = CurrentWorktreeSessionResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "worktree_sessions.current"
    }

    async fn handle(&self, _params: Self::Request) -> Result<Self::Response, Self::Error> {
        let session_id = self.state.engine().current_session_id().to_string();
        let session = get_active_after_reconcile(&session_id)?;
        Ok(CurrentWorktreeSessionResponse {
            ok: true,
            session: session.map(worktree_session_summary),
        })
    }
}

#[derive(Clone)]
pub struct WorktreeSessionsBySessionProcessor {
    state: WebState,
}

impl From<WebState> for WorktreeSessionsBySessionProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for WorktreeSessionsBySessionProcessor {
    type Request = WorktreeSessionBySessionParams;
    type Response = WorktreeSessionsResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "worktree_sessions.by_session"
    }

    async fn handle(&self, params: Self::Request) -> Result<Self::Response, Self::Error> {
        let _ = &self.state;
        let sessions = list_session_after_reconcile(&params.session_id)?;
        Ok(WorktreeSessionsResponse {
            ok: true,
            sessions: sessions.into_iter().map(worktree_session_summary).collect(),
        })
    }
}

async fn list_handler(
    State(state): State<WebState>,
    Query(params): Query<WorktreeSessionsQuery>,
) -> Response {
    rest_processor_response::<WorktreeSessionsListProcessor>(
        state,
        ApiMethod::WorktreeSessionsList,
        params,
    )
    .await
}

async fn current_handler(State(state): State<WebState>) -> Response {
    rest_processor_response::<WorktreeSessionsCurrentProcessor>(
        state,
        ApiMethod::WorktreeSessionsCurrent,
        allthecodes_protocol::NoParams {},
    )
    .await
}

async fn by_session_handler(
    State(state): State<WebState>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    rest_processor_response::<WorktreeSessionsBySessionProcessor>(
        state,
        ApiMethod::WorktreeSessionsBySession,
        WorktreeSessionBySessionParams { session_id },
    )
    .await
}

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::WorktreeSessionsList, get(list_handler))
        .handle(ApiMethod::WorktreeSessionsCurrent, get(current_handler))
        .handle(
            ApiMethod::WorktreeSessionsBySession,
            get(by_session_handler),
        )
}

fn query_worktree_sessions(
    state: &WebState,
    params: &WorktreeSessionsQuery,
) -> Result<Vec<WorktreeSessionRecord>, ProtocolApiError> {
    if let Some(goal_id) = non_empty(params.goal_id.as_deref()) {
        let mut sessions = list_goal_after_reconcile(goal_id)?;
        if let Some(repo_id) = non_empty(params.repo_id.as_deref()) {
            sessions.retain(|session| session.repo_id == repo_id);
        }
        return Ok(sessions);
    }

    let repo_id = match non_empty(params.repo_id.as_deref()) {
        Some(repo_id) => repo_id.to_string(),
        None => repo_id_from_path(state, params.path.as_deref())?,
    };

    list_repo_after_reconcile(&repo_id)
}

fn repo_id_from_path(state: &WebState, path: Option<&str>) -> Result<String, ProtocolApiError> {
    let cwd = path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(state.engine().cwd()));
    let git_root = find_git_root(&cwd).ok_or_else(|| ProtocolApiError::BadRequest {
        code: "not_a_git_repo",
        message: "path is not inside a git repository".to_string(),
    })?;
    Ok(workspace_key(&git_root))
}

fn list_repo_after_reconcile(
    repo_id: &str,
) -> Result<Vec<WorktreeSessionRecord>, ProtocolApiError> {
    let sessions = list_worktree_sessions_for_repo(repo_id).map_err(store_error)?;
    let changed = reconcile_best_effort(&sessions)?;
    if changed > 0 {
        list_worktree_sessions_for_repo(repo_id).map_err(store_error)
    } else {
        Ok(sessions)
    }
}

fn list_goal_after_reconcile(
    goal_id: &str,
) -> Result<Vec<WorktreeSessionRecord>, ProtocolApiError> {
    let sessions = list_worktree_sessions_for_goal(goal_id).map_err(store_error)?;
    let changed = reconcile_best_effort(&sessions)?;
    if changed > 0 {
        list_worktree_sessions_for_goal(goal_id).map_err(store_error)
    } else {
        Ok(sessions)
    }
}

fn list_session_after_reconcile(
    session_id: &str,
) -> Result<Vec<WorktreeSessionRecord>, ProtocolApiError> {
    let sessions = list_worktree_sessions_for_session(session_id).map_err(store_error)?;
    let changed = reconcile_best_effort(&sessions)?;
    if changed > 0 {
        list_worktree_sessions_for_session(session_id).map_err(store_error)
    } else {
        Ok(sessions)
    }
}

fn get_active_after_reconcile(
    session_id: &str,
) -> Result<Option<WorktreeSessionRecord>, ProtocolApiError> {
    let active = get_active_worktree_session(session_id).map_err(store_error)?;
    if let Some(record) = active.as_ref() {
        reconcile_best_effort(std::slice::from_ref(record))?;
        get_active_worktree_session(session_id).map_err(store_error)
    } else {
        Ok(None)
    }
}

fn reconcile_best_effort(records: &[WorktreeSessionRecord]) -> Result<usize, ProtocolApiError> {
    reconcile_worktree_session_records(records).map_err(store_error)
}

fn worktree_session_summary(record: WorktreeSessionRecord) -> WorktreeSessionSummary {
    WorktreeSessionSummary {
        schema_version: record.schema_version,
        session_id: record.session_id,
        repo_id: record.repo_id,
        worktree_path: path_to_string(&record.worktree_path),
        branch: record.branch,
        base_commit: record.base_commit,
        linked_goal_id: record.linked_goal_id,
        created_by: match record.created_by {
            allthecodes_session::worktree_sessions::WorktreeSessionCreator::Human => {
                WorktreeSessionCreator::Human
            }
            allthecodes_session::worktree_sessions::WorktreeSessionCreator::Agent => {
                WorktreeSessionCreator::Agent
            }
        },
        git_root: path_to_string(&record.git_root),
        original_cwd: record.original_cwd.as_deref().map(path_to_string),
        status: match record.status {
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::Active => {
                WorktreeSessionStatus::Active
            }
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::Kept => {
                WorktreeSessionStatus::Kept
            }
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::Removed => {
                WorktreeSessionStatus::Removed
            }
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::CleanupFailed => {
                WorktreeSessionStatus::CleanupFailed
            }
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::Orphaned => {
                WorktreeSessionStatus::Orphaned
            }
        },
        created_at: record.created_at.to_rfc3339(),
        updated_at: record.updated_at.to_rfc3339(),
        kept_at: record.kept_at.map(|time| time.to_rfc3339()),
        removed_at: record.removed_at.map(|time| time.to_rfc3339()),
        source: match record.source {
            allthecodes_session::worktree_sessions::WorktreeSessionSource::EnterWorktree => {
                WorktreeSessionSource::EnterWorktree
            }
            allthecodes_session::worktree_sessions::WorktreeSessionSource::AgentIsolation => {
                WorktreeSessionSource::AgentIsolation
            }
            allthecodes_session::worktree_sessions::WorktreeSessionSource::WorktreeCreateHook => {
                WorktreeSessionSource::WorktreeCreateHook
            }
            allthecodes_session::worktree_sessions::WorktreeSessionSource::Imported => {
                WorktreeSessionSource::Imported
            }
        },
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn store_error(error: anyhow::Error) -> ProtocolApiError {
    warn!(%error, "worktree session store operation failed");
    ProtocolApiError::Internal {
        message: format!("Failed to read worktree sessions: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use axum::response::IntoResponse;
    use serde_json::json;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn list_handler_returns_repo_worktree_sessions() {
        let (_home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        git2::Repository::init(project.path()).expect("init repo");
        let state = make_web_state_with_cwd(project.path());
        let session_id = state.engine().current_session_id().to_string();
        let repo_id = workspace_key(project.path());
        seed_worktree_session(&session_id, &repo_id, project.path());

        let response = list_handler(
            State(state),
            Query(WorktreeSessionsQuery {
                repo_id: Some(repo_id),
                goal_id: None,
                path: None,
            }),
        )
        .await
        .into_response();

        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["sessions"].as_array().expect("sessions").len(), 1);
        assert_eq!(body["sessions"][0]["sessionId"], json!(session_id));
        assert_eq!(body["sessions"][0]["status"], json!("active"));
    }

    #[tokio::test]
    #[serial]
    async fn current_handler_drops_orphaned_active_session() {
        let (_home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        git2::Repository::init(project.path()).expect("init repo");
        let state = make_web_state_with_cwd(project.path());
        let session_id = state.engine().current_session_id().to_string();
        let repo_id = workspace_key(project.path());
        seed_worktree_session(&session_id, &repo_id, &project.path().join("missing"));

        let response = current_handler(State(state)).await.into_response();

        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert!(body.get("session").is_none());
        let sessions = list_worktree_sessions_for_session(&session_id).unwrap();
        assert_eq!(
            sessions[0].status,
            allthecodes_session::worktree_sessions::WorktreeSessionStatus::Orphaned
        );
    }

    fn seed_worktree_session(session_id: &str, repo_id: &str, worktree_path: &Path) {
        let record = WorktreeSessionRecord::new_active(
            session_id,
            repo_id,
            worktree_path,
            "main",
            None,
            Some("goal-1".to_string()),
            allthecodes_session::worktree_sessions::WorktreeSessionCreator::Human,
            worktree_path,
            None,
            allthecodes_session::worktree_sessions::WorktreeSessionSource::Imported,
        );
        allthecodes_session::worktree_sessions::upsert_worktree_session(&record)
            .expect("seed worktree session");
    }
}
