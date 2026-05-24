use std::fs;
use std::path::Path;

use allthecodes_types::plan_workflow::{PlanWorkflowRecord, PlanWorkflowStatus};
use anyhow::{Context, Result};

use crate::tool::{PermissionMode, ToolAppState};

const DEFAULT_OWNER: &str = "main";

pub fn default_owner() -> &'static str {
    DEFAULT_OWNER
}

pub fn load(cwd: &Path) -> Result<Option<PlanWorkflowRecord>> {
    let path = allthecodes_config::paths::current_plan_workflow_file_path(cwd);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read plan workflow: {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse plan workflow: {}", path.display()))?;
    Ok(Some(record))
}

pub fn persist(cwd: &Path, record: &PlanWorkflowRecord) -> Result<()> {
    let path = allthecodes_config::paths::current_plan_workflow_file_path(cwd);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create plan workflow dir: {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(record)?;
    fs::write(&path, raw)
        .with_context(|| format!("failed to write plan workflow: {}", path.display()))
}

pub fn enter_plan_mode_state(
    app_state: &mut ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    description: Option<&str>,
    classifier_reason: Option<&str>,
) -> PlanWorkflowRecord {
    if app_state.tool_permission_context.mode != PermissionMode::Plan {
        app_state.tool_permission_context.pre_plan_mode =
            Some(app_state.tool_permission_context.mode.clone());
        allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
            &mut app_state.tool_permission_context,
            PermissionMode::Plan,
        );
    }

    let plan_file = allthecodes_config::paths::current_plan_file_path(cwd);
    let mut record = app_state
        .plan_workflow
        .clone()
        .or(existing)
        .unwrap_or_else(|| {
            PlanWorkflowRecord::new(display_path(&plan_file), Some(owner.to_string()), source)
        });

    record.file_path = display_path(&plan_file);
    if record.owner.is_none() {
        record.owner = Some(owner.to_string());
    }
    record.enter_plan_mode(source, description, classifier_reason);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn request_approval_state(
    app_state: &mut ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    plan_text: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    record.request_approval(source, plan_text);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn approve_and_exit_state(
    app_state: &mut ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    plan_text: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    if plan_text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
    {
        record.plan_text = plan_text;
    }
    record.approve(source);

    let restore_mode = app_state
        .tool_permission_context
        .pre_plan_mode
        .take()
        .unwrap_or(PermissionMode::Default);
    allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
        &mut app_state.tool_permission_context,
        restore_mode,
    );
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn maybe_link_implementation_task_state(
    app_state: &mut ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    task_id: String,
    summary: Option<String>,
) -> Option<PlanWorkflowRecord> {
    let record = app_state.plan_workflow.clone().or(existing)?;
    if !matches!(
        record.status,
        PlanWorkflowStatus::Approved | PlanWorkflowStatus::Implementing
    ) {
        return None;
    }

    Some(link_task_state(
        app_state,
        cwd,
        Some(record),
        owner,
        source,
        task_id,
        summary,
    ))
}

fn link_task_state(
    app_state: &mut ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    task_id: String,
    summary: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    record.link_task(source, task_id, summary);
    app_state.plan_workflow = Some(record.clone());
    record
}

fn ensure_record(
    app_state: &ToolAppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
) -> PlanWorkflowRecord {
    let plan_file = allthecodes_config::paths::current_plan_file_path(cwd);
    let mut record = app_state
        .plan_workflow
        .clone()
        .or(existing)
        .unwrap_or_else(|| {
            PlanWorkflowRecord::new(display_path(&plan_file), Some(owner.to_string()), source)
        });
    record.file_path = display_path(&plan_file);
    if record.owner.is_none() {
        record.owner = Some(owner.to_string());
    }
    record
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
