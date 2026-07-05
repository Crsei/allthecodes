//! `/subagents` slash command: agent runtime dashboard snapshot.

use anyhow::Result;
use async_trait::async_trait;

use allthecodes_services::agent_runtime_history::{load_dashboard_snapshot, AgentRuntimeAgentStatus};
use allthecodes_types::agent_runtime_dashboard::{
    AgentRuntimeAgentSummary, AgentRuntimeDashboardQuery, AgentRuntimeDashboardResponse,
    AgentRuntimeExecutionRecordItem,
};

use crate::{CommandContext, CommandHandler, CommandResult};

const DEFAULT_LIMIT: usize = 50;

pub struct SubagentsHandler;

#[async_trait]
impl CommandHandler for SubagentsHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        let limit = parse_limit(args).unwrap_or(DEFAULT_LIMIT);
        let snapshot = load_dashboard_snapshot(AgentRuntimeDashboardQuery {
            session_id: Some(ctx.session_id.as_str().to_string()),
            limit: Some(limit),
        })?;
        Ok(CommandResult::Output(render_dashboard(&snapshot)))
    }
}

fn parse_limit(args: &str) -> Option<usize> {
    let mut parts = args.split_whitespace();
    match parts.next() {
        None => None,
        Some("list") | Some("summary") => parts.next().and_then(|value| value.parse().ok()),
        Some("limit") => parts.next().and_then(|value| value.parse().ok()),
        Some(value) => value.parse().ok(),
    }
}

fn render_dashboard(snapshot: &AgentRuntimeDashboardResponse) -> String {
    let mut out = String::new();
    out.push_str("Subagent runtime dashboard\n");
    out.push_str("--------------------------\n");
    out.push_str(&format!(
        "Session: {}\n",
        snapshot.session_id.as_deref().unwrap_or("(all sessions)")
    ));
    out.push_str(&format!(
        "Agents: {} total, {} running, {} completed, {} failed, {} cancelled\n",
        snapshot.summary.total_agents,
        snapshot.summary.running_agents,
        snapshot.summary.completed_agents,
        snapshot.summary.failed_agents,
        snapshot.summary.cancelled_agents
    ));
    out.push_str(&format!(
        "Tool calls: {} total, {} failed\n\n",
        snapshot.summary.tool_calls, snapshot.summary.failed_tool_calls
    ));

    if snapshot.agents.is_empty() {
        out.push_str("No subagent runtime activity has been recorded for this session.\n");
        return out;
    }

    out.push_str("Agents\n");
    for agent in snapshot.agents.iter().take(20) {
        out.push_str(&format_agent_line(agent));
        out.push('\n');
    }

    if !snapshot.execution_records.is_empty() {
        out.push_str("\nRecent tool executions\n");
        for item in snapshot.execution_records.iter().take(10) {
            out.push_str(&format_execution_line(item));
            out.push('\n');
        }
    }

    out
}

fn format_agent_line(agent: &AgentRuntimeAgentSummary) -> String {
    let role = agent
        .description
        .as_deref()
        .or(agent.parent_agent_id.as_deref())
        .unwrap_or("subagent");
    format!(
        "- {} [{}] {} tools={} failed={}",
        agent.agent_id,
        status_label(agent.status),
        role,
        agent.tool_call_count,
        agent.failed_tool_call_count
    )
}

fn format_execution_line(item: &AgentRuntimeExecutionRecordItem) -> String {
    let record = &item.record;
    let status = if record.had_error { "failed" } else { "ok" };
    let command = record
        .command
        .as_deref()
        .unwrap_or(record.tool.as_str());
    format!(
        "- {} {} [{}] {}",
        record.agent_id, record.tool, status, command
    )
}

fn status_label(status: AgentRuntimeAgentStatus) -> &'static str {
    match status {
        AgentRuntimeAgentStatus::Running => "running",
        AgentRuntimeAgentStatus::Completed => "completed",
        AgentRuntimeAgentStatus::Failed => "failed",
        AgentRuntimeAgentStatus::Cancelled => "cancelled",
        AgentRuntimeAgentStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use allthecodes_services::agent_runtime_history::persist_execution_record;
    use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
    use std::path::PathBuf;

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn make_ctx(session_id: &str) -> CommandContext {
        CommandContext {
            messages: vec![],
            cwd: PathBuf::from("."),
            app_state: AppState::default(),
            session_id: SessionId::from_string(session_id),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn subagents_command_renders_current_session_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = HomeGuard::set(temp.path());
        persist_execution_record(&AgentRuntimeExecutionRecord {
            session_id: "session-command".to_string(),
            agent_id: "agent-command".to_string(),
            tool: "shell".to_string(),
            command: Some("cargo test".to_string()),
            ..AgentRuntimeExecutionRecord::default()
        })
        .expect("persist execution record");

        let handler = SubagentsHandler;
        let mut ctx = make_ctx("session-command");
        let result = handler.execute("", &mut ctx).await.expect("execute");

        match result {
            CommandResult::Output(output) => {
                assert!(output.contains("Subagent runtime dashboard"));
                assert!(output.contains("agent-command"));
                assert!(output.contains("cargo test"));
            }
            _ => panic!("expected output"),
        }
    }
}
