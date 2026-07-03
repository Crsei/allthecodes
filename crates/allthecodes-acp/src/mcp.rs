//! ACP MCP policy boundary.
//!
//! Per-session MCP transports are intentionally unadvertised in this branch.
//! Lifecycle requests may include the schema field, but any non-empty server
//! list is rejected until separate MCP-over-ACP feature work lands.

/// Reject non-empty ACP per-session MCP server declarations.
pub fn reject_session_mcp_servers<T>(servers: &[T]) -> Result<(), String> {
    if servers.is_empty() {
        Ok(())
    } else {
        Err("ACP MCP server connections are not yet supported".to_string())
    }
}
