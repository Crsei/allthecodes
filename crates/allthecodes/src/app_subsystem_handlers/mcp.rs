use std::path::{Path, PathBuf};

use allthecodes_ipc_protocol::subsystem_events::{McpCommand, McpEvent};
// Note: subsystem_types imported as needed per item below
use allthecodes_ipc_protocol::BackendMessage;

use super::mcp_config::{remove_mcp_entry, toggle_mcp_entry_enabled, upsert_mcp_entry};
use super::snapshot::{
    build_mcp_server_config_entries, build_mcp_server_info_list_for_cwd,
    build_mcp_server_info_list_for_cwd_async,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRuntimeOperation {
    Connect,
    Disconnect,
    Reconnect,
}

impl McpRuntimeOperation {
    fn verb(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Disconnect => "disconnect",
            Self::Reconnect => "reconnect",
        }
    }

    fn past_tense(self) -> &'static str {
        match self {
            Self::Connect => "Connected",
            Self::Disconnect => "Disconnected",
            Self::Reconnect => "Reconnected",
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpRuntimeReport {
    pub server_name: String,
    pub state: String,
    pub error: Option<String>,
    pub text: String,
    pub level: String,
}

/// Handle an MCP subsystem command from the frontend.
///
/// Lifecycle operations (`ConnectServer` / `DisconnectServer` /
/// `ReconnectServer`) surface an info-level system message pointing at the
/// `/mcp` slash command; the actual live-state mutation lives in the
/// manager owned by the query engine.
///
/// `QueryStatus` builds a runtime-state list; `QueryConfig`/`UpsertConfig`/
/// `RemoveConfig` implement the scope-aware config editor (issue #44).
pub fn handle_mcp_command(cmd: McpCommand) -> Vec<BackendMessage> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    handle_mcp_command_at_cwd(cmd, &cwd)
}

pub async fn handle_mcp_command_with_runtime(cmd: McpCommand, cwd: &Path) -> Vec<BackendMessage> {
    match cmd {
        McpCommand::ConnectServer { server_name } => {
            let report =
                run_mcp_runtime_operation(cwd, McpRuntimeOperation::Connect, &server_name).await;
            mcp_runtime_report_messages(report)
        }
        McpCommand::DisconnectServer { server_name } => {
            let report =
                run_mcp_runtime_operation(cwd, McpRuntimeOperation::Disconnect, &server_name).await;
            mcp_runtime_report_messages(report)
        }
        McpCommand::ReconnectServer { server_name } => {
            let report =
                run_mcp_runtime_operation(cwd, McpRuntimeOperation::Reconnect, &server_name).await;
            mcp_runtime_report_messages(report)
        }
        McpCommand::QueryStatus => {
            let servers = build_mcp_server_info_list_for_cwd_async(cwd).await;
            vec![BackendMessage::McpEvent {
                event: McpEvent::ServerList { servers },
            }]
        }
        McpCommand::StartAuth { server_name } => start_mcp_auth(cwd, &server_name).await,
        McpCommand::CompleteAuth {
            server_name,
            code,
            state,
        } => complete_mcp_auth(cwd, &server_name, &code, state.as_deref()).await,
        other => handle_mcp_command_at_cwd(other, cwd),
    }
}

pub async fn run_mcp_runtime_operation(
    cwd: &Path,
    operation: McpRuntimeOperation,
    server_name: &str,
) -> McpRuntimeReport {
    tracing::info!(
        server_name = %server_name,
        operation = operation.verb(),
        "MCP runtime command requested"
    );

    let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
        return mcp_runtime_report(
            server_name,
            "error",
            Some("MCP runtime manager is not available".to_string()),
            format!(
                "Cannot {} MCP server `{}` because the runtime manager is not available.",
                operation.verb(),
                server_name
            ),
            "error",
        );
    };

    if operation == McpRuntimeOperation::Disconnect {
        let had_client = {
            let mut manager = manager.lock().await;
            manager.disconnect_server(server_name).await
        };
        let text = if had_client {
            format!("Disconnected MCP server `{}`.", server_name)
        } else {
            format!(
                "MCP server `{}` had no active connection; marked disconnected.",
                server_name
            )
        };
        return mcp_runtime_report(server_name, "disconnected", None, text, "info");
    }

    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => {
            return mcp_runtime_report(
                server_name,
                "error",
                Some(message.clone()),
                message,
                "error",
            );
        }
    };
    let disabled = config.disabled.unwrap_or(false);

    let result = {
        let mut manager = manager.lock().await;
        match operation {
            McpRuntimeOperation::Connect => manager.connect_server(config).await,
            McpRuntimeOperation::Reconnect => manager.reconnect_server(config).await,
            McpRuntimeOperation::Disconnect => unreachable!("disconnect handled above"),
        }
    };

    match result {
        Ok(()) if disabled => mcp_runtime_report(
            server_name,
            "disabled",
            None,
            format!(
                "MCP server `{}` is disabled in settings; no live client was kept.",
                server_name
            ),
            "info",
        ),
        Ok(()) => mcp_runtime_report(
            server_name,
            "connected",
            None,
            format!("{} MCP server `{}`.", operation.past_tense(), server_name),
            "info",
        ),
        Err(err) => {
            let state = if allthecodes_mcp::client::is_auth_needed_error(&err) {
                "auth-needed"
            } else {
                "error"
            };
            let message = format!(
                "Failed to {} MCP server `{}`: {}",
                operation.verb(),
                server_name,
                err
            );
            mcp_runtime_report(server_name, state, Some(err.to_string()), message, "error")
        }
    }
}

fn handle_mcp_command_at_cwd(cmd: McpCommand, cwd: &Path) -> Vec<BackendMessage> {
    match cmd {
        McpCommand::ConnectServer { server_name } => mcp_runtime_report_messages(
            mcp_runtime_unavailable_report(McpRuntimeOperation::Connect, &server_name),
        ),
        McpCommand::DisconnectServer { server_name } => mcp_runtime_report_messages(
            mcp_runtime_unavailable_report(McpRuntimeOperation::Disconnect, &server_name),
        ),
        McpCommand::ReconnectServer { server_name } => {
            tracing::info!(server_name = %server_name, "MCP reconnect requested via IPC");
            // Emit a live `connecting` state plus a system note. The runtime
            // still drives reconnection on the next connection pass — the UI
            // reflects intent immediately so the user sees feedback instead of
            // a stale status row.
            let mut messages = Vec::with_capacity(2);
            messages.push(BackendMessage::McpEvent {
                event: McpEvent::ServerStateChanged {
                    server_name: server_name.clone(),
                    state: "pending".to_string(),
                    error: None,
                },
            });
            messages.push(BackendMessage::SystemInfo {
                text: format!(
                    "Queued reconnect for MCP server `{}`. The active session will cycle its connection.",
                    server_name
                ),
                level: "info".to_string(),
            });
            messages
        }
        McpCommand::QueryStatus => {
            let servers = build_mcp_server_info_list_for_cwd(cwd);
            vec![BackendMessage::McpEvent {
                event: McpEvent::ServerList { servers },
            }]
        }
        McpCommand::QueryConfig => {
            let entries = build_mcp_server_config_entries(cwd);
            vec![BackendMessage::McpEvent {
                event: McpEvent::ConfigList { entries },
            }]
        }
        McpCommand::UpsertConfig { entry } => match upsert_mcp_entry(cwd, *entry) {
            Ok(updated) => vec![BackendMessage::McpEvent {
                event: McpEvent::ConfigChanged {
                    server_name: updated.name.clone(),
                    entry: Some(Box::new(updated)),
                },
            }],
            Err((server_name, message)) => {
                tracing::warn!(
                    server = %server_name,
                    error = %message,
                    "MCP: upsert_config rejected"
                );
                vec![BackendMessage::McpEvent {
                    event: McpEvent::ConfigError {
                        server_name,
                        error: message,
                    },
                }]
            }
        },
        McpCommand::RemoveConfig { server_name, scope } => {
            match remove_mcp_entry(cwd, &server_name, &scope) {
                Ok(()) => vec![BackendMessage::McpEvent {
                    event: McpEvent::ConfigChanged {
                        server_name,
                        entry: None,
                    },
                }],
                Err(message) => {
                    tracing::warn!(
                        server = %server_name,
                        error = %message,
                        "MCP: remove_config rejected"
                    );
                    vec![BackendMessage::McpEvent {
                        event: McpEvent::ConfigError {
                            server_name,
                            error: message,
                        },
                    }]
                }
            }
        }
        McpCommand::ToggleEnabled { server_name, scope } => {
            match toggle_mcp_entry_enabled(cwd, &server_name, scope.as_ref()) {
                Ok(updated) => {
                    // Sync runtime state — flipping `disabled` immediately
                    // surfaces in the MCP status list so the UI reflects the
                    // new state without waiting for a full re-discovery.
                    let now_disabled = updated.disabled.unwrap_or(false);
                    let next_state = if now_disabled { "disabled" } else { "pending" };
                    vec![
                        BackendMessage::McpEvent {
                            event: McpEvent::ConfigChanged {
                                server_name: updated.name.clone(),
                                entry: Some(Box::new(updated.clone())),
                            },
                        },
                        BackendMessage::McpEvent {
                            event: McpEvent::ServerStateChanged {
                                server_name: updated.name.clone(),
                                state: next_state.to_string(),
                                error: None,
                            },
                        },
                    ]
                }
                Err(message) => {
                    tracing::warn!(
                        server = %server_name,
                        error = %message,
                        "MCP: toggle_enabled rejected"
                    );
                    vec![BackendMessage::McpEvent {
                        event: McpEvent::ConfigError {
                            server_name,
                            error: message,
                        },
                    }]
                }
            }
        }
        McpCommand::ClearAuth { server_name } => clear_mcp_auth(cwd, &server_name),
        McpCommand::QueryAuth { server_name } => query_mcp_auth(cwd, &server_name),
        McpCommand::StartAuth { server_name } | McpCommand::CompleteAuth { server_name, .. } => {
            vec![BackendMessage::McpEvent {
                event: McpEvent::ConfigError {
                    server_name,
                    error: "MCP OAuth command requires the async runtime handler".to_string(),
                },
            }]
        }
    }
}

async fn start_mcp_auth(cwd: &Path, server_name: &str) -> Vec<BackendMessage> {
    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => return vec![mcp_config_error_message(server_name, message)],
    };
    match allthecodes_mcp::oauth_login::start_auto_authorization(&config).await {
        Ok((handle, start)) => {
            spawn_mcp_oauth_wait_task(config.name.clone(), handle);
            vec![
                BackendMessage::McpEvent {
                    event: McpEvent::AuthStarted {
                        server_name: server_name.to_string(),
                        authorization_url: start.authorization_url,
                        state: start.state,
                        redirect_uri: start.redirect_uri,
                        token_store_path: start.token_store_path.display().to_string(),
                    },
                },
                BackendMessage::SystemInfo {
                    text: format!(
                        "OAuth authorization started for MCP server `{}`. allthecodes is waiting for the browser callback; use CompleteAuth only if the provider returns a manual code.",
                        server_name
                    ),
                    level: "info".to_string(),
                },
            ]
        }
        Err(err) => vec![mcp_config_error_message(server_name, err.to_string())],
    }
}

async fn complete_mcp_auth(
    cwd: &Path,
    server_name: &str,
    code: &str,
    state: Option<&str>,
) -> Vec<BackendMessage> {
    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => return vec![mcp_config_error_message(server_name, message)],
    };
    match allthecodes_mcp::auth::complete_authorization(&config, code, state).await {
        Ok(_) => {
            let mut messages = query_mcp_auth(cwd, server_name);
            messages.push(BackendMessage::SystemInfo {
                text: format!(
                    "Stored OAuth credentials for MCP server `{}` in {}.",
                    server_name,
                    allthecodes_mcp::auth::token_store_path().display()
                ),
                level: "info".to_string(),
            });
            messages
        }
        Err(err) => vec![mcp_config_error_message(server_name, err.to_string())],
    }
}

fn clear_mcp_auth(cwd: &Path, server_name: &str) -> Vec<BackendMessage> {
    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => return vec![mcp_config_error_message(server_name, message)],
    };
    match allthecodes_mcp::auth::clear_stored_token(&config) {
        Ok(_) => query_mcp_auth(cwd, server_name),
        Err(err) => vec![mcp_config_error_message(server_name, err.to_string())],
    }
}

fn query_mcp_auth(cwd: &Path, server_name: &str) -> Vec<BackendMessage> {
    let config = match find_mcp_runtime_config(cwd, server_name) {
        Ok(config) => config,
        Err(message) => return vec![mcp_config_error_message(server_name, message)],
    };
    match allthecodes_mcp::auth::credential_status(&config) {
        Ok(status) => vec![BackendMessage::McpEvent {
            event: McpEvent::AuthStatus {
                server_name: server_name.to_string(),
                configured: status.configured,
                authorized: status.authorized,
                expired: status.expired,
                can_refresh: status.can_refresh,
                token_store_path: status.token_store_path.display().to_string(),
            },
        }],
        Err(err) => vec![mcp_config_error_message(server_name, err.to_string())],
    }
}

fn spawn_mcp_oauth_wait_task(
    server_name: String,
    handle: allthecodes_mcp::oauth_login::McpOAuthLoginHandle,
) {
    tokio::spawn(async move {
        match handle.wait().await {
            Ok(_) => tracing::info!(
                server = %server_name,
                "MCP OAuth callback completed and credentials were stored"
            ),
            Err(err) => tracing::warn!(
                server = %server_name,
                error = %err,
                "MCP OAuth callback did not complete"
            ),
        }
    });
}

pub(crate) fn mcp_config_error_message(server_name: &str, error: String) -> BackendMessage {
    BackendMessage::McpEvent {
        event: McpEvent::ConfigError {
            server_name: server_name.to_string(),
            error,
        },
    }
}

fn mcp_runtime_unavailable_report(
    operation: McpRuntimeOperation,
    server_name: &str,
) -> McpRuntimeReport {
    mcp_runtime_report(
        server_name,
        "error",
        Some("MCP runtime manager is not available".to_string()),
        format!(
            "Cannot {} MCP server `{}` because the live runtime manager is not available.",
            operation.verb(),
            server_name
        ),
        "error",
    )
}

fn mcp_runtime_report(
    server_name: &str,
    state: &str,
    error: Option<String>,
    text: String,
    level: &str,
) -> McpRuntimeReport {
    allthecodes_mcp::runtime::record_server_state(server_name, state, error.clone());
    McpRuntimeReport {
        server_name: server_name.to_string(),
        state: state.to_string(),
        error,
        text,
        level: level.to_string(),
    }
}

fn mcp_runtime_report_messages(report: McpRuntimeReport) -> Vec<BackendMessage> {
    vec![
        BackendMessage::McpEvent {
            event: McpEvent::ServerStateChanged {
                server_name: report.server_name,
                state: report.state,
                error: report.error,
            },
        },
        BackendMessage::SystemInfo {
            text: report.text,
            level: report.level,
        },
    ]
}

fn find_mcp_runtime_config(
    cwd: &Path,
    server_name: &str,
) -> Result<allthecodes_mcp::McpServerConfig, String> {
    let configs = allthecodes_mcp::discovery::discover_mcp_servers(cwd)
        .map_err(|err| format!("Failed to discover MCP servers: {err}"))?;
    configs
        .into_iter()
        .find(|cfg| cfg.name == server_name)
        .ok_or_else(|| {
            format!(
                "No MCP server named `{}` was found in the current discovery scope.",
                server_name
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_ipc_protocol::subsystem_types::{ConfigScope, McpServerConfigEntry};

    pub(super) struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        pub(super) fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    pub(super) struct RuntimeMcpGuard;

    impl RuntimeMcpGuard {
        pub(super) fn install(
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
    fn handle_mcp_query_status_returns_server_list() {
        let msgs = handle_mcp_command(McpCommand::QueryStatus);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], BackendMessage::McpEvent { .. }));
    }

    #[test]
    #[serial_test::serial]
    fn build_mcp_server_info_list_defaults_to_pending() {
        allthecodes_mcp::runtime::clear_for_tests();
        let infos = super::super::snapshot::build_mcp_server_info_list();
        for info in &infos {
            assert_eq!(info.state, "pending");
        }
    }

    #[test]
    #[serial_test::serial]
    fn handle_mcp_upsert_config_emits_config_changed() {
        let home = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "h-test".to_string(),
            scope: ConfigScope::User,
            transport: "stdio".to_string(),
            command: Some("t".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };
        let msgs = handle_mcp_command(McpCommand::UpsertConfig {
            entry: Box::new(entry),
        });
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            BackendMessage::McpEvent {
                event:
                    McpEvent::ConfigChanged {
                        server_name,
                        entry: Some(e),
                    },
            } => {
                assert_eq!(server_name, "h-test");
                assert_eq!(e.name, "h-test");
                assert_eq!(e.scope, ConfigScope::User);
            }
            other => panic!("unexpected response: {:?}", other),
        }
    }

    #[test]
    #[serial_test::serial]
    fn handle_mcp_upsert_config_on_read_only_emits_config_error() {
        let entry = McpServerConfigEntry {
            name: "plugin-srv".to_string(),
            scope: ConfigScope::Plugin {
                id: "com.example".to_string(),
            },
            transport: "stdio".to_string(),
            command: Some("x".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };
        let msgs = handle_mcp_command(McpCommand::UpsertConfig {
            entry: Box::new(entry),
        });
        match &msgs[0] {
            BackendMessage::McpEvent {
                event: McpEvent::ConfigError { server_name, .. },
            } => assert_eq!(server_name, "plugin-srv"),
            other => panic!("expected ConfigError, got {:?}", other),
        }
    }

    #[test]
    #[serial_test::serial]
    fn handle_mcp_query_config_returns_config_list() {
        let msgs = handle_mcp_command(McpCommand::QueryConfig);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            BackendMessage::McpEvent {
                event: McpEvent::ConfigList { .. },
            } => {}
            other => panic!("expected ConfigList, got {:?}", other),
        }
    }

    #[test]
    #[serial_test::serial]
    fn handle_mcp_toggle_enabled_emits_config_changed_and_state() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        // Seed an entry in user scope via the handler so the `cwd` used
        // to discover matches the one the toggle handler uses.
        let prev_cwd = std::env::current_dir().ok();
        std::env::set_current_dir(cwd.path()).expect("set cwd");
        let seed = McpServerConfigEntry {
            name: "h-tog".to_string(),
            scope: ConfigScope::User,
            transport: "stdio".to_string(),
            command: Some("t".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };
        let _ = handle_mcp_command(McpCommand::UpsertConfig {
            entry: Box::new(seed),
        });

        let msgs = handle_mcp_command(McpCommand::ToggleEnabled {
            server_name: "h-tog".to_string(),
            scope: Some(ConfigScope::User),
        });
        assert_eq!(msgs.len(), 2, "expected ConfigChanged + ServerStateChanged");
        match &msgs[0] {
            BackendMessage::McpEvent {
                event:
                    McpEvent::ConfigChanged {
                        server_name,
                        entry: Some(e),
                    },
            } => {
                assert_eq!(server_name, "h-tog");
                assert_eq!(e.disabled, Some(true));
            }
            other => panic!("unexpected first message: {:?}", other),
        }
        match &msgs[1] {
            BackendMessage::McpEvent {
                event: McpEvent::ServerStateChanged { state, .. },
            } => {
                assert_eq!(state, "disabled");
            }
            other => panic!("unexpected second message: {:?}", other),
        }

        if let Some(p) = prev_cwd {
            let _ = std::env::set_current_dir(p);
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn handle_mcp_reconnect_uses_runtime_manager_and_emits_final_state() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
        let manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            allthecodes_mcp::manager::McpManager::new(),
        ));
        let _runtime = RuntimeMcpGuard::install(manager.clone());
        std::fs::write(
            home.path().join("settings.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "rec-srv": {
                        "type": "stdio",
                        "command": "unused",
                        "disabled": true
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let msgs = handle_mcp_command_with_runtime(
            McpCommand::ReconnectServer {
                server_name: "rec-srv".to_string(),
            },
            cwd.path(),
        )
        .await;
        assert!(
            manager.lock().await.clients.is_empty(),
            "disabled reconnect must not keep a live client"
        );
        assert_eq!(msgs.len(), 2);
        match &msgs[0] {
            BackendMessage::McpEvent {
                event:
                    McpEvent::ServerStateChanged {
                        server_name, state, ..
                    },
            } => {
                assert_eq!(server_name, "rec-srv");
                assert_eq!(state, "disabled");
            }
            other => panic!("expected ServerStateChanged, got {:?}", other),
        }
        match &msgs[1] {
            BackendMessage::SystemInfo { text, .. } => {
                assert!(text.contains("rec-srv"));
                assert!(text.contains("disabled"));
            }
            other => panic!("expected SystemInfo, got {:?}", other),
        }
    }

    #[test]
    #[serial_test::serial]
    fn build_mcp_server_config_entries_tags_user_scope() {
        use super::super::snapshot::build_mcp_server_config_entries;
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        std::fs::write(
            home.path().join("settings.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "u-srv": {"transport": "stdio", "command": "u-cmd"}
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let entries = build_mcp_server_config_entries(cwd.path());
        let entry = entries
            .iter()
            .find(|e| e.name == "u-srv")
            .expect("user entry present");
        assert_eq!(entry.scope, ConfigScope::User);
        assert_eq!(entry.command.as_deref(), Some("u-cmd"));
    }
}
