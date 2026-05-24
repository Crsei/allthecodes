//! MCP configuration parsing warning renderer.

pub fn render_mcp_parsing_warnings(warnings: &[String]) -> String {
    if warnings.is_empty() {
        return "No MCP parsing warnings".to_string();
    }
    let mut lines = vec![format!("{} MCP parsing warning(s)", warnings.len())];
    for warning in warnings {
        lines.push(format!("- {warning}"));
    }
    lines.join("\n")
}
