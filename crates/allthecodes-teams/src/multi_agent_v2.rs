//! Multi-agent v2 tool surfaces built on the existing Agent Teams registry.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration, Instant};

use allthecodes_tools::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

use crate::backend::TeammateExecutor;
use crate::constants::TEAM_LEAD_NAME;
use crate::helpers;
use crate::in_process::{InProcessBackend, TeammateTaskSnapshot};
use crate::mailbox;
use crate::types::{TaskStatus, TeamFile, TeamMember, TeammateMessage};

pub fn tools() -> Tools {
    vec![
        std::sync::Arc::new(ListAgentsTool),
        std::sync::Arc::new(ListAgentsAliasTool),
        std::sync::Arc::new(FollowupTaskTool),
        std::sync::Arc::new(FollowupTaskAliasTool),
        std::sync::Arc::new(WaitAgentTool),
        std::sync::Arc::new(WaitAgentAliasTool),
        std::sync::Arc::new(CloseAgentTool),
        std::sync::Arc::new(CloseAgentAliasTool),
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

fn target_param(input: &Value) -> Option<(&'static str, &str)> {
    ["agent_id", "task_path", "task_id", "nickname", "name"]
        .into_iter()
        .find_map(|key| string_param(input, key).map(|value| (key, value)))
}

fn member_matches(member: &TeamMember, query: &str) -> bool {
    member.name == query
        || member.agent_id == query
        || member.task_id.as_deref() == Some(query)
        || member.task_path.as_deref() == Some(query)
}

fn find_member<'a>(team_file: &'a TeamFile, target: &str) -> Option<&'a TeamMember> {
    team_file
        .members
        .iter()
        .find(|member| member_matches(member, target))
}

fn runtime_snapshots_for_team(team_name: &str) -> Vec<TeammateTaskSnapshot> {
    InProcessBackend::task_snapshots()
        .into_iter()
        .filter(|snapshot| snapshot.team_name == team_name)
        .collect()
}

fn snapshot_map_by_agent(team_name: &str) -> HashMap<String, Vec<TeammateTaskSnapshot>> {
    let mut by_agent: HashMap<String, Vec<TeammateTaskSnapshot>> = HashMap::new();
    for snapshot in runtime_snapshots_for_team(team_name) {
        by_agent
            .entry(snapshot.agent_id.clone())
            .or_default()
            .push(snapshot);
    }
    by_agent
}

fn snapshot_for_member<'a>(
    snapshots: &'a [TeammateTaskSnapshot],
    member: &TeamMember,
) -> Option<&'a TeammateTaskSnapshot> {
    if let Some(task_id) = member.task_id.as_deref() {
        if let Some(snapshot) = snapshots.iter().find(|snapshot| snapshot.id == task_id) {
            return Some(snapshot);
        }
    }
    snapshots
        .iter()
        .find(|snapshot| snapshot.agent_id == member.agent_id)
}

fn runtime_status(snapshot: &TeammateTaskSnapshot) -> &'static str {
    if snapshot.has_error {
        return "error";
    }
    match snapshot.status {
        TaskStatus::Running if snapshot.is_idle => "idle",
        TaskStatus::Running => "working",
        TaskStatus::Stopped => "stopped",
        TaskStatus::Completed => "completed",
    }
}

fn visible_member_status(member: &TeamMember, snapshot: Option<&TeammateTaskSnapshot>) -> String {
    if member.close_state.as_deref() == Some("pending") {
        return "close_pending".to_string();
    }
    if let Some(snapshot) = snapshot {
        return runtime_status(snapshot).to_string();
    }
    if member.is_active == Some(false) {
        "stopped".to_string()
    } else {
        "active".to_string()
    }
}

fn member_reached_final_state(
    member: &TeamMember,
    snapshot: Option<&TeammateTaskSnapshot>,
) -> bool {
    if let Some(snapshot) = snapshot {
        return snapshot.status != TaskStatus::Running;
    }
    member.is_active == Some(false)
}

fn wait_result_payload(
    team: &str,
    member: &TeamMember,
    snapshot: Option<&TeammateTaskSnapshot>,
    timed_out: bool,
) -> Value {
    let status = visible_member_status(member, snapshot);
    let has_error = snapshot.map(|snapshot| snapshot.has_error).unwrap_or(false);
    let final_state = !timed_out && member_reached_final_state(member, snapshot);
    json!({
        "team": team,
        "agent_id": member.agent_id,
        "name": member.name,
        "status": status,
        "completed": final_state && !has_error,
        "final": final_state,
        "timed_out": timed_out,
        "summary": member.prompt,
        "task_id": member.task_id,
        "task_path": member.task_path,
        "runtime_task_id": snapshot.map(|snapshot| snapshot.id.as_str()),
        "runtime_status": snapshot.map(runtime_status),
        "is_idle": snapshot.map(|snapshot| snapshot.is_idle),
        "awaiting_plan_approval": snapshot.map(|snapshot| snapshot.awaiting_plan_approval),
        "has_error": has_error,
        "runtime_error": snapshot.and_then(|snapshot| snapshot.error_message.as_deref()),
        "close_state": member.close_state,
        "close_requested_at": member.close_requested_at,
    })
}

fn preview_tool_result(data: Value, preview: impl Into<String>) -> ToolResult {
    let preview = preview.into();
    ToolResult {
        data,
        display_preview: Some(preview),
        ..Default::default()
    }
}

fn wait_result_preview(data: &Value) -> String {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let status = data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    if data
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        format!("WaitAgent timed out; agent {name} is {status}")
    } else {
        format!("WaitAgent finished; agent {name} is {status}")
    }
}

fn format_agents(team_file: &TeamFile) -> Vec<Value> {
    let snapshots_by_agent = snapshot_map_by_agent(&team_file.name);
    team_file
        .members
        .iter()
        .map(|member| {
            let snapshots = snapshots_by_agent
                .get(&member.agent_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let snapshot = snapshot_for_member(snapshots, member);
            json!({
                "agent_id": member.agent_id,
                "name": member.name,
                "status": visible_member_status(member, snapshot),
                "last_activity": member.joined_at,
                "task_summary": member.prompt,
                "task_id": member.task_id,
                "task_path": member.task_path,
                "runtime_task_id": snapshot.map(|snapshot| snapshot.id.as_str()),
                "runtime_status": snapshot.map(runtime_status),
                "is_idle": snapshot.map(|snapshot| snapshot.is_idle),
                "awaiting_plan_approval": snapshot.map(|snapshot| snapshot.awaiting_plan_approval),
                "runtime_error": snapshot.and_then(|snapshot| snapshot.error_message.as_deref()),
                "close_state": member.close_state,
                "close_requested_at": member.close_requested_at,
                "agent_type": member.agent_type,
                "session_id": member.session_id,
                "backend": member.backend_type,
            })
        })
        .collect()
}

pub struct ListAgentsTool;
pub struct ListAgentsAliasTool;

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
        let agents = format_agents(&team_file);
        let agent_count = agents.len();
        Ok(preview_tool_result(
            json!({
                "team": team,
                "agents": agents,
            }),
            format!("Listed {agent_count} agent(s) in team {}", team_file.name),
        ))
    }

    async fn prompt(&self) -> String {
        "List current session/team agents, their ids, names, statuses, and task summaries.".into()
    }
}

pub struct FollowupTaskTool;
pub struct FollowupTaskAliasTool;

#[async_trait]
impl Tool for FollowupTaskTool {
    fn name(&self) -> &str {
        "FollowupTask"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send a follow-up task to an existing running teammate via its mailbox.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent id or teammate name."},
                "task_id": {"type": "string", "description": "Persisted runtime task id."},
                "task_path": {"type": "string", "description": "Persisted runtime task metadata path."},
                "nickname": {"type": "string", "description": "Teammate display name."},
                "name": {"type": "string", "description": "Teammate display name."},
                "message": {"type": "string"},
                "summary": {"type": "string"},
                "team": {"type": "string"}
            },
            "required": ["message"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if target_param(input).is_none() || string_param(input, "message").is_none() {
            return ValidationResult::Error {
                message: "agent_id, task_id, task_path, name, or nickname and message are required"
                    .into(),
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
        let (target_key, target) = target_param(&input).ok_or_else(|| {
            anyhow!("Missing target parameter (agent_id, task_path, task_id, nickname, or name)")
        })?;
        let member = find_member(&team_file, target)
            .ok_or_else(|| {
                anyhow!("No agent matching {target_key}='{target}' exists in team '{team}'")
            })?
            .clone();
        if member.name == TEAM_LEAD_NAME {
            bail!("FollowupTask targets teammates, not the team lead");
        }
        if member.close_state.as_deref() == Some("pending") {
            bail!(
                "Agent '{}' has a pending close request and cannot receive follow-up tasks",
                member.name
            );
        }
        let snapshots = runtime_snapshots_for_team(&team);
        if let Some(snapshot) = snapshot_for_member(&snapshots, &member) {
            if snapshot.status != TaskStatus::Running {
                bail!(
                    "Agent '{}' is {} and cannot receive follow-up tasks; spawn a new teammate or clear the stopped task",
                    member.name,
                    runtime_status(snapshot)
                );
            }
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
                text: string_param(&input, "message")
                    .ok_or_else(|| anyhow!("Missing 'message' parameter for followup task"))?
                    .to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                read: false,
                color: None,
                summary: string_param(&input, "summary").map(ToOwned::to_owned),
            },
            &team,
        )?;
        let preview = format!(
            "Queued follow-up task for agent {} in team {team}",
            member.name
        );
        Ok(preview_tool_result(
            json!({
                "sent": true,
                "team": team,
                "agent_id": member.agent_id,
                "name": member.name,
                "target_kind": target_key,
                "task_id": member.task_id,
                "task_path": member.task_path,
                "resumed": member.is_active == Some(false),
            }),
            preview,
        ))
    }

    async fn prompt(&self) -> String {
        "Send a follow-up task to an existing teammate. Running teammates receive mailbox work; stopped runtime tasks return a clear error instead of silently queuing work.".into()
    }
}

pub struct WaitAgentTool;
pub struct WaitAgentAliasTool;

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
                "agent_id": {"type": "string", "description": "Agent id or teammate name."},
                "task_id": {"type": "string", "description": "Persisted runtime task id."},
                "task_path": {"type": "string", "description": "Persisted runtime task metadata path."},
                "nickname": {"type": "string", "description": "Teammate display name."},
                "name": {"type": "string", "description": "Teammate display name."},
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
        if target_param(input).is_none() && string_param(input, "workflow_id").is_none() {
            return ValidationResult::Error {
                message: "agent_id, task_id, task_path, name, nickname, or workflow_id is required"
                    .into(),
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
            return Ok(preview_tool_result(
                json!({"workflow_id": workflow_id, "workflow": workflow}),
                format!("WaitAgent inspected workflow {workflow_id}"),
            ));
        }

        let team = string_param(&input, "team")
            .map(ToOwned::to_owned)
            .or_else(|| active_team_name(ctx))
            .ok_or_else(|| anyhow!("No active team. Create a team first."))?;
        let (target_key, target) = target_param(&input).ok_or_else(|| {
            anyhow!("Missing target parameter (agent_id, task_path, task_id, nickname, or name)")
        })?;
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
            let member = find_member(&team_file, target).ok_or_else(|| {
                anyhow!("No agent matching {target_key}='{target}' exists in team '{team}'")
            })?;
            let snapshots = runtime_snapshots_for_team(&team);
            let snapshot = snapshot_for_member(&snapshots, member);
            let final_state = member_reached_final_state(member, snapshot);
            if final_state || Instant::now() >= deadline {
                let data = wait_result_payload(&team, member, snapshot, !final_state);
                let preview = wait_result_preview(&data);
                return Ok(preview_tool_result(data, preview));
            }
            sleep(Duration::from_millis(250)).await;
        }
    }

    async fn prompt(&self) -> String {
        "Wait for an existing agent, or inspect a persisted workflow by workflow_id.".into()
    }
}

pub struct CloseAgentTool;
pub struct CloseAgentAliasTool;

#[async_trait]
impl Tool for CloseAgentTool {
    fn name(&self) -> &str {
        "CloseAgent"
    }

    async fn description(&self, _input: &Value) -> String {
        "Request an existing teammate to stop and mark it pending close until the runtime acknowledges shutdown.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent id or teammate name."},
                "task_id": {"type": "string", "description": "Persisted runtime task id."},
                "task_path": {"type": "string", "description": "Persisted runtime task metadata path."},
                "nickname": {"type": "string", "description": "Teammate display name."},
                "name": {"type": "string", "description": "Teammate display name."},
                "reason": {"type": "string"},
                "team": {"type": "string"}
            }
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if target_param(input).is_none() {
            return ValidationResult::Error {
                message: "agent_id, task_id, task_path, name, or nickname is required".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow CloseAgent to stop {}?",
                target_param(input)
                    .map(|(_, target)| target)
                    .unwrap_or("<missing target>")
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
        let (target_key, target) = target_param(&input).ok_or_else(|| {
            anyhow!("Missing target parameter (agent_id, task_path, task_id, nickname, or name)")
        })?;
        let team_file = helpers::read_team_file(&team)?;
        let member = find_member(&team_file, target).ok_or_else(|| {
            anyhow!("No agent matching {target_key}='{target}' exists in team '{team}'")
        })?;
        if member.name == TEAM_LEAD_NAME {
            bail!("CloseAgent cannot close the team lead");
        }
        let reason = string_param(&input, "reason").unwrap_or("CloseAgent requested shutdown");
        let already_stopped = member.is_active == Some(false);
        let runtime_requested = if already_stopped {
            false
        } else if member.backend_type == Some(crate::types::BackendType::InProcess) {
            InProcessBackend::new()
                .terminate(&member.agent_id, &team, Some(reason))
                .await
        } else {
            false
        };
        if !runtime_requested && !already_stopped {
            mailbox::write_to_mailbox(
                &member.name,
                TeammateMessage {
                    from: TEAM_LEAD_NAME.to_string(),
                    text: serde_json::to_string(&json!({
                        "type": "shutdown_request",
                        "reason": reason,
                    }))?,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    read: false,
                    color: None,
                    summary: Some("Shutdown request".into()),
                },
                &team,
            )?;
        }
        if already_stopped {
            helpers::set_member_active(&team, &member.agent_id, false)?;
        } else {
            helpers::mark_member_close_pending(&team, &member.agent_id)?;
        }
        let preview = if already_stopped {
            format!("CloseAgent confirmed agent {} is stopped", member.name)
        } else {
            format!(
                "CloseAgent requested shutdown for agent {} in team {team}",
                member.name
            )
        };
        Ok(preview_tool_result(
            json!({
                "closed": already_stopped,
                "close_requested": !already_stopped,
                "pending_close": !already_stopped,
                "runtime_shutdown_requested": runtime_requested,
                "team": team,
                "agent_id": member.agent_id,
                "name": member.name,
                "target_kind": target_key,
                "task_id": member.task_id,
                "task_path": member.task_path,
                "close_state": if already_stopped { "stopped" } else { "pending" },
            }),
            preview,
        ))
    }

    async fn prompt(&self) -> String {
        "Request a teammate shutdown and mark the known team member pending close; this requires permission and never force-kills unknown processes.".into()
    }
}

macro_rules! multi_agent_alias_tool {
    ($alias:ident, $name:literal, $target:ident) => {
        #[async_trait]
        impl Tool for $alias {
            fn name(&self) -> &str {
                $name
            }

            async fn description(&self, input: &Value) -> String {
                $target.description(input).await
            }

            fn input_json_schema(&self) -> Value {
                $target.input_json_schema()
            }

            fn is_concurrency_safe(&self, input: &Value) -> bool {
                $target.is_concurrency_safe(input)
            }

            fn is_read_only(&self, input: &Value) -> bool {
                $target.is_read_only(input)
            }

            fn is_destructive(&self, input: &Value) -> bool {
                $target.is_destructive(input)
            }

            async fn validate_input(
                &self,
                input: &Value,
                ctx: &ToolUseContext,
            ) -> ValidationResult {
                $target.validate_input(input, ctx).await
            }

            async fn check_permissions(
                &self,
                input: &Value,
                ctx: &ToolUseContext,
            ) -> PermissionResult {
                $target.check_permissions(input, ctx).await
            }

            async fn call(
                &self,
                input: Value,
                ctx: &ToolUseContext,
                parent: &AssistantMessage,
                on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
            ) -> Result<ToolResult> {
                $target.call(input, ctx, parent, on_progress).await
            }

            async fn prompt(&self) -> String {
                $target.prompt().await
            }

            fn user_facing_name(&self, input: Option<&Value>) -> String {
                $target.user_facing_name(input)
            }
        }
    };
}

multi_agent_alias_tool!(ListAgentsAliasTool, "list_agents", ListAgentsTool);
multi_agent_alias_tool!(FollowupTaskAliasTool, "followup_task", FollowupTaskTool);
multi_agent_alias_tool!(WaitAgentAliasTool, "wait_agent", WaitAgentTool);
multi_agent_alias_tool!(CloseAgentAliasTool, "close_agent", CloseAgentTool);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use allthecodes_tools::tool::{FileStateCache, PermissionMode, ToolAppState, ToolUseOptions};
    use serial_test::serial;
    use tempfile::TempDir;

    use crate::identity;
    use crate::types::{BackendType, InProcessTeammateTaskState, TeammateIdentity};

    #[test]
    fn multi_agent_v2_tool_names_include_compatibility_aliases() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "ListAgents",
                "list_agents",
                "FollowupTask",
                "followup_task",
                "WaitAgent",
                "wait_agent",
                "CloseAgent",
                "close_agent"
            ]
        );
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn create_test_context() -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolUseContext {
            cwd: ".".to_string(),
            options: ToolUseOptions {
                debug: false,
                main_loop_model: "test".into(),
                verbose: false,
                is_non_interactive_session: false,
                custom_system_prompt: None,
                append_system_prompt: None,
                max_budget_usd: None,
            },
            abort_signal: rx,
            read_file_state: FileStateCache::default(),
            get_app_state: Arc::new(ToolAppState::default),
            set_app_state: Arc::new(|_| {}),
            session_id: "test-session".to_string(),
            langfuse_session_id: "test-session".to_string(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            permission_event_callback: None,
            ask_user_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
            taint_context: Default::default(),
        }
    }

    fn dummy_parent() -> AssistantMessage {
        AssistantMessage {
            uuid: uuid::Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".to_string(),
            content: vec![],
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    fn create_member(team_name: &str, name: &str, task_id: &str, active: bool) -> TeamMember {
        let task_path = helpers::team_task_path(team_name, task_id)
            .to_string_lossy()
            .into_owned();
        TeamMember {
            agent_id: identity::format_agent_id(name, team_name),
            name: name.to_string(),
            agent_type: Some("teammate".into()),
            model: None,
            prompt: Some("test worker".into()),
            color: Some("blue".into()),
            plan_mode_required: None,
            joined_at: chrono::Utc::now().timestamp(),
            tmux_pane_id: String::new(),
            cwd: ".".into(),
            worktree_path: None,
            session_id: None,
            task_id: Some(task_id.to_string()),
            task_path: Some(task_path),
            subscriptions: vec![],
            backend_type: Some(BackendType::InProcess),
            is_active: Some(active),
            mode: None,
            close_state: None,
            close_requested_at: None,
        }
    }

    fn create_team_with_member(
        tmp: &TempDir,
        base_name: &str,
        member_name: &str,
        task_id: &str,
        active: bool,
    ) -> (EnvGuard, String, String, String) {
        let home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let team = helpers::create_team(base_name, None, Some("session".into()), ".")
            .expect("create test team");
        let member = create_member(&team.name, member_name, task_id, active);
        let agent_id = member.agent_id.clone();
        let task_path = member.task_path.clone().unwrap();
        helpers::add_member(&team.name, member).expect("add test member");
        (home, team.name, agent_id, task_path)
    }

    fn register_task(
        team_name: &str,
        agent_name: &str,
        task_id: &str,
        status: TaskStatus,
        idle: bool,
    ) {
        let agent_id = identity::format_agent_id(agent_name, team_name);
        InProcessBackend::register_task(InProcessTeammateTaskState {
            id: task_id.to_string(),
            status,
            identity: TeammateIdentity {
                agent_id,
                agent_name: agent_name.to_string(),
                team_name: team_name.to_string(),
                color: None,
                plan_mode_required: false,
                parent_session_id: "session".into(),
            },
            prompt: "test worker".into(),
            model: None,
            abort_handle: None,
            cancellation_token: None,
            awaiting_plan_approval: false,
            permission_mode: PermissionMode::Default,
            error: None,
            pending_user_messages: vec![],
            is_idle: idle,
            shutdown_requested: false,
            last_reported_tool_count: 0,
            last_reported_token_count: 0,
        });
    }

    #[tokio::test]
    #[serial]
    async fn list_agents_includes_task_path_and_runtime_status() {
        InProcessBackend::clear_registry();
        let tmp = TempDir::new().unwrap();
        let (_home, team, _agent_id, task_path) =
            create_team_with_member(&tmp, "multi-agent-v2-list", "worker", "task-list", true);
        register_task(&team, "worker", "task-list", TaskStatus::Running, true);

        let result = ListAgentsTool
            .call(
                json!({"team": team}),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();
        let agents = result.data["agents"].as_array().unwrap();
        assert!(result
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("Listed"));
        let worker = agents
            .iter()
            .find(|agent| agent["name"] == "worker")
            .expect("worker listed");

        assert_eq!(worker["task_id"], "task-list");
        assert_eq!(worker["task_path"], task_path);
        assert_eq!(worker["runtime_task_id"], "task-list");
        assert_eq!(worker["runtime_status"], "idle");
        assert_eq!(worker["status"], "idle");
        InProcessBackend::clear_registry();
    }

    #[tokio::test]
    #[serial]
    async fn followup_task_targets_task_path_and_rejects_stopped_runtime() {
        InProcessBackend::clear_registry();
        let tmp = TempDir::new().unwrap();
        let (_home, team, agent_id, task_path) = create_team_with_member(
            &tmp,
            "multi-agent-v2-followup",
            "worker",
            "task-follow",
            true,
        );
        register_task(&team, "worker", "task-follow", TaskStatus::Running, true);

        let result = FollowupTaskTool
            .call(
                json!({
                    "team": team,
                    "task_path": task_path,
                    "message": "continue",
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.data["sent"], true);
        assert_eq!(result.data["agent_id"], agent_id);
        assert!(result
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("Queued follow-up task"));
        assert_eq!(mailbox::read_mailbox("worker", &team).unwrap().len(), 1);

        InProcessBackend::update_task_status("task-follow", TaskStatus::Stopped);
        let err = FollowupTaskTool
            .call(
                json!({
                    "team": team,
                    "task_path": task_path,
                    "message": "continue again",
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cannot receive follow-up tasks"));
        InProcessBackend::clear_registry();
    }

    #[tokio::test]
    #[serial]
    async fn wait_agent_reports_timeout_then_completion_for_task_path() {
        InProcessBackend::clear_registry();
        let tmp = TempDir::new().unwrap();
        let (_home, team, _agent_id, task_path) =
            create_team_with_member(&tmp, "multi-agent-v2-wait", "worker", "task-wait", true);
        register_task(&team, "worker", "task-wait", TaskStatus::Running, true);

        let timed_out = WaitAgentTool
            .call(
                json!({
                    "team": team,
                    "task_path": task_path,
                    "timeout_ms": 0,
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(timed_out.data["timed_out"], true);
        assert_eq!(timed_out.data["status"], "idle");
        assert_eq!(timed_out.data["final"], false);
        assert!(timed_out
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("timed out"));

        InProcessBackend::update_task_status("task-wait", TaskStatus::Completed);
        let completed = WaitAgentTool
            .call(
                json!({
                    "team": team,
                    "task_path": task_path,
                    "timeout_ms": 1000,
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(completed.data["timed_out"], false);
        assert_eq!(completed.data["completed"], true);
        assert_eq!(completed.data["status"], "completed");
        assert!(completed
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("finished"));
        InProcessBackend::clear_registry();
    }

    #[tokio::test]
    #[serial]
    async fn close_agent_rejects_lead_and_marks_runtime_close_pending() {
        InProcessBackend::clear_registry();
        let tmp = TempDir::new().unwrap();
        let (_home, team, agent_id, task_path) =
            create_team_with_member(&tmp, "multi-agent-v2-close", "worker", "task-close", true);
        register_task(&team, "worker", "task-close", TaskStatus::Running, true);

        let lead_err = CloseAgentTool
            .call(
                json!({
                    "team": team,
                    "agent_id": identity::lead_agent_id(&team),
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap_err();
        assert!(lead_err.to_string().contains("cannot close the team lead"));

        let closed = CloseAgentTool
            .call(
                json!({
                    "team": team,
                    "task_path": task_path,
                    "reason": "test shutdown",
                }),
                &create_test_context(),
                &dummy_parent(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(closed.data["closed"], false);
        assert_eq!(closed.data["close_requested"], true);
        assert_eq!(closed.data["pending_close"], true);
        assert_eq!(closed.data["runtime_shutdown_requested"], true);
        assert_eq!(closed.data["agent_id"], agent_id);
        assert!(closed
            .display_preview
            .as_deref()
            .unwrap_or("")
            .contains("requested shutdown"));

        let updated = helpers::read_team_file(&team).unwrap();
        let member = updated
            .members
            .iter()
            .find(|member| member.name == "worker")
            .unwrap();
        assert_eq!(member.is_active, Some(true));
        assert_eq!(member.close_state.as_deref(), Some("pending"));
        assert!(member.close_requested_at.is_some());
        let messages = mailbox::read_mailbox("worker", &team).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].text.contains("shutdown_request"));
        InProcessBackend::clear_registry();
    }
}
