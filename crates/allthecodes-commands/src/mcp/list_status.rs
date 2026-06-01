use anyhow::Result;

use super::settings::{describe_entry, discover_config_entries};
use super::{CommandContext, CommandResult};
use allthecodes_ipc_protocol::subsystem_types::{
    McpServerConfigEntry, McpServerInfoBrief, McpServerStatusInfo,
};

// ---------------------------------------------------------------------------
// list / status
// ---------------------------------------------------------------------------

pub(super) async fn handle_list(ctx: &CommandContext) -> Result<CommandResult> {
    let entries = discover_config_entries(&ctx.cwd);
    let status = build_status_from_discovery(&ctx.cwd).await;

    if entries.is_empty() {
        return Ok(CommandResult::Output(
            "No MCP servers discovered.\n\n\
             Add servers to ~/.allthecodes/settings.json or .allthecodes/settings.json, or run:\n  \
               /mcp add <name> --command=<cmd> [--arg=<arg> …]"
                .to_string(),
        ));
    }

    let browser_count = entries
        .iter()
        .filter(|e| {
            e.browser_mcp.unwrap_or(false)
                || allthecodes_browser::detection::is_browser_server(&e.name)
        })
        .count();

    let mut lines = Vec::new();
    if browser_count > 0 {
        lines.push(format!(
            "Discovered MCP servers ({}; {} browser):",
            entries.len(),
            browser_count
        ));
    } else {
        lines.push(format!("Discovered MCP servers ({}):", entries.len()));
    }
    lines.push(String::new());

    // Group by scope label for readability.
    let mut by_scope: Vec<(String, Vec<&McpServerConfigEntry>)> = Vec::new();
    for entry in &entries {
        let label = entry.scope.label();
        if let Some(bucket) = by_scope.iter_mut().find(|(l, _)| *l == label) {
            bucket.1.push(entry);
        } else {
            by_scope.push((label, vec![entry]));
        }
    }

    for (label, bucket) in &by_scope {
        lines.push(format!("[{}]", label));
        for entry in bucket {
            let state = status
                .iter()
                .find(|s| s.name == entry.name)
                .map(|s| s.state.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let desc = describe_entry(entry);
            let tag = if entry.browser_mcp.unwrap_or(false)
                || allthecodes_browser::detection::is_browser_server(&entry.name)
            {
                " [browser]"
            } else {
                ""
            };
            lines.push(format!("  {}{} -- {} -- {}", entry.name, tag, state, desc));
        }
        lines.push(String::new());
    }

    if browser_count > 0 {
        lines.push(
            "Browser-tagged servers expose browser-automation tools (navigate, \
             read_page, click, -. See development/reference/browser-mcp-config.md."
                .to_string(),
        );
    }

    Ok(CommandResult::Output(
        lines.join("\n").trim_end().to_string(),
    ))
}

pub(super) async fn handle_status(ctx: &CommandContext) -> Result<CommandResult> {
    let status = build_status_from_discovery(&ctx.cwd).await;
    if status.is_empty() {
        return Ok(CommandResult::Output(
            "No MCP servers discovered.".to_string(),
        ));
    }

    let mut lines = Vec::new();
    lines.push(format!("MCP server status ({}):", status.len()));
    lines.push(String::new());
    for info in &status {
        let err = info
            .error
            .as_ref()
            .map(|e| format!(" -- {}", e))
            .unwrap_or_default();
        lines.push(format!(
            "  {} -- {} ({} tools, {} resources){}",
            info.name, info.state, info.tools_count, info.resources_count, err
        ));
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

async fn build_status_from_discovery(cwd: &std::path::Path) -> Vec<McpServerStatusInfo> {
    let configs = match allthecodes_mcp::discovery::discover_mcp_servers(cwd) {
        Ok(configs) => configs,
        Err(err) => {
            return vec![McpServerStatusInfo {
                name: "discovery".to_string(),
                state: "error".to_string(),
                transport: "settings".to_string(),
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: Some(format!("Failed to discover MCP servers: {err:#}")),
            }];
        }
    };

    let manager_handle = allthecodes_mcp::runtime::current_manager();
    let manager = match manager_handle.as_ref() {
        Some(manager) => Some(manager.lock().await),
        None => None,
    };

    configs
        .into_iter()
        .map(|cfg| {
            if cfg.disabled.unwrap_or(false) {
                return McpServerStatusInfo {
                    name: cfg.name,
                    state: "disabled".to_string(),
                    transport: cfg.transport,
                    tools_count: 0,
                    resources_count: 0,
                    server_info: None,
                    instructions: None,
                    error: None,
                };
            }

            if let Some(client) = manager.as_ref().and_then(|m| m.clients.get(&cfg.name)) {
                let (state, error) = match &client.state {
                    allthecodes_mcp::McpConnectionState::Pending => ("pending".to_string(), None),
                    allthecodes_mcp::McpConnectionState::Connected => {
                        ("connected".to_string(), None)
                    }
                    allthecodes_mcp::McpConnectionState::Disconnected => {
                        ("disconnected".to_string(), None)
                    }
                    allthecodes_mcp::McpConnectionState::Error(error) => {
                        ("error".to_string(), Some(error.clone()))
                    }
                };
                let server_info =
                    (!client.server_info.name.is_empty()).then(|| McpServerInfoBrief {
                        name: client.server_info.name.clone(),
                        version: client.server_info.version.clone(),
                    });
                return McpServerStatusInfo {
                    name: cfg.name,
                    state,
                    transport: cfg.transport,
                    tools_count: client.tools.len(),
                    resources_count: client.resources.len(),
                    server_info,
                    instructions: client.instructions.clone(),
                    error,
                };
            }

            let remembered = allthecodes_mcp::runtime::server_state(&cfg.name);
            McpServerStatusInfo {
                name: cfg.name,
                state: remembered
                    .as_ref()
                    .map(|state| state.state.clone())
                    .unwrap_or_else(|| "pending".to_string()),
                transport: cfg.transport,
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: remembered.and_then(|state| state.error),
            }
        })
        .collect()
}
