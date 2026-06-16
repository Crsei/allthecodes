//! MVP Kanban REST handlers backed by a local JSON file.

use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use axum::extract::{Path as AxumPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use allthecodes_config::paths;

use allthecodes_protocol::ApiError as ProtocolApiError;

use allthecodes_protocol::v1::kanban::KanbanBoardsResponse as ProtocolKanbanBoardsResponse;
use allthecodes_protocol::v1::kanban::KanbanTaskMutationResponse as ProtocolKanbanTaskMutationResponse;
use allthecodes_protocol::ApiMethod;
use async_trait::async_trait;
use axum::extract::State;
use axum::routing::{get, post};

use crate::api_dispatcher::rest_processor_response;
use crate::handler_registry::HandlerRegistry;
use crate::processors::Processor;

type BoxResponse = Box<Response>;
use crate::serialization::SerializationLayer;
use crate::state::WebState;

// ---------------------------------------------------------------------------
// Processors
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct KanbanBoardsProcessor {
    state: WebState,
}

impl From<WebState> for KanbanBoardsProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for KanbanBoardsProcessor {
    type Request = allthecodes_protocol::v1::kanban::KanbanQuery;
    type Response = ProtocolKanbanBoardsResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "kanban.boards"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, query: Self::Request) -> Result<Self::Response, Self::Error> {
        let store = read_store().map_err(|e| ProtocolApiError::Internal { message: e })?;
        let boards: Vec<allthecodes_protocol::v1::kanban::KanbanBoardSummary> = store
            .boards
            .iter()
            .map(|b| {
                serde_json::from_value(serde_json::to_value(board_summary(b)).unwrap()).unwrap()
            })
            .collect();
        Ok(ProtocolKanbanBoardsResponse {
            profile_id: query.profile_id,
            active_board_id: store.active_board_id,
            boards,
        })
    }
}

#[derive(Clone)]
pub struct KanbanTaskCreateProcessor {
    state: WebState,
}

impl From<WebState> for KanbanTaskCreateProcessor {
    fn from(state: WebState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Processor for KanbanTaskCreateProcessor {
    type Request = allthecodes_protocol::v1::kanban::KanbanTaskCreateRequest;
    type Response = ProtocolKanbanTaskMutationResponse;
    type Error = ProtocolApiError;

    fn handler_name() -> &'static str {
        "kanban.task_create"
    }

    fn serialization_layer(&self) -> Option<SerializationLayer> {
        Some(self.state.serialization.clone())
    }

    async fn handle(&self, request: Self::Request) -> Result<Self::Response, Self::Error> {
        // Bridge protocol request to handler request for the existing logic
        let handler_req: KanbanTaskCreateRequest =
            serde_json::from_value(serde_json::to_value(&request).map_err(|e| {
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
            })?)
            .map_err(|e| ProtocolApiError::Internal {
                message: e.to_string(),
            })?;

        match mutate_store(|store| create_task(store, handler_req)) {
            Ok(response) => {
                // Bridge handler response to protocol response
                Ok(
                    serde_json::from_value(serde_json::to_value(&response).map_err(|e| {
                        ProtocolApiError::Internal {
                            message: e.to_string(),
                        }
                    })?)
                    .map_err(|e| ProtocolApiError::Internal {
                        message: e.to_string(),
                    })?,
                )
            }
            Err(response) => {
                let status = response.status();
                Err(ProtocolApiError::Internal {
                    message: format!("Kanban operation failed (HTTP {})", status.as_u16()),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub(crate) fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::KanbanBoards, get(kanban_boards_handler))
        .handle(
            ApiMethod::KanbanTaskCreate,
            post(kanban_task_create_handler),
        )
}

// ---------------------------------------------------------------------------
// Handler types
// ---------------------------------------------------------------------------

static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactLink {
    #[serde(default)]
    pub id: Option<String>,
    pub label: String,
    pub href: String,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KanbanColumn {
    pub id: String,
    pub title: String,
    pub status: String,
    pub order: u32,
    #[serde(default)]
    pub wip_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KanbanBoardSummary {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    pub revision: u64,
    #[serde(default)]
    pub columns: Vec<KanbanColumn>,
    #[serde(default)]
    pub task_count: Option<usize>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KanbanTaskComment {
    pub id: String,
    #[serde(default)]
    pub author: Option<String>,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KanbanTask {
    pub id: String,
    pub board_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub status: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<ArtifactLink>,
    #[serde(default)]
    pub comments: Vec<KanbanTaskComment>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KanbanBoard {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub columns: Vec<KanbanColumn>,
    #[serde(default)]
    pub tasks: Vec<KanbanTask>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KanbanStore {
    #[serde(default = "default_store_version")]
    pub version: u32,
    #[serde(default)]
    pub active_board_id: Option<String>,
    #[serde(default)]
    pub boards: Vec<KanbanBoard>,
}

#[derive(Debug, Deserialize)]
pub struct KanbanQuery {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KanbanBoardsResponse {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub active_board_id: Option<String>,
    pub boards: Vec<KanbanBoardSummary>,
}

#[derive(Debug, Serialize)]
pub struct KanbanBoardDetailResponse {
    #[serde(default)]
    pub profile_id: Option<String>,
    pub board: KanbanBoardSummary,
    #[serde(default)]
    pub columns: Vec<KanbanColumn>,
    pub tasks: Vec<KanbanTask>,
}

#[derive(Debug, Deserialize)]
pub struct KanbanTaskCreateRequest {
    pub board_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub artifact_links: Vec<ArtifactLink>,
    #[serde(default)]
    pub revision: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct KanbanTaskUpdateRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<Option<String>>,
    #[serde(default)]
    pub assignee: Option<Option<String>>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub session_id: Option<Option<String>>,
    #[serde(default)]
    pub artifact_links: Option<Vec<ArtifactLink>>,
    pub revision: u64,
}

#[derive(Debug, Serialize)]
pub struct KanbanTaskMutationResponse {
    pub task: KanbanTask,
    #[serde(default)]
    pub board: Option<KanbanBoardSummary>,
}

#[derive(Debug, Deserialize)]
pub struct KanbanCommentCreateRequest {
    pub body: String,
    pub revision: u64,
}

/// GET /api/kanban/boards?profile_id=
pub async fn kanban_boards_handler(
    State(state): State<WebState>,
    Query(query): Query<allthecodes_protocol::v1::kanban::KanbanQuery>,
) -> Response {
    rest_processor_response::<KanbanBoardsProcessor>(state, ApiMethod::KanbanBoards, query).await
}

/// GET /api/kanban/boards/{id}?profile_id=
pub async fn kanban_board_detail_handler(
    AxumPath(id): AxumPath<String>,
    Query(query): Query<KanbanQuery>,
) -> Response {
    match read_store() {
        Ok(store) => match store.boards.iter().find(|board| board.id == id) {
            Some(board) => Json(KanbanBoardDetailResponse {
                profile_id: query.profile_id,
                board: board_summary(board),
                columns: sorted_columns(board.columns.clone()),
                tasks: board.tasks.clone(),
            })
            .into_response(),
            None => not_found(format!("Kanban board '{}' not found", id)),
        },
        Err(error) => internal_error(error),
    }
}

/// POST /api/kanban/tasks
pub async fn kanban_task_create_handler(
    State(state): State<WebState>,
    Json(req): Json<allthecodes_protocol::v1::kanban::KanbanTaskCreateRequest>,
) -> Response {
    rest_processor_response::<KanbanTaskCreateProcessor>(state, ApiMethod::KanbanTaskCreate, req)
        .await
}

/// PATCH /api/kanban/tasks/{id}
pub async fn kanban_task_update_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<KanbanTaskUpdateRequest>,
) -> Response {
    match mutate_store(|store| update_task(store, &id, req)) {
        Ok(response) => Json(response).into_response(),
        Err(error) => (*error).into_response(),
    }
}

/// POST /api/kanban/tasks/{id}/comments
pub async fn kanban_task_comment_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<KanbanCommentCreateRequest>,
) -> Response {
    match mutate_store(|store| add_comment(store, &id, req)) {
        Ok(response) => Json(response).into_response(),
        Err(error) => (*error).into_response(),
    }
}

fn read_store() -> Result<KanbanStore, String> {
    let _guard = lock_store()?;
    load_store_locked()
}

fn mutate_store<T>(
    mutate: impl FnOnce(&mut KanbanStore) -> Result<T, BoxResponse>,
) -> Result<T, BoxResponse> {
    let _guard = lock_store().map_err(|e| Box::new(internal_error(e)))?;
    let mut store = load_store_locked().map_err(|e| Box::new(internal_error(e)))?;
    let result = mutate(&mut store)?;
    write_store_locked(&store).map_err(|e| Box::new(internal_error(e)))?;
    Ok(result)
}

fn lock_store() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    STORE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Kanban store lock is poisoned".to_string())
}

fn load_store_locked() -> Result<KanbanStore, String> {
    let path = store_path();
    let mut should_write = false;
    let mut store = if !path.exists() {
        should_write = true;
        default_store()
    } else {
        let raw = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
        if raw.trim().is_empty() {
            should_write = true;
            default_store()
        } else {
            serde_json::from_str::<KanbanStore>(&raw).map_err(|err| err.to_string())?
        }
    };

    if normalize_store(&mut store) {
        should_write = true;
    }
    if should_write {
        write_store_locked(&store)?;
    }
    Ok(store)
}

fn write_store_locked(store: &KanbanStore) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(store).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn store_path() -> PathBuf {
    paths::data_root().join("web").join("kanban.json")
}

fn default_store_version() -> u32 {
    1
}

fn normalize_store(store: &mut KanbanStore) -> bool {
    let mut changed = false;
    if store.version == 0 {
        store.version = 1;
        changed = true;
    }
    if store.boards.is_empty() {
        *store = default_store();
        return true;
    }
    if store.active_board_id.is_none()
        || !store
            .boards
            .iter()
            .any(|board| Some(&board.id) == store.active_board_id.as_ref())
    {
        store.active_board_id = store.boards.first().map(|board| board.id.clone());
        changed = true;
    }
    for board in &mut store.boards {
        if board.columns.is_empty() {
            board.columns = default_columns();
            changed = true;
        }
        if board.revision == 0 {
            board.revision = 1;
            changed = true;
        }
        for task in &mut board.tasks {
            if task.revision == 0 {
                task.revision = 1;
                changed = true;
            }
        }
    }
    changed
}

fn default_store() -> KanbanStore {
    let now = now_timestamp();
    KanbanStore {
        version: 1,
        active_board_id: Some("default".to_string()),
        boards: vec![KanbanBoard {
            id: "default".to_string(),
            name: "Default Board".to_string(),
            description: Some("Default Kanban board".to_string()),
            profile_id: None,
            revision: 1,
            columns: default_columns(),
            tasks: Vec::new(),
            updated_at: Some(now),
        }],
    }
}

fn default_columns() -> Vec<KanbanColumn> {
    [
        ("backlog", "Backlog"),
        ("todo", "To Do"),
        ("in_progress", "In Progress"),
        ("review", "Review"),
        ("blocked", "Blocked"),
        ("done", "Done"),
    ]
    .into_iter()
    .enumerate()
    .map(|(order, (status, title))| KanbanColumn {
        id: status.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        order: order as u32,
        wip_limit: None,
    })
    .collect()
}

fn create_task(
    store: &mut KanbanStore,
    req: KanbanTaskCreateRequest,
) -> Result<KanbanTaskMutationResponse, BoxResponse> {
    let board = store
        .boards
        .iter_mut()
        .find(|board| board.id == req.board_id)
        .ok_or_else(|| {
            Box::new(not_found(format!(
                "Kanban board '{}' not found",
                req.board_id
            )))
        })?;
    let title = clean_required(req.title, "Task title")?;
    let now = now_timestamp();
    let task = KanbanTask {
        id: next_id("task"),
        board_id: board.id.clone(),
        title,
        description: clean_optional(req.description),
        status: req
            .status
            .and_then(|status| clean_optional(Some(status)))
            .unwrap_or_else(|| "backlog".to_string()),
        priority: clean_optional(req.priority),
        assignee: clean_optional(req.assignee),
        labels: clean_labels(req.labels),
        session_id: clean_optional(req.session_id),
        artifact_links: req.artifact_links,
        comments: Vec::new(),
        revision: 1,
        created_at: Some(now),
        updated_at: Some(now),
    };
    board.revision = board.revision.saturating_add(1);
    board.updated_at = Some(now);
    board.tasks.push(task.clone());
    Ok(KanbanTaskMutationResponse {
        task,
        board: Some(board_summary(board)),
    })
}

fn update_task(
    store: &mut KanbanStore,
    id: &str,
    req: KanbanTaskUpdateRequest,
) -> Result<KanbanTaskMutationResponse, BoxResponse> {
    let (board, task_index) = find_task_mut(store, id)?;
    if board.tasks[task_index].revision != req.revision {
        return Err(Box::new(revision_conflict(
            "Kanban task revision conflict".to_string(),
        )));
    }

    let task = &mut board.tasks[task_index];
    if let Some(title) = req.title {
        task.title = clean_required(title, "Task title")?;
    }
    if let Some(description) = req.description {
        task.description = clean_optional(description);
    }
    if let Some(status) = req.status {
        task.status = clean_required(status, "Task status")?;
    }
    if let Some(priority) = req.priority {
        task.priority = clean_optional(priority);
    }
    if let Some(assignee) = req.assignee {
        task.assignee = clean_optional(assignee);
    }
    if let Some(labels) = req.labels {
        task.labels = clean_labels(labels);
    }
    if let Some(session_id) = req.session_id {
        task.session_id = clean_optional(session_id);
    }
    if let Some(artifact_links) = req.artifact_links {
        task.artifact_links = artifact_links;
    }

    let now = now_timestamp();
    task.revision = task.revision.saturating_add(1);
    task.updated_at = Some(now);
    board.revision = board.revision.saturating_add(1);
    board.updated_at = Some(now);

    Ok(KanbanTaskMutationResponse {
        task: board.tasks[task_index].clone(),
        board: Some(board_summary(board)),
    })
}

fn add_comment(
    store: &mut KanbanStore,
    id: &str,
    req: KanbanCommentCreateRequest,
) -> Result<KanbanTaskMutationResponse, BoxResponse> {
    let body = clean_required(req.body, "Comment body")?;
    let (board, task_index) = find_task_mut(store, id)?;
    if board.tasks[task_index].revision != req.revision {
        return Err(Box::new(revision_conflict(
            "Kanban task revision conflict".to_string(),
        )));
    }

    let now = now_timestamp();
    let task = &mut board.tasks[task_index];
    task.comments.push(KanbanTaskComment {
        id: next_id("comment"),
        author: None,
        body,
        created_at: now,
    });
    task.revision = task.revision.saturating_add(1);
    task.updated_at = Some(now);
    board.revision = board.revision.saturating_add(1);
    board.updated_at = Some(now);

    Ok(KanbanTaskMutationResponse {
        task: board.tasks[task_index].clone(),
        board: Some(board_summary(board)),
    })
}

fn find_task_mut<'a>(
    store: &'a mut KanbanStore,
    id: &str,
) -> Result<(&'a mut KanbanBoard, usize), BoxResponse> {
    for board in &mut store.boards {
        if let Some(index) = board.tasks.iter().position(|task| task.id == id) {
            return Ok((board, index));
        }
    }
    Err(Box::new(not_found(format!(
        "Kanban task '{}' not found",
        id
    ))))
}

fn board_summary(board: &KanbanBoard) -> KanbanBoardSummary {
    KanbanBoardSummary {
        id: board.id.clone(),
        name: board.name.clone(),
        description: board.description.clone(),
        profile_id: board.profile_id.clone(),
        revision: board.revision,
        columns: sorted_columns(board.columns.clone()),
        task_count: Some(board.tasks.len()),
        updated_at: board.updated_at,
    }
}

fn sorted_columns(mut columns: Vec<KanbanColumn>) -> Vec<KanbanColumn> {
    columns.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
    columns
}

fn clean_required(value: String, label: &str) -> Result<String, BoxResponse> {
    let value = value.trim();
    if value.is_empty() {
        Err(Box::new(validation_error(format!(
            "{} cannot be empty",
            label
        ))))
    } else {
        Ok(value.to_string())
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

fn clean_labels(labels: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for label in labels {
        let label = label.trim();
        if !label.is_empty()
            && !out
                .iter()
                .any(|existing: &String| existing.as_str() == label)
        {
            out.push(label.to_string());
        }
    }
    out
}

fn next_id(prefix: &str) -> String {
    let ts = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_else(|| Utc::now().timestamp_micros().saturating_mul(1_000));
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}-{}-{}", prefix, ts, process::id(), counter)
}

fn now_timestamp() -> i64 {
    Utc::now().timestamp()
}

fn validation_error(error: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "validation_error",
        message: error,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn revision_conflict(error: String) -> Response {
    let body = ProtocolApiError::Conflict { reason: error }.into_body();
    (StatusCode::CONFLICT, Json(body)).into_response()
}

fn not_found(error: String) -> Response {
    let body = ProtocolApiError::NotFound {
        entity: "kanban_item",
        id: error,
    }
    .into_body();
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

fn internal_error(error: String) -> Response {
    let body = ProtocolApiError::Internal { message: error }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_store_has_required_columns() {
        let store = default_store();
        let board = &store.boards[0];
        let statuses: Vec<&str> = board
            .columns
            .iter()
            .map(|column| column.status.as_str())
            .collect();

        assert_eq!(
            statuses,
            vec![
                "backlog",
                "todo",
                "in_progress",
                "review",
                "blocked",
                "done"
            ]
        );
        assert_eq!(store.active_board_id.as_deref(), Some("default"));
    }

    #[test]
    fn update_rejects_stale_revision() {
        let mut store = default_store();
        let created = create_task(
            &mut store,
            KanbanTaskCreateRequest {
                board_id: "default".to_string(),
                title: "Ship Kanban".to_string(),
                description: None,
                status: None,
                priority: None,
                assignee: None,
                labels: Vec::new(),
                session_id: None,
                artifact_links: Vec::new(),
                revision: None,
            },
        )
        .expect("task is created");

        let err = update_task(
            &mut store,
            &created.task.id,
            KanbanTaskUpdateRequest {
                title: Some("New title".to_string()),
                description: None,
                status: None,
                priority: None,
                assignee: None,
                labels: None,
                session_id: None,
                artifact_links: None,
                revision: 0,
            },
        )
        .expect_err("stale revision is rejected");

        assert_eq!(err.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn comment_increments_task_and_board_revision() {
        let mut store = default_store();
        let created = create_task(
            &mut store,
            KanbanTaskCreateRequest {
                board_id: "default".to_string(),
                title: "Task".to_string(),
                description: None,
                status: None,
                priority: None,
                assignee: None,
                labels: Vec::new(),
                session_id: None,
                artifact_links: Vec::new(),
                revision: None,
            },
        )
        .expect("task is created");

        let response = add_comment(
            &mut store,
            &created.task.id,
            KanbanCommentCreateRequest {
                body: "Looks good".to_string(),
                revision: created.task.revision,
            },
        )
        .expect("comment is added");

        assert_eq!(response.task.revision, created.task.revision + 1);
        assert_eq!(response.task.comments.len(), 1);
        let created_board_revision = created.board.as_ref().map(|board| board.revision);
        assert_eq!(
            response.board.as_ref().map(|board| board.revision),
            created_board_revision.map(|revision| revision + 1)
        );
    }
}
