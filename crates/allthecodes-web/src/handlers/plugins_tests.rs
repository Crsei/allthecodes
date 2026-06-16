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
async fn plugins_local_install_list_marketplace_and_uninstall_round_trip() {
    let (_home, _guard) = temp_home();
    allthecodes_plugins::clear_plugins();
    let state = make_web_state();
    let plugin_source = tempfile::tempdir().expect("plugin source");
    std::fs::write(
        plugin_source.path().join("plugin.json"),
        r#"{
            "name": "local-plugin",
            "display_name": "Local Plugin",
            "version": "1.0.0",
            "description": "Local test plugin"
        }"#,
    )
    .expect("plugin manifest");

    let response = plugins_install_handler(
        State(state.clone()),
        Json(allthecodes_protocol::v1::plugins::PluginInstallRequest {
            source: plugin_source.path().to_string_lossy().to_string(),
            scope: Some("user".to_string()),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["plugin"]["id"], json!("local-plugin@local"));
    assert_eq!(body["fresh_install"], json!(true));

    let response = plugins_list_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .any(|plugin| plugin["id"] == json!("local-plugin@local")));
    assert!(body["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .is_empty());

    let response = plugins_marketplace_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"].is_array());

    let response = plugins_uninstall_handler(
        AxumPath("local-plugin@local".to_string()),
        Json(PluginUninstallRequest { purge: false }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let response = plugins_list_handler(State(state)).await.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["plugins"].as_array().expect("plugins").is_empty());
    allthecodes_plugins::clear_plugins();
}
