//! Shared MCP server safety copy.

pub fn mcp_server_safety_copy() -> &'static str {
    "MCP servers may execute code or access system resources. All tool calls require approval. Learn more in the MCP documentation."
}

pub fn render_mcp_server_dialog_copy() -> String {
    [
        "MCP server security",
        mcp_server_safety_copy(),
        "Enter continue | Esc cancel",
    ]
    .join("\n")
}
