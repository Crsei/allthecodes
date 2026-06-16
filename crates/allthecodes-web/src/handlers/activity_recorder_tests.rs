use super::*;
use crate::handlers::test_support::*;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn activity_recorder_empty_store_status_sessions_and_clear_round_trip() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = activity_recorder_status_handler(State(state))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["enabled"], json!(false));
    assert_eq!(body["available"], json!(false));

    let response = activity_recorder_sessions_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["sessions"], json!([]));

    let response = activity_recorder_clear_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["cleared"], json!(0));
    assert_eq!(body["sessions"], json!([]));
}
