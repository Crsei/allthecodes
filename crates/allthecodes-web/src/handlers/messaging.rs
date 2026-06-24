//! Messaging sections handlers.
//!
//! Manages collapsible session groups ("messaging sections") stored in SQLite.

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Json;
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};

use allthecodes_protocol::v1::messaging::{
    MessagingSectionCreateRequest, MessagingSectionCreateResponse, MessagingSectionDeleteResponse,
    MessagingSectionUpdateRequest, MessagingSectionUpdateResponse, MessagingSectionsListResponse,
    MessagingSectionsQuery,
};

use crate::handler_registry::HandlerRegistry;
use crate::state::WebState;
use allthecodes_protocol::ApiMethod;
use allthecodes_web_state::{normalize_owner, MessagingSectionRow};

static MSG_SECTION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_section_id() -> String {
    let now = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let counter = MSG_SECTION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("ms-{now}-{counter}")
}

// ---------------------------------------------------------------------------
// Axum handler bridge functions
// ---------------------------------------------------------------------------

fn section_from_row(
    row: MessagingSectionRow,
) -> allthecodes_protocol::v1::messaging::MessagingSection {
    let session_ids = row.session_ids();
    allthecodes_protocol::v1::messaging::MessagingSection {
        id: row.id,
        name: row.name,
        session_ids,
        position: row.position,
        created_at: row.created_at,
        profile_id: Some(row.owner_profile_id),
    }
}

async fn list_handler(
    State(state): State<WebState>,
    Query(query): Query<MessagingSectionsQuery>,
) -> Response {
    let owner = normalize_owner(query.profile_id.as_deref().unwrap_or_default());
    match state.web_ui_store.list_messaging_sections(&owner).await {
        Ok(rows) => {
            let sections = rows.into_iter().map(section_from_row).collect();
            Json(MessagingSectionsListResponse {
                sections,
                profile_id: Some(owner),
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn create_handler(
    State(state): State<WebState>,
    Json(req): Json<MessagingSectionCreateRequest>,
) -> Response {
    let owner = normalize_owner(req.profile_id.as_deref().unwrap_or_default());
    let row = allthecodes_web_state::MessagingSectionRow {
        id: new_section_id(),
        owner_profile_id: owner.clone(),
        name: req.name,
        session_ids_json: serde_json::to_string(&req.session_ids.unwrap_or_default())
            .unwrap_or_else(|_| "[]".into()),
        position: 0,
        created_at: Utc::now().to_rfc3339(),
    };
    match state
        .web_ui_store
        .create_messaging_section(&owner, &row)
        .await
    {
        Ok(()) => {
            let section = section_from_row(row);
            Json(MessagingSectionCreateResponse { section }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn update_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<MessagingSectionUpdateRequest>,
) -> Response {
    let owner = normalize_owner(req.profile_id.as_deref().unwrap_or_default());
    let session_ids_json = req
        .session_ids
        .as_ref()
        .map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()));
    match state
        .web_ui_store
        .update_messaging_section(
            &owner,
            &id,
            req.name.as_deref(),
            session_ids_json.as_deref(),
            req.position,
        )
        .await
    {
        Ok(Some(row)) => {
            let section = section_from_row(row);
            Json(MessagingSectionUpdateResponse { section }).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "section not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_handler(
    State(state): State<WebState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<MessagingSectionsQuery>,
) -> Response {
    let owner = normalize_owner(query.profile_id.as_deref().unwrap_or_default());
    match state
        .web_ui_store
        .delete_messaging_section(&owner, &id)
        .await
    {
        Ok(ok) => Json(MessagingSectionDeleteResponse { ok }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Handler registry
// ---------------------------------------------------------------------------

pub fn handlers() -> HandlerRegistry {
    HandlerRegistry::new()
        .handle(ApiMethod::MessagingSectionsList, get(list_handler))
        .handle(ApiMethod::MessagingSectionCreate, post(create_handler))
        .handle(ApiMethod::MessagingSectionUpdate, patch(update_handler))
        .handle(ApiMethod::MessagingSectionDelete, delete(delete_handler))
}
