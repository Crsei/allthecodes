use std::sync::Arc;

use allthecodes_engine::types::tool::Tools;
use tracing::{info, warn};

use crate::startup::settings_runtime::SettingsRuntime;
use crate::startup::startup_context::StartupContext;
use crate::startup::tool_catalog::ToolCatalog;
use crate::startup_skills::log_skill_report;

pub(crate) struct McpRuntime {
    pub(crate) tools: Tools,
}

pub(crate) struct McpRuntimeBuilder;

impl McpRuntimeBuilder {
    pub(crate) async fn build(
        startup: &StartupContext,
        _settings: &SettingsRuntime,
        mut tool_catalog: ToolCatalog,
    ) -> anyhow::Result<McpRuntime> {
        use allthecodes_engine::mcp_tool_adapter::mcp_tools_to_tools_for_context;
        use allthecodes_mcp::bindings::canonical_workspace_root;
        use allthecodes_mcp::discovery::{discover_bound_mcp_servers, BoundMcpServerConfig};
        use allthecodes_mcp::manager::McpManager;
        use allthecodes_mcp::{McpBinding, McpBindingContext, McpToolScope};

        let mut discovered = match discover_bound_mcp_servers(startup.cwd.as_path(), None) {
            Ok(discovered) => discovered,
            Err(err) => {
                warn!(error = %err, "MCP server discovery failed");
                Default::default()
            }
        };
        let mcp_manager = Arc::new(tokio::sync::Mutex::new(McpManager::new()));
        let workspace_root = canonical_workspace_root(startup.cwd.as_path());

        if startup.chrome_enabled {
            if let Ok(exe) = std::env::current_exe() {
                let name = allthecodes_browser::common::CLAUDE_IN_CHROME_MCP_SERVER_NAME;
                if !discovered
                    .servers
                    .iter()
                    .any(|server| server.server_id == name || server.display_name == name)
                {
                    discovered.servers.push(BoundMcpServerConfig {
                        server_id: name.to_string(),
                        display_name: name.to_string(),
                        source_scope: "builtin".to_string(),
                        config: allthecodes_mcp::McpServerConfig {
                            name: name.to_string(),
                            transport: "stdio".to_string(),
                            command: Some(exe.to_string_lossy().into_owned()),
                            args: Some(vec!["--claude-in-chrome-mcp".to_string()]),
                            url: None,
                            headers: None,
                            oauth: None,
                            env: None,
                            browser_mcp: Some(true),
                            disabled: None,
                            bearer_token_env_var: None,
                            env_http_headers: None,
                            auth: None,
                        },
                    });
                    discovered.bindings.push(McpBinding {
                        server_id: name.to_string(),
                        scope: McpToolScope::Global,
                        permissions: McpBinding::full_permissions(),
                        source_scope: Some("builtin".to_string()),
                        ..Default::default()
                    });
                    info!(
                        "MCP: registered first-party claude-in-chrome bridge (spawns --claude-in-chrome-mcp subprocess)"
                    );
                }
            }
        }

        let configs_for_browser = discovered
            .servers
            .iter()
            .map(|bound| {
                let mut config = bound.config.clone();
                config.name = bound.server_id.clone();
                config
            })
            .collect::<Vec<_>>();

        if !discovered.servers.is_empty() {
            info!(
                count = discovered.servers.len(),
                "MCP: connecting to configured servers"
            );
            let mut mgr = mcp_manager.lock().await;
            if let Err(e) = mgr
                .connect_all_bound(discovered.servers, discovered.bindings)
                .await
            {
                warn!(error = %e, "MCP: some servers failed to connect");
            }

            let startup_context = McpBindingContext::startup(Some(workspace_root.clone()));
            let mcp_tool_defs = mgr.tools_for_context(&startup_context);
            if !mcp_tool_defs.is_empty() {
                let mcp_tools = mcp_tools_to_tools_for_context(
                    mcp_tool_defs,
                    mcp_manager.clone(),
                    startup_context.clone(),
                );
                info!(
                    count = mcp_tools.len(),
                    "MCP: discovered tools, merging with base tools"
                );
                tool_catalog.tools.extend(mcp_tools);
            }

            let (mcp_skills, mcp_skill_diagnostics) =
                allthecodes_engine::mcp_tool_adapter::discover_mcp_skill_resources_for_context(
                    &mgr,
                    &startup_context,
                )
                .await;
            if !mcp_skills.is_empty() || !mcp_skill_diagnostics.is_empty() {
                let report = allthecodes_skills::register_skills_resolved_with_diagnostics(
                    mcp_skills,
                    mcp_skill_diagnostics,
                    allthecodes_skills::SkillLoadOptions::for_app_version(env!(
                        "CARGO_PKG_VERSION"
                    )),
                );
                log_skill_report("mcp", &report);
            }
        }

        let tool_names = tool_catalog
            .tools
            .iter()
            .map(|tool| tool.user_facing_name(None))
            .collect::<Vec<_>>();
        let mut browser_servers =
            allthecodes_browser::detection::detect_browser_servers_from_tool_names(
                configs_for_browser
                    .iter()
                    .map(|config| (config.name.as_str(), config.browser_mcp.unwrap_or(false))),
                tool_names.iter().map(String::as_str),
            );
        if startup.chrome_enabled {
            browser_servers
                .insert(allthecodes_browser::common::CLAUDE_IN_CHROME_MCP_SERVER_NAME.to_string());
        }
        if !browser_servers.is_empty() {
            info!(
                count = browser_servers.len(),
                "Browser MCP: detected browser-shaped MCP server(s)"
            );
        }
        allthecodes_browser::detection::install_browser_servers(browser_servers);

        allthecodes_mcp::runtime::install_manager(mcp_manager.clone());

        if startup.computer_use_enabled {
            let cu_tools = allthecodes_computer_use::setup::register_cu_tools();
            info!(
                count = cu_tools.len(),
                "Computer Use: registered native desktop control tools"
            );
            tool_catalog.tools.extend(cu_tools);
        }

        allthecodes_tools::runtime::tool_search::install_runtime_tool_catalog(&tool_catalog.tools);

        Ok(McpRuntime {
            tools: tool_catalog.tools,
        })
    }
}
