//! Product-facing KAIROS configuration and lifecycle handlers.

use std::path::{Path, PathBuf};

use allthecodes_protocol::v1::kairos::{
    KairosApplyMode, KairosConfigUpdateRequest, KairosControlParameters, KairosResponse,
};
use allthecodes_types::kairos::{KairosControlAction, KairosControlRequest};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::state::WebState;

pub async fn status(State(state): State<WebState>) -> Response {
    let cwd = PathBuf::from(state.engine().cwd());
    run_snapshot(cwd).await
}

pub async fn configure(
    State(state): State<WebState>,
    Json(request): Json<KairosConfigUpdateRequest>,
) -> Response {
    if request.profile.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "kairos_invalid_profile",
            "KAIROS profile patch must contain at least one field",
            false,
        );
    }
    let cwd = PathBuf::from(state.engine().cwd());
    let operation_cwd = cwd.clone();
    let configured = tokio::task::spawn_blocking(move || {
        allthecodes_daemon::process_state::configure_kairos(&cwd, request.scope, &request.profile)
    })
    .await;
    let snapshot = match join_result(configured) {
        Ok(snapshot) => snapshot,
        Err(response) => return response,
    };
    if request.apply == KairosApplyMode::None {
        return Json(KairosResponse {
            snapshot,
            operation: None,
        })
        .into_response();
    }
    run_control(
        KairosControlAction::Reconcile,
        operation_cwd,
        KairosControlParameters::default(),
    )
    .await
}

pub async fn start(
    State(state): State<WebState>,
    Json(request): Json<KairosControlParameters>,
) -> Response {
    control_from_state(state, KairosControlAction::Start, request).await
}

pub async fn stop(
    State(state): State<WebState>,
    Json(request): Json<KairosControlParameters>,
) -> Response {
    control_from_state(state, KairosControlAction::Stop, request).await
}

pub async fn restart(
    State(state): State<WebState>,
    Json(request): Json<KairosControlParameters>,
) -> Response {
    control_from_state(state, KairosControlAction::Restart, request).await
}

async fn control_from_state(
    state: WebState,
    action: KairosControlAction,
    request: KairosControlParameters,
) -> Response {
    let workspace = PathBuf::from(state.engine().cwd());
    let cwd = match authorized_cwd(&workspace, request.cwd.as_deref()) {
        Ok(cwd) => cwd,
        Err(message) => {
            return error_response(
                StatusCode::FORBIDDEN,
                "kairos_workspace_forbidden",
                &message,
                false,
            );
        }
    };
    run_control(action, cwd, request).await
}

async fn run_snapshot(cwd: PathBuf) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        allthecodes_daemon::process_state::kairos_snapshot(&cwd)
    })
    .await;
    match join_result(result) {
        Ok(snapshot) => Json(KairosResponse {
            snapshot,
            operation: None,
        })
        .into_response(),
        Err(response) => response,
    }
}

async fn run_control(
    action: KairosControlAction,
    cwd: PathBuf,
    request: KairosControlParameters,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        allthecodes_daemon::process_state::LocalKairosController.control(KairosControlRequest {
            action,
            cwd: Some(cwd.display().to_string()),
            port: request.port,
            readiness_timeout_ms: request.readiness_timeout_ms,
        })
    })
    .await;
    match join_result(result) {
        Ok(operation) => Json(KairosResponse {
            snapshot: operation.snapshot.clone(),
            operation: Some(operation),
        })
        .into_response(),
        Err(response) => response,
    }
}

fn join_result<T>(
    result: Result<anyhow::Result<T>, tokio::task::JoinError>,
) -> Result<T, Response> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(controller_error_response(&error)),
        Err(error) => Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "kairos_operation_failed",
            &format!("KAIROS operation task failed: {error}"),
            true,
        )),
    }
}

fn controller_error_response(error: &anyhow::Error) -> Response {
    let message = error.to_string();
    let (status, code, retryable) = if message.contains("disabled") {
        (StatusCode::CONFLICT, "kairos_disabled", false)
    } else if message.contains("readiness") || message.contains("ready") {
        (
            StatusCode::GATEWAY_TIMEOUT,
            "kairos_readiness_timeout",
            true,
        )
    } else if message.contains("lock") {
        (StatusCode::CONFLICT, "kairos_operation_conflict", true)
    } else if message.contains("spawn") {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "kairos_spawn_failed",
            true,
        )
    } else if message.contains("settings") || message.contains("write") {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "kairos_settings_write_failed",
            false,
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "kairos_operation_failed",
            true,
        )
    };
    error_response(status, code, &message, retryable)
}

fn error_response(
    status: StatusCode,
    code: &'static str,
    message: &str,
    retryable: bool,
) -> Response {
    (
        status,
        Json(json!({
            "error": message,
            "code": code,
            "retryable": retryable,
        })),
    )
        .into_response()
}

fn authorized_cwd(workspace: &Path, requested: Option<&str>) -> Result<PathBuf, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| format!("workspace path is unavailable: {error}"))?;
    let Some(requested) = requested else {
        return Ok(workspace);
    };
    let requested = PathBuf::from(requested)
        .canonicalize()
        .map_err(|error| format!("requested cwd is unavailable: {error}"))?;
    if requested == workspace || requested.starts_with(&workspace) {
        Ok(requested)
    } else {
        Err("requested cwd is outside the active workspace".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_authorization_rejects_sibling_workspace() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let sibling = root.path().join("sibling");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(authorized_cwd(&workspace, Some(sibling.to_str().unwrap())).is_err());
        assert!(authorized_cwd(&workspace, Some(workspace.to_str().unwrap())).is_ok());
    }
}
