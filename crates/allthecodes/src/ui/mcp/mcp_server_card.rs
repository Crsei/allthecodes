//! Compact MCP server card rendering.

use super::index::McpServer;

pub fn render_mcp_server_card(server: &McpServer, selected: bool) -> String {
    let marker = if selected { ">" } else { " " };
    let mut lines = vec![
        format!(
            "{marker} {} [{}/{}]",
            server.name,
            server.kind.label(),
            server.status.label()
        ),
        format!(
            "  tools={} capabilities={} warnings={}",
            server.tools.len(),
            server.capabilities.len(),
            server.warnings.len()
        ),
    ];
    if !server.command_or_url.is_empty() {
        lines.push(format!("  {}", server.command_or_url));
    }
    let mut metadata = Vec::new();
    if let Some(source) = &server.config_source {
        metadata.push(format!("source={source}"));
    }
    if let Some(scope) = &server.binding_scope {
        metadata.push(format!("binding={scope}"));
    }
    if !server.binding_permissions.is_empty() {
        metadata.push(format!(
            "permissions={}",
            server.binding_permissions.join(",")
        ));
    }
    if !metadata.is_empty() {
        lines.push(format!("  {}", metadata.join("  ")));
    }
    if !server.warnings.is_empty() {
        lines.push(format!("  warning: {}", server.warnings.join("; ")));
    }
    lines.join("\n")
}
