//! Shared MCP runtime manager handle.
//!
//! Startup owns the concrete `McpManager`, while slash commands and IPC
//! lifecycle commands need to reach that same manager later in the session.
//! This module keeps the handle and the last observed lifecycle state in one
//! narrow place so command surfaces do not create their own managers.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

use super::manager::McpManager;

pub type SharedMcpManager = Arc<tokio::sync::Mutex<McpManager>>;
type McpSkillCleanupHook = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct RuntimeMcpServerState {
    pub state: String,
    pub error: Option<String>,
}

static MANAGER: LazyLock<RwLock<Option<SharedMcpManager>>> = LazyLock::new(|| RwLock::new(None));
static SERVER_STATES: LazyLock<RwLock<HashMap<String, RuntimeMcpServerState>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static MCP_SKILL_CLEANUP_HOOK: LazyLock<RwLock<Option<McpSkillCleanupHook>>> =
    LazyLock::new(|| RwLock::new(None));

pub fn install_manager(manager: SharedMcpManager) {
    *MANAGER.write() = Some(manager);
}

pub fn current_manager() -> Option<SharedMcpManager> {
    MANAGER.read().clone()
}

pub fn install_mcp_skill_cleanup_hook<F>(hook: F)
where
    F: Fn(&str) + Send + Sync + 'static,
{
    *MCP_SKILL_CLEANUP_HOOK.write() = Some(Arc::new(hook));
}

pub(crate) fn clear_mcp_skills_for_server(server_name: &str) {
    let hook = MCP_SKILL_CLEANUP_HOOK.read().clone();
    if let Some(hook) = hook {
        hook(server_name);
    }
}

pub fn record_server_state(
    server_name: impl Into<String>,
    state: impl Into<String>,
    error: Option<String>,
) {
    SERVER_STATES.write().insert(
        server_name.into(),
        RuntimeMcpServerState {
            state: state.into(),
            error,
        },
    );
}

pub fn server_state(server_name: &str) -> Option<RuntimeMcpServerState> {
    SERVER_STATES.read().get(server_name).cloned()
}

#[doc(hidden)]
pub fn clear_for_tests() {
    *MANAGER.write() = None;
    SERVER_STATES.write().clear();
    *MCP_SKILL_CLEANUP_HOOK.write() = None;
}
