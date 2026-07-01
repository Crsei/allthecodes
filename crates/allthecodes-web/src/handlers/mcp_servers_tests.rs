use super::*;
use crate::handlers::test_support::*;
use allthecodes_ipc_protocol::subsystem_types::ConfigScope;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

#[test]
fn mcp_server_list_redacts_account_access_token_env() {
    let redacted = redact_server_env(Some(std::collections::HashMap::from([
        (
            "ALLTHECODES_COM_ACCESS_TOKEN".to_string(),
            "secret-token".to_string(),
        ),
        (
            "ALLTHECODES_COM_BASE_URL".to_string(),
            "https://allthecodes.cc".to_string(),
        ),
    ])))
    .expect("env");

    assert_eq!(
        redacted
            .get("ALLTHECODES_COM_ACCESS_TOKEN")
            .map(String::as_str),
        Some("[redacted]")
    );
    assert_eq!(
        redacted.get("ALLTHECODES_COM_BASE_URL").map(String::as_str),
        Some("https://allthecodes.cc")
    );
}

#[tokio::test]
#[serial]
async fn mcp_servers_crud_uses_editable_settings_scopes() {
    let (home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let mut user = allthecodes_config::settings::RawSettings::default();
    user.extra.insert(
        "mcpServers".to_string(),
        json!({
            "user-srv": {
                "type": "stdio",
                "command": "user-cmd"
            }
        }),
    );
    allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

    let mut project_raw = allthecodes_config::settings::RawSettings::default();
    project_raw.extra.insert(
        "mcpServers".to_string(),
        json!({
            "project-srv": {
                "type": "stdio",
                "command": "project-cmd"
            }
        }),
    );
    let project_settings = project.path().join(".allthecodes").join("settings.json");
    allthecodes_config::settings::write_settings_file(&project_settings, &project_raw)
        .expect("seed project settings");

    let response = mcp_servers_list_handler(State(state.clone()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let servers = body["servers"].as_array().expect("servers");
    assert!(servers.iter().any(|server| {
        server["name"] == json!("user-srv") && server["scope"]["kind"] == json!("user")
    }));
    assert!(servers.iter().any(|server| {
        server["name"] == json!("project-srv") && server["scope"]["kind"] == json!("project")
    }));

    let response = mcp_servers_create_handler(
        State(state.clone()),
        Json(McpServerUpsertRequest::Entry(make_mcp_entry(
            "created",
            ConfigScope::User,
        ))),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert_eq!(raw.extra["mcpServers"]["created"]["command"], json!("echo"));

    let mut updated = make_mcp_entry("created", ConfigScope::User);
    updated.command = Some("printf".to_string());
    let response = mcp_servers_update_handler(
        State(state.clone()),
        AxumPath("created".to_string()),
        Json(McpServerUpsertRequest::Wrapped { entry: updated }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert_eq!(
        raw.extra["mcpServers"]["created"]["command"],
        json!("printf")
    );

    let response = mcp_servers_delete_handler(
        State(state.clone()),
        AxumPath("created".to_string()),
        Query(McpServerDeleteQuery {
            scope: "user".to_string(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let raw = read_user_settings(&home);
    assert!(raw.extra["mcpServers"].get("created").is_none());

    let response = mcp_servers_create_handler(
        State(state.clone()),
        Json(McpServerUpsertRequest::Entry(make_mcp_entry(
            "plugin-owned",
            ConfigScope::Plugin {
                id: "plug".to_string(),
            },
        ))),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("validation_error"));

    let response = mcp_servers_delete_handler(
        State(state),
        AxumPath("project-srv".to_string()),
        Query(McpServerDeleteQuery {
            scope: "plugin:plug".to_string(),
        }),
    )
    .await
    .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn mcp_servers_oauth_status_uses_codex_style_status_without_tokens() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let mut user = allthecodes_config::settings::RawSettings::default();
    user.extra.insert(
        "mcpServers".to_string(),
        json!({
            "remote-oauth": {
                "type": "streamable-http",
                "url": "https://mcp.example.com/mcp",
                "oauth": {
                    "authServerMetadataUrl": "https://auth.example.com/.well-known/oauth-authorization-server",
                    "credentialsStore": "file"
                }
            }
        }),
    );
    allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

    let response =
        mcp_servers_auth_status_handler(State(state), AxumPath("remote-oauth".to_string()))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["status"], json!("not_logged_in"));
    assert_eq!(body["configured"], json!(true));
    assert_eq!(body["authorized"], json!(false));
    assert!(body["message"].as_str().unwrap().contains("OAuth"));
    assert!(body["oauth_flow"].is_null());
    assert!(
        !body.to_string().contains("access_token"),
        "status response must not leak token fields"
    );
}

#[tokio::test]
#[serial]
async fn mcp_servers_oauth_start_records_unsupported_flow_error() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let mut user = allthecodes_config::settings::RawSettings::default();
    user.extra.insert(
        "mcpServers".to_string(),
        json!({
            "plain-remote": {
                "type": "streamable-http",
                "url": "https://mcp.example.com/mcp"
            }
        }),
    );
    allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

    let response =
        mcp_servers_auth_start_handler(State(state.clone()), AxumPath("plain-remote".to_string()))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("auth_unsupported"));
    assert!(!body.to_string().contains("access_token"));

    let response =
        mcp_servers_auth_status_handler(State(state), AxumPath("plain-remote".to_string()))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["status"], json!("unsupported"));
    assert_eq!(body["oauth_flow"]["state"], json!("failed"));
    assert_eq!(body["oauth_flow"]["error_code"], json!("auth_unsupported"));
    clear_oauth_flow_status("plain-remote");
}

#[test]
fn mcp_servers_oauth_error_codes_distinguish_timeout_and_unsupported() {
    assert_eq!(
        oauth_error_code("OAuth callback timed out before a response was received"),
        "oauth_timeout"
    );
    assert_eq!(
        oauth_error_status("oauth_timeout"),
        StatusCode::GATEWAY_TIMEOUT
    );
    assert_eq!(
        oauth_error_code("MCP server `remote` has no OAuth configuration"),
        "auth_unsupported"
    );
    assert_eq!(
        oauth_error_code("token endpoint returned HTTP 500"),
        "oauth_error"
    );
}

#[test]
fn mcp_servers_oauth_flow_status_records_redacted_failure() {
    record_oauth_flow_status(
        "flow-test".to_string(),
        "failed",
        Some("OAuth callback timed out".to_string()),
    );

    let flow = oauth_flow_json(oauth_flow_status("flow-test"));

    assert_eq!(flow["state"], json!("failed"));
    assert_eq!(flow["error_code"], json!("oauth_timeout"));
    assert!(
        !flow.to_string().contains("access_token"),
        "flow status must not leak token fields"
    );
    clear_oauth_flow_status("flow-test");
}
