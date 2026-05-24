//! MCP reconnect status rendering.

use super::utils::reconnect_helpers::{reconnect_label, ReconnectAttempt};

pub fn render_mcp_reconnect(attempt: &ReconnectAttempt) -> String {
    format!("{}\nEnter retry | Esc cancel", reconnect_label(attempt))
}
