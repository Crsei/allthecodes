use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::common::string_param;
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::AssistantMessage;
use allthecodes_types::sdk::UsageTracking;

pub fn tools() -> Tools {
    vec![
        Arc::new(GetGoalTool),
        Arc::new(GetGoalAliasTool),
        Arc::new(CreateGoalTool),
        Arc::new(CreateGoalAliasTool),
        Arc::new(UpdateGoalTool),
        Arc::new(UpdateGoalAliasTool),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    #[serde(alias = "completed")]
    Complete,
    Blocked,
    BudgetLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalRecord {
    pub objective: String,
    #[serde(default)]
    pub token_budget: Option<u64>,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub time_used_seconds: u64,
    #[serde(default = "default_goal_status")]
    pub status: GoalStatus,
    #[serde(default = "now_rfc3339")]
    pub created_at: String,
    #[serde(default = "now_rfc3339")]
    pub updated_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub status_reason: Option<String>,
}

fn default_goal_status() -> GoalStatus {
    GoalStatus::Active
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

impl GoalRecord {
    fn is_open(&self) -> bool {
        matches!(self.status, GoalStatus::Active | GoalStatus::BudgetLimited)
    }
}

pub fn goal_file_path_for_session(session_id: &str) -> PathBuf {
    allthecodes_config::paths::goal_file_path(session_id)
}

fn goal_path(ctx: &ToolUseContext) -> PathBuf {
    goal_file_path_for_session(&ctx.session_id)
}

pub fn load_goal_for_session(session_id: &str) -> Result<Option<GoalRecord>> {
    let path = goal_file_path_for_session(session_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn load_goal(ctx: &ToolUseContext) -> Result<Option<GoalRecord>> {
    load_goal_for_session(&ctx.session_id)
}

pub fn save_goal_for_session(session_id: &str, goal: &GoalRecord) -> Result<()> {
    let path = goal_file_path_for_session(session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(goal)?)?;
    Ok(())
}

fn save_goal(ctx: &ToolUseContext, goal: &GoalRecord) -> Result<()> {
    save_goal_for_session(&ctx.session_id, goal)
}

fn total_usage_tokens(usage: &UsageTracking) -> u64 {
    usage
        .total_input_tokens
        .saturating_add(usage.total_output_tokens)
        .saturating_add(usage.total_cache_read_tokens)
        .saturating_add(usage.total_cache_creation_tokens)
}

fn elapsed_goal_seconds(created_at: &str, now: DateTime<Utc>) -> u64 {
    DateTime::parse_from_rfc3339(created_at)
        .map(|created| {
            now.signed_duration_since(created.with_timezone(&Utc))
                .num_seconds()
                .max(0) as u64
        })
        .unwrap_or_default()
}

fn goal_budget_report(goal: &GoalRecord) -> Value {
    let remaining_tokens = goal
        .token_budget
        .map(|budget| budget.saturating_sub(goal.tokens_used));
    let over_budget_tokens = goal
        .token_budget
        .map(|budget| goal.tokens_used.saturating_sub(budget));
    json!({
        "token_budget": goal.token_budget,
        "tokens_used": goal.tokens_used,
        "remaining_tokens": remaining_tokens,
        "over_budget_tokens": over_budget_tokens,
        "time_used_seconds": goal.time_used_seconds,
        "created_at": goal.created_at,
        "updated_at": goal.updated_at,
        "status": goal.status,
    })
}

fn refresh_goal_runtime(goal: &mut GoalRecord, usage: &UsageTracking, now: DateTime<Utc>) {
    goal.tokens_used = total_usage_tokens(usage);
    goal.time_used_seconds = elapsed_goal_seconds(&goal.created_at, now);
    goal.updated_at = now.to_rfc3339();
    if goal.status == GoalStatus::Active {
        if let Some(token_budget) = goal.token_budget {
            if goal.tokens_used >= token_budget {
                goal.status = GoalStatus::BudgetLimited;
                goal.status_reason = Some(format!(
                    "token budget exceeded: used {} of {} tokens",
                    goal.tokens_used, token_budget
                ));
            }
        }
    }
}

pub fn account_goal_runtime_for_session(
    session_id: &str,
    usage: &UsageTracking,
) -> Result<Option<GoalRecord>> {
    let Some(mut goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if goal.is_open() {
        refresh_goal_runtime(&mut goal, usage, Utc::now());
        save_goal_for_session(session_id, &goal)?;
    }
    Ok(Some(goal))
}

pub fn mark_goal_budget_limited_for_session(
    session_id: &str,
    usage: &UsageTracking,
    reason: impl Into<String>,
) -> Result<Option<GoalRecord>> {
    let Some(mut goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if goal.is_open() {
        refresh_goal_runtime(&mut goal, usage, Utc::now());
        goal.status = GoalStatus::BudgetLimited;
        goal.status_reason = Some(reason.into());
        goal.updated_at = Utc::now().to_rfc3339();
        save_goal_for_session(session_id, &goal)?;
    }
    Ok(Some(goal))
}

pub struct GetGoalTool;
pub struct CreateGoalTool;
pub struct UpdateGoalTool;
pub struct GetGoalAliasTool;
pub struct CreateGoalAliasTool;
pub struct UpdateGoalAliasTool;

#[async_trait]
impl Tool for GetGoalTool {
    fn name(&self) -> &str {
        "GetGoal"
    }

    async fn description(&self, _input: &Value) -> String {
        "Get the current session goal, if one exists.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        Ok(ToolResult {
            data: json!({
                "goal": load_goal(ctx)?,
                "path": goal_path(ctx),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Read the active session goal and completion status.".into()
    }
}

#[async_trait]
impl Tool for CreateGoalTool {
    fn name(&self) -> &str {
        "CreateGoal"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create one active session goal with an optional token budget.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "objective": {"type": "string"},
                "token_budget": {"type": "integer", "minimum": 1}
            },
            "required": ["objective"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "objective").is_none() {
            return ValidationResult::Error {
                message: "objective is required".into(),
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
        if let Some(existing) = load_goal(ctx)? {
            if existing.is_open() {
                return Ok(ToolResult {
                    data: json!({
                        "error": "active_goal_exists",
                        "message": "A session can only have one unfinished goal. Complete or block it with UpdateGoal before creating another.",
                        "goal": existing,
                    }),
                    ..Default::default()
                });
            }
        }
        let now = Utc::now().to_rfc3339();
        let goal = GoalRecord {
            objective: string_param(&input, "objective").unwrap().to_string(),
            token_budget: input.get("token_budget").and_then(Value::as_u64),
            tokens_used: 0,
            time_used_seconds: 0,
            status: GoalStatus::Active,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
            status_reason: None,
        };
        save_goal(ctx, &goal)?;
        Ok(ToolResult {
            data: json!({
                "created": true,
                "goal": goal,
                "path": goal_path(ctx),
                "runtime": goal_budget_report(&goal),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Create a single active session goal. Goals are completion criteria, not todo lists.".into()
    }
}

#[async_trait]
impl Tool for UpdateGoalTool {
    fn name(&self) -> &str {
        "UpdateGoal"
    }

    async fn description(&self, _input: &Value) -> String {
        "Update the current session goal by marking it complete or blocked.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["complete", "blocked"]},
                "reason": {"type": "string"}
            },
            "required": ["status"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if !matches!(string_param(input, "status"), Some("complete" | "blocked")) {
            return ValidationResult::Error {
                message: "status must be complete or blocked".into(),
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
        let Some(mut goal) = load_goal(ctx)? else {
            return Ok(ToolResult {
                data: json!({"error": "goal_not_found", "message": "No session goal exists."}),
                ..Default::default()
            });
        };
        let now = Utc::now();
        goal.time_used_seconds = elapsed_goal_seconds(&goal.created_at, now);
        goal.updated_at = now.to_rfc3339();
        goal.status = match string_param(&input, "status") {
            Some("complete") => GoalStatus::Complete,
            Some("blocked") => GoalStatus::Blocked,
            _ => unreachable!("validated status"),
        };
        goal.completed_at = (goal.status == GoalStatus::Complete).then(|| goal.updated_at.clone());
        goal.status_reason = string_param(&input, "reason").map(ToOwned::to_owned);
        save_goal(ctx, &goal)?;
        let completion_budget_report = goal_budget_report(&goal);
        Ok(ToolResult {
            data: json!({
                "updated": true,
                "goal": goal,
                "completion_budget_report": completion_budget_report,
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Mark the active session goal complete only when the objective is genuinely achieved, or blocked when progress is impossible without external input.".into()
    }
}

crate::common::tool_alias!(GetGoalAliasTool, "get_goal", GetGoalTool);
crate::common::tool_alias!(CreateGoalAliasTool, "create_goal", CreateGoalTool);
crate::common::tool_alias!(UpdateGoalAliasTool, "update_goal", UpdateGoalTool);
