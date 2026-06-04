use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub mod plan;

use crate::common::{current_dir, string_param, task_list_id_for_context, validate_enum};
use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::AssistantMessage;

pub fn tools() -> Tools {
    vec![
        Arc::new(VerifyPlanExecutionTool),
        Arc::new(WorkflowTool),
        Arc::new(WorkflowAliasTool),
    ]
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
                "strict": {"type": "boolean"},
                "plan_summary": {"type": "string", "description": "Compatibility field describing the plan being verified."},
                "verification_notes": {"type": "string", "description": "Compatibility field with model-supplied verification notes."},
                "all_steps_completed": {"type": "boolean", "description": "Compatibility field for the caller's completion claim; allthecodes still performs read-only checks."}
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
        let workflow = crate::workflow::plan::load(&cwd)?;
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
        if input.get("all_steps_completed") == Some(&Value::Bool(false)) {
            recommendations.push(
                "The caller reported all_steps_completed=false; do not claim plan execution complete.",
            );
        }
        let compatibility_claim = if input.get("plan_summary").is_some()
            || input.get("verification_notes").is_some()
            || input.get("all_steps_completed").is_some()
        {
            Some(json!({
                "plan_summary": input.get("plan_summary").cloned(),
                "verification_notes": input.get("verification_notes").cloned(),
                "all_steps_completed": input.get("all_steps_completed").cloned(),
            }))
        } else {
            None
        };

        Ok(ToolResult {
            data: json!({
                "plan_path": plan_path,
                "plan_file_exists": plan_path.exists(),
                "workflow": workflow,
                "linked_tasks": tasks,
                "unfinished_todos": unfinished_todos,
                "strict": strict,
                "compatibility_claim": compatibility_claim,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRunRecord {
    workflow_id: String,
    status: String,
    ready_steps: Vec<String>,
    completed_steps: Vec<String>,
    updated_at: String,
}

pub struct WorkflowTool;
pub struct WorkflowAliasTool;

fn sanitize_workflow_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn project_workflows_dir() -> PathBuf {
    allthecodes_config::paths::project_allthecodes_dir(&current_dir()).join("workflows")
}

fn project_workflow_runs_dir() -> PathBuf {
    allthecodes_config::paths::project_allthecodes_dir(&current_dir()).join("workflow-runs")
}

fn project_workflow_file_path(workflow_id: &str) -> PathBuf {
    project_workflows_dir().join(format!("{}.json", sanitize_workflow_segment(workflow_id)))
}

fn project_workflow_run_file_path(workflow_id: &str) -> PathBuf {
    project_workflow_runs_dir().join(format!("{}.json", sanitize_workflow_segment(workflow_id)))
}

fn load_workflow_by_id(workflow_id: &str) -> Result<WorkflowRecord> {
    let project_path = project_workflow_file_path(workflow_id);
    if project_path.exists() {
        let raw = fs::read_to_string(&project_path)
            .with_context(|| format!("failed to read workflow {}", project_path.display()))?;
        return Ok(serde_json::from_str(&raw)?);
    }

    let path = allthecodes_config::paths::workflow_file_path(workflow_id);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read workflow {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

fn save_workflow(record: &WorkflowRecord) -> Result<PathBuf> {
    let path = project_workflow_file_path(&record.workflow_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(record)?)?;
    Ok(path)
}

fn workflow_run_from_record(record: &WorkflowRecord) -> WorkflowRunRecord {
    WorkflowRunRecord {
        workflow_id: record.workflow_id.clone(),
        status: record.status.clone(),
        ready_steps: record
            .steps
            .iter()
            .filter(|step| step.status == "ready")
            .map(|step| step.id.clone())
            .collect(),
        completed_steps: record
            .steps
            .iter()
            .filter(|step| step.status == "completed")
            .map(|step| step.id.clone())
            .collect(),
        updated_at: record.updated_at.clone(),
    }
}

fn save_workflow_run(record: &WorkflowRecord) -> Result<PathBuf> {
    let path = project_workflow_run_file_path(&record.workflow_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&workflow_run_from_record(record))?,
    )?;
    Ok(path)
}

fn load_workflow_run(workflow_id: &str) -> Result<Option<WorkflowRunRecord>> {
    let path = project_workflow_run_file_path(workflow_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read workflow run {}", path.display()))?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn list_project_workflows() -> Result<Vec<WorkflowRecord>> {
    let dir = project_workflows_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(entry.path())?;
        let record: WorkflowRecord = serde_json::from_str(&raw)?;
        records.push(record);
    }
    records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(records)
}

fn workflow_action(input: &Value) -> Result<&str> {
    let action = string_param(input, "action");
    let mode = string_param(input, "mode");
    if let (Some(action), Some(mode)) = (action, mode) {
        if action != mode {
            bail!("workflow action and legacy mode must match when both are provided");
        }
    }
    Ok(action.or(mode).unwrap_or("start"))
}

fn workflow_step_ids(input: &Value) -> Vec<String> {
    input
        .get("steps")
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .filter_map(|step| string_param(step, "id").map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn preview_tool_result(data: Value, preview: impl Into<String>) -> ToolResult {
    let preview = preview.into();
    ToolResult {
        data,
        display_preview: Some(preview),
        ..Default::default()
    }
}

fn workflow_progress_summary(record: &WorkflowRecord) -> String {
    let completed = record
        .steps
        .iter()
        .filter(|step| step.status == "completed")
        .count();
    let ready = record
        .steps
        .iter()
        .filter(|step| step.status == "ready")
        .count();
    format!(
        "Workflow {} '{}' is {}; {}/{} step(s) completed, {} ready",
        record.workflow_id,
        record.name,
        record.status,
        completed,
        record.steps.len(),
        ready
    )
}

fn update_ready_workflow_steps(record: &mut WorkflowRecord) {
    let completed = record
        .steps
        .iter()
        .filter(|step| step.status == "completed")
        .map(|step| step.id.clone())
        .collect::<HashSet<_>>();
    for step in &mut record.steps {
        if step.status == "pending" && step.depends_on.iter().all(|dep| completed.contains(dep)) {
            step.status = "ready".into();
        }
    }
    if record.steps.iter().all(|step| step.status == "completed") {
        record.status = "completed".into();
    }
}

fn advance_workflow(input: &Value) -> Result<(WorkflowRecord, String, PathBuf, PathBuf)> {
    let workflow_id = string_param(input, "workflow_id")
        .ok_or_else(|| anyhow!("Missing required parameter: workflow_id"))?;
    let mut record = load_workflow_by_id(workflow_id)?;
    if matches!(record.status.as_str(), "cancelled" | "completed" | "failed") {
        bail!(
            "workflow {} is already in final status {}",
            record.workflow_id,
            record.status
        );
    }

    let step_id = string_param(input, "step_id")
        .map(ToOwned::to_owned)
        .or_else(|| {
            record
                .steps
                .iter()
                .find(|step| step.status == "ready")
                .map(|step| step.id.clone())
        })
        .ok_or_else(|| anyhow!("step_id is required when no workflow step is ready"))?;
    let next_status = string_param(input, "step_status")
        .or_else(|| string_param(input, "status"))
        .unwrap_or("completed");
    if !matches!(next_status, "completed" | "failed" | "cancelled") {
        bail!("step_status must be completed, failed, or cancelled");
    }

    let now = Utc::now().to_rfc3339();
    let Some(step) = record.steps.iter_mut().find(|step| step.id == step_id) else {
        bail!("workflow {} has no step {}", record.workflow_id, step_id);
    };
    if !matches!(step.status.as_str(), "ready" | "running") {
        bail!(
            "workflow step {} is {}, not ready to advance",
            step.id,
            step.status
        );
    }
    step.status = next_status.to_string();
    step.result = string_param(input, "result").map(ToOwned::to_owned);
    step.updated_at = Some(now.clone());
    record.updated_at = now;
    if next_status == "failed" {
        record.status = "failed".into();
    } else if next_status == "cancelled" {
        record.status = "cancelled".into();
    } else {
        update_ready_workflow_steps(&mut record);
    }

    let workflow_path = save_workflow(&record)?;
    let run_path = save_workflow_run(&record)?;
    Ok((record, step_id, workflow_path, run_path))
}

#[async_trait]
impl Tool for WorkflowTool {
    fn name(&self) -> &str {
        "Workflow"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create, list, inspect, advance, or cancel a project-local workflow.".into()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["start", "status", "advance", "cancel", "list"], "description": "Preferred compatibility field."},
                "mode": {"type": "string", "enum": ["start", "status", "advance", "cancel", "list"], "description": "Legacy allthecodes alias for action."},
                "workflow_id": {"type": "string"},
                "name": {"type": "string"},
                "goal": {"type": "string"},
                "step_id": {"type": "string"},
                "step_status": {"type": "string", "enum": ["completed", "failed", "cancelled"]},
                "result": {"type": "string"},
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

    fn is_read_only(&self, input: &Value) -> bool {
        matches!(workflow_action(input).unwrap_or("start"), "list" | "status")
    }

    fn is_destructive(&self, input: &Value) -> bool {
        workflow_action(input).unwrap_or("start") == "cancel"
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if let Some(result) = validate_enum(
            input,
            "action",
            &["start", "status", "advance", "cancel", "list"],
        ) {
            return result;
        }
        if let Some(result) = validate_enum(
            input,
            "mode",
            &["start", "status", "advance", "cancel", "list"],
        ) {
            return result;
        }
        if let (Some(action), Some(mode)) =
            (string_param(input, "action"), string_param(input, "mode"))
        {
            if action != mode {
                return ValidationResult::Error {
                    message: "workflow action and legacy mode must match when both are provided"
                        .into(),
                    error_code: 400,
                };
            }
        }
        let action = string_param(input, "action")
            .or_else(|| string_param(input, "mode"))
            .unwrap_or("start");
        if action == "start" {
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
        } else if action != "list" && string_param(input, "workflow_id").is_none() {
            return ValidationResult::Error {
                message: "workflow_id is required when action is status, advance, or cancel".into(),
                error_code: 400,
            };
        }
        if let Some(result) =
            validate_enum(input, "step_status", &["completed", "failed", "cancelled"])
        {
            return result;
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        match workflow_action(input) {
            Ok("list" | "status") => PermissionResult::Allow {
                updated_input: input.clone(),
            },
            Ok("start") => {
                let name = string_param(input, "name").unwrap_or("<missing name>");
                let goal = string_param(input, "goal").unwrap_or("<missing goal>");
                let step_count = input
                    .get("steps")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                PermissionResult::Ask {
                    message: format!(
                        "Allow Workflow start for '{name}' with {step_count} step(s)? Goal: {goal}"
                    ),
                }
            }
            Ok("advance") => PermissionResult::Ask {
                message: format!(
                    "Allow Workflow advance for {} step {} to {}?",
                    string_param(input, "workflow_id").unwrap_or("<missing workflow_id>"),
                    string_param(input, "step_id").unwrap_or("<next ready step>"),
                    string_param(input, "step_status")
                        .or_else(|| string_param(input, "status"))
                        .unwrap_or("completed")
                ),
            },
            Ok("cancel") => PermissionResult::Ask {
                message: format!(
                    "Allow Workflow cancel for {}?",
                    string_param(input, "workflow_id").unwrap_or("<missing workflow_id>")
                ),
            },
            Ok(other) => PermissionResult::Deny {
                message: format!("unsupported workflow action: {other}"),
            },
            Err(err) => PermissionResult::Deny {
                message: err.to_string(),
            },
        }
    }

    fn to_auto_classifier_input(&self, input: &Value) -> Value {
        let action = workflow_action(input).unwrap_or("start");
        let step_ids = workflow_step_ids(input);
        json!({
            "operation": "workflow",
            "action": action,
            "workflow_id": string_param(input, "workflow_id"),
            "name": string_param(input, "name"),
            "goal": string_param(input, "goal"),
            "step_id": string_param(input, "step_id"),
            "step_status": string_param(input, "step_status").or_else(|| string_param(input, "status")),
            "step_count": input.get("steps").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "step_ids": step_ids,
            "has_result": string_param(input, "result").is_some(),
        })
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        match workflow_action(&input)? {
            "list" => {
                let records = list_project_workflows()?;
                let workflows = records
                    .iter()
                    .map(|record| {
                        json!({
                            "workflow_id": record.workflow_id,
                            "name": record.name,
                            "status": record.status,
                            "updated_at": record.updated_at,
                            "ready_steps": record.steps.iter().filter(|step| step.status == "ready").count(),
                            "completed_steps": record.steps.iter().filter(|step| step.status == "completed").count(),
                            "total_steps": record.steps.len(),
                        })
                    })
                    .collect::<Vec<_>>();
                let count = workflows.len();
                Ok(preview_tool_result(
                    json!({
                        "workflows": workflows,
                        "workflow_dir": project_workflows_dir().display().to_string(),
                    }),
                    format!("Listed {count} project workflow(s)"),
                ))
            }
            "status" => {
                let workflow_id = string_param(&input, "workflow_id")
                    .ok_or_else(|| anyhow!("workflow_id is required for workflow status"))?;
                let record = load_workflow_by_id(workflow_id)?;
                let run = load_workflow_run(workflow_id)?;
                let preview = workflow_progress_summary(&record);
                Ok(preview_tool_result(
                    json!({
                        "workflow": record,
                        "run": run,
                        "workflow_path": project_workflow_file_path(workflow_id).display().to_string(),
                        "run_path": project_workflow_run_file_path(workflow_id).display().to_string(),
                    }),
                    preview,
                ))
            }
            "advance" => {
                let (record, step_id, workflow_path, run_path) = advance_workflow(&input)?;
                let run = workflow_run_from_record(&record);
                let preview = format!(
                    "Advanced workflow {} step {}; {}",
                    record.workflow_id,
                    step_id,
                    workflow_progress_summary(&record)
                );
                Ok(preview_tool_result(
                    json!({
                        "advanced": true,
                        "advanced_step_id": step_id,
                        "workflow": record,
                        "run": run,
                        "workflow_path": workflow_path.display().to_string(),
                        "run_path": run_path.display().to_string(),
                    }),
                    preview,
                ))
            }
            "cancel" => {
                let workflow_id = string_param(&input, "workflow_id")
                    .ok_or_else(|| anyhow!("workflow_id is required for workflow cancel"))?;
                let mut record = load_workflow_by_id(workflow_id)?;
                record.status = "cancelled".into();
                record.updated_at = Utc::now().to_rfc3339();
                for step in &mut record.steps {
                    if step.status == "pending"
                        || step.status == "ready"
                        || step.status == "running"
                    {
                        step.status = "cancelled".into();
                        step.updated_at = Some(record.updated_at.clone());
                    }
                }
                let workflow_path = save_workflow(&record)?;
                let run_path = save_workflow_run(&record)?;
                let run = workflow_run_from_record(&record);
                let preview = workflow_progress_summary(&record);
                Ok(preview_tool_result(
                    json!({
                        "cancelled": true,
                        "workflow": record,
                        "run": run,
                        "workflow_path": workflow_path.display().to_string(),
                        "run_path": run_path.display().to_string(),
                    }),
                    preview,
                ))
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
                        result: None,
                        updated_at: None,
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
                    name: string_param(&input, "name")
                        .ok_or_else(|| anyhow!("Missing required parameter: name"))?
                        .to_string(),
                    goal: string_param(&input, "goal")
                        .ok_or_else(|| anyhow!("Missing required parameter: goal"))?
                        .to_string(),
                    status: "started".into(),
                    created_at: now.clone(),
                    updated_at: now,
                    steps,
                };
                let workflow_path = save_workflow(&record)?;
                let run_path = save_workflow_run(&record)?;
                let run = workflow_run_from_record(&record);
                let preview = workflow_progress_summary(&record);
                Ok(preview_tool_result(
                    json!({
                        "started": true,
                        "workflow": record,
                        "run": run,
                        "workflow_path": workflow_path.display().to_string(),
                        "run_path": run_path.display().to_string(),
                    }),
                    preview,
                ))
            }
        }
    }

    async fn prompt(&self) -> String {
        "Use workflow action=start/status/advance/cancel/list for durable project-local workflows stored under .allthecodes/workflows and .allthecodes/workflow-runs."
            .into()
    }
}

crate::common::tool_alias!(WorkflowAliasTool, "workflow", WorkflowTool);
