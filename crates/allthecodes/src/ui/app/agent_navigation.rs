//! Pure multi-agent navigation state for the allthecodes TUI.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadEntry {
    pub thread_id: String,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub is_primary: bool,
    pub is_closed: bool,
}

impl AgentThreadEntry {
    pub fn label(&self) -> String {
        if self.is_primary {
            return "Primary".to_string();
        }
        self.agent_nickname
            .as_deref()
            .or(self.agent_role.as_deref())
            .unwrap_or("Agent")
            .to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentThreadStatus {
    Running,
    Thinking,
    Streaming,
    ToolRunning,
    WaitingPermission,
    Succeeded,
    Failed,
    Canceled,
    Closed,
}

impl AgentThreadStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "active",
            Self::Thinking => "thinking",
            Self::Streaming => "streaming",
            Self::ToolRunning => "tool",
            Self::WaitingPermission => "permission",
            Self::Succeeded => "done",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Closed => "closed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Canceled | Self::Closed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentToolActivitySummary {
    pub tool_use_id: String,
    pub tool_name: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentThreadRuntimeInfo {
    pub status: AgentThreadStatus,
    pub status_summary: Option<String>,
    pub duration_ms: Option<u64>,
    tool_uses: BTreeMap<String, AgentToolActivitySummary>,
}

impl Default for AgentThreadRuntimeInfo {
    fn default() -> Self {
        Self {
            status: AgentThreadStatus::Running,
            status_summary: None,
            duration_ms: None,
            tool_uses: BTreeMap::new(),
        }
    }
}

impl AgentThreadRuntimeInfo {
    pub fn tool_use_count(&self) -> usize {
        self.tool_uses.len()
    }

    pub fn recent_tool_uses(&self) -> impl DoubleEndedIterator<Item = &AgentToolActivitySummary> {
        self.tool_uses.values()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentNavigationDirection {
    Previous,
    Next,
}

#[derive(Debug, Default, Clone)]
pub struct AgentNavigationState {
    entries: BTreeMap<String, AgentThreadEntry>,
    order: Vec<String>,
    runtime: BTreeMap<String, AgentThreadRuntimeInfo>,
}

impl AgentNavigationState {
    pub fn upsert(&mut self, entry: AgentThreadEntry) {
        if !self.entries.contains_key(&entry.thread_id) {
            self.order.push(entry.thread_id.clone());
        }
        self.runtime.entry(entry.thread_id.clone()).or_default();
        self.entries.insert(entry.thread_id.clone(), entry);
    }

    pub fn mark_closed(&mut self, thread_id: &str) {
        if let Some(entry) = self.entries.get_mut(thread_id) {
            entry.is_closed = true;
        }
        let runtime = self.runtime.entry(thread_id.to_string()).or_default();
        if !runtime.status.is_terminal() {
            runtime.status = AgentThreadStatus::Closed;
        }
    }

    pub fn mark_status(
        &mut self,
        thread_id: &str,
        status: AgentThreadStatus,
        summary: Option<String>,
    ) {
        let runtime = self.runtime.entry(thread_id.to_string()).or_default();
        runtime.status = status;
        runtime.status_summary = summary;
        if status.is_terminal() {
            if let Some(entry) = self.entries.get_mut(thread_id) {
                entry.is_closed = true;
            }
        }
    }

    pub fn set_duration_ms(&mut self, thread_id: &str, duration_ms: Option<u64>) {
        self.runtime
            .entry(thread_id.to_string())
            .or_default()
            .duration_ms = duration_ms;
    }

    pub fn mark_tool_use(
        &mut self,
        thread_id: &str,
        tool_use_id: &str,
        tool_name: &str,
        summary: impl Into<String>,
    ) {
        let runtime = self.runtime.entry(thread_id.to_string()).or_default();
        runtime.status = AgentThreadStatus::ToolRunning;
        runtime.tool_uses.insert(
            tool_use_id.to_string(),
            AgentToolActivitySummary {
                tool_use_id: tool_use_id.to_string(),
                tool_name: tool_name.to_string(),
                summary: summary.into(),
            },
        );
    }

    pub fn runtime_info(&self, thread_id: &str) -> Option<&AgentThreadRuntimeInfo> {
        self.runtime.get(thread_id)
    }

    pub fn remove(&mut self, thread_id: &str) {
        self.entries.remove(thread_id);
        self.runtime.remove(thread_id);
        self.order.retain(|candidate| candidate != thread_id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.runtime.clear();
    }

    pub fn ordered_threads(&self) -> Vec<&AgentThreadEntry> {
        self.order
            .iter()
            .filter_map(|thread_id| self.entries.get(thread_id))
            .collect()
    }

    pub fn thread_count(&self) -> usize {
        self.entries.len()
    }

    pub fn contains_thread(&self, thread_id: &str) -> bool {
        self.entries.contains_key(thread_id)
    }

    pub fn entry(&self, thread_id: &str) -> Option<&AgentThreadEntry> {
        self.entries.get(thread_id)
    }

    pub fn active_non_primary_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| !entry.is_primary && !entry.is_closed)
            .count()
    }

    pub fn active_non_primary_thread_ids(&self) -> Vec<String> {
        self.order
            .iter()
            .filter_map(|thread_id| self.entries.get(thread_id))
            .filter(|entry| !entry.is_primary && !entry.is_closed)
            .map(|entry| entry.thread_id.clone())
            .collect()
    }

    pub fn adjacent_thread_id(
        &self,
        current_thread_id: &str,
        direction: AgentNavigationDirection,
    ) -> Option<String> {
        let ordered = self.ordered_threads();
        if ordered.len() < 2 {
            return None;
        }
        let current = ordered
            .iter()
            .position(|entry| entry.thread_id == current_thread_id)?;
        let next = match direction {
            AgentNavigationDirection::Next => (current + 1) % ordered.len(),
            AgentNavigationDirection::Previous => {
                if current == 0 {
                    ordered.len() - 1
                } else {
                    current - 1
                }
            }
        };
        Some(ordered[next].thread_id.clone())
    }

    #[cfg(test)]
    pub fn render_agent_tree(&self, current_thread_id: &str) -> String {
        let ordered = self.ordered_threads();
        if ordered.is_empty() {
            return "Agents: none".to_string();
        }

        let mut lines = vec![format!("Agents ({})", ordered.len())];
        for entry in ordered {
            let current = if entry.thread_id == current_thread_id {
                ">"
            } else {
                " "
            };
            let state = if entry.is_closed {
                "closed"
            } else {
                self.runtime_info(&entry.thread_id)
                    .map(|runtime| runtime.status.label())
                    .unwrap_or("active")
            };
            let role = entry.agent_role.as_deref().unwrap_or("default");
            lines.push(format!(
                "{current} {:<18} {:<7} role={} thread={}",
                entry.label(),
                state,
                role,
                short_thread_id(&entry.thread_id)
            ));
        }
        lines.join("\n")
    }
}

pub(super) fn short_thread_id(thread_id: &str) -> &str {
    thread_id.get(..8).unwrap_or(thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> AgentThreadEntry {
        AgentThreadEntry {
            thread_id: id.to_string(),
            agent_nickname: None,
            agent_role: None,
            is_primary: false,
            is_closed: false,
        }
    }

    #[test]
    fn navigates_in_insert_order() {
        let mut state = AgentNavigationState::default();
        state.upsert(entry("a"));
        state.upsert(entry("b"));
        assert_eq!(
            state.adjacent_thread_id("a", AgentNavigationDirection::Next),
            Some("b".to_string())
        );
    }

    #[test]
    fn renders_agent_tree_statuses() {
        let mut state = AgentNavigationState::default();
        state.upsert(AgentThreadEntry {
            thread_id: "primary-thread".to_string(),
            agent_nickname: None,
            agent_role: Some("leader".to_string()),
            is_primary: true,
            is_closed: false,
        });
        state.upsert(AgentThreadEntry {
            thread_id: "worker-thread".to_string(),
            agent_nickname: Some("builder".to_string()),
            agent_role: Some("executor".to_string()),
            is_primary: false,
            is_closed: true,
        });

        let rendered = state.render_agent_tree("worker-thread");
        assert!(rendered.contains("Agents (2)"));
        assert!(rendered.contains("Primary"));
        assert!(rendered.contains("> builder"));
        assert!(rendered.contains("closed"));
    }
}
