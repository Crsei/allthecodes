//! MCP settings summary rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSettingsSummary {
    pub enabled: bool,
    pub config_path: String,
    pub server_count: usize,
    pub warning_count: usize,
}

pub fn render_mcp_settings(summary: &McpSettingsSummary) -> String {
    [
        "MCP settings".to_string(),
        format!("enabled: {}", summary.enabled),
        format!("config: {}", summary.config_path),
        format!("servers: {}", summary.server_count),
        format!("warnings: {}", summary.warning_count),
    ]
    .join("\n")
}
