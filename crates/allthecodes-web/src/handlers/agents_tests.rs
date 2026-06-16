use super::*;
use crate::handlers::test_support::*;
use allthecodes_ipc_protocol::subsystem_types::AgentDefinitionSource;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn agents_rest_handlers_list_persist_and_restore() {
    let (home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = agents_list_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .any(|agent| agent["name"] == json!("general-purpose")));
    assert!(body["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .any(|tool| tool["name"] == json!("Read")));

    let response = agents_create_handler(
        State(state.clone()),
        Json(AgentUpsertRequest {
            entry: make_agent_entry("web-user", AgentDefinitionSource::User),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(home.path().join("agents/web-user.md").exists());

    let response = agents_create_handler(
        State(state.clone()),
        Json(AgentUpsertRequest {
            entry: make_agent_entry("web-project", AgentDefinitionSource::Project),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(project
        .path()
        .join(".allthecodes/agents/web-project.md")
        .exists());

    let response = agents_update_handler(
        State(state.clone()),
        AxumPath("general-purpose".to_string()),
        Json(AgentUpsertRequest {
            entry: make_agent_entry("general-purpose", AgentDefinitionSource::Builtin),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("validation_error"));

    let response = agents_delete_handler(
        State(state.clone()),
        AxumPath("general-purpose".to_string()),
        Query(AgentDeleteQuery {
            source: "builtin".to_string(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = agents_create_handler(
        State(state.clone()),
        Json(AgentUpsertRequest {
            entry: make_agent_entry("general-purpose", AgentDefinitionSource::User),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(home.path().join("agents/general-purpose.md").exists());

    let response = agents_restore_handler(
        State(state.clone()),
        AxumPath("general-purpose".to_string()),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!home.path().join("agents/general-purpose.md").exists());
    let body = response_json(response).await;
    assert!(body["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .any(|agent| {
            agent["name"] == json!("general-purpose") && agent["source"]["kind"] == json!("builtin")
        }));
}
