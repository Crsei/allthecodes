use super::*;
use crate::handlers::test_support::*;
use allthecodes_ipc_protocol::subsystem_types::ConfigScope;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::json;
use serial_test::serial;

struct RuntimeMcpGuard;

impl RuntimeMcpGuard {
    fn install(
        manager: std::sync::Arc<tokio::sync::Mutex<allthecodes_mcp::manager::McpManager>>,
    ) -> Self {
        allthecodes_mcp::runtime::clear_for_tests();
        allthecodes_mcp::runtime::install_manager(manager);
        Self
    }
}

impl Drop for RuntimeMcpGuard {
    fn drop(&mut self) {
        allthecodes_mcp::runtime::clear_for_tests();
    }
}

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
async fn mcp_servers_health_exposes_runtime_health_details() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());
    let server_name = "broken-health";
    let command = "allthecodes-test-missing-mcp-web-health";

    let mut user = allthecodes_config::settings::RawSettings::default();
    let mut servers = serde_json::Map::new();
    servers.insert(
        server_name.to_string(),
        json!({
            "type": "stdio",
            "command": command
        }),
    );
    user.extra
        .insert("mcpServers".to_string(), serde_json::Value::Object(servers));
    allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

    let mut manager = allthecodes_mcp::manager::McpManager::new();
    let config = allthecodes_mcp::McpServerConfig {
        name: server_name.to_string(),
        transport: "stdio".to_string(),
        command: Some(command.to_string()),
        args: None,
        url: None,
        headers: None,
        oauth: None,
        env: None,
        browser_mcp: None,
        disabled: None,
        bearer_token_env_var: None,
        env_http_headers: None,
        auth: None,
    };
    manager
        .connect_server(config)
        .await
        .expect_err("missing stdio command should fail");
    let _runtime = RuntimeMcpGuard::install(std::sync::Arc::new(tokio::sync::Mutex::new(manager)));

    let response = mcp_servers_health_handler(State(state))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let servers = body["servers"].as_array().expect("servers");
    let server = servers
        .iter()
        .find(|server| server["name"] == json!(server_name))
        .unwrap_or_else(|| panic!("broken health server missing in body: {body}"));
    assert_eq!(server["state"], json!("error"));
    assert_eq!(server["last_error_kind"], json!("spawn_failed"));
    assert_eq!(server["failure_count"], json!(3));
    assert_eq!(server["connect_attempt_count"], json!(3));
    assert_eq!(server["retry_scheduled_count"], json!(2));
    assert_eq!(server["retry_exhausted_count"], json!(1));
    assert_eq!(server["recovered_count"], json!(0));
    assert_eq!(server["stderr_tail_dropped_line_count"], json!(0));
    assert!(server["last_attempt_at"].is_number());
    assert!(server["error"]
        .as_str()
        .unwrap_or_default()
        .contains("failed to spawn"));
}

#[tokio::test]
#[serial]
async fn mcp_servers_probe_accepts_inline_config_without_live_manager() {
    let (_home, _guard) = temp_home();
    allthecodes_mcp::runtime::clear_for_tests();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response = mcp_servers_probe_handler(
        State(state),
        Json(McpServerProbeRequest {
            name: None,
            config: Some(allthecodes_mcp::McpServerConfig {
                name: "inline-probe".to_string(),
                transport: "stdio".to_string(),
                command: None,
                args: None,
                url: None,
                headers: None,
                oauth: None,
                env: None,
                browser_mcp: None,
                disabled: None,
                bearer_token_env_var: None,
                env_http_headers: None,
                auth: None,
            }),
            entry: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["server"], json!("inline-probe"));
    assert_eq!(body["status"], json!("failed"));
    assert_eq!(
        body["message"],
        json!("stdio MCP server is missing a command")
    );
    assert!(body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| { check["name"] == json!("command") && check["status"] == json!("failed") }));
    assert!(!body["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == json!("connect")));
}

#[tokio::test]
#[serial]
async fn mcp_servers_probe_named_server_uses_settings_config() {
    let (_home, _guard) = temp_home();
    allthecodes_mcp::runtime::clear_for_tests();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());
    let server_name = "named-probe";

    let mut user = allthecodes_config::settings::RawSettings::default();
    let mut servers = serde_json::Map::new();
    servers.insert(
        server_name.to_string(),
        json!({
            "type": "stdio"
        }),
    );
    user.extra
        .insert("mcpServers".to_string(), serde_json::Value::Object(servers));
    allthecodes_config::settings::write_user_settings(&user).expect("seed user settings");

    let response = mcp_servers_probe_handler(
        State(state),
        Json(McpServerProbeRequest {
            name: Some(server_name.to_string()),
            config: None,
            entry: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["server"], json!(server_name));
    assert_eq!(body["status"], json!("failed"));
    assert_eq!(
        body["message"],
        json!("stdio MCP server is missing a command")
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
async fn mcp_servers_oauth_clear_unknown_server_returns_404() {
    let (_home, _guard) = temp_home();
    let project = tempfile::tempdir().expect("project");
    let state = make_web_state_with_cwd(project.path());

    let response =
        mcp_servers_auth_clear_handler(State(state), AxumPath("missing-remote".to_string()))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("not_found"));
    assert!(!body.to_string().contains("access_token"));
}

#[tokio::test]
#[serial]
async fn mcp_servers_oauth_clear_removes_token_and_flow_status() {
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

    let token_store_path = allthecodes_mcp::auth::token_store_path();
    std::fs::create_dir_all(token_store_path.parent().unwrap()).expect("token store parent");
    std::fs::write(
        &token_store_path,
        serde_json::to_string_pretty(&json!({
            "servers": {
                "remote-oauth|streamable-http|https://mcp.example.com/mcp": {
                    "access_token": "stored-secret",
                    "refresh_token": "refresh-secret",
                    "token_type": "Bearer",
                    "expires_at": 4102444800i64,
                    "scopes": ["tools.read"],
                    "authorization_server": "https://auth.example.com",
                    "token_endpoint": "https://auth.example.com/token",
                    "client_id": "test-client"
                }
            }
        }))
        .unwrap(),
    )
    .expect("seed token store");
    record_oauth_flow_status(
        "remote-oauth".to_string(),
        "failed",
        Some("OAuth callback timed out".to_string()),
    );

    let response =
        mcp_servers_auth_clear_handler(State(state.clone()), AxumPath("remote-oauth".to_string()))
            .await
            .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["cleared"], json!(true));
    assert!(!body.to_string().contains("stored-secret"));
    assert!(!body.to_string().contains("refresh-secret"));

    let response =
        mcp_servers_auth_status_handler(State(state), AxumPath("remote-oauth".to_string()))
            .await
            .into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["status"], json!("not_logged_in"));
    assert!(body["oauth_flow"].is_null());
    assert!(
        !body.to_string().contains("access_token"),
        "status response must not leak token fields after clear"
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
