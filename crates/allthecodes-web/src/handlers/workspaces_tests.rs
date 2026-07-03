use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn workspaces_patch_persists_sidebar_metadata() {
    let (home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());
    let workspace_key = allthecodes_session::storage::workspace_key(project.path());

    let response = workspace_patch_handler(
        AxumPath(workspace_key.clone()),
        State(state.clone()),
        Json(WorkspacePatchRequest {
            display_name: Some(Some("Frontend".to_string())),
            pinned: Some(true),
            hidden: Some(false),
            default_chat_mode: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["key"], json!(workspace_key));
    assert_eq!(body["display_name"], json!("Frontend"));
    assert_eq!(body["pinned"], json!(true));

    let metadata_path = home.path().join("web").join("workspaces.json");
    let persisted: Value =
        serde_json::from_str(&std::fs::read_to_string(metadata_path).expect("metadata file"))
            .expect("metadata json");
    assert_eq!(
        persisted["workspaces"][workspace_key.as_str()]["display_name"],
        json!("Frontend")
    );
}

#[tokio::test]
#[serial]
async fn workspace_archive_skips_active_and_archives_inactive_sessions() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());
    let inactive_id = "workspace-inactive-archive";
    allthecodes_session::storage::save_session(inactive_id, &[], project.path().to_str().unwrap())
        .expect("seed inactive session");
    allthecodes_session::storage::save_session(
        state.engine().current_session_id().as_ref(),
        &[],
        project.path().to_str().unwrap(),
    )
    .expect("seed active session");
    let workspace_key = allthecodes_session::storage::workspace_key(project.path());

    let response = workspace_sessions_archive_handler(
        AxumPath(workspace_key),
        State(state.clone()),
        Json(WorkspaceArchiveRequest {
            include_active: false,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["archived"], json!([inactive_id]));
    assert_eq!(body["skipped"][0]["reason"], json!("active_session"));
    assert!(!allthecodes_session::storage::get_session_file(inactive_id).exists());
    assert!(allthecodes_session::storage::get_archived_session_file(inactive_id).exists());
    assert!(allthecodes_session::storage::get_session_file(
        state.engine().current_session_id().as_ref()
    )
    .exists());
}
