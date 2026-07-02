use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

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
    Paused,
    #[serde(alias = "completed")]
    Complete,
    Blocked,
    UsageLimited,
    BudgetLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "new_goal_id")]
    pub goal_id: String,
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
    pub paused_at: Option<String>,
    #[serde(default)]
    pub blocked_at: Option<String>,
    #[serde(default)]
    pub usage_limited_at: Option<String>,
    #[serde(default)]
    pub budget_limited_at: Option<String>,
    #[serde(default)]
    pub status_reason: Option<String>,
}

pub const GOAL_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    GOAL_SCHEMA_VERSION
}

fn default_goal_status() -> GoalStatus {
    GoalStatus::Active
}

fn new_goal_id() -> String {
    format!("goal_{}", Uuid::new_v4().simple())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

impl GoalRecord {
    fn is_open(&self) -> bool {
        goal_is_open(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalError {
    ActiveGoalExists,
    GoalNotFound,
    InvalidGoalObjective,
    InvalidTokenBudget,
    InvalidGoalTransition { from: GoalStatus, to: GoalStatus },
    StaleGoal { expected: String, actual: String },
}

impl GoalError {
    pub fn code(&self) -> &'static str {
        match self {
            GoalError::ActiveGoalExists => "active_goal_exists",
            GoalError::GoalNotFound => "goal_not_found",
            GoalError::InvalidGoalObjective => "invalid_goal_objective",
            GoalError::InvalidTokenBudget => "invalid_token_budget",
            GoalError::InvalidGoalTransition { .. } => "invalid_goal_transition",
            GoalError::StaleGoal { .. } => "stale_goal",
        }
    }

    pub fn message(&self) -> String {
        match self {
            GoalError::ActiveGoalExists => {
                "A session can only have one unfinished goal.".to_string()
            }
            GoalError::GoalNotFound => "No session goal exists.".to_string(),
            GoalError::InvalidGoalObjective => "A goal objective is required.".to_string(),
            GoalError::InvalidTokenBudget => {
                "token_budget must be a positive integer when provided.".to_string()
            }
            GoalError::InvalidGoalTransition { from, to } => {
                format!("cannot transition goal from {from:?} to {to:?}")
            }
            GoalError::StaleGoal { expected, actual } => {
                format!("stale goal update rejected: expected {expected}, found {actual}")
            }
        }
    }
}

pub type GoalTransitionResult<T> = std::result::Result<T, GoalError>;

pub fn validate_goal_objective(objective: &str) -> GoalTransitionResult<String> {
    let objective = objective.trim();
    if objective.is_empty() {
        return Err(GoalError::InvalidGoalObjective);
    }
    Ok(objective.to_string())
}

pub fn validate_token_budget(token_budget: Option<u64>) -> GoalTransitionResult<Option<u64>> {
    match token_budget {
        Some(0) => Err(GoalError::InvalidTokenBudget),
        other => Ok(other),
    }
}

pub fn create_goal_record(
    objective: impl AsRef<str>,
    token_budget: Option<u64>,
    now: DateTime<Utc>,
) -> GoalTransitionResult<GoalRecord> {
    let objective = validate_goal_objective(objective.as_ref())?;
    let token_budget = validate_token_budget(token_budget)?;
    let now = now.to_rfc3339();
    Ok(GoalRecord {
        schema_version: GOAL_SCHEMA_VERSION,
        goal_id: new_goal_id(),
        objective,
        token_budget,
        tokens_used: 0,
        time_used_seconds: 0,
        status: GoalStatus::Active,
        created_at: now.clone(),
        updated_at: now,
        completed_at: None,
        paused_at: None,
        blocked_at: None,
        usage_limited_at: None,
        budget_limited_at: None,
        status_reason: None,
    })
}

pub fn replace_goal_record(
    _existing: &GoalRecord,
    objective: impl AsRef<str>,
    token_budget: Option<u64>,
    now: DateTime<Utc>,
) -> GoalTransitionResult<GoalRecord> {
    create_goal_record(objective, token_budget, now)
}

pub fn goal_is_open(goal: &GoalRecord) -> bool {
    !goal_is_terminal(goal)
}

pub fn goal_is_active(goal: &GoalRecord) -> bool {
    goal.status == GoalStatus::Active
}

pub fn goal_is_terminal(goal: &GoalRecord) -> bool {
    goal.status == GoalStatus::Complete
}

pub fn status_after_budget_limit(goal: &GoalRecord) -> GoalStatus {
    if goal.status == GoalStatus::Complete {
        GoalStatus::Complete
    } else {
        GoalStatus::BudgetLimited
    }
}

fn reject_stale_goal(
    goal: &GoalRecord,
    expected_goal_id: Option<&str>,
) -> GoalTransitionResult<()> {
    if let Some(expected) = expected_goal_id {
        if expected != goal.goal_id {
            return Err(GoalError::StaleGoal {
                expected: expected.to_string(),
                actual: goal.goal_id.clone(),
            });
        }
    }
    Ok(())
}

pub fn update_goal_status(
    mut goal: GoalRecord,
    status: GoalStatus,
    reason: Option<String>,
    now: DateTime<Utc>,
    expected_goal_id: Option<&str>,
) -> GoalTransitionResult<GoalRecord> {
    reject_stale_goal(&goal, expected_goal_id)?;
    let from = goal.status.clone();
    let allowed = match (&from, &status) {
        (current, next) if current == next => true,
        (GoalStatus::Active, GoalStatus::Paused) => true,
        (
            GoalStatus::Paused | GoalStatus::Blocked | GoalStatus::UsageLimited,
            GoalStatus::Active,
        ) => true,
        (
            GoalStatus::Active
            | GoalStatus::Paused
            | GoalStatus::Blocked
            | GoalStatus::UsageLimited
            | GoalStatus::BudgetLimited,
            GoalStatus::Complete,
        ) => true,
        (
            GoalStatus::Active | GoalStatus::Paused | GoalStatus::UsageLimited,
            GoalStatus::Blocked,
        ) => true,
        (GoalStatus::Active, GoalStatus::UsageLimited | GoalStatus::BudgetLimited) => true,
        _ => false,
    };
    if !allowed {
        return Err(GoalError::InvalidGoalTransition { from, to: status });
    }

    let now = now.to_rfc3339();
    goal.updated_at = now.clone();
    goal.status = status.clone();
    goal.status_reason = reason;
    match status {
        GoalStatus::Active => {
            goal.paused_at = None;
            goal.blocked_at = None;
            goal.usage_limited_at = None;
        }
        GoalStatus::Paused => {
            goal.paused_at = Some(now);
        }
        GoalStatus::Complete => {
            goal.completed_at = Some(now);
        }
        GoalStatus::Blocked => {
            goal.blocked_at = Some(now);
            goal.paused_at = None;
            goal.usage_limited_at = None;
        }
        GoalStatus::UsageLimited => {
            goal.usage_limited_at = Some(now);
        }
        GoalStatus::BudgetLimited => {
            goal.budget_limited_at = Some(now);
        }
    }
    Ok(goal)
}

pub fn apply_goal_usage_delta(
    mut goal: GoalRecord,
    token_delta: u64,
    seconds_delta: u64,
    now: DateTime<Utc>,
    expected_goal_id: Option<&str>,
) -> GoalTransitionResult<GoalRecord> {
    reject_stale_goal(&goal, expected_goal_id)?;
    if !goal_is_active(&goal) {
        return Ok(goal);
    }
    goal.tokens_used = goal.tokens_used.saturating_add(token_delta);
    goal.time_used_seconds = goal.time_used_seconds.saturating_add(seconds_delta);
    goal.updated_at = now.to_rfc3339();
    if let Some(token_budget) = goal.token_budget {
        if goal.tokens_used >= token_budget {
            let reason = Some(format!(
                "token budget exceeded: used {} of {} tokens",
                goal.tokens_used, token_budget
            ));
            goal = update_goal_status(
                goal,
                GoalStatus::BudgetLimited,
                reason,
                now,
                expected_goal_id,
            )?;
        }
    }
    Ok(goal)
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

pub fn active_goal_id_for_session(session_id: &str) -> Result<Option<String>> {
    Ok(load_goal_for_session(session_id)?
        .filter(goal_is_active)
        .map(|goal| goal.goal_id))
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

pub fn clear_goal_for_session(session_id: &str) -> Result<bool> {
    let path = goal_file_path_for_session(session_id);
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(path)?;
    Ok(true)
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

fn remaining_tokens(goal: &GoalRecord) -> Option<u64> {
    goal.token_budget
        .map(|budget| budget.saturating_sub(goal.tokens_used))
}

fn token_budget_param(input: &Value) -> GoalTransitionResult<Option<u64>> {
    match input.get("token_budget") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(budget) => validate_token_budget(Some(budget)),
            None => Err(GoalError::InvalidTokenBudget),
        },
    }
}

fn completion_budget_report(goal: &GoalRecord) -> Option<String> {
    if goal.status != GoalStatus::Complete {
        return None;
    }
    Some(match goal.token_budget {
        Some(budget) => format!(
            "Report final goal usage to the user: {} tokens used of {} budgeted, {} remaining, {} seconds elapsed.",
            goal.tokens_used,
            budget,
            budget.saturating_sub(goal.tokens_used),
            goal.time_used_seconds
        ),
        None => format!(
            "Report final goal usage to the user: {} tokens used, {} seconds elapsed.",
            goal.tokens_used, goal.time_used_seconds
        ),
    })
}

fn refresh_goal_runtime(goal: &mut GoalRecord, usage: &UsageTracking, now: DateTime<Utc>) {
    if !goal_is_active(goal) {
        return;
    }
    let total_tokens = total_usage_tokens(usage);
    let elapsed_seconds = elapsed_goal_seconds(&goal.created_at, now);
    let token_delta = total_tokens.saturating_sub(goal.tokens_used);
    let seconds_delta = elapsed_seconds.saturating_sub(goal.time_used_seconds);
    if let Ok(next) = apply_goal_usage_delta(goal.clone(), token_delta, seconds_delta, now, None) {
        *goal = next;
    }
}

pub fn account_goal_runtime_for_session(
    session_id: &str,
    usage: &UsageTracking,
) -> Result<Option<GoalRecord>> {
    let Some(mut goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if goal_is_active(&goal) {
        refresh_goal_runtime(&mut goal, usage, Utc::now());
        save_goal_for_session(session_id, &goal)?;
    }
    Ok(Some(goal))
}

pub fn account_goal_runtime_delta_for_session(
    session_id: &str,
    expected_goal_id: &str,
    token_delta: u64,
    seconds_delta: u64,
    now: DateTime<Utc>,
) -> Result<Option<GoalRecord>> {
    let Some(goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if !goal_is_active(&goal) {
        return Ok(Some(goal));
    }

    let goal = apply_goal_usage_delta(
        goal,
        token_delta,
        seconds_delta,
        now,
        Some(expected_goal_id),
    )
    .map_err(|error| anyhow::anyhow!(error.message()))?;
    save_goal_for_session(session_id, &goal)?;
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
    if goal_is_active(&goal) || goal.status == GoalStatus::BudgetLimited {
        refresh_goal_runtime(&mut goal, usage, Utc::now());
        goal = update_goal_status(
            goal,
            GoalStatus::BudgetLimited,
            Some(reason.into()),
            Utc::now(),
            None,
        )
        .map_err(|error| anyhow::anyhow!(error.message()))?;
        save_goal_for_session(session_id, &goal)?;
    }
    Ok(Some(goal))
}

pub fn mark_goal_usage_limited_for_session(
    session_id: &str,
    expected_goal_id: Option<&str>,
    reason: impl Into<String>,
) -> Result<Option<GoalRecord>> {
    let Some(goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if goal_is_active(&goal) || goal.status == GoalStatus::UsageLimited {
        let goal = update_goal_status(
            goal,
            GoalStatus::UsageLimited,
            Some(reason.into()),
            Utc::now(),
            expected_goal_id,
        )
        .map_err(|error| anyhow::anyhow!(error.message()))?;
        save_goal_for_session(session_id, &goal)?;
        return Ok(Some(goal));
    }
    Ok(Some(goal))
}

pub fn mark_goal_paused_for_session(
    session_id: &str,
    expected_goal_id: Option<&str>,
    reason: impl Into<String>,
) -> Result<Option<GoalRecord>> {
    let Some(goal) = load_goal_for_session(session_id)? else {
        return Ok(None);
    };
    if goal_is_active(&goal) || goal.status == GoalStatus::Paused {
        let goal = update_goal_status(
            goal,
            GoalStatus::Paused,
            Some(reason.into()),
            Utc::now(),
            expected_goal_id,
        )
        .map_err(|error| anyhow::anyhow!(error.message()))?;
        save_goal_for_session(session_id, &goal)?;
        return Ok(Some(goal));
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
        let goal = load_goal(ctx)?;
        let remaining_tokens = goal.as_ref().and_then(remaining_tokens);
        Ok(ToolResult {
            data: json!({
                "goal": goal,
                "remaining_tokens": remaining_tokens,
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
        if validate_goal_objective(string_param(input, "objective").unwrap_or_default()).is_err() {
            return ValidationResult::Error {
                message: "objective is required".into(),
                error_code: 400,
            };
        }
        if token_budget_param(input).is_err() {
            return ValidationResult::Error {
                message: "token_budget must be a positive integer".into(),
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
                    data: tool_error(
                        GoalError::ActiveGoalExists,
                        "Complete the current goal before creating another.",
                        Some(existing),
                    ),
                    ..Default::default()
                });
            }
        }
        let goal = match create_goal_record(
            string_param(&input, "objective").unwrap_or_default(),
            match token_budget_param(&input) {
                Ok(token_budget) => token_budget,
                Err(error) => {
                    return Ok(ToolResult {
                        data: tool_error(error, "", None),
                        ..Default::default()
                    });
                }
            },
            Utc::now(),
        ) {
            Ok(goal) => goal,
            Err(error) => {
                return Ok(ToolResult {
                    data: tool_error(error, "", None),
                    ..Default::default()
                });
            }
        };
        save_goal(ctx, &goal)?;
        Ok(ToolResult {
            data: json!({
                "created": true,
                "goal": goal,
                "path": goal_path(ctx),
                "remaining_tokens": remaining_tokens(&goal),
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
                data: tool_error(GoalError::GoalNotFound, "", None),
                ..Default::default()
            });
        };
        let now = Utc::now();
        goal.time_used_seconds = elapsed_goal_seconds(&goal.created_at, now);
        let Some(status) = string_param(&input, "status").and_then(|status| match status {
            "complete" => Some(GoalStatus::Complete),
            "blocked" => Some(GoalStatus::Blocked),
            _ => None,
        }) else {
            return Ok(ToolResult {
                data: tool_error_code(
                    "invalid_goal_transition",
                    "status must be complete or blocked",
                    Some(goal),
                ),
                ..Default::default()
            });
        };
        goal = match update_goal_status(
            goal,
            status,
            string_param(&input, "reason").map(ToOwned::to_owned),
            now,
            None,
        ) {
            Ok(goal) => goal,
            Err(error) => {
                return Ok(ToolResult {
                    data: tool_error(error, "", None),
                    ..Default::default()
                });
            }
        };
        save_goal(ctx, &goal)?;
        Ok(ToolResult {
            data: json!({
                "updated": true,
                "goal": goal,
                "remaining_tokens": remaining_tokens(&goal),
                "runtime": goal_budget_report(&goal),
                "completion_budget_report": completion_budget_report(&goal),
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

fn tool_error(error: GoalError, message_suffix: &str, goal: Option<GoalRecord>) -> Value {
    let mut message = error.message();
    if !message_suffix.is_empty() {
        message.push(' ');
        message.push_str(message_suffix);
    }
    json!({
        "error": error.code(),
        "message": message,
        "goal": goal,
    })
}

fn tool_error_code(code: &str, message: &str, goal: Option<GoalRecord>) -> Value {
    json!({
        "error": code,
        "message": message,
        "goal": goal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::path::Path;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-03T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn goal() -> GoalRecord {
        create_goal_record("ship", Some(100), now()).unwrap()
    }

    #[test]
    #[serial]
    fn active_goal_id_for_session_only_returns_active_goals() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let session_id = "goal-active-id";
        let goal = goal();
        let id = goal.goal_id.clone();
        save_goal_for_session(session_id, &goal).unwrap();

        assert_eq!(
            active_goal_id_for_session(session_id).unwrap().as_deref(),
            Some(id.as_str())
        );

        let completed =
            update_goal_status(goal, GoalStatus::Complete, None, now(), Some(&id)).unwrap();
        save_goal_for_session(session_id, &completed).unwrap();

        assert!(active_goal_id_for_session(session_id).unwrap().is_none());
    }

    #[test]
    fn loads_old_goal_json_with_defaults() {
        let raw = json!({
            "objective": "old objective",
            "status": "active"
        })
        .to_string();

        let goal: GoalRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(goal.schema_version, GOAL_SCHEMA_VERSION);
        assert!(!goal.goal_id.is_empty());
        assert_eq!(goal.objective, "old objective");
        assert_eq!(goal.status, GoalStatus::Active);
    }

    #[test]
    fn validates_objective_and_budget() {
        assert_eq!(
            validate_goal_objective("   ").unwrap_err(),
            GoalError::InvalidGoalObjective
        );
        assert_eq!(
            validate_token_budget(Some(0)).unwrap_err(),
            GoalError::InvalidTokenBudget
        );
        assert_eq!(validate_token_budget(Some(1)).unwrap(), Some(1));
    }

    #[test]
    fn transitions_active_paused_blocked_usage_limited_and_complete() {
        let goal = goal();
        let paused =
            update_goal_status(goal, GoalStatus::Paused, Some("wait".into()), now(), None).unwrap();
        assert_eq!(paused.status, GoalStatus::Paused);
        assert!(paused.paused_at.is_some());

        let resumed =
            update_goal_status(paused, GoalStatus::Active, Some("go".into()), now(), None).unwrap();
        assert_eq!(resumed.status, GoalStatus::Active);
        assert!(resumed.paused_at.is_none());

        let usage_limited =
            update_goal_status(resumed, GoalStatus::UsageLimited, None, now(), None).unwrap();
        assert_eq!(usage_limited.status, GoalStatus::UsageLimited);
        assert!(usage_limited.usage_limited_at.is_some());

        let resumed =
            update_goal_status(usage_limited, GoalStatus::Active, None, now(), None).unwrap();
        let blocked = update_goal_status(resumed, GoalStatus::Blocked, None, now(), None).unwrap();
        assert_eq!(blocked.status, GoalStatus::Blocked);
        assert!(blocked.blocked_at.is_some());

        let complete = update_goal_status(
            blocked,
            GoalStatus::Complete,
            Some("done".into()),
            now(),
            None,
        )
        .unwrap();
        assert!(goal_is_terminal(&complete));
        assert!(complete.completed_at.is_some());
    }

    #[test]
    fn rejects_invalid_transitions_and_stale_goal_ids() {
        let goal = goal();
        let paused = update_goal_status(goal, GoalStatus::Paused, None, now(), None).unwrap();
        let err = update_goal_status(
            paused.clone(),
            GoalStatus::Paused,
            None,
            now(),
            Some("other"),
        )
        .unwrap_err();
        assert!(matches!(err, GoalError::StaleGoal { .. }));

        let err = update_goal_status(
            paused,
            GoalStatus::BudgetLimited,
            Some("budget".into()),
            now(),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, GoalError::InvalidGoalTransition { .. }));
    }

    #[test]
    fn applies_usage_delta_without_decreasing_and_marks_budget_limited() {
        let goal = goal();
        let id = goal.goal_id.clone();
        let accounted = apply_goal_usage_delta(goal, 40, 5, now(), Some(&id)).unwrap();
        assert_eq!(accounted.tokens_used, 40);
        assert_eq!(accounted.time_used_seconds, 5);

        let budget_limited = apply_goal_usage_delta(accounted, 60, 2, now(), Some(&id)).unwrap();
        assert_eq!(budget_limited.tokens_used, 100);
        assert_eq!(budget_limited.time_used_seconds, 7);
        assert_eq!(budget_limited.status, GoalStatus::BudgetLimited);

        let unchanged = apply_goal_usage_delta(budget_limited, 10, 2, now(), Some(&id)).unwrap();
        assert_eq!(unchanged.tokens_used, 100);
        assert_eq!(unchanged.time_used_seconds, 7);
        assert_eq!(unchanged.status, GoalStatus::BudgetLimited);
    }

    #[test]
    #[serial]
    fn session_delta_accounting_rejects_stale_goal_id() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let session_id = "goal-delta-stale";
        let goal = goal();
        let id = goal.goal_id.clone();
        save_goal_for_session(session_id, &goal).unwrap();

        account_goal_runtime_delta_for_session(session_id, &id, 10, 1, now()).unwrap();
        let stale = account_goal_runtime_delta_for_session(session_id, "other", 10, 1, now());

        assert!(stale.is_err());
        let stored = load_goal_for_session(session_id).unwrap().unwrap();
        assert_eq!(stored.tokens_used, 10);
        assert_eq!(stored.time_used_seconds, 1);
    }

    #[test]
    #[serial]
    fn session_usage_limit_marks_usage_limited() {
        let tempdir = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tempdir.path());
        let session_id = "goal-usage-limited";
        let goal = goal();
        let id = goal.goal_id.clone();
        save_goal_for_session(session_id, &goal).unwrap();

        let limited =
            mark_goal_usage_limited_for_session(session_id, Some(&id), "cost limit reached")
                .unwrap()
                .unwrap();

        assert_eq!(limited.status, GoalStatus::UsageLimited);
        assert_eq!(limited.status_reason.as_deref(), Some("cost limit reached"));
        assert!(limited.usage_limited_at.is_some());
    }
}
