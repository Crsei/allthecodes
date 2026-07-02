//! Remote MCP server menu.

use super::index::{McpServer, McpServerKind};

pub fn render_mcp_remote_server_menu(server: &McpServer) -> String {
    let kind = if server.kind == McpServerKind::Remote {
        "remote"
    } else {
        "not-remote"
    };
    let mut lines = vec![
        format!("Server: {}", server.name),
        format!("kind: {kind}"),
        format!("url: {}", server.command_or_url),
        format!("status: {}", server.status.label()),
    ];
    if let Some(source) = &server.config_source {
        lines.push(format!("source: {source}"));
    }
    if let Some(scope) = &server.binding_scope {
        lines.push(format!("binding: {scope}"));
    }
    if !server.binding_permissions.is_empty() {
        lines.push(format!(
            "permissions: {}",
            server.binding_permissions.join(",")
        ));
    }
    lines.push("actions: reconnect | open | disable".to_string());
    lines.join("\n")
}
