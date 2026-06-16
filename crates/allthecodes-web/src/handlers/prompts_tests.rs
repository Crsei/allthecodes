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
async fn prompts_crud_round_trips_store_and_rejects_slash_names() {
    let (home, _guard) = temp_home();
    let state = make_web_state();
    use allthecodes_protocol::v1::prompts::PromptCreateRequest as ProtocolPromptCreateRequest;

    let response = prompts_create_handler(
        State(state.clone()),
        Json(ProtocolPromptCreateRequest {
            id: None,
            name: "Summarize Thread".to_string(),
            content: "Summarize this thread.".to_string(),
            description: "summary prompt".to_string(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["prompt"]["id"], json!("summarize-thread"));
    assert!(home.path().join("quick-prompts.json").exists());

    let response = prompts_create_handler(
        State(state.clone()),
        Json(ProtocolPromptCreateRequest {
            id: Some("summarize-thread".to_string()),
            name: "Duplicate".to_string(),
            content: "duplicate".to_string(),
            description: String::new(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = prompts_create_handler(
        State(state),
        Json(ProtocolPromptCreateRequest {
            id: None,
            name: "/bad".to_string(),
            content: "bad".to_string(),
            description: String::new(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = prompts_update_handler(
        AxumPath("summarize-thread".to_string()),
        Json(PromptUpdateRequest {
            name: Some("Summarize".to_string()),
            content: Some("Updated".to_string()),
            description: Some("updated description".to_string()),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["content"], json!("Updated"));

    let response = prompts_delete_handler(AxumPath("summarize-thread".to_string()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["prompts"], json!([]));
}
