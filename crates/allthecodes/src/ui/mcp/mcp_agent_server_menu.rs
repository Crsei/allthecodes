//! Agent-provided MCP server menu.

use super::index::{McpServer, McpServerKind};

pub fn render_mcp_agent_server_menu(server: &McpServer, owner_agent: &str) -> String {
    let kind = if server.kind == McpServerKind::Agent {
        "agent"
    } else {
        "not-agent"
    };
    [
        format!("Server: {}", server.name),
        format!("kind: {kind}"),
        format!("owner: {owner_agent}"),
        format!("status: {}", server.status.label()),
        "actions: view tools | reconnect".to_string(),
    ]
    .join("\n")
}
