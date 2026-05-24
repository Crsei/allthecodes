use std::sync::LazyLock;

use parking_lot::RwLock;

use crate::types::tool::Tools;

static RUNTIME_TOOLS: LazyLock<RwLock<Tools>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Install the session's merged tool list for search/catalog consumers.
///
/// The full `ToolSearch` runtime remains in the root tool registry for now;
/// the engine only needs to retain the refreshed catalog snapshot during tool
/// refreshes.
pub fn install_runtime_tool_catalog(tools: &Tools) {
    *RUNTIME_TOOLS.write() = tools.clone();
}

pub fn runtime_tool_catalog() -> Tools {
    RUNTIME_TOOLS.read().clone()
}
