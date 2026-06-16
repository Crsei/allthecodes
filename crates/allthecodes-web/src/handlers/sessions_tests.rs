use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn session_archive_handler_returns_404_for_missing_session() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = session_archive_handler(AxumPath("missing-session".to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
    assert_eq!(body["details"]["entity"], json!("session"));
    assert_eq!(body["details"]["id"], json!("missing-session"));
}

#[tokio::test]
#[serial]
async fn session_archive_handler_rejects_active_session_with_409() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let active_id = state.engine().current_session_id().to_string();

    let response = session_archive_handler(AxumPath(active_id), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("conflict"));
    assert!(body["details"]["reason"].is_string());
}

#[tokio::test]
#[serial]
async fn session_archive_handler_archives_inactive_session() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "inactive-web-archive";
    allthecodes_session::storage::save_session(session_id, &[], ".")
        .expect("seed inactive session");

    let response = session_archive_handler(AxumPath(session_id.to_string()), State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["message"], json!("Session archived"));
    assert!(!allthecodes_session::storage::get_session_file(session_id).exists());
    assert!(allthecodes_session::storage::get_archived_session_file(session_id).exists());
}

#[tokio::test]
#[serial]
async fn session_new_handler_can_target_known_workspace_cwd() {
    let (_home, _guard) = temp_home();
    let current = tempfile::tempdir().expect("current");
    let target = tempfile::tempdir().expect("target");
    let state = make_web_state_with_cwd(current.path());
    allthecodes_session::storage::save_session(
        "target-session",
        &[],
        target.path().to_str().unwrap(),
    )
    .expect("seed target workspace");
    let workspace_key = allthecodes_session::storage::workspace_key(target.path());

    let response = session_new_handler(
        State(state.clone()),
        Some(Json(NewSessionRequest {
            workspace_key: Some(workspace_key),
            cwd: Some(target.path().to_string_lossy().to_string()),
        })),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
        allthecodes_session::storage::workspace_key(target.path())
    );
}

#[tokio::test]
#[serial]
async fn session_new_handler_can_target_existing_local_cwd() {
    let (_home, _guard) = temp_home();
    let current = tempfile::tempdir().expect("current");
    let target = tempfile::tempdir().expect("target");
    let state = make_web_state_with_cwd(current.path());

    let response = session_new_handler(
        State(state.clone()),
        Some(Json(NewSessionRequest {
            workspace_key: None,
            cwd: Some(target.path().to_string_lossy().to_string()),
        })),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
        allthecodes_session::storage::workspace_key(target.path())
    );
}

#[tokio::test]
#[serial]
async fn session_resume_reuses_cached_engine_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = state.engine().current_session_id().to_string();
    allthecodes_session::storage::save_session(&session_id, &[], ".").expect("seed cached session");
    state.engine().update_app_state(|app| {
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let response = session_resume_handler(AxumPath(session_id.clone()), State(state.clone()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let app_state = state.engine().app_state();
    assert!(app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
    assert_eq!(state.engine().current_session_id().to_string(), session_id);
}

#[tokio::test]
#[serial]
async fn session_new_inherits_runtime_permissions_but_clears_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    state.engine().update_app_state(|app| {
        app.tool_permission_context.mode = allthecodes_types::permissions::PermissionMode::Auto;
        app.tool_permission_context
            .always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        app.tool_permission_context
            .always_deny_rules
            .insert("user".into(), vec!["Write".into()]);
        app.tool_permission_context
            .always_ask_rules
            .insert("user".into(), vec!["Bash".into()]);
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let response = session_new_handler(State(state.clone()), None)
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let app_state = state.engine().app_state();
    assert_eq!(
        app_state.tool_permission_context.mode,
        allthecodes_types::permissions::PermissionMode::Auto
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_allow_rules
            .get("user"),
        Some(&vec!["Read".to_string()])
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_deny_rules
            .get("user"),
        Some(&vec!["Write".to_string()])
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_ask_rules
            .get("user"),
        Some(&vec!["Bash".to_string()])
    );
    assert!(!app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
}

#[tokio::test]
#[serial]
async fn cold_session_engine_inherits_runtime_permissions_but_clears_session_grants() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let session_id = "cold-session-engine";
    allthecodes_session::storage::save_session(session_id, &[], ".").expect("seed cold session");
    state.engine().update_app_state(|app| {
        app.tool_permission_context.mode = allthecodes_types::permissions::PermissionMode::Plan;
        app.tool_permission_context
            .always_allow_rules
            .insert("user".into(), vec!["Read".into()]);
        app.tool_permission_context
            .grant_session_allow("mcp__computer-use__screenshot");
    });

    let engine =
        crate::handlers::sessions::build_engine_for_session(&state, session_id).expect("engine");

    let app_state = engine.app_state();
    assert_eq!(
        app_state.tool_permission_context.mode,
        allthecodes_types::permissions::PermissionMode::Plan
    );
    assert_eq!(
        app_state
            .tool_permission_context
            .always_allow_rules
            .get("user"),
        Some(&vec!["Read".to_string()])
    );
    assert!(!app_state
        .tool_permission_context
        .has_session_grant("mcp__computer-use__screenshot"));
}
