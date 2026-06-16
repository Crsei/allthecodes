use super::*;
use crate::handlers::test_support::*;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn channels_report_stopped_daemon_without_starting_it() {
    let (_home, _guard) = temp_home();

    let response = channels_list_handler().await.into_response();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("daemon_stopped"));
}
