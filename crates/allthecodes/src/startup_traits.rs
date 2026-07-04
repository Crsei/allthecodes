use std::sync::Arc;

use allthecodes_engine::agent_runtime::{AgentToolRegistry, DashboardEmitter};
use allthecodes_engine::types::tool::Tool;
use allthecodes_mcp::McpBindingContext;
use allthecodes_startup as startup;
use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
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
        if let Err(error) = crate::runtime_history::persist_subagent_event(
            crate::runtime_history::RuntimeSubagentEvent {
                kind,
                agent_id,
                parent_agent_id,
                description,
                model,
                depth,
                background,
                payload: payload.clone(),
            },
        ) {
            tracing::debug!(%error, agent_id, kind, "failed to persist agent runtime event");
        }

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

    fn emit_execution_record(&self, record: &AgentRuntimeExecutionRecord) -> anyhow::Result<()> {
        if let Err(error) = crate::runtime_history::persist_execution_record(record) {
            tracing::debug!(
                %error,
                agent_id = %record.agent_id,
                tool = %record.tool,
                "failed to persist agent runtime execution record"
            );
        }

        crate::dashboard::emit_execution_record(record)
    }
}

pub(crate) struct RootAgentToolRegistry;

impl AgentToolRegistry for RootAgentToolRegistry {
    fn get_all_tools(&self) -> Vec<Arc<dyn Tool>> {
        registry::get_all_tools()
    }

    fn get_tools_for_mcp_context(&self, ctx: &McpBindingContext) -> Vec<Arc<dyn Tool>> {
        let mut tools = registry::get_all_tools()
            .into_iter()
            .filter(|tool| tool.mcp_server_name().is_none())
            .collect::<Vec<_>>();
        let Some(manager) = allthecodes_mcp::runtime::current_manager() else {
            return tools;
        };
        let Ok(guard) = manager.try_lock() else {
            return tools;
        };
        let defs = guard.tools_for_context(ctx);
        tools.extend(
            allthecodes_engine::mcp_tool_adapter::mcp_tools_to_tools_for_context(
                defs,
                manager.clone(),
                ctx.clone(),
            ),
        );
        tools
    }
}
