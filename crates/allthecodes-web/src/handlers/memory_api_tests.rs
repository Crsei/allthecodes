use super::*;
use crate::handlers::test_support::*;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn memory_list_empty_returns_200() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = memory_list_handler(
        State(state),
        Query(crate::handlers::MemoryListQuery { profile_id: None }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["entries"], json!([]));
}

#[tokio::test]
#[serial]
async fn memory_list_with_profile_id_echoes_it() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = memory_list_handler(
        State(state),
        Query(crate::handlers::MemoryListQuery {
            profile_id: Some("prof-42".to_string()),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["profile_id"], json!("prof-42"));
    assert_eq!(body["entries"], json!([]));
}

#[tokio::test]
#[serial]
async fn memory_update_persists_changes() {
    let (home, _guard) = temp_home();
    let state = make_web_state();

    // Seed an entry.
    let entry = crate::handlers::MemoryEntry {
        id: "mem-1".to_string(),
        timestamp: "2026-01-01T00:00:00Z".to_string(),
        session_id: "sess-1".to_string(),
        workspace: "ws-1".to_string(),
        content: "original content".to_string(),
        tags: vec!["initial".to_string()],
        pinned: false,
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let store = crate::handlers::MemoryStore {
        entries: vec![entry],
    };
    crate::handlers::save_store(&store).expect("seed store");

    let response = memory_update_handler(
        AxumPath("mem-1".to_string()),
        State(state),
        Json(crate::handlers::MemoryUpdateRequest {
            content: Some("updated content".to_string()),
            tags: Some(vec!["  foo ".to_string(), "bar".to_string()]),
            pinned: Some(true),
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["entry"]["content"], json!("updated content"));
    assert_eq!(body["entry"]["pinned"], json!(true));
    assert_eq!(body["entry"]["session_id"], json!("sess-1"));
    assert_eq!(body["entry"]["workspace"], json!("ws-1"));
    assert_eq!(body["entry"]["timestamp"], json!("2026-01-01T00:00:00Z"));
    // Tags should be normalized.
    let tags = body["entry"]["tags"].as_array().expect("tags");
    assert_eq!(tags.len(), 2);
    assert!(tags.contains(&json!("bar")));
    assert!(tags.contains(&json!("foo")));
    // updated_at should have changed.
    assert_ne!(body["entry"]["updated_at"], json!("2026-01-01T00:00:00Z"));

    // Verify persistence.
    let path = home.path().join("memory").join("entries.json");
    assert!(path.exists());
}

#[tokio::test]
#[serial]
async fn memory_update_unknown_id_returns_404() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let response = memory_update_handler(
        AxumPath("nonexistent".to_string()),
        State(state),
        Json(crate::handlers::MemoryUpdateRequest {
            content: Some("anything".to_string()),
            tags: None,
            pinned: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
}

#[tokio::test]
#[serial]
async fn memory_update_content_too_long_returns_400() {
    let (_home, _guard) = temp_home();
    let state = make_web_state();

    let long_content = "x".repeat(100_001);

    let response = memory_update_handler(
        AxumPath("mem-1".to_string()),
        State(state),
        Json(crate::handlers::MemoryUpdateRequest {
            content: Some(long_content),
            tags: None,
            pinned: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("validation"));
}

// -------------------------------------------------------------------
// Files API tests
// -------------------------------------------------------------------
