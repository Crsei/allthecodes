//! MCP tool detail rendering.

use super::index::McpTool;

pub fn render_mcp_tool_detail_view(tool: &McpTool) -> String {
    let mut lines = vec![format!("Tool: {}", tool.name), tool.description.clone()];
    if tool.input_schema.is_empty() {
        lines.push("input: <none>".to_string());
    } else {
        lines.push("input:".to_string());
        for (name, ty) in &tool.input_schema {
            lines.push(format!("  - {name}: {ty}"));
        }
    }
    lines.join("\n")
}
