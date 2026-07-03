//! ACP MCP server input mapping.
//!
//! Maps ACP `McpServerInput` (part of `NewSessionRequest`) to
//! `allthecodes_mcp::McpServerConfig` for per-session MCP server connections.
//!
//! NOTE: This module is a stub that validates and stores MCP server configs.
//! Full per-session McpManager integration requires connecting the servers
//! from `NewSessionRequest.mcp_servers` field and merging their tools into
//! the session engine.

use allthecodes_engine::types::tool::Tools;

/// Validate MCP server configs from a session/new request.
///
/// Returns a list of `(name, McpServerConfig)` tuples if validation passes,
/// or an error describing the first invalid config.
pub fn validate_mcp_configs(
    _servers: &serde_json::Value,
) -> Result<Vec<(String, allthecodes_mcp::McpServerConfig)>, String> {
    if _servers.as_object().map_or(false, |m| !m.is_empty()) {
        return Err("MCP server connections from ACP are not yet supported".to_string());
    }
    Ok(Vec::new())
}

/// Connect per-session MCP servers and return their tools.
///
/// Stub: returns an empty vec until MCP integration is implemented.
pub async fn connect_session_mcp(
    _configs: Vec<(String, allthecodes_mcp::McpServerConfig)>,
    _session_id: &str,
) -> Result<Tools, String> {
    Ok(Vec::new())
}
