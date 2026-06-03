//! Activity Recorder status and stored-session handlers.

use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;

use allthecodes_config::paths;

use crate::handlers::{setting_bool, ApiError};
use crate::state::WebState;

#[derive(Serialize)]
pub struct ActivityRecorderStatusResponse {
    pub enabled: bool,
    pub available: bool,
    pub status: String,
    pub diagnostics: Vec<String>,
}

#[derive(Serialize)]
pub struct ActivityRecorderSessionsResponse {
    pub sessions: Vec<ActivityRecorderSession>,
}

#[derive(Serialize)]
pub struct ActivityRecorderSession {
    pub id: String,
    pub path: PathBuf,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Serialize)]
pub struct ActivityRecorderClearResponse {
    pub ok: bool,
    pub cleared: usize,
    pub sessions: Vec<ActivityRecorderSession>,
}

/// GET /api/activity-recorder/status
pub async fn activity_recorder_status_handler(State(state): State<WebState>) -> impl IntoResponse {
    Json(ActivityRecorderStatusResponse {
        enabled: setting_bool(&state, "activityRecorder.enabled")
            .or_else(|| setting_bool(&state, "activity_recorder.enabled"))
            .unwrap_or(false),
        available: false,
        status: "not_implemented".to_string(),
        diagnostics: vec![
            "activity recorder runtime is not implemented by this backend".to_string(),
        ],
    })
}

/// GET /api/activity-recorder/sessions
pub async fn activity_recorder_sessions_handler() -> Response {
    match list_sessions() {
        Ok(sessions) => Json(ActivityRecorderSessionsResponse { sessions }).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// POST /api/activity-recorder/clear
pub async fn activity_recorder_clear_handler() -> Response {
    match clear_sessions() {
        Ok(cleared) => Json(ActivityRecorderClearResponse {
            ok: true,
            cleared,
            sessions: Vec::new(),
        })
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

fn sessions_dir() -> PathBuf {
    paths::data_root()
        .join("activity-recorder")
        .join("sessions")
}

fn list_sessions() -> Result<Vec<ActivityRecorderSession>, String> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|error| format!("failed to read {}: {}", dir.display(), error))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read activity recorder session entry in {}: {}",
                dir.display(),
                error
            )
        })?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("failed to inspect {}: {}", path.display(), error))?;
        let id = entry.file_name().to_string_lossy().to_string();
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            "other"
        }
        .to_string();
        let modified_at = metadata
            .modified()
            .ok()
            .map(|time| DateTime::<Utc>::from(time).to_rfc3339());
        sessions.push(ActivityRecorderSession {
            id,
            path,
            kind,
            modified_at,
        });
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(sessions)
}

fn clear_sessions() -> Result<usize, String> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let sessions = list_sessions()?;
    for session in &sessions {
        if session.path.is_dir() {
            std::fs::remove_dir_all(&session.path).map_err(|error| {
                format!("failed to remove {}: {}", session.path.display(), error)
            })?;
        } else {
            std::fs::remove_file(&session.path).map_err(|error| {
                format!("failed to remove {}: {}", session.path.display(), error)
            })?;
        }
    }
    Ok(sessions.len())
}

fn internal_error(error: String) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error,
            code: "internal_error".into(),
        }),
    )
}
