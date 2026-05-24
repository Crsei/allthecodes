//! Stdio MCP server menu.

use super::index::{McpServer, McpServerKind};

pub fn render_mcp_stdio_server_menu(server: &McpServer) -> String {
    let kind = if server.kind == McpServerKind::Stdio {
        "stdio"
    } else {
        "not-stdio"
    };
    [
        format!("Server: {}", server.name),
        format!("kind: {kind}"),
        format!("command: {}", server.command_or_url),
        format!("status: {}", server.status.label()),
        "actions: reconnect | disable | remove".to_string(),
    ]
    .join("\n")
}
