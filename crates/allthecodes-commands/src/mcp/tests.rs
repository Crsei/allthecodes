use super::*;
use allthecodes_bootstrap::SessionId;
use allthecodes_engine::types::app_state::AppState;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn test_ctx(cwd: PathBuf) -> CommandContext {
    CommandContext {
        messages: Vec::new(),
        cwd,
        app_state: AppState::default(),
        session_id: SessionId::new(),
    }
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock poisoned");
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key,
            previous,
            _lock: lock,
        }
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

#[tokio::test]
async fn mcp_no_args_shows_help() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/test/project"));
    let result = handler.execute("", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(text.contains("MCP"));
            assert!(text.contains("/mcp add"));
        }
        _ => panic!("Expected Output result"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_list_no_servers_suggests_add() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    let result = handler.execute("list", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(text) => {
            assert!(
                text.contains("No MCP servers discovered") || text.contains("Discovered"),
                "unexpected output: {}",
                text
            );
        }
        _ => panic!("Expected Output result"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_add_persists_user_scope() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    let res = handler
        .execute(
            "add ctx7 --command=npx --arg=-y --arg=ctx7 --env=FOO=bar",
            &mut ctx,
        )
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("Upserted MCP server `ctx7`") && text.contains("scope `user`"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }

    let settings = home.path().join("settings.json");
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(disk["mcpServers"]["ctx7"]["command"], "npx");
    assert_eq!(disk["mcpServers"]["ctx7"]["args"][1], "ctx7");
    assert_eq!(disk["mcpServers"]["ctx7"]["env"]["FOO"], "bar");
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_add_persists_oauth_metadata_without_tokens() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    handler
            .execute(
                "add remote --transport=sse --url=https://mcp.example.com/sse \
                 --oauth-auth-server-metadata-url=https://auth.example.com/.well-known/oauth-authorization-server \
                 --oauth-client-id=allthecodes-test --oauth-callback-port=18888 --oauth-scope=tools.read",
                &mut ctx,
            )
            .await
            .unwrap();

    let settings = home.path().join("settings.json");
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let oauth = &disk["mcpServers"]["remote"]["oauth"];
    assert_eq!(oauth["clientId"], "allthecodes-test");
    assert_eq!(oauth["callbackPort"], 18888);
    assert_eq!(oauth["scopes"][0], "tools.read");
    assert!(oauth.get("accessToken").is_none());
    assert!(oauth.get("refreshToken").is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_edit_updates_command() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    handler
        .execute("add mysrv --command=./old.sh", &mut ctx)
        .await
        .unwrap();
    let res = handler
        .execute("edit mysrv --command=./new.sh --arg=foo", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("Upserted MCP server `mysrv`"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }

    let settings = home.path().join("settings.json");
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(disk["mcpServers"]["mysrv"]["command"], "./new.sh");
    assert_eq!(disk["mcpServers"]["mysrv"]["args"][0], "foo");
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_remove_deletes_from_user_scope() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    handler
        .execute("add goner --command=x", &mut ctx)
        .await
        .unwrap();
    let res = handler.execute("remove goner", &mut ctx).await.unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("Removed MCP server `goner`"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }

    let settings = home.path().join("settings.json");
    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let servers = disk
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .expect("mcpServers");
    assert!(!servers.contains_key("goner"), "goner should be removed");
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_remove_ambiguous_requires_scope() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    // Create both user and project rows with the same name.
    handler
        .execute("add dupe --command=u --scope=user", &mut ctx)
        .await
        .unwrap();
    handler
        .execute("add dupe --command=p --scope=project", &mut ctx)
        .await
        .unwrap();

    let res = handler.execute("remove dupe", &mut ctx).await.unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("exists in multiple scopes") && text.contains("--scope"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_approve_project_mcp_json_servers_records_enabled() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
    let project_settings = cwd.path().join(".allthecodes").join("settings.json");
    std::fs::create_dir_all(project_settings.parent().unwrap()).unwrap();
    std::fs::write(
        &project_settings,
        serde_json::to_string_pretty(&serde_json::json!({
            "enabledMcpjsonServers": ["existing"],
            "disabledMcpjsonServers": ["github", "old"],
            "enableAllProjectMcpServers": false
        }))
        .unwrap(),
    )
    .unwrap();

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    let res = handler
        .execute("approve github playwright --all-project", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("Approved .mcp.json server(s)")
                && text.contains("Future project MCP servers"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project_settings).unwrap()).unwrap();
    assert_eq!(disk["enabledMcpjsonServers"][0], "existing");
    assert_eq!(disk["enabledMcpjsonServers"][1], "github");
    assert_eq!(disk["enabledMcpjsonServers"][2], "playwright");
    assert_eq!(disk["disabledMcpjsonServers"][0], "old");
    assert_eq!(disk["enableAllProjectMcpServers"], true);
    assert!(
        !home.path().join("settings.json").exists(),
        "approval must stay in project .allthecodes settings"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_reject_project_mcp_json_servers_records_disabled() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
    let project_settings = cwd.path().join(".allthecodes").join("settings.json");
    std::fs::create_dir_all(project_settings.parent().unwrap()).unwrap();
    std::fs::write(
        &project_settings,
        serde_json::to_string_pretty(&serde_json::json!({
            "enabledMcpjsonServers": ["github", "keep"],
            "disabledMcpjsonServers": ["old"]
        }))
        .unwrap(),
    )
    .unwrap();

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    let res = handler
        .execute("reject github sentry", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(
            text.contains("Rejected .mcp.json server(s)"),
            "unexpected: {}",
            text
        ),
        _ => panic!("expected Output"),
    }

    let disk: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project_settings).unwrap()).unwrap();
    assert_eq!(disk["enabledMcpjsonServers"][0], "keep");
    assert_eq!(disk["disabledMcpjsonServers"][0], "old");
    assert_eq!(disk["disabledMcpjsonServers"][1], "github");
    assert_eq!(disk["disabledMcpjsonServers"][2], "sentry");
    assert!(
        !home.path().join("settings.json").exists(),
        "rejection must stay in project .allthecodes settings"
    );
}

#[tokio::test]
async fn mcp_add_stdio_requires_command() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler.execute("add nocmd", &mut ctx).await.unwrap();
    match res {
        CommandResult::Output(text) => {
            assert!(text.contains("requires --command"));
        }
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn mcp_sse_requires_url() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler
        .execute("add sse-only --transport=sse", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(text.contains("requires --url")),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn mcp_streamable_http_requires_url() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler
        .execute("add http-only --transport=streamable-http", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => assert!(text.contains("requires --url")),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn mcp_connect_requires_name() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler.execute("connect", &mut ctx).await.unwrap();
    match res {
        CommandResult::Output(text) => assert!(text.contains("Usage: /mcp connect")),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_reconnect_uses_runtime_manager() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
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

    let handler = McpHandler;
    let mut ctx = test_ctx(cwd.path().to_path_buf());
    let res = handler
        .execute("reconnect rec-srv", &mut ctx)
        .await
        .unwrap();
    match res {
        CommandResult::Output(text) => {
            assert!(text.contains("rec-srv"));
            assert!(text.contains("disabled"));
        }
        _ => panic!("expected Output"),
    }
    assert!(
        manager.lock().await.clients.is_empty(),
        "disabled reconnect must not keep a live client"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn mcp_status_reports_server_list() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler.execute("status", &mut ctx).await.unwrap();
    // Should return either "No MCP servers discovered." or a formatted list.
    match res {
        CommandResult::Output(text) => {
            assert!(
                text.contains("MCP server status") || text.contains("No MCP servers"),
                "unexpected output: {}",
                text
            );
        }
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn mcp_unknown_subcommand_shows_help() {
    let handler = McpHandler;
    let mut ctx = test_ctx(PathBuf::from("/tmp"));
    let res = handler.execute("foobar", &mut ctx).await.unwrap();
    match res {
        CommandResult::Output(text) => {
            assert!(text.contains("Unknown mcp subcommand"));
            assert!(text.contains("/mcp add"));
        }
        _ => panic!("expected Output"),
    }
}

#[test]
fn parse_flags_env_splits_on_first_equal() {
    let flags = parse_flags(&["--env=A=B=C"]);
    assert!(flags.error.is_none());
    assert_eq!(flags.env.get("A").map(String::as_str), Some("B=C"));
}

#[test]
fn parse_flags_invalid_scope_emits_error() {
    let flags = parse_flags(&["--scope=bogus"]);
    assert!(flags.error.as_ref().unwrap().contains("invalid --scope"));
}
