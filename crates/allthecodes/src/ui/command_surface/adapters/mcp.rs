use std::path::Path;

use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::mcp::index::{McpServer, McpServerKind, McpServerStatus, McpTool};
use crate::ui::mcp::mcp_list_panel::McpListPanelState;
pub(crate) fn build_mcp_servers(cwd: &Path) -> Vec<McpServer> {
    crate::app_runtime_adapters::ensure_installed();
    let entries = allthecodes_ipc::subsystem_handlers::build_mcp_server_config_entries(cwd);
    let status = allthecodes_ipc::subsystem_handlers::build_mcp_server_info_list();
    entries
        .into_iter()
        .map(|entry| {
            let live = status.iter().find(|item| item.name == entry.name);
            let kind = match entry.transport.as_str() {
                "sse" | "streamable-http" => McpServerKind::Remote,
                _ => McpServerKind::Stdio,
            };
            let mut server = McpServer::new(entry.name.clone(), kind);
            server.status = if entry.disabled.unwrap_or(false) {
                McpServerStatus::Disabled
            } else {
                live.map(|item| status_from_label(&item.state))
                    .unwrap_or(McpServerStatus::Connecting)
            };
            server.command_or_url = entry
                .url
                .or(entry.command)
                .unwrap_or_else(|| entry.scope.label());
            let tools_count = live.map(|item| item.tools_count).unwrap_or(0);
            server.tools = (0..tools_count)
                .map(|idx| McpTool::new(format!("tool-{}", idx + 1), "registered MCP tool"))
                .collect();
            if let Some(error) = live.and_then(|item| item.error.clone()) {
                server.warnings.push(error);
            }
            server
        })
        .collect()
}

pub(crate) fn status_from_label(value: &str) -> McpServerStatus {
    match value {
        "connected" | "running" => McpServerStatus::Connected,
        "connecting" | "starting" => McpServerStatus::Connecting,
        "disabled" | "disconnected" | "stopped" => McpServerStatus::Disabled,
        _ => McpServerStatus::Failed,
    }
}

pub(crate) fn selected_server_command(
    state: &McpListPanelState,
    prefix: &str,
    suffix: &str,
) -> CommandSurfaceOutcome {
    state
        .selected_server()
        .map(|server| {
            if prefix.contains(" edit ") {
                CommandSurfaceOutcome::FillPrompt(format!("{prefix}{}{suffix}", server.name))
            } else {
                CommandSurfaceOutcome::Submit(format!("{prefix}{}{suffix}", server.name))
            }
        })
        .unwrap_or(CommandSurfaceOutcome::None)
}
