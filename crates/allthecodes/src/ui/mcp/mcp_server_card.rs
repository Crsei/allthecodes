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
    let health_fields = server.health.summary_fields();
    if !health_fields.is_empty() {
        lines.push(format!("  health: {}", health_fields.join("  ")));
    }
    if let Some(stderr) = server.health.stderr_tail.last() {
        lines.push(format!("  stderr: {stderr}"));
    }
    if !server.warnings.is_empty() {
        lines.push(format!("  warning: {}", server.warnings.join("; ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mcp::index::{McpServerKind, McpServerStatus};

    #[test]
    fn server_card_includes_health_details() {
        let mut server = McpServer::new("bridge", McpServerKind::Stdio);
        server.status = McpServerStatus::Failed;
        server.health.last_error_kind = Some("spawn_failed".to_string());
        server.health.failure_count = Some(3);
        server.health.stderr_tail = vec!["startup failed".to_string()];

        let rendered = render_mcp_server_card(&server, true);

        assert!(rendered.contains("health: error_kind=spawn_failed"));
        assert!(rendered.contains("failures=3"));
        assert!(rendered.contains("stderr: startup failed"));
    }
}
