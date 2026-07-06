use crossterm::event::{KeyCode, KeyEvent};

use allthecodes_services::agent_runtime_history::load_dashboard_snapshot;
use allthecodes_types::agent_runtime_dashboard::{
    AgentRuntimeAgentStatus, AgentRuntimeAgentSummary, AgentRuntimeDashboardQuery,
    AgentRuntimeDashboardResponse, AgentRuntimeDashboardSummary,
};

use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentsSurface {
    session_id: Option<String>,
    summary: AgentRuntimeDashboardSummary,
    rows: Vec<SubagentRow>,
    selected_index: usize,
    updated_at_ms: Option<i64>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubagentRow {
    agent_id: String,
    status: AgentRuntimeAgentStatus,
    description: String,
    model: String,
    tool_call_count: usize,
    failed_tool_call_count: usize,
}

impl SubagentsSurface {
    pub(crate) fn new(session_id: Option<&str>) -> Self {
        let mut surface = Self {
            session_id: session_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            summary: AgentRuntimeDashboardSummary::default(),
            rows: Vec::new(),
            selected_index: 0,
            updated_at_ms: None,
            error: None,
        };
        surface.refresh();
        surface
    }

    pub(crate) fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Subagents\n");
        out.push_str("---------\n");
        out.push_str(&format!(
            "session={} | agents={} running={} completed={} failed={} tools={} failed_tools={}\n",
            self.session_id.as_deref().unwrap_or("(all)"),
            self.summary.total_agents,
            self.summary.running_agents,
            self.summary.completed_agents,
            self.summary.failed_agents,
            self.summary.tool_calls,
            self.summary.failed_tool_calls
        ));
        if let Some(updated_at_ms) = self.updated_at_ms {
            out.push_str(&format!("updated_at_ms={updated_at_ms}\n"));
        }
        out.push('\n');

        if let Some(error) = &self.error {
            out.push_str(&format!("Failed to load dashboard: {error}\n\n"));
        }

        if self.rows.is_empty() {
            out.push_str("No subagent runtime activity recorded for this session.\n");
        } else {
            for (index, row) in self.rows.iter().enumerate() {
                let marker = if index == self.selected_index {
                    ">"
                } else {
                    " "
                };
                out.push_str(&format!(
                    "{marker} {} [{}] tools={} failed={} model={} {}\n",
                    row.agent_id,
                    status_label(row.status),
                    row.tool_call_count,
                    row.failed_tool_call_count,
                    row.model,
                    row.description
                ));
            }
        }

        out.push_str("\nUp/Down select | r refresh | Enter print summary | Esc close");
        out
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up => {
                self.selected_index = cycle_index(self.selected_index, self.rows.len(), -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected_index = cycle_index(self.selected_index, self.rows.len(), 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('r') => {
                self.refresh();
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => CommandSurfaceOutcome::Submit("/subagents summary".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn refresh(&mut self) {
        match load_dashboard_snapshot(AgentRuntimeDashboardQuery {
            session_id: self.session_id.clone(),
            limit: Some(100),
        }) {
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(error) => {
                self.summary = AgentRuntimeDashboardSummary::default();
                self.rows.clear();
                self.selected_index = 0;
                self.updated_at_ms = None;
                self.error = Some(error.to_string());
            }
        }
    }

    fn apply_snapshot(&mut self, snapshot: AgentRuntimeDashboardResponse) {
        self.summary = snapshot.summary;
        self.updated_at_ms = Some(snapshot.updated_at_ms);
        self.rows = snapshot.agents.iter().map(row_from_agent).collect();
        self.selected_index = self.selected_index.min(self.rows.len().saturating_sub(1));
        self.error = None;
    }
}

fn row_from_agent(agent: &AgentRuntimeAgentSummary) -> SubagentRow {
    SubagentRow {
        agent_id: agent.agent_id.clone(),
        status: agent.status,
        description: agent
            .description
            .clone()
            .or_else(|| agent.parent_agent_id.clone())
            .unwrap_or_else(|| "subagent".to_string()),
        model: agent.model.clone().unwrap_or_else(|| "-".to_string()),
        tool_call_count: agent.tool_call_count,
        failed_tool_call_count: agent.failed_tool_call_count,
    }
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
    use allthecodes_services::agent_runtime_history::persist_execution_record;
    use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;

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

    #[test]
    #[serial_test::serial]
    fn subagents_surface_renders_runtime_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = HomeGuard::set(temp.path());
        persist_execution_record(&AgentRuntimeExecutionRecord {
            session_id: "surface-session".to_string(),
            agent_id: "surface-agent".to_string(),
            tool: "shell".to_string(),
            model: Some("gpt-5.5".to_string()),
            ..AgentRuntimeExecutionRecord::default()
        })
        .expect("persist execution record");

        let surface = SubagentsSurface::new(Some("surface-session"));
        let rendered = surface.render();

        assert!(rendered.contains("Subagents"));
        assert!(rendered.contains("surface-agent"));
        assert!(rendered.contains("gpt-5.5"));
    }
}
