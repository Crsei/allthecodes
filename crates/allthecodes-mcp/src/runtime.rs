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

/// Disconnect every live client without holding the manager lock across I/O.
pub async fn disconnect_all(manager: &SharedMcpManager) {
    let clients = manager.lock().await.take_all_clients();
    let mut disconnected = Vec::with_capacity(clients.len());
    for (name, mut client) in clients {
        client.disconnect().await;
        clear_mcp_skills_for_server(&name);
        disconnected.push(name);
    }

    let mut manager = manager.lock().await;
    for name in disconnected {
        manager.mark_disconnected(&name);
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serial_test::serial;
    use tokio::sync::Mutex;

    use super::*;

    struct RuntimeGuard;

    impl RuntimeGuard {
        fn reset() -> Self {
            clear_for_tests();
            Self
        }
    }

    impl Drop for RuntimeGuard {
        fn drop(&mut self) {
            clear_for_tests();
        }
    }

    #[test]
    #[serial]
    fn taking_installed_manager_clears_global_owner() {
        let _guard = RuntimeGuard::reset();
        let manager = Arc::new(Mutex::new(McpManager::new()));
        install_manager(manager.clone());

        let taken = take_installed_manager().expect("installed manager must be returned");

        assert!(current_manager().is_none(), "static owner was not cleared");
        assert!(Arc::ptr_eq(&taken, &manager));
    }
}
