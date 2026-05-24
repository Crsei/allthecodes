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

#[derive(Debug, Clone)]
pub struct RuntimeMcpServerState {
    pub state: String,
    pub error: Option<String>,
}

static MANAGER: LazyLock<RwLock<Option<SharedMcpManager>>> = LazyLock::new(|| RwLock::new(None));
static SERVER_STATES: LazyLock<RwLock<HashMap<String, RuntimeMcpServerState>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn install_manager(manager: SharedMcpManager) {
    *MANAGER.write() = Some(manager);
}

pub fn current_manager() -> Option<SharedMcpManager> {
    MANAGER.read().clone()
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
}
