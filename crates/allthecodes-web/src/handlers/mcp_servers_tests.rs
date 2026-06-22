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
