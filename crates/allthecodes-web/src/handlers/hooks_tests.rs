use super::*;
use crate::handlers::test_support::*;
use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn hooks_crud_updates_user_settings_only_and_test_is_explicit_501() {
    let (home, _guard) = temp_home();
    let initial = allthecodes_config::settings::RawSettings {
        language: Some("en".to_string()),
        ..Default::default()
    };
    allthecodes_config::settings::write_user_settings(&initial).expect("seed settings");

    let config = json!({
        "matcher": "Read",
        "hooks": [{ "type": "command", "command": "echo ok" }]
    });
    let response = hooks_create_handler(Json(HookEventRequest {
        event: "PreToolUse".to_string(),
        configs: vec![config.clone()],
    }))
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert_eq!(raw.language.as_deref(), Some("en"));
    assert_eq!(
        raw.hooks.as_ref().and_then(|hooks| hooks.get("PreToolUse")),
        Some(&json!([config.clone()]))
    );

    let response = hooks_create_handler(Json(HookEventRequest {
        event: "PreToolUse".to_string(),
        configs: vec![config.clone()],
    }))
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let response = hooks_update_handler(
        AxumPath("PreToolUse".to_string()),
        Json(HookEventUpdateRequest {
            configs: vec![json!({ "matcher": "*", "hooks": [] })],
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let response = hooks_create_handler(Json(HookEventRequest {
        event: "PostToolUse".to_string(),
        configs: vec![json!({ "matcher": "Read" })],
    }))
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = hooks_test_handler(Json(HookEventRequest {
        event: "PreToolUse".to_string(),
        configs: vec![json!({ "matcher": "*", "hooks": [] })],
    }))
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("hook_test_not_implemented"));

    let response = hooks_delete_handler(AxumPath("PreToolUse".to_string()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert!(!raw.hooks.unwrap_or_default().contains_key("PreToolUse"));
}
