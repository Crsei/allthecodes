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
async fn people_crud_round_trips_json_and_validates_ids() {
    let (home, _guard) = temp_home();
    let state = make_web_state();
    use allthecodes_protocol::v1::people::PersonCreateRequest as ProtocolPersonCreateRequest;

    let response = people_create_handler(
        State(state.clone()),
        Json(ProtocolPersonCreateRequest {
            id: None,
            name: "Ada Lovelace".to_string(),
            telegram_id: Some("ada-tg".to_string()),
            discord_id: None,
            discord_username: Some("ada".to_string()),
            feishu_id: None,
            username: Some("ada".to_string()),
            profile_content: "First programmer".to_string(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["person"]["id"], json!("ada-lovelace"));
    assert!(home.path().join("people/ada-lovelace.json").exists());

    let response = people_create_handler(
        State(state.clone()),
        Json(ProtocolPersonCreateRequest {
            id: Some("ada-lovelace".to_string()),
            name: "Ada Duplicate".to_string(),
            telegram_id: None,
            discord_id: None,
            discord_username: None,
            feishu_id: None,
            username: None,
            profile_content: String::new(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = people_update_handler(
        AxumPath("ada-lovelace".to_string()),
        Json(PersonUpdateRequest {
            name: Some("Ada Byron".to_string()),
            telegram_id: Some(None),
            discord_id: None,
            discord_username: None,
            feishu_id: None,
            username: None,
            profile_content: Some("Updated profile".to_string()),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["name"], json!("Ada Byron"));
    assert_eq!(body["telegram_id"], Value::Null);

    let response = people_create_handler(
        State(state),
        Json(ProtocolPersonCreateRequest {
            id: Some("bad/id".to_string()),
            name: "Bad".to_string(),
            telegram_id: None,
            discord_id: None,
            discord_username: None,
            feishu_id: None,
            username: None,
            profile_content: String::new(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = people_delete_handler(AxumPath("ada-lovelace".to_string()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!home.path().join("people/ada-lovelace.json").exists());
}
