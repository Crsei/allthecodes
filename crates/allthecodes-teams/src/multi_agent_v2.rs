//! Multi-agent v2 tool surfaces built on the existing Agent Teams registry.

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration, Instant};

use allthecodes_tools::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

use crate::constants::TEAM_LEAD_NAME;
use crate::helpers;
use crate::mailbox;
use crate::types::TeammateMessage;

pub fn tools() -> Tools {
    vec![
        std::sync::Arc::new(ListAgentsTool),
        std::sync::Arc::new(FollowupTaskTool),
        std::sync::Arc::new(WaitAgentTool),
        std::sync::Arc::new(CloseAgentTool),
    ]
}

fn active_team_name(ctx: &ToolUseContext) -> Option<String> {
    (ctx.get_app_state)()
        .team_context
        .as_ref()
        .map(|team| team.team_name.trim().to_string())
        .filter(|team| !team.is_empty())
}

fn string_param<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn member_matches(member_name: &str, agent_id: &str, query: &str) -> bool {
    member_name == query || agent_id == query
}

fn find_member<'a>(
    team_file: &'a crate::types::TeamFile,
    id_or_name: &str,
) -> Option<&'a crate::types::TeamMember> {
    team_file
        .members
        .iter()
        .find(|member| member_matches(&member.name, &member.agent_id, id_or_name))
}

fn format_agents(team_file: &crate::types::TeamFile) -> Vec<Value> {
    team_file
        .members
        .iter()
        .map(|member| {
            json!({
                "agent_id": member.agent_id,
                "name": member.name,
                "status": if member.is_active == Some(false) { "stopped" } else { "active" },
                "last_activity": member.joined_at,
                "task_summary": member.prompt,
                "agent_type": member.agent_type,
                "session_id": member.session_id,
                "backend": member.backend_type,
            })
        })
        .collect()
}

pub struct ListAgentsTool;

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "ListAgents"
    }

    async fn description(&self, _input: &Value) -> String {
        "List agents in the current Agent Teams session.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team": {"type": "string", "description": "Optional team name; defaults to current team."}
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let team = string_param(&input, "team")
            .map(ToOwned::to_owned)
            .or_else(|| active_team_name(ctx))
            .ok_or_else(|| anyhow!("No active team. Create a team first."))?;
        let team_file = helpers::read_team_file(&team)?;
        Ok(ToolResult {
            data: json!({
                "team": team,
                "agents": format_agents(&team_file),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "List current session/team agents, their ids, names, statuses, and task summaries.".into()
    }
}

pub struct FollowupTaskTool;

#[async_trait]
impl Tool for FollowupTaskTool {
    fn name(&self) -> &str {
        "FollowupTask"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a follow-up task to an existing teammate via its mailbox.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "message": {"type": "string"},
                "summary": {"type": "string"},
                "team": {"type": "string"}
            },
            "required": ["agent_id", "message"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "agent_id").is_none() || string_param(input, "message").is_none() {
            return ValidationResult::Error {
                message: "agent_id and message are required".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let team = string_param(&input, "team")
            .map(ToOwned::to_owned)
            .or_else(|| active_team_name(ctx))
            .ok_or_else(|| anyhow!("No active team. Create a team first."))?;
        let mut team_file = helpers::read_team_file(&team)?;
        let agent_id = string_param(&input, "agent_id").unwrap();
        let member = find_member(&team_file, agent_id)
            .ok_or_else(|| {
                anyhow!("No agent named or identified by '{agent_id}' exists in team '{team}'")
            })?
            .clone();
        if member.name == TEAM_LEAD_NAME {
            bail!("FollowupTask targets teammates, not the team lead");
        }
        if member.is_active == Some(false) {
            helpers::set_member_active(&team, &member.agent_id, true)?;
            team_file = helpers::read_team_file(&team)?;
        }
        let sender = team_file
            .members
            .iter()
            .find(|member| member.name == TEAM_LEAD_NAME)
            .map(|member| member.name.clone())
            .unwrap_or_else(|| TEAM_LEAD_NAME.to_string());
        mailbox::write_to_mailbox(
            &member.name,
            TeammateMessage {
                from: sender,
                text: string_param(&input, "message").unwrap().to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: None,
                summary: string_param(&input, "summary").map(ToOwned::to_owned),
            },
            &team,
        )?;
        Ok(ToolResult {
            data: json!({
                "sent": true,
                "team": team,
                "agent_id": member.agent_id,
                "name": member.name,
                "resumed": member.is_active == Some(false),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Send a follow-up task to an existing teammate. Stopped teammates are resumed by marking them active and writing to their mailbox.".into()
    }
}

pub struct WaitAgentTool;

#[async_trait]
impl Tool for WaitAgentTool {
    fn name(&self) -> &str {
        "WaitAgent"
    }

    async fn description(&self, _input: &Value) -> String {
        "Wait for a teammate to stop, or return its current status after a timeout.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "workflow_id": {"type": "string"},
                "timeout_ms": {"type": "integer", "minimum": 0, "maximum": 600000},
                "team": {"type": "string"}
            }
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "agent_id").is_none() && string_param(input, "workflow_id").is_none()
        {
            return ValidationResult::Error {
                message: "agent_id or workflow_id is required".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        if let Some(workflow_id) = string_param(&input, "workflow_id") {
            let path = allthecodes_config::paths::workflow_file_path(workflow_id);
            let raw = std::fs::read_to_string(&path)?;
            let workflow: Value = serde_json::from_str(&raw)?;
            return Ok(ToolResult {
                data: json!({"workflow_id": workflow_id, "workflow": workflow}),
                ..Default::default()
            });
        }

        let team = string_param(&input, "team")
            .map(ToOwned::to_owned)
            .or_else(|| active_team_name(ctx))
            .ok_or_else(|| anyhow!("No active team. Create a team first."))?;
        let agent_id = string_param(&input, "agent_id").unwrap();
        let timeout = Duration::from_millis(
            input
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(30_000)
                .min(600_000),
        );
        let deadline = Instant::now() + timeout;
        loop {
            let team_file = helpers::read_team_file(&team)?;
            let member = find_member(&team_file, agent_id).ok_or_else(|| {
                anyhow!("No agent named or identified by '{agent_id}' exists in team '{team}'")
            })?;
            let stopped = member.is_active == Some(false);
            if stopped || Instant::now() >= deadline {
                return Ok(ToolResult {
                    data: json!({
                        "team": team,
                        "agent_id": member.agent_id,
                        "name": member.name,
                        "status": if stopped { "stopped" } else { "active" },
                        "completed": stopped,
                        "timed_out": !stopped,
                        "summary": member.prompt,
                    }),
                    ..Default::default()
                });
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    async fn prompt(&self) -> String {
        "Wait for an existing agent, or inspect a persisted workflow by workflow_id.".into()
    }
}

pub struct CloseAgentTool;

#[async_trait]
impl Tool for CloseAgentTool {
    fn name(&self) -> &str {
        "CloseAgent"
    }

    async fn description(&self, _input: &Value) -> String {
        "Request an existing teammate to stop and mark it inactive.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "reason": {"type": "string"},
                "team": {"type": "string"}
            },
            "required": ["agent_id"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "agent_id").is_none() {
            return ValidationResult::Error {
                message: "agent_id is required".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow CloseAgent to stop {}?",
                string_param(input, "agent_id").unwrap_or("<missing agent_id>")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let team = string_param(&input, "team")
            .map(ToOwned::to_owned)
            .or_else(|| active_team_name(ctx))
            .ok_or_else(|| anyhow!("No active team. Create a team first."))?;
        let agent_id = string_param(&input, "agent_id").unwrap();
        let team_file = helpers::read_team_file(&team)?;
        let member = find_member(&team_file, agent_id).ok_or_else(|| {
            anyhow!("No agent named or identified by '{agent_id}' exists in team '{team}'")
        })?;
        if member.name == TEAM_LEAD_NAME {
            bail!("CloseAgent cannot close the team lead");
        }
        mailbox::write_to_mailbox(
            &member.name,
            TeammateMessage {
                from: TEAM_LEAD_NAME.to_string(),
                text: serde_json::to_string(&json!({
                    "type": "shutdown_request",
                    "reason": string_param(&input, "reason").unwrap_or("CloseAgent requested shutdown"),
                }))?,
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: None,
                summary: Some("Shutdown request".into()),
            },
            &team,
        )?;
        helpers::set_member_active(&team, &member.agent_id, false)?;
        Ok(ToolResult {
            data: json!({
                "closed": true,
                "team": team,
                "agent_id": member.agent_id,
                "name": member.name,
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Request a teammate shutdown and mark the known team member inactive; this requires permission and never force-kills unknown processes.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_agent_v2_tool_names_are_camel_case() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["ListAgents", "FollowupTask", "WaitAgent", "CloseAgent"]
        );
    }
}
