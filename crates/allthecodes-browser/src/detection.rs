//! Pure browser-MCP detection primitives.
//!
//! Split out of the root crate's `browser::detection` in Phase 4 (issue #73)
//! so cc-browser sub-modules (`permissions`, `tool_rendering`) can call
//! these helpers without pulling in `Arc<dyn Tool>` — the registry walker
//! (`detect_browser_servers` / `detect_browser_tools`) stays in the root
//! crate until `Tool` moves out in Phase 5.

use std::collections::HashSet;

/// MCP tool-name prefix for all MCP-wrapped tools.
pub const MCP_PREFIX: &str = "mcp__";

/// Known browser-automation tool basenames.
///
/// Intentionally generous — different servers use different names for
/// essentially the same actions (`click` vs `browser_click`, `navigate`
/// vs `goto`). We match on basename only, so the leading `mcp__{server}__`
/// has already been stripped.
pub const BROWSER_TOOL_BASENAMES: &[&str] = &[
    // Navigation / tabs
    "navigate",
    "navigate_page",
    "goto",
    "tabs_create",
    "tabs_create_mcp",
    "tabs_close",
    "tabs_close_mcp",
    "tabs_context",
    "tabs_context_mcp",
    "new_page",
    "close_page",
    "switch_browser",
    "select_page",
    "list_pages",
    // Page reading
    "read_page",
    "get_page_text",
    "take_snapshot",
    "snapshot",
    "get_page",
    // DOM / element interaction
    "click",
    "browser_click",
    "double_click",
    "hover",
    "drag",
    "press_key",
    "type_text",
    "fill",
    "fill_form",
    "form_input",
    "select",
    // File upload
    "upload_file",
    "file_upload",
    // JavaScript execution
    "evaluate_script",
    "javascript_tool",
    "evaluate",
    // Console / network observability
    "get_console_message",
    "list_console_messages",
    "read_console_messages",
    "get_network_request",
    "list_network_requests",
    "read_network_requests",
    // Screenshots / visual
    "take_screenshot",
    "screenshot",
    // Misc
    "wait_for",
    "find",
    "resize_page",
    "resize_window",
    "emulate",
    "handle_dialog",
];

/// Parse an MCP tool name into `(server, action)` if the action matches a
/// recognized browser basename.
pub fn extract_browser_action(tool_name: &str) -> Option<(&str, &str)> {
    let rest = tool_name.strip_prefix(MCP_PREFIX)?;
    let (server, action) = rest.split_once("__")?;
    if BROWSER_TOOL_BASENAMES.contains(&action) {
        Some((server, action))
    } else {
        None
    }
}

/// Detect browser MCP servers from server configs and already-registered MCP
/// tool display names.
///
/// This keeps browser classification independent of the host's concrete Tool
/// trait: callers only need to pass names such as `mcp__chrome__navigate`.
pub fn detect_browser_servers_from_tool_names<'a, 'b>(
    config_flags: impl IntoIterator<Item = (&'b str, bool)>,
    tool_names: impl IntoIterator<Item = &'a str>,
) -> HashSet<String> {
    let mut servers: HashSet<String> = config_flags
        .into_iter()
        .filter(|(_, browser_mcp)| *browser_mcp)
        .map(|(name, _)| name.to_string())
        .collect();

    for name in tool_names {
        if let Some((server, _)) = extract_browser_action(name) {
            servers.insert(server.to_string());
        }
    }

    servers
}

// ---------------------------------------------------------------------------
// Process-wide registry of browser server names
// ---------------------------------------------------------------------------
//
// Populated once at startup after MCP discovery + tool registration, then
// consulted by the system-prompt assembler, the permission decision path,
// and `/mcp list`.

static BROWSER_SERVERS: parking_lot::RwLock<Option<HashSet<String>>> =
    parking_lot::RwLock::new(None);

/// Install the set of browser MCP server names for the rest of the process.
///
/// Call this once after MCP discovery + tool registration. Subsequent calls
/// overwrite the registry.
pub fn install_browser_servers(servers: HashSet<String>) {
    *BROWSER_SERVERS.write() = Some(servers);
}

/// Snapshot the current set of browser server names (empty if not installed).
pub fn browser_servers_snapshot() -> HashSet<String> {
    BROWSER_SERVERS.read().clone().unwrap_or_default()
}

/// Check whether a server name is registered as a browser server. Consults the
/// global registry and falls back to `false` if the registry was never installed.
pub fn is_browser_server(name: &str) -> bool {
    match BROWSER_SERVERS.read().as_ref() {
        Some(set) => set.contains(name),
        None => false,
    }
}

/// Clear the browser-server registry — tests only. `#[doc(hidden)]` keeps
/// it out of rustdoc; the `pub` visibility lets tests in the root crate
/// exercise detection flows with a clean registry.
#[doc(hidden)]
pub fn clear_browser_servers_for_tests() {
    *BROWSER_SERVERS.write() = None;
}
