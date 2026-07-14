//! Runtime IPC handlers for agent and team commands.

use std::sync::{Arc, OnceLock};

use allthecodes_ipc_protocol::protocol::BackendMessage;
use allthecodes_types::agent_events::{AgentCommand, AgentEvent, TeamCommand, TeamEvent};
use allthecodes_types::agent_types::{AgentNode, TeamMemberInfo};
use allthecodes_types::output::OutputReadBatch;

const DEFAULT_AGENT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;

pub trait AgentRuntimeHost: Send + Sync + 'static {
    fn cancel_agent(&self, agent_id: &str) -> Option<String>;
    fn agent_output(&self, agent_id: &str) -> Option<AgentTaskOutput>;
    fn agent_output_batch(
        &self,
        agent_id: &str,
        after_seq: Option<u64>,
        limit_bytes: usize,
    ) -> Option<AgentTaskOutputBatch> {
        let _ = (agent_id, after_seq, limit_bytes);
        None
    }
    fn update_agent_state(
        &self,
        agent_id: &str,
        state: &str,
        result_preview: Option<String>,
        duration_ms: Option<u64>,
        had_error: bool,
    );
    fn agent_tree_snapshot(&self) -> Vec<AgentNode>;
    fn write_team_message(&self, team_name: &str, to: &str, text: &str) -> Result<(), String>;
    fn team_members(&self, team_name: &str) -> Result<Vec<TeamMemberInfo>, String>;
}

#[derive(Debug, Clone)]
pub struct AgentTaskOutput {
    pub id: String,
    pub output: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct AgentTaskOutputBatch {
    pub id: String,
    pub output: OutputReadBatch,
    pub metadata: Option<serde_json::Value>,
}

static HOST: OnceLock<Arc<dyn AgentRuntimeHost>> = OnceLock::new();

pub fn set_runtime_host(host: Arc<dyn AgentRuntimeHost>) {
    let _ = HOST.set(host);
}

pub fn handle_agent_command(cmd: AgentCommand) -> Vec<BackendMessage> {
    match cmd {
        AgentCommand::AbortAgent { agent_id } => {
            let task_id = HOST.get().and_then(|host| host.cancel_agent(&agent_id));
            if let Some(host) = HOST.get() {
                host.update_agent_state(&agent_id, "aborted", None, None, false);
            }
            let roots = HOST
                .get()
                .map(|host| host.agent_tree_snapshot())
                .unwrap_or_default();

            let mut messages = vec![
                BackendMessage::AgentEvent {
                    event: AgentEvent::Aborted {
                        agent_id: agent_id.clone(),
                    },
                },
                BackendMessage::AgentEvent {
                    event: AgentEvent::TreeSnapshot { roots },
                },
            ];
            if let Some(task_id) = task_id {
                messages.push(BackendMessage::SystemInfo {
                    text: format!(
                        "Cancellation requested for background agent task {}.",
                        task_id
                    ),
                    level: "info".into(),
                });
            }
            messages
        }
        AgentCommand::QueryActiveAgents => {
            vec![BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot {
                    roots: HOST
                        .get()
                        .map(|host| host.agent_tree_snapshot())
                        .unwrap_or_default(),
                },
            }]
        }
        AgentCommand::QueryAgentOutput {
            agent_id,
            after_seq,
            limit_bytes,
        } => match HOST.get().and_then(|host| {
            // Persisted task metadata reconstructs fork context mode and live
            // paths after a UI refresh/reconnect. TODO(upstream fork parity):
            // resume an interrupted child runtime and re-register its live
            // publisher, matching resumeAgent.ts semantics.
            host.agent_output_batch(
                &agent_id,
                after_seq,
                limit_bytes.unwrap_or(DEFAULT_AGENT_OUTPUT_LIMIT_BYTES),
            )
        }) {
            Some(task) => vec![BackendMessage::AgentEvent {
                event: AgentEvent::OutputBatch {
                    agent_id,
                    task_id: task.id,
                    output: task.output,
                    fork_metadata: task
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("fork"))
                        .cloned()
                        .and_then(|metadata| serde_json::from_value(metadata).ok()),
                },
            }],
            None => match HOST.get().and_then(|host| host.agent_output(&agent_id)) {
                Some(task) => vec![BackendMessage::SystemInfo {
                    text: if task.output.is_empty() {
                        format!(
                            "Agent {} is tracked as task {}, but has no retained output yet.",
                            agent_id, task.id
                        )
                    } else {
                        format!(
                            "Agent {} output from task {}:\n{}",
                            agent_id, task.id, task.output
                        )
                    },
                    level: "info".into(),
                }],
                None => vec![BackendMessage::SystemInfo {
                    text: format!("No retained output is available for agent {}.", agent_id),
                    level: "warning".into(),
                }],
            },
        },
    }
}

pub fn handle_team_command(cmd: TeamCommand) -> Vec<BackendMessage> {
    match cmd {
        TeamCommand::InjectMessage {
            team_name,
            to,
            text,
        } => match HOST
            .get()
            .map(|host| host.write_team_message(&team_name, &to, &text))
        {
            Some(Ok(())) => vec![BackendMessage::TeamEvent {
                event: TeamEvent::MessageRouted {
                    team_name: team_name.clone(),
                    from: "__frontend__".into(),
                    to: to.clone(),
                    text: text.clone(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    summary: None,
                },
            }],
            Some(Err(e)) => vec![BackendMessage::SystemInfo {
                text: format!("Failed to inject message: {}", e),
                level: "error".into(),
            }],
            None => vec![BackendMessage::SystemInfo {
                text: "Agent Teams feature is not available.".into(),
                level: "warning".into(),
            }],
        },
        TeamCommand::QueryTeamStatus { team_name } => build_team_status_events(&team_name),
    }
}

pub fn build_team_status_events(team_name: &str) -> Vec<BackendMessage> {
    match HOST.get().map(|host| host.team_members(team_name)) {
        Some(Ok(members)) => {
            let pending_messages = members.iter().map(|m| m.unread_messages).sum();
            vec![BackendMessage::TeamEvent {
                event: TeamEvent::StatusSnapshot {
                    team_name: team_name.to_string(),
                    members,
                    pending_messages,
                },
            }]
        }
        Some(Err(e)) => vec![BackendMessage::SystemInfo {
            text: format!("Team '{}' not found: {}", team_name, e),
            level: "warning".into(),
        }],
        None => vec![BackendMessage::SystemInfo {
            text: "Agent Teams feature is not available.".into(),
            level: "warning".into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_agent_query_active_returns_tree_snapshot() {
        let msgs = handle_agent_command(AgentCommand::QueryActiveAgents);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(
            &msgs[0],
            BackendMessage::AgentEvent {
                event: AgentEvent::TreeSnapshot { .. }
            }
        ));
    }

    #[test]
    fn handle_agent_abort_returns_two_messages_without_host() {
        let msgs = handle_agent_command(AgentCommand::AbortAgent {
            agent_id: "nonexistent".into(),
        });
        assert_eq!(msgs.len(), 2);
        assert!(matches!(
            &msgs[0],
            BackendMessage::AgentEvent {
                event: AgentEvent::Aborted { .. }
            }
        ));
    }
}
