use super::*;
use crate::handlers::test_support::*;
use crate::handlers::{
    appshots_capture_handler, chrome_relay_launch_handler, chrome_relay_token_regenerate_handler,
};
use axum::extract::Path as AxumPath;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn system_action_endpoints_return_explicit_501_codes() {
    let (_home, _guard) = temp_home();

    let response = computer_use_test_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("computer_use_test_not_implemented"));

    let response = computer_use_permission_request_handler(AxumPath("accessibility".to_string()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(
        body["code"],
        json!("computer_use_permission_request_not_implemented")
    );

    let response = appshots_capture_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("appshots_capture_not_implemented"));

    let response = chrome_relay_launch_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("chrome_relay_launch_not_implemented"));

    let response = chrome_relay_token_regenerate_handler()
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let body = response_json(response).await;
    assert_eq!(
        body["code"],
        json!("chrome_relay_token_regenerate_not_implemented")
    );
}
