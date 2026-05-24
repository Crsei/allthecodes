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
    if !server.warnings.is_empty() {
        lines.push(format!("  warning: {}", server.warnings.join("; ")));
    }
    lines.join("\n")
}
