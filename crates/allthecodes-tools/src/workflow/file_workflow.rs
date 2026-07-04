use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use allthecodes_tasks::{TaskCreateOptions, TaskStatus, TASK_KIND_LOCAL_WORKFLOW};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::common::string_param;
use crate::tool::ToolResult;

pub const WORKFLOW_EXTENSIONS: &[&str] = &["md", "yaml", "yml"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedStep {
    pub name: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedWorkflow {
    pub steps: Vec<ParsedStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileWorkflowStep {
    pub name: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

impl FileWorkflowStep {
    fn pending(step: ParsedStep) -> Self {
        Self {
            name: step.name,
            prompt: step.prompt,
            run: step.run,
            status: "pending".into(),
            started_at: None,
            completed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWorkflowRunRecord {
    pub run_id: String,
    pub workflow: String,
    pub workflow_file: String,
    pub workflow_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step_index: Option<usize>,
    pub steps: Vec<FileWorkflowStep>,
    pub created_at: String,
    pub updated_at: String,
}

impl FileWorkflowRunRecord {
    fn current_step(&self) -> Option<&FileWorkflowStep> {
        self.current_step_index
            .and_then(|index| self.steps.get(index))
    }

    fn completed_step_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == "completed")
            .count()
    }

    fn refresh_status(&mut self) {
        self.updated_at = Utc::now().to_rfc3339();
        if self.steps.iter().all(|step| step.status == "completed") {
            self.status = "completed".into();
            self.current_step_index = None;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileWorkflowScriptSummary {
    pub name: String,
    pub file: String,
    pub path: String,
    pub step_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

pub struct MarkdownWorkflowParser;

impl MarkdownWorkflowParser {
    pub fn parse(content: &str) -> Result<ParsedWorkflow> {
        let mut steps = Vec::new();
        let mut seen = HashSet::new();
        let mut in_code_block = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block || trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(step) = Self::parse_step_line(trimmed) {
                if seen.insert(step.name.clone()) {
                    steps.push(step);
                }
            }
        }

        if steps.is_empty() {
            let prompt = content.trim();
            if !prompt.is_empty() {
                steps.push(ParsedStep {
                    name: "Execute workflow".into(),
                    prompt: prompt.to_string(),
                    run: None,
                });
            }
        }

        Ok(ParsedWorkflow { steps })
    }

    fn parse_step_line(line: &str) -> Option<ParsedStep> {
        let name = if line.starts_with("- [") {
            Self::match_checkbox(line)?
        } else {
            Self::match_bullet(line).or_else(|| Self::match_numbered(line))?
        };

        Some(ParsedStep {
            prompt: name.clone(),
            name,
            run: None,
        })
    }

    fn match_checkbox(line: &str) -> Option<String> {
        let rest = line.strip_prefix("- [")?;
        let mut chars = rest.chars();
        let status = chars.next()?;
        if chars.next()? != ']' {
            return None;
        }
        let text = chars.as_str().trim();
        if matches!(status, 'x' | 'X') || text.is_empty() {
            return None;
        }
        if status == ' ' {
            return Some(text.to_string());
        }
        None
    }

    fn match_bullet(line: &str) -> Option<String> {
        for prefix in ["- ", "* ", "+ "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                let name = rest.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
        None
    }

    fn match_numbered(line: &str) -> Option<String> {
        let mut chars = line.char_indices();
        let mut number_end = None;
        for (idx, ch) in &mut chars {
            if ch.is_ascii_digit() {
                number_end = Some(idx + ch.len_utf8());
                continue;
            }
            if matches!(ch, '.' | ')') && number_end == Some(idx) {
                let rest = &line[idx + ch.len_utf8()..];
                if rest.starts_with(char::is_whitespace) {
                    let name = rest.trim();
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
            break;
        }
        None
    }
}

pub struct YamlWorkflowParser;

impl YamlWorkflowParser {
    pub fn parse(content: &str) -> Result<ParsedWorkflow> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(content).with_context(|| "failed to parse workflow YAML")?;
        let sequence = match &value {
            serde_yaml::Value::Sequence(sequence) => Some(sequence),
            serde_yaml::Value::Mapping(mapping) => mapping
                .get(serde_yaml_key("steps"))
                .or_else(|| mapping.get(serde_yaml_key("workflow")))
                .and_then(serde_yaml::Value::as_sequence),
            _ => None,
        };

        let mut steps = Vec::new();
        if let Some(sequence) = sequence {
            for (index, value) in sequence.iter().enumerate() {
                if let Some(step) = Self::parse_step(index, value)? {
                    steps.push(step);
                }
            }
        }

        if steps.is_empty() {
            let prompt = content.trim();
            if !prompt.is_empty() {
                steps.push(ParsedStep {
                    name: "Execute workflow".into(),
                    prompt: prompt.to_string(),
                    run: None,
                });
            }
        }

        Ok(ParsedWorkflow { steps })
    }

    fn parse_step(index: usize, value: &serde_yaml::Value) -> Result<Option<ParsedStep>> {
        match value {
            serde_yaml::Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return Ok(None);
                }
                Ok(Some(ParsedStep {
                    name: text.to_string(),
                    prompt: text.to_string(),
                    run: None,
                }))
            }
            serde_yaml::Value::Mapping(mapping) => {
                let run = yaml_string(mapping, "run").or_else(|| yaml_string(mapping, "command"));
                let prompt = yaml_string(mapping, "prompt")
                    .or_else(|| yaml_string(mapping, "description"))
                    .or_else(|| yaml_string(mapping, "name"))
                    .or_else(|| yaml_string(mapping, "title"))
                    .unwrap_or_else(|| format!("Step {}", index + 1));
                let name = yaml_string(mapping, "name")
                    .or_else(|| yaml_string(mapping, "title"))
                    .unwrap_or_else(|| first_prompt_line(&prompt, index));
                Ok(Some(ParsedStep { name, prompt, run }))
            }
            _ => Ok(None),
        }
    }
}

fn serde_yaml_key(key: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(key.to_string())
}

fn yaml_string(mapping: &serde_yaml::Mapping, key: &str) -> Option<String> {
    mapping
        .get(serde_yaml_key(key))
        .and_then(serde_yaml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn first_prompt_line(prompt: &str, index: usize) -> String {
    prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Step {}", index + 1))
}

pub fn workflow_scripts_dir(cwd: &Path) -> PathBuf {
    allthecodes_config::paths::project_allthecodes_dir(cwd).join("workflows")
}

pub fn workflow_runs_dir(cwd: &Path) -> PathBuf {
    allthecodes_config::paths::project_allthecodes_dir(cwd).join("workflow-runs")
}

pub fn is_valid_workflow_file(path: &Path) -> bool {
    path.is_file()
        && !hidden_file(path)
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| WORKFLOW_EXTENSIONS.contains(&extension))
            .unwrap_or(false)
}

pub fn parse_workflow_script(path: &Path) -> Result<ParsedWorkflow> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow script {}", path.display()))?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => MarkdownWorkflowParser::parse(&content),
        Some("yaml" | "yml") => YamlWorkflowParser::parse(&content),
        Some(extension) => bail!("unsupported workflow script extension: {extension}"),
        None => bail!("workflow script has no extension: {}", path.display()),
    }
}

pub fn list_workflow_scripts(cwd: &Path) -> Result<Vec<FileWorkflowScriptSummary>> {
    let dir = workflow_scripts_dir(cwd);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut scripts = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !is_valid_workflow_file(&path) {
            continue;
        }
        let file = path
            .file_name()
            .and_then(|file| file.to_str())
            .unwrap_or_default()
            .to_string();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        let (step_count, parse_error) = match parse_workflow_script(&path) {
            Ok(parsed) => (parsed.steps.len(), None),
            Err(err) => (0, Some(err.to_string())),
        };
        scripts.push(FileWorkflowScriptSummary {
            name,
            file,
            path: path.display().to_string(),
            step_count,
            parse_error,
        });
    }
    scripts.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.file.cmp(&b.file)));
    Ok(scripts)
}

pub fn call_file_workflow(input: &Value, cwd: &Path) -> Result<ToolResult> {
    match file_workflow_action(input)? {
        "list" => list_tool_result(cwd),
        "start" => {
            let workflow = string_param(input, "workflow")
                .ok_or_else(|| anyhow!("workflow is required when action=start"))?;
            let args = input.get("args").cloned();
            let (record, path) = start_file_workflow(cwd, workflow, args)?;
            Ok(run_tool_result("start", record, path))
        }
        "status" => {
            let run_id = string_param(input, "run_id")
                .ok_or_else(|| anyhow!("run_id is required when action=status"))?;
            let (record, path) = load_file_workflow_run(cwd, run_id)?;
            Ok(run_tool_result("status", record, path))
        }
        "advance" => {
            let run_id = string_param(input, "run_id")
                .ok_or_else(|| anyhow!("run_id is required when action=advance"))?;
            let applied_status = string_param(input, "applied_status")
                .or_else(|| string_param(input, "step_status"))
                .or_else(|| string_param(input, "status"));
            let (record, path) = advance_file_workflow(cwd, run_id, applied_status)?;
            Ok(run_tool_result("advance", record, path))
        }
        "cancel" => {
            let run_id = string_param(input, "run_id")
                .ok_or_else(|| anyhow!("run_id is required when action=cancel"))?;
            let (record, path) = cancel_file_workflow(cwd, run_id)?;
            Ok(run_tool_result("cancel", record, path))
        }
        action => bail!("unsupported file workflow action: {action}"),
    }
}

pub fn file_workflow_action(input: &Value) -> Result<&str> {
    let action = string_param(input, "action");
    let mode = string_param(input, "mode");
    if let (Some(action), Some(mode)) = (action, mode) {
        if action != mode {
            bail!("workflow action and legacy mode must match when both are provided");
        }
    }
    Ok(action.or(mode).unwrap_or("start"))
}

pub fn file_workflow_action_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {"type": "string", "enum": ["start", "status", "advance", "cancel", "list"]},
            "mode": {"type": "string", "enum": ["start", "status", "advance", "cancel", "list"]},
            "workflow_script": {"type": "boolean"},
            "workflow": {"type": "string"},
            "run_id": {"type": "string"},
            "args": {"type": "object"},
            "applied_status": {"type": "string", "enum": ["completed", "failed", "cancelled"]},
            "step_status": {"type": "string", "enum": ["completed", "failed", "cancelled"]}
        }
    })
}

pub fn start_file_workflow(
    cwd: &Path,
    workflow: &str,
    args: Option<Value>,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let script = resolve_workflow_script(cwd, workflow)?;
    let parsed = parse_workflow_script(&script)?;
    if parsed.steps.is_empty() {
        bail!(
            "workflow script {} has no executable steps",
            script.display()
        );
    }

    let now = Utc::now().to_rfc3339();
    let mut steps = parsed
        .steps
        .into_iter()
        .map(FileWorkflowStep::pending)
        .collect::<Vec<_>>();
    if let Some(first) = steps.first_mut() {
        first.status = "running".into();
        first.started_at = Some(now.clone());
    }

    let workflow_name = script
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(workflow)
        .to_string();
    let workflow_file = script
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or(workflow)
        .to_string();
    let run_id = format!("workflow-run-{}", Uuid::new_v4());
    let record = FileWorkflowRunRecord {
        run_id,
        workflow: workflow_name,
        workflow_file,
        workflow_path: script.display().to_string(),
        args,
        status: "running".into(),
        current_step_index: Some(0),
        steps,
        created_at: now.clone(),
        updated_at: now,
    };
    let path = save_file_workflow_run(cwd, &record)?;
    sync_file_workflow_task(&record, &path)?;
    Ok((record, path))
}

pub fn load_file_workflow_run(
    cwd: &Path,
    run_id: &str,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let path = workflow_run_file(cwd, run_id);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read workflow run {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse workflow run {}", path.display()))?;
    sync_file_workflow_task(&record, &path)?;
    Ok((record, path))
}

pub fn advance_file_workflow(
    cwd: &Path,
    run_id: &str,
    applied_status: Option<&str>,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let (mut record, _) = load_file_workflow_run(cwd, run_id)?;
    if matches!(record.status.as_str(), "completed" | "cancelled" | "failed") {
        bail!(
            "workflow run {} is already in final status {}",
            record.run_id,
            record.status
        );
    }
    let applied_status = applied_status.unwrap_or("completed");
    if !matches!(applied_status, "completed" | "failed" | "cancelled") {
        bail!("applied_status must be completed, failed, or cancelled");
    }

    let index = active_step_index(&record)
        .ok_or_else(|| anyhow!("workflow run {} has no active step", record.run_id))?;
    let now = Utc::now().to_rfc3339();
    record.steps[index].status = applied_status.to_string();
    record.steps[index].completed_at = Some(now.clone());

    match applied_status {
        "completed" => {
            if let Some(next_index) = record
                .steps
                .iter()
                .position(|step| step.status == "pending")
            {
                record.steps[next_index].status = "running".into();
                record.steps[next_index].started_at = Some(now);
                record.current_step_index = Some(next_index);
                record.status = "running".into();
            } else {
                record.current_step_index = None;
                record.status = "completed".into();
            }
        }
        "failed" => {
            record.current_step_index = None;
            record.status = "failed".into();
        }
        "cancelled" => {
            record.current_step_index = None;
            record.status = "cancelled".into();
        }
        _ => unreachable!("validated applied_status"),
    }
    record.refresh_status();
    let path = save_file_workflow_run(cwd, &record)?;
    sync_file_workflow_task(&record, &path)?;
    Ok((record, path))
}

pub fn cancel_file_workflow(cwd: &Path, run_id: &str) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let (mut record, _) = load_file_workflow_run(cwd, run_id)?;
    let now = Utc::now().to_rfc3339();
    for step in &mut record.steps {
        if matches!(step.status.as_str(), "pending" | "running" | "ready") {
            step.status = "cancelled".into();
            step.completed_at = Some(now.clone());
        }
    }
    record.current_step_index = None;
    record.status = "cancelled".into();
    record.updated_at = now;
    let path = save_file_workflow_run(cwd, &record)?;
    sync_file_workflow_task(&record, &path)?;
    Ok((record, path))
}

fn sync_file_workflow_task(record: &FileWorkflowRunRecord, run_path: &Path) -> Result<()> {
    let status = task_status_for_run(&record.status);
    let current_step = record.current_step();
    let current_step_number = record
        .current_step_index
        .map(|index| index.saturating_add(1));
    let metadata = json!({
        "workflow_name": &record.workflow,
        "workflow_file": &record.workflow_file,
        "workflow_path": &record.workflow_path,
        "run_id": &record.run_id,
        "run_path": run_path.display().to_string(),
        "current_step_index": record.current_step_index,
        "current_step_number": current_step_number,
        "current_step_name": current_step.map(|step| step.name.clone()),
        "total_steps": record.steps.len(),
        "completed_steps": record.completed_step_count(),
        "run_status": &record.status,
    });
    let subject = format!("Workflow: {}", record.workflow);
    let output = workflow_task_output(record, run_path);
    let options = TaskCreateOptions {
        kind: Some(TASK_KIND_LOCAL_WORKFLOW.to_string()),
        metadata: Some(metadata),
        ..Default::default()
    };
    allthecodes_tasks::global_store().try_upsert_with_id(
        &record.run_id,
        &subject,
        &record.workflow_path,
        status,
        &output,
        options,
    )?;
    Ok(())
}

fn task_status_for_run(status: &str) -> TaskStatus {
    match status {
        "running" => TaskStatus::InProgress,
        "completed" => TaskStatus::Completed,
        "failed" => TaskStatus::Failed,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Pending,
    }
}

fn workflow_task_output(record: &FileWorkflowRunRecord, run_path: &Path) -> String {
    let mut lines = vec![
        format!("Workflow: {}", record.workflow),
        format!("Run id: {}", record.run_id),
        format!("Status: {}", record.status),
        format!("File: {}", record.workflow_file),
        format!("Path: {}", record.workflow_path),
        format!("Run record: {}", run_path.display()),
        format!(
            "Progress: {}/{} completed",
            record.completed_step_count(),
            record.steps.len()
        ),
    ];
    if let (Some(index), Some(step)) = (record.current_step_index, record.current_step()) {
        lines.push(format!(
            "Current step: {}/{} {}",
            index.saturating_add(1),
            record.steps.len(),
            step.name
        ));
    } else {
        lines.push("Current step: none".to_string());
    }
    lines.push("Steps:".to_string());
    for (index, step) in record.steps.iter().enumerate() {
        lines.push(format!(
            "{}. {} [{}]",
            index.saturating_add(1),
            step.name,
            step.status
        ));
    }
    lines.join("\n")
}

fn active_step_index(record: &FileWorkflowRunRecord) -> Option<usize> {
    record
        .current_step_index
        .filter(|index| *index < record.steps.len())
        .or_else(|| {
            record
                .steps
                .iter()
                .position(|step| matches!(step.status.as_str(), "running" | "ready"))
        })
}

fn save_file_workflow_run(cwd: &Path, record: &FileWorkflowRunRecord) -> Result<PathBuf> {
    let path = workflow_run_file(cwd, &record.run_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_string_pretty(record)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn list_tool_result(cwd: &Path) -> Result<ToolResult> {
    let scripts = list_workflow_scripts(cwd)?;
    let preview = if scripts.is_empty() {
        format!(
            "No workflow scripts found in {}",
            workflow_scripts_dir(cwd).display()
        )
    } else {
        format!("Listed {} workflow script(s)", scripts.len())
    };
    Ok(ToolResult {
        data: json!({
            "workflow_script": true,
            "action": "list",
            "workflow_dir": workflow_scripts_dir(cwd).display().to_string(),
            "workflow_scripts": scripts,
        }),
        display_preview: Some(preview),
        ..Default::default()
    })
}

fn run_tool_result(action: &str, record: FileWorkflowRunRecord, run_path: PathBuf) -> ToolResult {
    let current_step = record.current_step().map(|step| {
        json!({
            "index": record.current_step_index,
            "name": step.name,
            "prompt": step.prompt,
            "run": step.run,
            "status": step.status,
        })
    });
    let model_text = model_instruction(action, &record);
    let preview = if let Some(step) = record.current_step() {
        format!(
            "Workflow script '{}' run {} is {}; current step: {}",
            record.workflow, record.run_id, record.status, step.name
        )
    } else {
        format!(
            "Workflow script '{}' run {} is {}",
            record.workflow, record.run_id, record.status
        )
    };
    ToolResult {
        data: json!({
            "workflow_script": true,
            "action": action,
            "workflow": record.workflow,
            "workflow_file": record.workflow_file,
            "run_id": record.run_id,
            "status": record.status,
            "current_step": current_step,
            "completed_steps": record.completed_step_count(),
            "total_steps": record.steps.len(),
            "run_path": run_path.display().to_string(),
            "run": record,
        }),
        model_content: Some(allthecodes_types::message::ToolResultContent::Text(
            model_text,
        )),
        display_preview: Some(preview),
        ..Default::default()
    }
}

fn model_instruction(action: &str, record: &FileWorkflowRunRecord) -> String {
    if let Some(step) = record.current_step() {
        let mut text = format!(
            "Workflow script '{}' run {} is {} after action '{}'.\n\nCurrent step: {}\n\n{}\n",
            record.workflow, record.run_id, record.status, action, step.name, step.prompt
        );
        if let Some(run) = &step.run {
            text.push_str(&format!("\nSuggested command from workflow file:\n{run}\n"));
        }
        text.push_str(&format!(
            "\nWhen this step is handled, call Workflow with action=advance and run_id={}.",
            record.run_id
        ));
        text
    } else {
        format!(
            "Workflow script '{}' run {} is {} after action '{}'.",
            record.workflow, record.run_id, record.status, action
        )
    }
}

fn resolve_workflow_script(cwd: &Path, workflow: &str) -> Result<PathBuf> {
    let workflow = workflow.trim();
    if workflow.is_empty() {
        bail!("workflow must not be empty");
    }
    let dir = workflow_scripts_dir(cwd);
    let mut matches = Vec::new();
    if dir.exists() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !is_valid_workflow_file(&path) {
                continue;
            }
            let stem_matches = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem == workflow)
                .unwrap_or(false);
            let file_matches = path
                .file_name()
                .and_then(|file| file.to_str())
                .map(|file| file == workflow)
                .unwrap_or(false);
            if stem_matches || file_matches {
                matches.push(path);
            }
        }
    }
    matches.sort();
    match matches.len() {
        0 => bail!(
            "workflow script '{}' not found in {}",
            workflow,
            dir.display()
        ),
        1 => Ok(matches.remove(0)),
        _ => bail!(
            "workflow script '{}' is ambiguous in {}",
            workflow,
            dir.display()
        ),
    }
}

fn workflow_run_file(cwd: &Path, run_id: &str) -> PathBuf {
    workflow_runs_dir(cwd).join(format!("{}.json", sanitize_segment(run_id)))
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn hidden_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}
