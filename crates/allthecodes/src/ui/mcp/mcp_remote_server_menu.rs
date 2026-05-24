//! Remote MCP server menu.

use super::index::{McpServer, McpServerKind};

pub fn render_mcp_remote_server_menu(server: &McpServer) -> String {
    let kind = if server.kind == McpServerKind::Remote {
        "remote"
    } else {
        "not-remote"
    };
    [
        format!("Server: {}", server.name),
        format!("kind: {kind}"),
        format!("url: {}", server.command_or_url),
        format!("status: {}", server.status.label()),
        "actions: reconnect | open | disable".to_string(),
    ]
    .join("\n")
}
