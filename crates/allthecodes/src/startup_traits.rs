use std::sync::Arc;

use allthecodes_engine::agent_runtime::{AgentToolRegistry, DashboardEmitter};
use allthecodes_engine::types::tool::Tool;
use allthecodes_startup as startup;
use serde_json::Value;

use crate::cli::Cli;
use startup::runtime_config::StartupCli;
use startup::tool_registry as registry;

impl StartupCli for Cli {
    fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    fn chrome(&self) -> bool {
        self.chrome
    }

    fn no_chrome(&self) -> bool {
        self.no_chrome
    }
}

impl startup::fast_paths::DumpSystemPromptCli for Cli {
    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    fn append_system_prompt(&self) -> Option<&str> {
        self.append_system_prompt.as_deref()
    }
}

pub(crate) struct RootDashboardEmitter;

impl DashboardEmitter for RootDashboardEmitter {
    fn emit_subagent_event(
        &self,
        kind: &str,
        agent_id: &str,
        parent_agent_id: Option<&str>,
        description: Option<&str>,
        model: Option<&str>,
        depth: usize,
        background: bool,
        payload: Option<Value>,
    ) -> anyhow::Result<()> {
        crate::dashboard::emit_subagent_event(
            kind,
            agent_id,
            parent_agent_id,
            description,
            model,
            depth,
            background,
            payload,
        )
    }
}

pub(crate) struct RootAgentToolRegistry;

impl AgentToolRegistry for RootAgentToolRegistry {
    fn get_all_tools(&self) -> Vec<Arc<dyn Tool>> {
        registry::get_all_tools()
    }
}
