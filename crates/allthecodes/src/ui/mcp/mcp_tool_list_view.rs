//! MCP tool list rendering.

use super::index::McpTool;

pub fn render_mcp_tool_list_view(tools: &[McpTool], selected_index: usize) -> String {
    if tools.is_empty() {
        return "Tools\nNo tools".to_string();
    }
    let mut lines = vec![format!("Tools ({})", tools.len())];
    for (idx, tool) in tools.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        lines.push(format!("{marker} {:<20} {}", tool.name, tool.description));
    }
    lines.join("\n")
}
