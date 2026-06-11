//! Axum route handlers for the web chat API.
//!
//! This module is organized by feature group. Each submodule exposes its
//! handler functions and request/response types; `mod.rs` re-exports all
//! public items so that `handlers::chat_handler` and similar paths used in
//! the router builder continue to resolve.

use std::sync::{OnceLock, RwLock};

use allthecodes_commands::Command;

pub mod activity_recorder;
pub mod admin;
pub mod agents;
pub mod appshots;
pub mod auth;
pub mod backend_services;
pub mod capabilities;
pub mod channels;
pub mod chat;
pub mod chat_modes;
pub mod chrome_relay;
pub mod computer_use;
pub mod credentials;
pub mod files;
pub mod gateways;
pub mod git;
pub mod group_chat;
pub mod health;
pub mod hooks;
pub mod jobs;
pub mod kanban;
pub mod launchpad;
pub mod logs;
pub mod mcp_servers;
pub mod memory;
pub mod models;
pub mod people;
pub mod plugins;
pub mod profiles;
pub mod prompts;
pub mod providers;
pub mod proxy;
pub mod sessions;
pub mod settings_phase1;
pub mod skills;
pub mod usage;
pub mod workspaces;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export all public items from each submodule so the router builder
// and external callers can still use `handlers::*` paths.
pub use crate::api_errors::{api_error_body, ApiError};
pub use activity_recorder::*;
pub use admin::*;
pub use agents::*;
pub use appshots::*;
pub use auth::*;
pub use backend_services::*;
pub use capabilities::*;
pub use channels::*;
pub use chat::*;
pub use chat_modes::*;
pub use chrome_relay::*;
pub use computer_use::*;
pub use credentials::*;
pub use files::*;
pub use gateways::*;
pub use git::*;
pub use group_chat::*;
pub use health::*;
pub use hooks::*;
pub use jobs::*;
pub use kanban::*;
pub use launchpad::*;
pub use logs::*;
pub use mcp_servers::*;
pub use memory::*;
pub use models::*;
pub use people::*;
pub use plugins::*;
pub use profiles::*;
pub use prompts::*;
pub use providers::*;
pub use proxy::*;
pub use sessions::*;
pub use settings_phase1::*;
pub use skills::*;
pub use usage::*;
pub use workspaces::*;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Command-provider registry
// ---------------------------------------------------------------------------

type CommandProvider = fn() -> Vec<Command>;

static COMMAND_PROVIDER: OnceLock<RwLock<Option<CommandProvider>>> = OnceLock::new();

/// Install the root-owned slash-command registry used by web command routes.
pub fn set_command_provider(provider: CommandProvider) {
    let slot = COMMAND_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

pub(crate) fn get_all_commands() -> Vec<Command> {
    COMMAND_PROVIDER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
        .map(|provider| provider())
        .unwrap_or_default()
}

pub(crate) fn setting_bool(state: &crate::state::WebState, path: &str) -> Option<bool> {
    let map = state.engine().app_state().settings.settings_map();
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = map.get(first)?;
    for part in parts {
        value = value.get(part)?;
    }
    value.as_bool()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use allthecodes_ipc_protocol::subsystem_types::{AgentDefinitionSource, ConfigScope};
    use allthecodes_skills::{SkillDefinition, SkillFrontmatter, SkillSource};
    use axum::extract::{Path as AxumPath, Query, State};
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    use axum::Json;
    use serde_json::{json, Value};
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn session_archive_handler_returns_404_for_missing_session() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();

        let response =
            session_archive_handler(AxumPath("missing-session".to_string()), State(state))
                .await
                .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("not_found"));
        assert_eq!(body["details"]["entity"], json!("session"));
        assert_eq!(body["details"]["id"], json!("missing-session"));
    }

    #[tokio::test]
    #[serial]
    async fn session_archive_handler_rejects_active_session_with_409() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let active_id = state.engine().current_session_id().to_string();

        let response = session_archive_handler(AxumPath(active_id), State(state))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("conflict"));
        assert!(body["details"]["reason"].is_string());
    }

    #[tokio::test]
    #[serial]
    async fn session_archive_handler_archives_inactive_session() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = "inactive-web-archive";
        allthecodes_session::storage::save_session(session_id, &[], ".")
            .expect("seed inactive session");

        let response = session_archive_handler(AxumPath(session_id.to_string()), State(state))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["message"], json!("Session archived"));
        assert!(!allthecodes_session::storage::get_session_file(session_id).exists());
        assert!(allthecodes_session::storage::get_archived_session_file(session_id).exists());
    }

    #[tokio::test]
    #[serial]
    async fn workspaces_patch_persists_sidebar_metadata() {
        let (home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());
        let workspace_key = allthecodes_session::storage::workspace_key(project.path());

        let response = workspace_patch_handler(
            AxumPath(workspace_key.clone()),
            State(state.clone()),
            Json(WorkspacePatchRequest {
                display_name: Some(Some("Frontend".to_string())),
                pinned: Some(true),
                hidden: Some(false),
                default_chat_mode: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["key"], json!(workspace_key));
        assert_eq!(body["display_name"], json!("Frontend"));
        assert_eq!(body["pinned"], json!(true));

        let metadata_path = home.path().join("web").join("workspaces.json");
        let persisted: Value =
            serde_json::from_str(&std::fs::read_to_string(metadata_path).expect("metadata file"))
                .expect("metadata json");
        assert_eq!(
            persisted["workspaces"][workspace_key.as_str()]["display_name"],
            json!("Frontend")
        );
    }

    #[tokio::test]
    #[serial]
    async fn session_new_handler_can_target_known_workspace_cwd() {
        let (_home, _guard) = temp_home();
        let current = tempfile::tempdir().expect("current");
        let target = tempfile::tempdir().expect("target");
        let state = make_web_state_with_cwd(current.path());
        allthecodes_session::storage::save_session(
            "target-session",
            &[],
            target.path().to_str().unwrap(),
        )
        .expect("seed target workspace");
        let workspace_key = allthecodes_session::storage::workspace_key(target.path());

        let response = session_new_handler(
            State(state.clone()),
            Some(Json(NewSessionRequest {
                workspace_key: Some(workspace_key),
                cwd: Some(target.path().to_string_lossy().to_string()),
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
            allthecodes_session::storage::workspace_key(target.path())
        );
    }

    #[tokio::test]
    #[serial]
    async fn session_new_handler_can_target_existing_local_cwd() {
        let (_home, _guard) = temp_home();
        let current = tempfile::tempdir().expect("current");
        let target = tempfile::tempdir().expect("target");
        let state = make_web_state_with_cwd(current.path());

        let response = session_new_handler(
            State(state.clone()),
            Some(Json(NewSessionRequest {
                workspace_key: None,
                cwd: Some(target.path().to_string_lossy().to_string()),
            })),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            allthecodes_session::storage::workspace_key(std::path::Path::new(state.engine().cwd())),
            allthecodes_session::storage::workspace_key(target.path())
        );
    }

    #[tokio::test]
    #[serial]
    async fn session_resume_reuses_cached_engine_session_grants() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = state.engine().current_session_id().to_string();
        allthecodes_session::storage::save_session(&session_id, &[], ".")
            .expect("seed cached session");
        state.engine().update_app_state(|app| {
            app.tool_permission_context
                .grant_session_allow("mcp__computer-use__screenshot");
        });

        let response = session_resume_handler(AxumPath(session_id.clone()), State(state.clone()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let app_state = state.engine().app_state();
        assert!(
            app_state
                .tool_permission_context
                .has_session_grant("mcp__computer-use__screenshot")
        );
        assert_eq!(state.engine().current_session_id().to_string(), session_id);
    }

    #[tokio::test]
    #[serial]
    async fn session_new_inherits_runtime_permissions_but_clears_session_grants() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|app| {
            app.tool_permission_context.mode =
                allthecodes_types::permissions::PermissionMode::Auto;
            app.tool_permission_context
                .always_allow_rules
                .insert("user".into(), vec!["Read".into()]);
            app.tool_permission_context
                .always_deny_rules
                .insert("user".into(), vec!["Write".into()]);
            app.tool_permission_context
                .always_ask_rules
                .insert("user".into(), vec!["Bash".into()]);
            app.tool_permission_context
                .grant_session_allow("mcp__computer-use__screenshot");
        });

        let response = session_new_handler(State(state.clone()), None)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let app_state = state.engine().app_state();
        assert_eq!(
            app_state.tool_permission_context.mode,
            allthecodes_types::permissions::PermissionMode::Auto
        );
        assert_eq!(
            app_state
                .tool_permission_context
                .always_allow_rules
                .get("user"),
            Some(&vec!["Read".to_string()])
        );
        assert_eq!(
            app_state.tool_permission_context.always_deny_rules.get("user"),
            Some(&vec!["Write".to_string()])
        );
        assert_eq!(
            app_state.tool_permission_context.always_ask_rules.get("user"),
            Some(&vec!["Bash".to_string()])
        );
        assert!(
            !app_state
                .tool_permission_context
                .has_session_grant("mcp__computer-use__screenshot")
        );
    }

    #[tokio::test]
    #[serial]
    async fn cold_session_engine_inherits_runtime_permissions_but_clears_session_grants() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let session_id = "cold-session-engine";
        allthecodes_session::storage::save_session(session_id, &[], ".")
            .expect("seed cold session");
        state.engine().update_app_state(|app| {
            app.tool_permission_context.mode =
                allthecodes_types::permissions::PermissionMode::Plan;
            app.tool_permission_context
                .always_allow_rules
                .insert("user".into(), vec!["Read".into()]);
            app.tool_permission_context
                .grant_session_allow("mcp__computer-use__screenshot");
        });

        let engine = crate::handlers::sessions::build_engine_for_session(&state, session_id)
            .expect("engine");

        let app_state = engine.app_state();
        assert_eq!(
            app_state.tool_permission_context.mode,
            allthecodes_types::permissions::PermissionMode::Plan
        );
        assert_eq!(
            app_state
                .tool_permission_context
                .always_allow_rules
                .get("user"),
            Some(&vec!["Read".to_string()])
        );
        assert!(
            !app_state
                .tool_permission_context
                .has_session_grant("mcp__computer-use__screenshot")
        );
    }

    #[tokio::test]
    #[serial]
    async fn workspace_archive_skips_active_and_archives_inactive_sessions() {
        let (_home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());
        let inactive_id = "workspace-inactive-archive";
        allthecodes_session::storage::save_session(
            inactive_id,
            &[],
            project.path().to_str().unwrap(),
        )
        .expect("seed inactive session");
        allthecodes_session::storage::save_session(
            &state.engine().current_session_id().to_string(),
            &[],
            project.path().to_str().unwrap(),
        )
        .expect("seed active session");
        let workspace_key = allthecodes_session::storage::workspace_key(project.path());

        let response = workspace_sessions_archive_handler(
            AxumPath(workspace_key),
            State(state.clone()),
            Json(WorkspaceArchiveRequest {
                include_active: false,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["archived"], json!([inactive_id]));
        assert_eq!(body["skipped"][0]["reason"], json!("active_session"));
        assert!(!allthecodes_session::storage::get_session_file(inactive_id).exists());
        assert!(allthecodes_session::storage::get_archived_session_file(inactive_id).exists());
        assert!(allthecodes_session::storage::get_session_file(
            &state.engine().current_session_id().to_string()
        )
        .exists());
    }

    #[tokio::test]
    #[serial]
    async fn models_set_default_persists_model_setting() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.available_models = vec!["gpt-4o".to_string()];
        });

        let response = models_set_default_handler(
            State(state.clone()),
            Json(SetDefaultModelRequest {
                model_id: "gpt-4o".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.model.as_deref(), Some("gpt-4o"));
        assert_eq!(state.engine().app_state().main_loop_model, "gpt-4o");
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

    #[tokio::test]
    #[serial]
    async fn channels_report_stopped_daemon_without_starting_it() {
        let (_home, _guard) = temp_home();

        let response = channels_list_handler().await.into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("daemon_stopped"));
    }

    #[tokio::test]
    #[serial]
    async fn system_action_endpoints_return_explicit_501_codes() {
        let (_home, _guard) = temp_home();

        let response = computer_use_test_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("computer_use_test_not_implemented"));

        let response =
            computer_use_permission_request_handler(AxumPath("accessibility".to_string()))
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

    #[tokio::test]
    #[serial]
    async fn agents_rest_handlers_list_persist_and_restore() {
        let (home, _guard) = temp_home();
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = agents_list_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .any(|agent| agent["name"] == json!("general-purpose")));
        assert!(body["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["name"] == json!("Read")));

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("web-user", AgentDefinitionSource::User),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("agents/web-user.md").exists());

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("web-project", AgentDefinitionSource::Project),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(project
            .path()
            .join(".allthecodes/agents/web-project.md")
            .exists());

        let response = agents_update_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("general-purpose", AgentDefinitionSource::Builtin),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("validation_error"));

        let response = agents_delete_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
            Query(AgentDeleteQuery {
                source: "builtin".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = agents_create_handler(
            State(state.clone()),
            Json(AgentUpsertRequest {
                entry: make_agent_entry("general-purpose", AgentDefinitionSource::User),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("agents/general-purpose.md").exists());

        let response = agents_restore_handler(
            State(state.clone()),
            AxumPath("general-purpose".to_string()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("agents/general-purpose.md").exists());
        let body = response_json(response).await;
        assert!(body["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .any(|agent| {
                agent["name"] == json!("general-purpose")
                    && agent["source"]["kind"] == json!("builtin")
            }));
    }

    #[tokio::test]
    #[serial]
    async fn people_crud_round_trips_json_and_validates_ids() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        use allthecodes_protocol::v1::people::PersonCreateRequest as ProtocolPersonCreateRequest;

        let response = people_create_handler(
            State(state.clone()),
            Json(ProtocolPersonCreateRequest {
                id: None,
                name: "Ada Lovelace".to_string(),
                telegram_id: Some("ada-tg".to_string()),
                discord_id: None,
                discord_username: Some("ada".to_string()),
                feishu_id: None,
                username: Some("ada".to_string()),
                profile_content: "First programmer".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["person"]["id"], json!("ada-lovelace"));
        assert!(home.path().join("people/ada-lovelace.json").exists());

        let response = people_create_handler(
            State(state.clone()),
            Json(ProtocolPersonCreateRequest {
                id: Some("ada-lovelace".to_string()),
                name: "Ada Duplicate".to_string(),
                telegram_id: None,
                discord_id: None,
                discord_username: None,
                feishu_id: None,
                username: None,
                profile_content: String::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = people_update_handler(
            AxumPath("ada-lovelace".to_string()),
            Json(PersonUpdateRequest {
                name: Some("Ada Byron".to_string()),
                telegram_id: Some(None),
                discord_id: None,
                discord_username: None,
                feishu_id: None,
                username: None,
                profile_content: Some("Updated profile".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["name"], json!("Ada Byron"));
        assert_eq!(body["telegram_id"], Value::Null);

        let response = people_create_handler(
            State(state),
            Json(ProtocolPersonCreateRequest {
                id: Some("bad/id".to_string()),
                name: "Bad".to_string(),
                telegram_id: None,
                discord_id: None,
                discord_username: None,
                feishu_id: None,
                username: None,
                profile_content: String::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = people_delete_handler(AxumPath("ada-lovelace".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("people/ada-lovelace.json").exists());
    }

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

    #[tokio::test]
    #[serial]
    async fn prompts_crud_round_trips_store_and_rejects_slash_names() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        use allthecodes_protocol::v1::prompts::PromptCreateRequest as ProtocolPromptCreateRequest;

        let response = prompts_create_handler(
            State(state.clone()),
            Json(ProtocolPromptCreateRequest {
                id: None,
                name: "Summarize Thread".to_string(),
                content: "Summarize this thread.".to_string(),
                description: "summary prompt".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["prompt"]["id"], json!("summarize-thread"));
        assert!(home.path().join("quick-prompts.json").exists());

        let response = prompts_create_handler(
            State(state.clone()),
            Json(ProtocolPromptCreateRequest {
                id: Some("summarize-thread".to_string()),
                name: "Duplicate".to_string(),
                content: "duplicate".to_string(),
                description: String::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = prompts_create_handler(
            State(state),
            Json(ProtocolPromptCreateRequest {
                id: None,
                name: "/bad".to_string(),
                content: "bad".to_string(),
                description: String::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = prompts_update_handler(
            AxumPath("summarize-thread".to_string()),
            Json(PromptUpdateRequest {
                name: Some("Summarize".to_string()),
                content: Some("Updated".to_string()),
                description: Some("updated description".to_string()),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["content"], json!("Updated"));

        let response = prompts_delete_handler(AxumPath("summarize-thread".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["prompts"], json!([]));
    }

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

    #[tokio::test]
    #[serial]
    async fn files_tree_lists_workspace_root() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("hello.txt"), b"hello").expect("seed file");
        std::fs::create_dir(project.path().join("sub")).expect("seed dir");
        let state = make_web_state_with_cwd(project.path());

        let response = files_tree_handler(
            State(state),
            Query(FileTreeQuery {
                path: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let entries = body["entries"].as_array().expect("entries");
        assert!(entries.iter().any(|e| e["name"] == json!("hello.txt")));
        assert!(entries.iter().any(|e| e["name"] == json!("sub")));
        assert_eq!(body["truncated"], json!(false));
    }

    #[tokio::test]
    #[serial]
    async fn files_tree_rejects_path_traversal() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_tree_handler(
            State(state),
            Query(FileTreeQuery {
                path: Some("../etc".to_string()),
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("path_traversal"));
    }

    #[tokio::test]
    #[serial]
    async fn files_stat_returns_file_metadata() {
        let project = tempfile::tempdir().expect("project");
        let file_path = project.path().join("test.txt");
        std::fs::write(&file_path, b"content").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_stat_handler(
            State(state),
            Query(FileStatQuery {
                path: Some("test.txt".to_string()),
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["exists"], json!(true));
        assert_eq!(body["is_file"], json!(true));
        assert_eq!(body["size"], json!(7));
        assert!(body["hash"].as_str().unwrap().len() >= 10);
    }

    #[tokio::test]
    #[serial]
    async fn files_stat_with_profile_id_echoes_it() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("a.txt"), b"data").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_stat_handler(
            State(state),
            Query(FileStatQuery {
                path: Some("a.txt".to_string()),
                profile_id: Some("prof-99".to_string()),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("prof-99"));
    }

    #[tokio::test]
    #[serial]
    async fn files_read_returns_text_content() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("readme.md"), b"# Hello\n\nWorld!").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_read_handler(
            State(state),
            Query(FileReadQuery {
                path: Some("readme.md".to_string()),
                profile_id: None,
                max_bytes: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["content"], json!("# Hello\n\nWorld!"));
        assert_eq!(body["is_binary"], json!(false));
        assert_eq!(body["lines"], json!(3));
        assert_eq!(body["truncated"], json!(false));
    }

    #[tokio::test]
    #[serial]
    async fn files_read_detects_binary() {
        let project = tempfile::tempdir().expect("project");
        let binary = vec![0x00, 0x01, 0x02, 0x03];
        std::fs::write(project.path().join("binary.bin"), &binary).expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_read_handler(
            State(state),
            Query(FileReadQuery {
                path: Some("binary.bin".to_string()),
                profile_id: None,
                max_bytes: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["is_binary"], json!(true));
        assert_eq!(body["content"], json!(""));
    }

    #[tokio::test]
    #[serial]
    async fn files_read_truncates_large_content() {
        let project = tempfile::tempdir().expect("project");
        let content = "x".repeat(2000);
        std::fs::write(project.path().join("large.txt"), &content).expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_read_handler(
            State(state),
            Query(FileReadQuery {
                path: Some("large.txt".to_string()),
                profile_id: None,
                max_bytes: Some(100),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["content"].as_str().unwrap().len(), 100);
        assert_eq!(body["truncated"], json!(true));
    }

    #[tokio::test]
    #[serial]
    async fn files_write_creates_new_file() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_write_handler(
            State(state),
            Json(FileWriteRequest {
                path: "new.txt".to_string(),
                content: "fresh content".to_string(),
                hash: None,
                overwrite: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert!(project.path().join("new.txt").exists());
        assert_eq!(
            std::fs::read_to_string(project.path().join("new.txt")).expect("read"),
            "fresh content"
        );
    }

    #[tokio::test]
    #[serial]
    async fn files_write_rejects_overwrite_without_flag() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("existing.txt"), b"original").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_write_handler(
            State(state),
            Json(FileWriteRequest {
                path: "existing.txt".to_string(),
                content: "overwritten".to_string(),
                hash: None,
                overwrite: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("conflict"));
        assert_eq!(
            std::fs::read_to_string(project.path().join("existing.txt")).expect("read"),
            "original"
        );
    }

    #[tokio::test]
    #[serial]
    async fn files_write_with_overwrite_flag_succeeds() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("replace.txt"), b"old").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_write_handler(
            State(state),
            Json(FileWriteRequest {
                path: "replace.txt".to_string(),
                content: "new".to_string(),
                hash: None,
                overwrite: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            std::fs::read_to_string(project.path().join("replace.txt")).expect("read"),
            "new"
        );
    }

    #[tokio::test]
    #[serial]
    async fn files_write_enforces_hash() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("hash.txt"), b"original").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        // Get current hash.
        let hash =
            crate::handlers::files::file_hash(&project.path().join("hash.txt")).expect("hash");

        // Write with wrong hash.
        let response = files_write_handler(
            State(state.clone()),
            Json(FileWriteRequest {
                path: "hash.txt".to_string(),
                content: "modified".to_string(),
                hash: Some("badbadbad".to_string()),
                overwrite: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("conflict"));

        // Write with correct hash.
        let response = files_write_handler(
            State(state),
            Json(FileWriteRequest {
                path: "hash.txt".to_string(),
                content: "modified".to_string(),
                hash: Some(hash),
                overwrite: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn files_read_with_profile_id_echoes_it() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("echo.txt"), b"data").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_read_handler(
            State(state),
            Query(FileReadQuery {
                path: Some("echo.txt".to_string()),
                profile_id: Some("prof-read".to_string()),
                max_bytes: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("prof-read"));
    }

    #[tokio::test]
    #[serial]
    async fn files_mkdir_creates_directory() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_mkdir_handler(
            State(state),
            Json(FileMkdirRequest {
                path: "newdir".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(project.path().join("newdir").is_dir());
    }

    #[tokio::test]
    #[serial]
    async fn files_mkdir_creates_nested_directories() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_mkdir_handler(
            State(state),
            Json(FileMkdirRequest {
                path: "a/b/c/d".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(project.path().join("a/b/c/d").is_dir());
    }

    #[tokio::test]
    #[serial]
    async fn files_mkdir_idempotent_on_existing() {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir(project.path().join("exists")).expect("seed dir");
        let state = make_web_state_with_cwd(project.path());

        let response = files_mkdir_handler(
            State(state),
            Json(FileMkdirRequest {
                path: "exists".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn files_delete_removes_file() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("delete_me.txt"), b"bye").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_delete_handler(
            State(state),
            Json(FileDeleteRequest {
                path: "delete_me.txt".to_string(),
                recursive: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!project.path().join("delete_me.txt").exists());
    }

    #[tokio::test]
    #[serial]
    async fn files_delete_rejects_non_empty_dir_without_recursive() {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir(project.path().join("nonempty")).expect("seed dir");
        std::fs::write(project.path().join("nonempty/file.txt"), b"x").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_delete_handler(
            State(state),
            Json(FileDeleteRequest {
                path: "nonempty".to_string(),
                recursive: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(project.path().join("nonempty/file.txt").exists());
    }

    #[tokio::test]
    #[serial]
    async fn files_delete_recursive_removes_directory() {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("nested/a/b")).expect("seed dirs");
        std::fs::write(project.path().join("nested/a/file.txt"), b"x").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_delete_handler(
            State(state),
            Json(FileDeleteRequest {
                path: "nested".to_string(),
                recursive: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!project.path().join("nested").exists());
    }

    #[tokio::test]
    #[serial]
    async fn files_copy_duplicates_file() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("src.txt"), b"copy me").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_copy_handler(
            State(state),
            Json(FileCopyRequest {
                source: "src.txt".to_string(),
                destination: "dst.txt".to_string(),
                overwrite: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(project.path().join("dst.txt").exists());
        assert_eq!(
            std::fs::read_to_string(project.path().join("dst.txt")).expect("read"),
            "copy me"
        );
    }

    #[tokio::test]
    #[serial]
    async fn files_copy_rejects_overwrite_without_flag() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("src.txt"), b"source").expect("seed file");
        std::fs::write(project.path().join("dst.txt"), b"dest").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_copy_handler(
            State(state),
            Json(FileCopyRequest {
                source: "src.txt".to_string(),
                destination: "dst.txt".to_string(),
                overwrite: Some(false),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    #[serial]
    async fn files_rename_moves_file() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("old.txt"), b"rename me").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_rename_handler(
            State(state),
            Json(FileRenameRequest {
                source: "old.txt".to_string(),
                destination: "new.txt".to_string(),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!project.path().join("old.txt").exists());
        assert!(project.path().join("new.txt").exists());
    }

    #[tokio::test]
    #[serial]
    async fn files_move_with_overwrite_replaces_destination() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("source.txt"), b"move me").expect("seed file");
        std::fs::write(project.path().join("dest.txt"), b"old dest").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_move_handler(
            State(state),
            Json(FileMoveRequest {
                source: "source.txt".to_string(),
                destination: "dest.txt".to_string(),
                overwrite: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!project.path().join("source.txt").exists());
        assert_eq!(
            std::fs::read_to_string(project.path().join("dest.txt")).expect("read"),
            "move me"
        );
    }

    #[tokio::test]
    #[serial]
    async fn files_upload_accepts_multiple_files() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_upload_handler(
            State(state),
            Json(FileUploadRequest {
                path: ".".to_string(),
                files: vec![
                    FileUploadItem {
                        name: "a.txt".to_string(),
                        content: "file a".to_string(),
                        encoding: None,
                    },
                    FileUploadItem {
                        name: "b.txt".to_string(),
                        content: "file b".to_string(),
                        encoding: None,
                    },
                ],
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["files"].as_array().unwrap().len(), 2);
        assert!(project.path().join("a.txt").exists());
        assert!(project.path().join("b.txt").exists());
    }

    #[tokio::test]
    #[serial]
    async fn files_download_streams_bytes() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(project.path().join("dl.txt"), b"download content").expect("seed file");
        let state = make_web_state_with_cwd(project.path());

        let response = files_download_handler(
            State(state),
            Query(FileDownloadQuery {
                path: Some("dl.txt".to_string()),
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain; charset=utf-8"
        );
        assert!(response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("dl.txt"));
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        assert_eq!(&body[..], b"download content");
    }

    #[tokio::test]
    #[serial]
    async fn files_endpoints_reject_path_traversal() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        // stat
        let response = files_stat_handler(
            State(state.clone()),
            Query(FileStatQuery {
                path: Some("../../etc/passwd".to_string()),
                profile_id: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // read
        let response = files_read_handler(
            State(state.clone()),
            Query(FileReadQuery {
                path: Some("../../etc/passwd".to_string()),
                profile_id: None,
                max_bytes: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        // write
        let response = files_write_handler(
            State(state.clone()),
            Json(FileWriteRequest {
                path: "../../etc/evil.txt".to_string(),
                content: "evil".to_string(),
                hash: None,
                overwrite: Some(true),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    #[serial]
    async fn files_tree_with_profile_id_echoes_it() {
        let project = tempfile::tempdir().expect("project");
        let state = make_web_state_with_cwd(project.path());

        let response = files_tree_handler(
            State(state),
            Query(FileTreeQuery {
                path: None,
                profile_id: Some("prof-tree".to_string()),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("prof-tree"));
    }

    // -------------------------------------------------------------------
    // Skills API tests
    // -------------------------------------------------------------------

    fn make_test_skill(name: &str) -> SkillDefinition {
        SkillDefinition {
            name: name.to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: format!("Skill {} description", name),
                version: Some("1.0.0".to_string()),
                user_invocable: true,
                ..Default::default()
            },
            prompt_body: format!("Do the {} thing.", name),
        }
    }

    #[tokio::test]
    #[serial]
    async fn skills_list_returns_all_registered_skills() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(make_test_skill("alpha"));
        allthecodes_skills::register_skill(make_test_skill("beta"));

        let response = skills_list_handler(
            State(make_web_state()),
            Query(SkillsListQuery { profile_id: None }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let names: Vec<&str> = body["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(body["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == json!("alpha")));
        assert!(body["revision"].as_u64().unwrap() > 0);
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_list_with_profile_id_echoes_it() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let response = skills_list_handler(
            State(make_web_state()),
            Query(SkillsListQuery {
                profile_id: Some("prof-skills".to_string()),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["skills"], json!([]));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_detail_returns_skill_info() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(make_test_skill("detail-test"));

        let response = skills_detail_handler(AxumPath("detail-test".to_string()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["skill"]["id"], json!("detail-test"));
        assert_eq!(body["skill"]["name"], json!("detail-test"));
        assert!(body["prompt_body"]
            .as_str()
            .unwrap()
            .contains("Do the detail-test thing"));
        assert_eq!(body["skill"]["source"], json!("Bundled"));
        assert_eq!(body["skill"]["enabled"], json!(false));
        assert_eq!(body["skill"]["pinned"], json!(false));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_detail_missing_returns_404() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let response = skills_detail_handler(AxumPath("nonexistent".to_string()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("not_found"));
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_missing_skill_returns_404() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let response = skills_files_handler(
            AxumPath("nosuch".to_string()),
            Query(SkillFileQuery {
                path: "SKILL.md".to_string(),
                max_bytes: None,
                profile_id: None,
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
    async fn skills_files_bundled_skill_has_no_files() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(make_test_skill("bundled-only"));

        let response = skills_files_handler(
            AxumPath("bundled-only".to_string()),
            Query(SkillFileQuery {
                path: "SKILL.md".to_string(),
                max_bytes: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("bad_request"));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_reads_file_from_user_skill() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let skill_dir = tempfile::tempdir().expect("skill dir");
        std::fs::write(skill_dir.path().join("hello.txt"), b"Hello, World!").expect("seed file");

        let skill = SkillDefinition {
            name: "file-skill".to_string(),
            source: SkillSource::User,
            base_dir: Some(skill_dir.path().to_path_buf()),
            frontmatter: SkillFrontmatter {
                description: "File skill".to_string(),
                ..Default::default()
            },
            prompt_body: String::new(),
        };
        allthecodes_skills::register_skill(skill);

        let response = skills_files_handler(
            AxumPath("file-skill".to_string()),
            Query(SkillFileQuery {
                path: "hello.txt".to_string(),
                max_bytes: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["skill_id"], json!("file-skill"));
        assert_eq!(body["path"], json!("hello.txt"));
        assert_eq!(body["content"], json!("Hello, World!"));
        assert_eq!(body["truncated"], json!(false));
        assert_eq!(body["is_binary"], json!(false));
        assert_eq!(body["size"], json!(13));
        assert_eq!(body["media_type"], json!("text/plain"));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_rejects_path_traversal() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let skill_dir = tempfile::tempdir().expect("skill dir");
        let skill = SkillDefinition {
            name: "traverse-test".to_string(),
            source: SkillSource::User,
            base_dir: Some(skill_dir.path().to_path_buf()),
            frontmatter: SkillFrontmatter {
                description: "test".to_string(),
                ..Default::default()
            },
            prompt_body: String::new(),
        };
        allthecodes_skills::register_skill(skill);

        let response = skills_files_handler(
            AxumPath("traverse-test".to_string()),
            Query(SkillFileQuery {
                path: "../../etc/passwd".to_string(),
                max_bytes: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        // Either forbidden or not_found (the canonicalize might resolve then
        // the starts_with check catches it, or the existence check catches it first)
        let status = response.status();
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "expected 403 or 404, got {}",
            status
        );
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_rejects_hidden_files() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let skill_dir = tempfile::tempdir().expect("skill dir");
        std::fs::write(skill_dir.path().join(".secret"), b"secret data").expect("seed hidden file");

        let skill = SkillDefinition {
            name: "hidden-test".to_string(),
            source: SkillSource::User,
            base_dir: Some(skill_dir.path().to_path_buf()),
            frontmatter: SkillFrontmatter {
                description: "test".to_string(),
                ..Default::default()
            },
            prompt_body: String::new(),
        };
        allthecodes_skills::register_skill(skill);

        let response = skills_files_handler(
            AxumPath("hidden-test".to_string()),
            Query(SkillFileQuery {
                path: ".secret".to_string(),
                max_bytes: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("path_traversal"));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_rejects_directory() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let skill_dir = tempfile::tempdir().expect("skill dir");

        let skill = SkillDefinition {
            name: "dir-test".to_string(),
            source: SkillSource::User,
            base_dir: Some(skill_dir.path().to_path_buf()),
            frontmatter: SkillFrontmatter {
                description: "test".to_string(),
                ..Default::default()
            },
            prompt_body: String::new(),
        };
        allthecodes_skills::register_skill(skill);

        // Passing the skill dir itself as a path should be rejected because
        // it's a directory.
        let response = skills_files_handler(
            AxumPath("dir-test".to_string()),
            Query(SkillFileQuery {
                path: ".".to_string(),
                max_bytes: None,
                profile_id: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("bad_request"));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_files_echoes_profile_id() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let skill_dir = tempfile::tempdir().expect("skill dir");
        std::fs::write(skill_dir.path().join("readme.md"), b"content").expect("seed file");

        let skill = SkillDefinition {
            name: "profile-echo".to_string(),
            source: SkillSource::User,
            base_dir: Some(skill_dir.path().to_path_buf()),
            frontmatter: SkillFrontmatter {
                description: "test".to_string(),
                ..Default::default()
            },
            prompt_body: String::new(),
        };
        allthecodes_skills::register_skill(skill);

        let response = skills_files_handler(
            AxumPath("profile-echo".to_string()),
            Query(SkillFileQuery {
                path: "readme.md".to_string(),
                max_bytes: None,
                profile_id: Some("prof-file".to_string()),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["profile_id"], json!("prof-file"));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_patch_persists_enabled_and_pinned() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(make_test_skill("patchable"));

        let response = skills_patch_handler(
            AxumPath("patchable".to_string()),
            Json(SkillPatchRequest {
                enabled: Some(true),
                pinned: Some(true),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(body["skill"]["id"], json!("patchable"));
        assert_eq!(body["skill"]["enabled"], json!(true));
        assert_eq!(body["skill"]["pinned"], json!(true));

        // Verify it persisted by checking detail
        let response = skills_detail_handler(AxumPath("patchable".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["skill"]["enabled"], json!(true));
        assert_eq!(body["skill"]["pinned"], json!(true));
        allthecodes_skills::clear_skills();
    }

    #[tokio::test]
    #[serial]
    async fn skills_patch_missing_skill_returns_404() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();

        let response = skills_patch_handler(
            AxumPath("does-not-exist".to_string()),
            Json(SkillPatchRequest {
                enabled: Some(true),
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
    async fn skills_patch_partial_update_only_changes_provided_fields() {
        let (_home, _guard) = temp_home();
        allthecodes_skills::clear_skills();
        allthecodes_skills::register_skill(make_test_skill("partial"));

        // First enable the skill
        let response = skills_patch_handler(
            AxumPath("partial".to_string()),
            Json(SkillPatchRequest {
                enabled: Some(true),
                pinned: Some(true),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        // Now only change pinned to false, enabled should remain true
        let response = skills_patch_handler(
            AxumPath("partial".to_string()),
            Json(SkillPatchRequest {
                enabled: None,
                pinned: Some(false),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["skill"]["enabled"], json!(true));
        assert_eq!(body["skill"]["pinned"], json!(false));
        allthecodes_skills::clear_skills();
    }
}
