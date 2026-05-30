//! Phase 5 tool implementations.
//!
//! These tools are intentionally grouped here instead of expanding
//! `product_tools.rs`; they cover the remaining low-frequency product and
//! orchestration surfaces from the full-build parity plan.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::fs::safe_write::{safe_write_text, SafeWriteOptions};
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::{AssistantMessage, ContentBlock, ImageSource, ToolResultContent};

pub fn tools() -> Tools {
    vec![
        Arc::new(DiscoverSkillsTool),
        Arc::new(ViewImageTool),
        Arc::new(GetGoalTool),
        Arc::new(CreateGoalTool),
        Arc::new(UpdateGoalTool),
        Arc::new(VerifyPlanExecutionTool),
        Arc::new(WorkflowTool),
        Arc::new(ApplyPatchTool),
        Arc::new(LocalMemoryRecallTool),
        Arc::new(VaultHttpFetchTool),
        Arc::new(PushNotificationTool),
    ]
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| allthecodes_config::paths::data_root())
}

fn task_list_id_for_context(ctx: &ToolUseContext) -> String {
    let app_state = (ctx.get_app_state)();
    allthecodes_tasks::task_list_id_from_parts(allthecodes_tasks::TaskListScope {
        explicit_task_list_id: None,
        scoped_team_name: None,
        app_team_name: app_state
            .team_context
            .as_ref()
            .map(|team| team.team_name.clone()),
        session_id: Some(ctx.session_id.clone()),
    })
}

fn string_param<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn validate_enum(input: &Value, key: &str, allowed: &[&str]) -> Option<ValidationResult> {
    let Some(value) = string_param(input, key) else {
        return None;
    };
    if allowed.contains(&value) {
        None
    } else {
        Some(ValidationResult::Error {
            message: format!("{key} must be one of: {}", allowed.join(", ")),
            error_code: 400,
        })
    }
}

fn source_label(source: &allthecodes_skills::SkillSource) -> String {
    match source {
        allthecodes_skills::SkillSource::Bundled => "bundled".to_string(),
        allthecodes_skills::SkillSource::User => "user".to_string(),
        allthecodes_skills::SkillSource::Project => "project".to_string(),
        allthecodes_skills::SkillSource::Plugin(name) => format!("plugin:{name}"),
        allthecodes_skills::SkillSource::Mcp(name) => format!("mcp:{name}"),
    }
}

fn source_matches(source: &allthecodes_skills::SkillSource, filter: &str) -> bool {
    match filter {
        "all" => true,
        "bundled" => matches!(source, allthecodes_skills::SkillSource::Bundled),
        "user" => matches!(source, allthecodes_skills::SkillSource::User),
        "project" => matches!(source, allthecodes_skills::SkillSource::Project),
        "plugin" => matches!(source, allthecodes_skills::SkillSource::Plugin(_)),
        "mcp" => matches!(source, allthecodes_skills::SkillSource::Mcp(_)),
        _ => false,
    }
}

fn text_score(query: &str, fields: &[&str]) -> usize {
    if query.trim().is_empty() {
        return 1;
    }
    let terms = query
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let haystack = fields.join("\n").to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

pub struct DiscoverSkillsTool;

#[async_trait]
impl Tool for DiscoverSkillsTool {
    fn name(&self) -> &str {
        "DiscoverSkills"
    }

    async fn description(&self, _input: &Value) -> String {
        "Discover bundled, user, project, plugin, and MCP skills available to this session.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Optional text query over skill names, descriptions, and usage hints."},
                "source": {"type": "string", "enum": ["bundled", "user", "project", "plugin", "mcp", "all"], "description": "Skill source filter."},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 100}
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
        if let Some(result) = validate_enum(
            input,
            "source",
            &["bundled", "user", "project", "plugin", "mcp", "all"],
        ) {
            return result;
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let query = string_param(&input, "query").unwrap_or("");
        let source_filter = string_param(&input, "source").unwrap_or("all");
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(25)
            .clamp(1, 100) as usize;

        let mut skills = allthecodes_skills::get_all_skills();
        if skills.is_empty() {
            skills = allthecodes_skills::bundled::bundled_skills();
        }

        let mut matches = skills
            .into_iter()
            .filter(|skill| source_matches(&skill.source, source_filter))
            .filter_map(|skill| {
                let score = text_score(
                    query,
                    &[
                        &skill.name,
                        skill.display_name(),
                        &skill.frontmatter.description,
                        skill.frontmatter.when_to_use.as_deref().unwrap_or(""),
                    ],
                );
                if score == 0 {
                    return None;
                }
                Some((
                    score,
                    json!({
                        "name": skill.name,
                        "display_name": skill.display_name(),
                        "source": source_label(&skill.source),
                        "description": skill.frontmatter.description,
                        "when_to_use": skill.frontmatter.when_to_use,
                        "allowed_tools": skill.frontmatter.allowed_tools,
                        "user_invocable": skill.is_user_invocable(),
                        "model_invocable": skill.is_model_invocable(),
                        "context": skill.frontmatter.context,
                        "agent": skill.frontmatter.agent,
                        "version": skill.effective_version(),
                    }),
                ))
            })
            .collect::<Vec<_>>();

        matches.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| {
                a.1["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b.1["name"].as_str().unwrap_or(""))
            })
        });
        matches.truncate(max_results);

        Ok(ToolResult {
            data: json!({
                "query": query,
                "source": source_filter,
                "count": matches.len(),
                "skills": matches.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Discover available skills by name, source, description, and when-to-use metadata.".into()
    }
}

pub struct ViewImageTool;

fn image_mime(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    {
        Some(ext) if ext == "png" => Some("image/png"),
        Some(ext) if ext == "jpg" || ext == "jpeg" => Some("image/jpeg"),
        Some(ext) if ext == "webp" => Some("image/webp"),
        Some(ext) if ext == "gif" => Some("image/gif"),
        _ => None,
    }
}

fn validate_read_path(path: &str, ctx: &ToolUseContext) -> Result<PathBuf> {
    let validated = allthecodes_permissions::path_validation::validate_file_path(path)?;
    let cwd = current_dir();
    let app_state = (ctx.get_app_state)();
    if !allthecodes_permissions::path_validation::is_path_within_allowed_directories(
        &validated,
        &cwd,
        &app_state.tool_permission_context,
    ) {
        bail!(
            "path is outside the current working directory and configured additional directories: {}",
            validated.display()
        );
    }
    Ok(validated)
}

#[async_trait]
impl Tool for ViewImageTool {
    fn name(&self) -> &str {
        "ViewImage"
    }

    async fn description(&self, _input: &Value) -> String {
        "Read a local image file and return it as an image content block.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to a png, jpeg, webp, or gif image."},
                "detail": {"type": "string", "enum": ["auto", "original"], "description": "Image detail hint for downstream rendering."}
            },
            "required": ["path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn get_path(&self, input: &Value) -> Option<String> {
        string_param(input, "path").map(ToOwned::to_owned)
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let Some(path) = string_param(input, "path") else {
            return ValidationResult::Error {
                message: "path is required".into(),
                error_code: 400,
            };
        };
        if image_mime(Path::new(path)).is_none() {
            return ValidationResult::Error {
                message: "path must point to a png, jpeg, webp, or gif image".into(),
                error_code: 400,
            };
        }
        if let Some(result) = validate_enum(input, "detail", &["auto", "original"]) {
            return result;
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
        let path = string_param(&input, "path").ok_or_else(|| anyhow!("path is required"))?;
        let validated = validate_read_path(path, ctx)?;
        let mime = image_mime(&validated).ok_or_else(|| anyhow!("unsupported image type"))?;
        let bytes = tokio::fs::read(&validated).await?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let detail = string_param(&input, "detail").unwrap_or("auto");

        Ok(ToolResult {
            data: json!({
                "path": validated.display().to_string(),
                "mime": mime,
                "size": bytes.len(),
                "sha256": sha256,
                "detail": detail,
            }),
            model_content: Some(ToolResultContent::Blocks(vec![ContentBlock::Image {
                source: ImageSource {
                    source_type: "base64".into(),
                    media_type: mime.into(),
                    data: encoded,
                },
            }])),
            display_preview: Some(format!("Viewed image {}", validated.display())),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Use ViewImage to inspect local png, jpeg, webp, or gif files without placing binary data in ordinary text output.".into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum GoalStatus {
    Active,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoalRecord {
    objective: String,
    token_budget: Option<u64>,
    status: GoalStatus,
    created_at: String,
    completed_at: Option<String>,
}

fn goal_path(ctx: &ToolUseContext) -> PathBuf {
    allthecodes_config::paths::goal_file_path(&ctx.session_id)
}

fn load_goal(ctx: &ToolUseContext) -> Result<Option<GoalRecord>> {
    let path = goal_path(ctx);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn save_goal(ctx: &ToolUseContext, goal: &GoalRecord) -> Result<()> {
    let path = goal_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(goal)?)?;
    Ok(())
}

pub struct GetGoalTool;
pub struct CreateGoalTool;
pub struct UpdateGoalTool;

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
            if existing.status == GoalStatus::Active {
                return Ok(ToolResult {
                    data: json!({
                        "error": "active_goal_exists",
                        "message": "A session can only have one active goal. Complete it with UpdateGoal before creating another.",
                        "goal": existing,
                    }),
                    ..Default::default()
                });
            }
        }
        let goal = GoalRecord {
            objective: string_param(&input, "objective").unwrap().to_string(),
            token_budget: input.get("token_budget").and_then(Value::as_u64),
            status: GoalStatus::Active,
            created_at: Utc::now().to_rfc3339(),
            completed_at: None,
        };
        save_goal(ctx, &goal)?;
        Ok(ToolResult {
            data: json!({"created": true, "goal": goal, "path": goal_path(ctx)}),
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
        "Update the current session goal. Only marking it complete is supported.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["complete"]}
            },
            "required": ["status"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "status") != Some("complete") {
            return ValidationResult::Error {
                message: "status must be complete".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        _input: Value,
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
        goal.status = GoalStatus::Completed;
        goal.completed_at = Some(Utc::now().to_rfc3339());
        save_goal(ctx, &goal)?;
        Ok(ToolResult {
            data: json!({"updated": true, "goal": goal}),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Mark the active session goal complete when the session-level objective is genuinely achieved.".into()
    }
}

pub struct VerifyPlanExecutionTool;

#[async_trait]
impl Tool for VerifyPlanExecutionTool {
    fn name(&self) -> &str {
        "VerifyPlanExecution"
    }

    async fn description(&self, _input: &Value) -> String {
        "Read-only verification of the current plan workflow, linked tasks, and unfinished todos."
            .into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan_path": {"type": "string"},
                "strict": {"type": "boolean"}
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
        let cwd = current_dir();
        let plan_path = string_param(&input, "plan_path")
            .map(PathBuf::from)
            .unwrap_or_else(|| allthecodes_config::paths::current_plan_file_path(&cwd));
        let workflow = crate::plan_workflow::load(&cwd)?;
        let task_store = allthecodes_tasks::store_for_task_list_id(&task_list_id_for_context(ctx));
        let tasks = task_store
            .list()
            .into_iter()
            .map(|task| {
                json!({
                    "id": task.id,
                    "subject": task.subject,
                    "status": task.status,
                    "kind": task.kind,
                    "depends_on": task.depends_on,
                })
            })
            .collect::<Vec<_>>();
        let todo_key = allthecodes_tasks::todo_owner_key(&ctx.session_id, ctx.agent_id.as_deref());
        let unfinished_todos = allthecodes_tasks::todos_for_key(&todo_key)
            .into_iter()
            .filter(|todo| todo.status != "completed")
            .collect::<Vec<_>>();
        let strict = input
            .get("strict")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut recommendations = Vec::new();
        if !plan_path.exists() {
            recommendations.push("No plan file exists at the selected plan_path.");
        }
        if workflow.is_none() {
            recommendations.push("No plan workflow record is persisted for this project/session.");
        }
        if strict && !unfinished_todos.is_empty() {
            recommendations.push("Strict mode found unfinished todos; complete or explain them before claiming execution complete.");
        }

        Ok(ToolResult {
            data: json!({
                "plan_path": plan_path,
                "plan_file_exists": plan_path.exists(),
                "workflow": workflow,
                "linked_tasks": tasks,
                "unfinished_todos": unfinished_todos,
                "strict": strict,
                "recommendations": recommendations,
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Verify plan execution state without mutating plan, task, or todo data.".into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRecord {
    workflow_id: String,
    name: String,
    goal: String,
    status: String,
    created_at: String,
    updated_at: String,
    steps: Vec<WorkflowStepRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowStepRecord {
    id: String,
    prompt: String,
    agent_type: Option<String>,
    depends_on: Vec<String>,
    tools: Vec<String>,
    status: String,
}

pub struct WorkflowTool;

fn load_workflow_by_id(workflow_id: &str) -> Result<WorkflowRecord> {
    let path = allthecodes_config::paths::workflow_file_path(workflow_id);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read workflow {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

fn save_workflow(record: &WorkflowRecord) -> Result<()> {
    let path = allthecodes_config::paths::workflow_file_path(&record.workflow_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(record)?)?;
    Ok(())
}

#[async_trait]
impl Tool for WorkflowTool {
    fn name(&self) -> &str {
        "Workflow"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create, inspect, or cancel a durable Rust-native workflow specification.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {"type": "string", "enum": ["start", "status", "cancel"]},
                "workflow_id": {"type": "string"},
                "name": {"type": "string"},
                "goal": {"type": "string"},
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "prompt": {"type": "string"},
                            "agent_type": {"type": "string"},
                            "depends_on": {"type": "array", "items": {"type": "string"}},
                            "tools": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["id", "prompt"]
                    }
                }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if let Some(result) = validate_enum(input, "mode", &["start", "status", "cancel"]) {
            return result;
        }
        let mode = string_param(input, "mode").unwrap_or("start");
        if mode == "start" {
            if string_param(input, "name").is_none() || string_param(input, "goal").is_none() {
                return ValidationResult::Error {
                    message: "name and goal are required when mode=start".into(),
                    error_code: 400,
                };
            }
            if input
                .get("steps")
                .and_then(Value::as_array)
                .map(Vec::is_empty)
                .unwrap_or(true)
            {
                return ValidationResult::Error {
                    message: "steps must contain at least one step when mode=start".into(),
                    error_code: 400,
                };
            }
        } else if string_param(input, "workflow_id").is_none() {
            return ValidationResult::Error {
                message: "workflow_id is required when mode is status or cancel".into(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        match string_param(&input, "mode").unwrap_or("start") {
            "status" => {
                let workflow_id = string_param(&input, "workflow_id").unwrap();
                let record = load_workflow_by_id(workflow_id)?;
                Ok(ToolResult {
                    data: json!({"workflow": record}),
                    ..Default::default()
                })
            }
            "cancel" => {
                let workflow_id = string_param(&input, "workflow_id").unwrap();
                let mut record = load_workflow_by_id(workflow_id)?;
                record.status = "cancelled".into();
                record.updated_at = Utc::now().to_rfc3339();
                for step in &mut record.steps {
                    if step.status == "pending" || step.status == "ready" {
                        step.status = "cancelled".into();
                    }
                }
                save_workflow(&record)?;
                Ok(ToolResult {
                    data: json!({"cancelled": true, "workflow": record}),
                    ..Default::default()
                })
            }
            _ => {
                let steps_value = input
                    .get("steps")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut ids = HashSet::new();
                let mut steps = Vec::new();
                for step in steps_value {
                    let id = string_param(&step, "id")
                        .ok_or_else(|| anyhow!("step.id is required"))?
                        .to_string();
                    if !ids.insert(id.clone()) {
                        bail!("duplicate workflow step id: {id}");
                    }
                    let depends_on = step
                        .get("depends_on")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToOwned::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    let tools = step
                        .get("tools")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToOwned::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    steps.push(WorkflowStepRecord {
                        id,
                        prompt: string_param(&step, "prompt")
                            .ok_or_else(|| anyhow!("step.prompt is required"))?
                            .to_string(),
                        agent_type: string_param(&step, "agent_type").map(ToOwned::to_owned),
                        depends_on,
                        tools,
                        status: "pending".into(),
                    });
                }
                for step in &steps {
                    for dep in &step.depends_on {
                        if !ids.contains(dep) {
                            bail!("workflow step {} depends on unknown step {}", step.id, dep);
                        }
                    }
                }
                for step in &mut steps {
                    if step.depends_on.is_empty() {
                        step.status = "ready".into();
                    }
                }
                let workflow_id = format!("workflow-{}", Uuid::new_v4());
                let now = Utc::now().to_rfc3339();
                let record = WorkflowRecord {
                    workflow_id,
                    name: string_param(&input, "name").unwrap().to_string(),
                    goal: string_param(&input, "goal").unwrap().to_string(),
                    status: "started".into(),
                    created_at: now.clone(),
                    updated_at: now,
                    steps,
                };
                save_workflow(&record)?;
                Ok(ToolResult {
                    data: json!({"started": true, "workflow": record}),
                    ..Default::default()
                })
            }
        }
    }

    async fn prompt(&self) -> String {
        "Use Workflow for durable multi-step workflow specs. Use mode=status or mode=cancel with workflow_id to inspect or stop a workflow.".into()
    }
}

pub struct ApplyPatchTool;

#[derive(Debug, Clone)]
enum PatchOp {
    Add {
        path: PathBuf,
        content: String,
    },
    Delete {
        path: PathBuf,
    },
    Update {
        path: PathBuf,
        move_to: Option<PathBuf>,
        lines: Vec<PatchLine>,
    },
}

#[derive(Debug, Clone)]
enum PatchLine {
    Context(String),
    Remove(String),
    Add(String),
}

fn parse_patch(raw: &str) -> Result<Vec<PatchOp>> {
    let lines = raw.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch") {
        bail!("patch must start with *** Begin Patch");
    }
    if lines.last().copied() != Some("*** End Patch") {
        bail!("patch must end with *** End Patch");
    }
    let mut ops = Vec::new();
    let mut i = 1;
    while i + 1 < lines.len() {
        let line = lines[i];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            i += 1;
            let mut content = String::new();
            while i + 1 < lines.len() && !lines[i].starts_with("*** ") {
                let Some(rest) = lines[i].strip_prefix('+') else {
                    bail!("add file lines must start with +");
                };
                content.push_str(rest);
                content.push('\n');
                i += 1;
            }
            ops.push(PatchOp::Add {
                path: PathBuf::from(path),
                content,
            });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(PatchOp::Delete {
                path: PathBuf::from(path),
            });
            i += 1;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            i += 1;
            let mut move_to = None;
            if i + 1 < lines.len() {
                if let Some(target) = lines[i].strip_prefix("*** Move to: ") {
                    move_to = Some(PathBuf::from(target));
                    i += 1;
                }
            }
            let mut patch_lines = Vec::new();
            while i + 1 < lines.len() && !lines[i].starts_with("*** ") {
                let current = lines[i];
                if current.starts_with("@@") {
                    i += 1;
                    continue;
                }
                let (prefix, text) = current.split_at(1);
                match prefix {
                    " " => patch_lines.push(PatchLine::Context(text.to_string())),
                    "-" => patch_lines.push(PatchLine::Remove(text.to_string())),
                    "+" => patch_lines.push(PatchLine::Add(text.to_string())),
                    _ => bail!("update lines must start with space, -, +, or @@"),
                }
                i += 1;
            }
            ops.push(PatchOp::Update {
                path: PathBuf::from(path),
                move_to,
                lines: patch_lines,
            });
        } else if line.trim().is_empty() {
            i += 1;
        } else {
            bail!("unknown patch section: {line}");
        }
    }
    Ok(ops)
}

fn apply_update(original: &str, patch: &[PatchLine]) -> Result<String> {
    let source = original.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for line in patch {
        match line {
            PatchLine::Context(expected) => {
                let pos = source[cursor..]
                    .iter()
                    .position(|actual| actual == expected)
                    .ok_or_else(|| anyhow!("context line not found: {expected}"))?;
                out.extend_from_slice(&source[cursor..cursor + pos]);
                out.push(expected.clone());
                cursor += pos + 1;
            }
            PatchLine::Remove(expected) => {
                if source.get(cursor) != Some(expected) {
                    bail!(
                        "remove line did not match current file at line {}: {}",
                        cursor + 1,
                        expected
                    );
                }
                cursor += 1;
            }
            PatchLine::Add(text) => out.push(text.clone()),
        }
    }
    out.extend_from_slice(&source[cursor..]);
    let mut text = out.join("\n");
    if original.ends_with('\n') || patch.iter().any(|line| matches!(line, PatchLine::Add(_))) {
        text.push('\n');
    }
    Ok(text)
}

fn ensure_fresh(path: &Path, ctx: &ToolUseContext) -> Result<()> {
    let content = fs::read(path)?;
    let hash = crate::tool::FileStateCache::hash_content(&content);
    let candidates = [
        path.to_string_lossy().to_string(),
        fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    ];
    if candidates
        .iter()
        .filter(|candidate| !candidate.is_empty())
        .any(|candidate| {
            ctx.read_file_state
                .get(candidate)
                .map(|entry| entry.content_hash == hash)
                .unwrap_or(false)
        })
    {
        Ok(())
    } else {
        bail!("refusing to modify {} because it has not been read in this session or has changed since it was read", path.display())
    }
}

fn path_within_allowed(path: &Path, ctx: &ToolUseContext) -> bool {
    let Ok(validated) =
        allthecodes_permissions::path_validation::validate_file_path(&path.to_string_lossy())
    else {
        return false;
    };
    let app_state = (ctx.get_app_state)();
    allthecodes_permissions::path_validation::is_path_within_allowed_directories(
        &validated,
        &current_dir(),
        &app_state.tool_permission_context,
    )
}

fn patch_needs_explicit_permission(ops: &[PatchOp], ctx: &ToolUseContext) -> Option<String> {
    for op in ops {
        match op {
            PatchOp::Add { path, .. } => {
                if !path_within_allowed(path, ctx) {
                    return Some(format!(
                        "add outside allowed directories: {}",
                        path.display()
                    ));
                }
            }
            PatchOp::Delete { path } => {
                return Some(format!("delete file: {}", path.display()));
            }
            PatchOp::Update { path, move_to, .. } => {
                if !path_within_allowed(path, ctx) {
                    return Some(format!(
                        "update outside allowed directories: {}",
                        path.display()
                    ));
                }
                if let Some(target) = move_to {
                    if !path_within_allowed(target, ctx) {
                        return Some(format!(
                            "move target outside allowed directories: {}",
                            target.display()
                        ));
                    }
                    return Some(format!(
                        "move file: {} -> {}",
                        path.display(),
                        target.display()
                    ));
                }
            }
        }
    }
    None
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "ApplyPatch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Apply a Codex patch grammar payload to local files using safe writes.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {"type": "string", "description": "Patch text using *** Begin Patch / *** End Patch grammar."}
            },
            "required": ["patch"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let Some(patch) = string_param(input, "patch") else {
            return ValidationResult::Error {
                message: "patch is required".into(),
                error_code: 400,
            };
        };
        match parse_patch(patch) {
            Ok(_) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionResult {
        let Some(patch) = string_param(input, "patch") else {
            return PermissionResult::Deny {
                message: "patch is required".into(),
            };
        };
        let ops = match parse_patch(patch) {
            Ok(ops) => ops,
            Err(err) => {
                return PermissionResult::Deny {
                    message: err.to_string(),
                }
            }
        };
        if let Some(reason) = patch_needs_explicit_permission(&ops, ctx) {
            return PermissionResult::Ask {
                message: format!("Allow ApplyPatch to {reason}?"),
            };
        }
        PermissionResult::Allow {
            updated_input: input.clone(),
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let patch = string_param(&input, "patch").ok_or_else(|| anyhow!("patch is required"))?;
        let ops = parse_patch(patch)?;
        let mut changed = Vec::new();
        for op in ops {
            match op {
                PatchOp::Add { path, content } => {
                    if path.exists() {
                        bail!(
                            "refusing to add file that already exists: {}",
                            path.display()
                        );
                    }
                    let report = safe_write_text(
                        &path,
                        &content,
                        &SafeWriteOptions {
                            session_id: Some(ctx.session_id.clone()),
                            ..Default::default()
                        },
                    )?;
                    changed.push(
                        json!({"operation": "add", "path": path, "bytes": report.bytes_written}),
                    );
                }
                PatchOp::Delete { path } => {
                    ensure_fresh(&path, ctx)?;
                    fs::remove_file(&path)
                        .with_context(|| format!("failed to delete {}", path.display()))?;
                    ctx.read_file_state.invalidate(&path.to_string_lossy());
                    changed.push(json!({"operation": "delete", "path": path}));
                }
                PatchOp::Update {
                    path,
                    move_to,
                    lines,
                } => {
                    ensure_fresh(&path, ctx)?;
                    let original = fs::read_to_string(&path)
                        .with_context(|| format!("failed to read {}", path.display()))?;
                    let updated = apply_update(&original, &lines)?;
                    let target = move_to.as_ref().unwrap_or(&path);
                    let report = safe_write_text(
                        target,
                        &updated,
                        &SafeWriteOptions {
                            session_id: Some(ctx.session_id.clone()),
                            ..Default::default()
                        },
                    )?;
                    if move_to.is_some() {
                        fs::remove_file(&path).with_context(|| {
                            format!("failed to remove moved source {}", path.display())
                        })?;
                    }
                    ctx.read_file_state.invalidate(&path.to_string_lossy());
                    changed.push(json!({"operation": if move_to.is_some() { "move_update" } else { "update" }, "path": path, "target": target, "bytes": report.bytes_written}));
                }
            }
        }
        Ok(ToolResult {
            data: json!({"changed": changed, "count": changed.len()}),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "ApplyPatch accepts a JSON object with a `patch` string in Codex patch grammar. Read existing files first before update/delete patches.".into()
    }
}

pub struct LocalMemoryRecallTool;

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn snippet_for(content: &str, query: &str) -> String {
    let lower = content.to_ascii_lowercase();
    let first = query
        .split_whitespace()
        .map(|term| term.to_ascii_lowercase())
        .find_map(|term| lower.find(&term));
    let start = first.unwrap_or(0).saturating_sub(120);
    let end = (first.unwrap_or(0) + 360).min(content.len());
    content[start..end].replace('\n', " ")
}

#[async_trait]
impl Tool for LocalMemoryRecallTool {
    fn name(&self) -> &str {
        "LocalMemoryRecall"
    }

    async fn description(&self, _input: &Value) -> String {
        "Search allthecodes memory, auto-memory, and transcripts without reading Codex legacy paths.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "scope": {"type": "string", "enum": ["current_project", "global", "session"]},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 50},
                "since": {"type": "string", "description": "RFC3339 timestamp lower bound."}
            },
            "required": ["query"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "query").is_none() {
            return ValidationResult::Error {
                message: "query is required".into(),
                error_code: 400,
            };
        }
        if let Some(result) =
            validate_enum(input, "scope", &["current_project", "global", "session"])
        {
            return result;
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
        let query = string_param(&input, "query").unwrap();
        let scope = string_param(&input, "scope").unwrap_or("current_project");
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 50) as usize;
        let since = string_param(&input, "since")
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let mut roots = vec![
            allthecodes_config::paths::memory_dir_global(),
            allthecodes_config::paths::auto_memory_dir(),
        ];
        if scope != "global" {
            roots.push(allthecodes_config::paths::transcripts_dir());
        }
        let mut files = Vec::new();
        for root in roots {
            collect_files(&root, &mut files);
        }
        let cwd = current_dir().to_string_lossy().to_string();
        let mut matches = Vec::new();
        for path in files {
            let path_text = path.to_string_lossy();
            if path_text.contains("/.codex/") || path_text.contains("/.Codex/") {
                continue;
            }
            if let Some(since) = since {
                let modified = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .map(DateTime::<Utc>::from);
                if modified.map(|dt| dt < since).unwrap_or(false) {
                    continue;
                }
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if scope == "session"
                && !content.contains(&ctx.session_id)
                && !path_text.contains(&ctx.session_id)
            {
                continue;
            }
            if scope == "current_project"
                && !content.contains(&cwd)
                && !path_text.contains(".allthecodes/memory")
            {
                // Global notes without cwd metadata can still be useful; keep them searchable.
            }
            let score = text_score(query, &[&content, &path_text]);
            if score == 0 {
                continue;
            }
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .map(DateTime::<Utc>::from)
                .map(|dt| dt.to_rfc3339());
            matches.push((
                score,
                json!({
                    "path": path,
                    "timestamp": modified,
                    "score": score,
                    "summary": snippet_for(&content, query),
                }),
            ));
        }
        matches.sort_by(|a, b| b.0.cmp(&a.0));
        matches.truncate(max_results);
        Ok(ToolResult {
            data: json!({
                "query": query,
                "scope": scope,
                "count": matches.len(),
                "results": matches.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Search local allthecodes memory and transcripts for short recalled summaries; use it when prior session context may matter.".into()
    }
}

pub struct VaultHttpFetchTool;

fn host_is_blocked(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(ip) => {
                ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
            }
            IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified(),
        };
    }
    false
}

fn credential_header(ref_name: &str) -> Result<Option<(String, String)>> {
    let path = allthecodes_config::paths::credentials_path();
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let Some(entry) = value.get("vault").and_then(|v| v.get(ref_name)) else {
        return Ok(None);
    };
    if let Some(token) = entry.as_str() {
        return Ok(Some(("authorization".into(), format!("Bearer {token}"))));
    }
    let kind = entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("bearer");
    let token = entry.get("token").and_then(Value::as_str);
    Ok(token.map(|token| {
        if kind.eq_ignore_ascii_case("basic") {
            ("authorization".into(), format!("Basic {token}"))
        } else {
            ("authorization".into(), format!("Bearer {token}"))
        }
    }))
}

fn redact_header(name: &str, value: &str) -> String {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "set-cookie" | "x-api-key"
    ) {
        "[redacted]".into()
    } else {
        value.to_string()
    }
}

#[async_trait]
impl Tool for VaultHttpFetchTool {
    fn name(&self) -> &str {
        "VaultHttpFetch"
    }

    async fn description(&self, _input: &Value) -> String {
        "Fetch HTTPS URLs with optional allthecodes vault credentials and redacted output.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "method": {"type": "string", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"]},
                "headers": {"type": "object", "additionalProperties": {"type": "string"}},
                "body": {"type": "string"},
                "credential_ref": {"type": "string"}
            },
            "required": ["url"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        let Some(raw_url) = string_param(input, "url") else {
            return ValidationResult::Error {
                message: "url is required".into(),
                error_code: 400,
            };
        };
        let Ok(url) = Url::parse(raw_url) else {
            return ValidationResult::Error {
                message: "url must be an absolute URL".into(),
                error_code: 400,
            };
        };
        if url.scheme() != "https" {
            return ValidationResult::Error {
                message: "VaultHttpFetch only allows HTTPS URLs".into(),
                error_code: 400,
            };
        }
        if host_is_blocked(&url) {
            return ValidationResult::Error {
                message:
                    "localhost, private, link-local, and unspecified network targets are blocked"
                        .into(),
                error_code: 400,
            };
        }
        if let Some(result) =
            validate_enum(input, "method", &["GET", "POST", "PUT", "PATCH", "DELETE"])
        {
            return result;
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow VaultHttpFetch to request {}?",
                string_param(input, "url").unwrap_or("<missing url>")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let url = string_param(&input, "url").unwrap();
        let method = string_param(&input, "method").unwrap_or("GET");
        let client = reqwest::Client::new();
        let mut request = client.request(method.parse()?, url);
        if let Some(headers) = input.get("headers").and_then(Value::as_object) {
            for (name, value) in headers {
                if let Some(value) = value.as_str() {
                    request = request.header(name, value);
                }
            }
        }
        if let Some(credential_ref) = string_param(&input, "credential_ref") {
            let Some((name, value)) = credential_header(credential_ref)? else {
                bail!(
                    "credential_ref not found in allthecodes credentials vault: {credential_ref}"
                );
            };
            request = request.header(name, value);
        }
        if let Some(body) = input.get("body").and_then(Value::as_str) {
            request = request.body(body.to_string());
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let name = name.as_str().to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "content-type"
                        | "content-length"
                        | "etag"
                        | "last-modified"
                        | "cache-control"
                        | "www-authenticate"
                ) {
                    Some((
                        name.clone(),
                        redact_header(&name, value.to_str().unwrap_or("<binary>")),
                    ))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();
        let text = response.text().await.unwrap_or_default();
        let preview = if text.len() > 4096 {
            format!("{}…", &text[..4096])
        } else {
            text
        };
        Ok(ToolResult {
            data: json!({
                "status": status,
                "headers": headers,
                "body_preview": preview,
                "body_truncated": preview.len() >= 4096,
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Fetch HTTPS resources with optional allthecodes vault credentials. Never use it for localhost or private network targets unless policy changes explicitly allow that.".into()
    }
}

pub struct PushNotificationTool;

#[async_trait]
impl Tool for PushNotificationTool {
    fn name(&self) -> &str {
        "PushNotification"
    }

    async fn description(&self, _input: &Value) -> String {
        "Send an audited local notification record or HTTPS webhook push notification.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "message": {"type": "string"},
                "priority": {"type": "string", "enum": ["normal", "urgent"]},
                "target": {"type": "string", "description": "local, file, or webhook:https://..."}
            },
            "required": ["title", "message"]
        })
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if string_param(input, "title").is_none() || string_param(input, "message").is_none() {
            return ValidationResult::Error {
                message: "title and message are required".into(),
                error_code: 400,
            };
        }
        if let Some(result) = validate_enum(input, "priority", &["normal", "urgent"]) {
            return result;
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow PushNotification to target {}?",
                string_param(input, "target").unwrap_or("local")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let title = string_param(&input, "title").unwrap();
        let message = string_param(&input, "message").unwrap();
        let priority = string_param(&input, "priority").unwrap_or("normal");
        let target = string_param(&input, "target").unwrap_or("local");
        let target_hash = {
            let mut hasher = Sha256::new();
            hasher.update(target.as_bytes());
            hex::encode(hasher.finalize())
        };
        let record = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "title": title,
            "message": message,
            "priority": priority,
            "target_hash": target_hash,
            "provider": if target.starts_with("webhook:https://") { "webhook" } else { "local" },
        });
        fs::create_dir_all(allthecodes_config::paths::notifications_dir())?;
        let audit_path = allthecodes_config::paths::notifications_dir().join("notifications.jsonl");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&audit_path)?;
        writeln!(file, "{}", serde_json::to_string(&record)?)?;

        let mut delivered = "local";
        if let Some(webhook_url) = target.strip_prefix("webhook:") {
            let url = Url::parse(webhook_url)?;
            if url.scheme() != "https" || host_is_blocked(&url) {
                bail!("webhook target must be public HTTPS");
            }
            reqwest::Client::new()
                .post(webhook_url)
                .json(&json!({"title": title, "message": message, "priority": priority}))
                .send()
                .await?
                .error_for_status()?;
            delivered = "webhook";
        }
        Ok(ToolResult {
            data: json!({
                "sent": true,
                "provider": delivered,
                "audit_path": audit_path,
                "target_hash": target_hash,
            }),
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Send a push notification only after permission. Local provider writes an audit record; webhook targets must be public HTTPS.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{FileStateCache, ToolAppState, ToolUseOptions};
    use allthecodes_types::message::ContentBlock;

    fn test_context(session_id: &str) -> ToolUseContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let app_state = ToolAppState::default();
        ToolUseContext {
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
            get_app_state: Arc::new(move || app_state.clone()),
            set_app_state: Arc::new(|_| {}),
            session_id: session_id.into(),
            langfuse_session_id: session_id.into(),
            messages: vec![],
            agent_id: None,
            agent_type: None,
            query_tracking: None,
            permission_callback: None,
            ask_user_callback: None,
            permission_event_callback: None,
            bg_agent_tx: None,
            hook_runner: Arc::new(allthecodes_types::hooks::NoopHookRunner::new()),
            command_dispatcher: Arc::new(allthecodes_types::commands::NoopCommandDispatcher::new()),
            available_tools: vec![],
            execute_deferred_tool: None,
        }
    }

    fn parent_message() -> AssistantMessage {
        AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: Vec::<ContentBlock>::new(),
            usage: None,
            stop_reason: None,
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        }
    }

    #[tokio::test]
    async fn phase5_goal_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("ALLTHECODES_HOME", tmp.path());
        let ctx = test_context("goal-session");
        let parent = parent_message();

        let created = CreateGoalTool
            .call(
                json!({"objective": "ship phase5", "token_budget": 1000}),
                &ctx,
                &parent,
                None,
            )
            .await
            .unwrap();
        assert_eq!(created.data["created"], true);
        let duplicate = CreateGoalTool
            .call(json!({"objective": "second"}), &ctx, &parent, None)
            .await
            .unwrap();
        assert_eq!(duplicate.data["error"], "active_goal_exists");
        let updated = UpdateGoalTool
            .call(json!({"status": "complete"}), &ctx, &parent, None)
            .await
            .unwrap();
        assert_eq!(updated.data["updated"], true);
    }

    #[test]
    fn phase5_parse_patch_add_update_delete() {
        let patch = "*** Begin Patch\n*** Add File: a.txt\n+hello\n*** Update File: b.txt\n@@\n old\n-old\n+new\n*** Delete File: c.txt\n*** End Patch";
        let ops = parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn phase5_vault_rejects_private_targets() {
        let tool = VaultHttpFetchTool;
        let ctx = test_context("vault-session");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.validate_input(&json!({"url": "https://127.0.0.1/x"}), &ctx));
        assert!(matches!(result, ValidationResult::Error { .. }));
    }
}
