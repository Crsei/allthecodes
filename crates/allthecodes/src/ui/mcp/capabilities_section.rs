//! MCP capabilities summary.

use super::index::McpCapability;

pub fn render_capabilities_section(capabilities: &[McpCapability]) -> String {
    if capabilities.is_empty() {
        return "Capabilities\nnone advertised".to_string();
    }
    let mut lines = vec!["Capabilities".to_string()];
    for capability in capabilities {
        lines.push(format!("- {:<12} {}", capability.name, capability.count));
    }
    lines.join("\n")
}
