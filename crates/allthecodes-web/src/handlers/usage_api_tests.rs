use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn usage_empty_store_returns_200_with_zero_totals() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let query = crate::handlers::UsageQuery {
        period: None,
        profile_id: None,
    };

    let response = usage_handler(State(state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["period"], json!("7d"));
    assert_eq!(body["totals"]["total_input_tokens"], json!(0));
    assert_eq!(body["totals"]["total_output_tokens"], json!(0));
    assert_eq!(body["totals"]["total_cache_read_tokens"], json!(0));
    assert_eq!(body["totals"]["total_cache_creation_tokens"], json!(0));
    assert_eq!(body["totals"]["total_cost_usd"], json!(0.0));
    assert_eq!(body["totals"]["api_call_count"], json!(0));
    assert_eq!(body["totals"]["session_count"], json!(0));
    assert_eq!(body["partial"], json!(false));
    assert!(body["buckets"].as_array().unwrap().is_empty());
    assert!(body["by_model"].as_array().unwrap().is_empty());
    assert!(body["by_provider"].as_array().unwrap().is_empty());
    assert!(body["generated_at"].as_u64().unwrap() > 0);
    assert!(body["warnings"].is_null());
}

#[tokio::test]
#[serial]
async fn usage_invalid_period_returns_400() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let query = crate::handlers::UsageQuery {
        period: Some("forever".to_string()),
        profile_id: None,
    };

    let response = usage_handler(State(state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("invalid_period"));
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("invalid period 'forever'"));
}

#[tokio::test]
#[serial]
async fn usage_profile_id_is_echoed() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let query = crate::handlers::UsageQuery {
        period: Some("30d".to_string()),
        profile_id: Some("test-profile-123".to_string()),
    };

    let response = usage_handler(State(state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("test-profile-123"));
    assert_eq!(body["period"], json!("30d"));
}

#[tokio::test]
#[serial]
async fn usage_period_24h_is_accepted() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let query = crate::handlers::UsageQuery {
        period: Some("24h".to_string()),
        profile_id: None,
    };

    let response = usage_handler(State(state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["period"], json!("24h"));
}

#[tokio::test]
#[serial]
async fn usage_period_all_is_accepted() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();
    let query = crate::handlers::UsageQuery {
        period: Some("all".to_string()),
        profile_id: None,
    };

    let response = usage_handler(State(state), Query(query))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["period"], json!("all"));
}

// -------------------------------------------------------------------
// Memory API tests
// -------------------------------------------------------------------
