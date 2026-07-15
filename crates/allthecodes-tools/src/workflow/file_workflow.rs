use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use allthecodes_tasks::{TaskCreateOptions, TaskStatus, TASK_KIND_LOCAL_WORKFLOW};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::common::string_param;
use crate::tool::ToolResult;

pub const WORKFLOW_EXTENSIONS: &[&str] = &["md", "yaml", "yml"];

const MAX_DEFINITION_BYTES: u64 = 256 * 1024;
const MAX_WORKFLOW_STEPS: usize = 256;
const MAX_STEP_PROMPT_BYTES: usize = 16 * 1024;
const MAX_STEP_RUN_BYTES: usize = 16 * 1024;
const MAX_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_IDENTIFIER_BYTES: usize = 160;
const MAX_RUN_SCAN: usize = 10_000;
const DEFAULT_RUN_PAGE_SIZE: usize = 50;
const MAX_RUN_PAGE_SIZE: usize = 100;
const LOCK_RETRIES: usize = 9;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowServiceErrorKind {
    InvalidInput,
    Forbidden,
    NotFound,
    Conflict,
    TooLarge,
    Store,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowServiceError {
    pub kind: WorkflowServiceErrorKind,
    pub code: &'static str,
    pub message: String,
}

impl WorkflowServiceError {
    fn new(kind: WorkflowServiceErrorKind, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
        }
    }

    fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(WorkflowServiceErrorKind::InvalidInput, code, message)
    }

    fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(WorkflowServiceErrorKind::Forbidden, code, message)
    }

    fn not_found(entity: &'static str, id: &str) -> Self {
        Self::new(
            WorkflowServiceErrorKind::NotFound,
            "not_found",
            format!("{entity} '{id}' was not found"),
        )
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(WorkflowServiceErrorKind::Conflict, code, message)
    }

    fn too_large(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(WorkflowServiceErrorKind::TooLarge, code, message)
    }

    fn store(message: impl Into<String>) -> Self {
        Self::new(
            WorkflowServiceErrorKind::Store,
            "workflow_store_error",
            message,
        )
    }
}

impl fmt::Display for WorkflowServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WorkflowServiceError {}

pub type WorkflowServiceResult<T> = std::result::Result<T, WorkflowServiceError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl WorkflowRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> WorkflowServiceResult<Self> {
        match value {
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(WorkflowServiceError::invalid(
                "invalid_workflow_status",
                "workflow status must be running, completed, failed, or cancelled",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowAuthorizationDecision {
    Allow,
    Ask,
    Deny,
}

/// Shared action classification used by both the tool and remote adapters.
pub fn classify_workflow_action(action: &str) -> WorkflowAuthorizationDecision {
    match action {
        "list" | "detail" | "runs_list" | "status" => WorkflowAuthorizationDecision::Allow,
        "start" | "advance" | "cancel" => WorkflowAuthorizationDecision::Ask,
        _ => WorkflowAuthorizationDecision::Deny,
    }
}

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
    /// Stable digest of the canonical workspace. Legacy records deserialize
    /// with an empty identity and are accepted only from the active workspace's
    /// already-contained run directory.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace_id: String,
    #[serde(default = "initial_workflow_revision")]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_request_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_request_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation: Option<String>,
}

const fn initial_workflow_revision() -> u64 {
    1
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionSummary {
    pub workflow: String,
    pub workflow_file: String,
    pub step_count: usize,
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionDetail {
    pub workflow: String,
    pub workflow_file: String,
    pub steps: Vec<ParsedStep>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowRunListOptions {
    pub status: Option<WorkflowRunStatus>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct WorkflowRunPage {
    pub runs: Vec<FileWorkflowRunRecord>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
    pub corrupt_entry_count: usize,
}

#[derive(Debug, Clone)]
pub struct WorkflowMutationOutcome {
    pub record: FileWorkflowRunRecord,
    pub replayed: bool,
    pub projection_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkflowStartOptions {
    pub workflow: String,
    pub args: Option<Value>,
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowAdvanceOptions {
    pub run_id: String,
    pub applied_status: WorkflowRunStatus,
    pub expected_revision: Option<u64>,
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowCancelOptions {
    pub run_id: String,
    pub expected_revision: Option<u64>,
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub struct FileWorkflowService {
    workspace: PathBuf,
    workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRunCursor {
    workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<WorkflowRunStatus>,
    updated_at: String,
    run_id: String,
}

impl FileWorkflowService {
    pub fn new(cwd: &Path) -> WorkflowServiceResult<Self> {
        let workspace = fs::canonicalize(cwd).map_err(|error| {
            WorkflowServiceError::forbidden(
                "invalid_workspace",
                format!("active workspace cannot be resolved: {error}"),
            )
        })?;
        let metadata = fs::metadata(&workspace).map_err(|error| {
            WorkflowServiceError::forbidden(
                "invalid_workspace",
                format!("active workspace cannot be inspected: {error}"),
            )
        })?;
        if !metadata.is_dir() {
            return Err(WorkflowServiceError::forbidden(
                "invalid_workspace",
                "active workspace is not a directory",
            ));
        }
        let workspace_id = digest_bytes(workspace.to_string_lossy().as_bytes());
        Ok(Self {
            workspace,
            workspace_id: format!("workspace:{workspace_id}"),
        })
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn list_definitions(&self) -> WorkflowServiceResult<Vec<WorkflowDefinitionSummary>> {
        let Some(dir) = secure_project_subdir(&self.workspace, "workflows", false)? else {
            return Ok(Vec::new());
        };
        let mut definitions = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|error| {
            WorkflowServiceError::store(format!(
                "failed to enumerate workflow definitions: {error}"
            ))
        })?;
        for entry in entries.take(MAX_RUN_SCAN) {
            let entry = entry.map_err(|error| {
                WorkflowServiceError::store(format!(
                    "failed to inspect workflow definition: {error}"
                ))
            })?;
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() || hidden_file(&path) {
                continue;
            }
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_allowed_workflow_file_name(file) {
                continue;
            }
            let workflow = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_string();
            let (step_count, parse_error) = match parse_bounded_workflow(&path) {
                Ok(parsed) => (parsed.steps.len(), None),
                Err(error) => (0, Some(truncate_diagnostic(&error.message))),
            };
            definitions.push(WorkflowDefinitionSummary {
                workflow,
                workflow_file: file.to_string(),
                step_count,
                parse_error,
            });
        }
        definitions.sort_by(|left, right| {
            left.workflow
                .cmp(&right.workflow)
                .then_with(|| left.workflow_file.cmp(&right.workflow_file))
        });
        Ok(definitions)
    }

    pub fn definition(&self, workflow: &str) -> WorkflowServiceResult<WorkflowDefinitionDetail> {
        let (detail, _) = self.load_definition(workflow)?;
        Ok(detail)
    }

    fn load_definition(
        &self,
        workflow: &str,
    ) -> WorkflowServiceResult<(WorkflowDefinitionDetail, PathBuf)> {
        validate_workflow_identifier(workflow)?;
        let dir = secure_project_subdir(&self.workspace, "workflows", false)?
            .ok_or_else(|| WorkflowServiceError::not_found("workflow definition", workflow))?;
        let mut matches = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|error| {
            WorkflowServiceError::store(format!(
                "failed to enumerate workflow definitions: {error}"
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                WorkflowServiceError::store(format!(
                    "failed to inspect workflow definition: {error}"
                ))
            })?;
            let path = entry.path();
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let stem_matches = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem == workflow)
                .unwrap_or(false);
            if file != workflow && !stem_matches {
                continue;
            }
            if !is_allowed_workflow_file_name(file) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                WorkflowServiceError::store(format!(
                    "failed to inspect workflow definition: {error}"
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(WorkflowServiceError::forbidden(
                    "workflow_symlink_rejected",
                    "workflow definition symlinks are not allowed",
                ));
            }
            if metadata.is_file() {
                let canonical = fs::canonicalize(&path).map_err(|error| {
                    WorkflowServiceError::store(format!(
                        "failed to resolve workflow definition: {error}"
                    ))
                })?;
                if !canonical.starts_with(&dir) {
                    return Err(WorkflowServiceError::forbidden(
                        "workflow_path_outside_workspace",
                        "workflow definition is outside the active workspace",
                    ));
                }
                matches.push(canonical);
            }
        }
        matches.sort();
        let path = match matches.len() {
            0 => {
                return Err(WorkflowServiceError::not_found(
                    "workflow definition",
                    workflow,
                ))
            }
            1 => matches.remove(0),
            _ => {
                return Err(WorkflowServiceError::conflict(
                    "ambiguous_workflow",
                    format!("workflow definition '{workflow}' is ambiguous"),
                ))
            }
        };
        let parsed = parse_bounded_workflow(&path)?;
        let workflow_file = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                WorkflowServiceError::invalid(
                    "invalid_workflow_identifier",
                    "workflow definition file name is not valid UTF-8",
                )
            })?
            .to_string();
        let workflow = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(workflow)
            .to_string();
        Ok((
            WorkflowDefinitionDetail {
                workflow,
                workflow_file,
                steps: parsed.steps,
            },
            path,
        ))
    }

    pub fn list_runs(
        &self,
        options: WorkflowRunListOptions,
    ) -> WorkflowServiceResult<WorkflowRunPage> {
        let limit = options.limit.unwrap_or(DEFAULT_RUN_PAGE_SIZE);
        if limit == 0 || limit > MAX_RUN_PAGE_SIZE {
            return Err(WorkflowServiceError::invalid(
                "invalid_limit",
                format!("workflow run limit must be between 1 and {MAX_RUN_PAGE_SIZE}"),
            ));
        }
        let Some(dir) = secure_project_subdir(&self.workspace, "workflow-runs", false)? else {
            if options.cursor.is_some() {
                return Err(WorkflowServiceError::invalid(
                    "invalid_cursor",
                    "workflow run cursor is not valid for this workspace",
                ));
            }
            return Ok(WorkflowRunPage {
                runs: Vec::new(),
                next_cursor: None,
                truncated: false,
                corrupt_entry_count: 0,
            });
        };

        let mut runs = Vec::new();
        let mut corrupt_entry_count = 0usize;
        let mut scanned = 0usize;
        let mut scan_truncated = false;
        let entries = fs::read_dir(&dir).map_err(|error| {
            WorkflowServiceError::store(format!("failed to enumerate workflow runs: {error}"))
        })?;
        for entry in entries {
            if scanned >= MAX_RUN_SCAN {
                scan_truncated = true;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    corrupt_entry_count = corrupt_entry_count.saturating_add(1);
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            scanned = scanned.saturating_add(1);
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    corrupt_entry_count = corrupt_entry_count.saturating_add(1);
                    continue;
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                corrupt_entry_count = corrupt_entry_count.saturating_add(1);
                continue;
            }
            match self.load_run_path(&path, None) {
                Ok(record)
                    if options
                        .status
                        .map(|status| record.status == status.as_str())
                        .unwrap_or(true) =>
                {
                    runs.push(record)
                }
                Ok(_) => {}
                Err(_) => corrupt_entry_count = corrupt_entry_count.saturating_add(1),
            }
        }

        runs.sort_by(|left, right| {
            workflow_timestamp(&right.updated_at)
                .cmp(&workflow_timestamp(&left.updated_at))
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
        let start = if let Some(cursor) = options.cursor.as_deref() {
            let cursor = decode_run_cursor(cursor)?;
            if cursor.workspace_id != self.workspace_id || cursor.status != options.status {
                return Err(WorkflowServiceError::invalid(
                    "invalid_cursor",
                    "workflow run cursor is not valid for this workspace or filter",
                ));
            }
            runs.iter()
                .position(|record| {
                    record.updated_at == cursor.updated_at && record.run_id == cursor.run_id
                })
                .map(|index| index + 1)
                .ok_or_else(|| {
                    WorkflowServiceError::invalid(
                        "invalid_cursor",
                        "workflow run cursor no longer identifies this result set",
                    )
                })?
        } else {
            0
        };
        let end = start.saturating_add(limit).min(runs.len());
        let has_more = end < runs.len();
        let page_runs = runs[start..end].to_vec();
        let next_cursor = if has_more {
            page_runs
                .last()
                .map(|record| {
                    encode_run_cursor(&WorkflowRunCursor {
                        workspace_id: self.workspace_id.clone(),
                        status: options.status,
                        updated_at: record.updated_at.clone(),
                        run_id: record.run_id.clone(),
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(WorkflowRunPage {
            runs: page_runs,
            next_cursor,
            truncated: scan_truncated || has_more,
            corrupt_entry_count,
        })
    }

    pub fn load_run(&self, run_id: &str) -> WorkflowServiceResult<FileWorkflowRunRecord> {
        validate_run_id(run_id)?;
        let dir = secure_project_subdir(&self.workspace, "workflow-runs", false)?
            .ok_or_else(|| WorkflowServiceError::not_found("workflow run", run_id))?;
        self.load_run_path(&dir.join(format!("{run_id}.json")), Some(run_id))
    }

    fn load_run_path(
        &self,
        path: &Path,
        expected_run_id: Option<&str>,
    ) -> WorkflowServiceResult<FileWorkflowRunRecord> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(WorkflowServiceError::not_found(
                    "workflow run",
                    expected_run_id.unwrap_or("unknown"),
                ))
            }
            Err(error) => {
                return Err(WorkflowServiceError::store(format!(
                    "failed to inspect workflow run: {error}"
                )))
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(WorkflowServiceError::forbidden(
                "workflow_symlink_rejected",
                "workflow run symlinks are not allowed",
            ));
        }
        if !metadata.is_file() {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_run",
                "workflow run entry is not a regular file",
            ));
        }
        if metadata.len() > MAX_DEFINITION_BYTES * 4 {
            return Err(WorkflowServiceError::too_large(
                "workflow_run_too_large",
                "workflow run record exceeds the configured size limit",
            ));
        }
        let file_run_id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                WorkflowServiceError::invalid("invalid_run_id", "workflow run file name is invalid")
            })?;
        validate_run_id(file_run_id)?;
        if let Some(expected) = expected_run_id {
            if expected != file_run_id {
                return Err(WorkflowServiceError::forbidden(
                    "workflow_run_identity_mismatch",
                    "workflow run identity does not match its canonical file",
                ));
            }
        }
        let raw = fs::read_to_string(path).map_err(|error| {
            WorkflowServiceError::store(format!("failed to read workflow run: {error}"))
        })?;
        let record: FileWorkflowRunRecord = serde_json::from_str(&raw).map_err(|_| {
            WorkflowServiceError::invalid(
                "invalid_workflow_run",
                "workflow run record is malformed",
            )
        })?;
        self.validate_run_record(&record, file_run_id)?;
        Ok(record)
    }

    fn validate_run_record(
        &self,
        record: &FileWorkflowRunRecord,
        file_run_id: &str,
    ) -> WorkflowServiceResult<()> {
        if record.run_id != file_run_id {
            return Err(WorkflowServiceError::forbidden(
                "workflow_run_identity_mismatch",
                "workflow run identity does not match its canonical file",
            ));
        }
        validate_run_id(&record.run_id)?;
        if !record.workspace_id.is_empty() && record.workspace_id != self.workspace_id {
            return Err(WorkflowServiceError::forbidden(
                "workflow_workspace_mismatch",
                "workflow run belongs to a different workspace",
            ));
        }
        WorkflowRunStatus::parse(&record.status)?;
        if record.steps.is_empty() || record.steps.len() > MAX_WORKFLOW_STEPS {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_run",
                "workflow run has an invalid number of steps",
            ));
        }
        if record.revision == 0 {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_revision",
                "workflow run revision must be positive",
            ));
        }
        if chrono::DateTime::parse_from_rfc3339(&record.created_at).is_err()
            || chrono::DateTime::parse_from_rfc3339(&record.updated_at).is_err()
        {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_run",
                "workflow run timestamps are malformed",
            ));
        }
        for step in &record.steps {
            if step.name.is_empty()
                || step.name.len() > MAX_STEP_PROMPT_BYTES
                || step.prompt.len() > MAX_STEP_PROMPT_BYTES
                || step
                    .run
                    .as_ref()
                    .map(|run| run.len() > MAX_STEP_RUN_BYTES)
                    .unwrap_or(false)
                || !matches!(
                    step.status.as_str(),
                    "pending" | "running" | "ready" | "completed" | "failed" | "cancelled"
                )
            {
                return Err(WorkflowServiceError::invalid(
                    "invalid_workflow_run",
                    "workflow run contains an invalid step",
                ));
            }
        }
        let active = record
            .steps
            .iter()
            .filter(|step| matches!(step.status.as_str(), "running" | "ready"))
            .count();
        if record.status == "running" {
            if active != 1
                || record
                    .current_step_index
                    .and_then(|index| record.steps.get(index))
                    .map(|step| !matches!(step.status.as_str(), "running" | "ready"))
                    .unwrap_or(true)
            {
                return Err(WorkflowServiceError::invalid(
                    "invalid_workflow_run",
                    "running workflow must have exactly one active step",
                ));
            }
        } else if record.current_step_index.is_some() || active != 0 {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_run",
                "terminal workflow must not have an active step",
            ));
        }
        Ok(())
    }

    pub fn start(
        &self,
        options: WorkflowStartOptions,
    ) -> WorkflowServiceResult<WorkflowMutationOutcome> {
        validate_request_id(&options.request_id)?;
        validate_arguments(options.args.as_ref())?;
        let (definition, script) = self.load_definition(&options.workflow)?;
        let request_digest = canonical_request_digest(&json!({
            "operation": "start",
            "workflow_file": &definition.workflow_file,
            "args": &options.args,
        }))?;
        let runs_dir = secure_project_subdir(&self.workspace, "workflow-runs", true)?
            .ok_or_else(|| WorkflowServiceError::store("workflow run directory was not created"))?;
        let _lock = WorkflowFileLock::acquire(
            &runs_dir,
            &format!(
                "start-{}",
                digest_bytes(definition.workflow_file.as_bytes())
            ),
        )?;

        if let Some(existing) =
            self.find_start_request(&runs_dir, &definition.workflow_file, &options.request_id)?
        {
            if existing.start_request_digest.as_deref() == Some(request_digest.as_str()) {
                return Ok(WorkflowMutationOutcome {
                    record: existing,
                    replayed: true,
                    projection_warning: None,
                });
            }
            return Err(WorkflowServiceError::conflict(
                "idempotency_conflict",
                "request_id was already used with different workflow start input",
            ));
        }

        let now = Utc::now().to_rfc3339();
        let mut steps = definition
            .steps
            .into_iter()
            .map(FileWorkflowStep::pending)
            .collect::<Vec<_>>();
        if let Some(first) = steps.first_mut() {
            first.status = "running".into();
            first.started_at = Some(now.clone());
        }
        let run_id = format!("workflow-run-{}", Uuid::new_v4());
        let record = FileWorkflowRunRecord {
            run_id: run_id.clone(),
            workflow: definition.workflow,
            workflow_file: definition.workflow_file,
            workflow_path: script.display().to_string(),
            args: options.args,
            status: "running".into(),
            current_step_index: Some(0),
            steps,
            created_at: now.clone(),
            updated_at: now,
            workspace_id: self.workspace_id.clone(),
            revision: 1,
            start_request_id: Some(options.request_id.clone()),
            start_request_digest: Some(request_digest.clone()),
            last_request_id: Some(options.request_id),
            last_request_digest: Some(request_digest),
            last_operation: Some("start".to_string()),
        };
        let path = runs_dir.join(format!("{run_id}.json"));
        write_run_atomic(&path, &record)?;
        drop(_lock);
        Ok(self.with_projection(record, &path, false))
    }

    fn find_start_request(
        &self,
        runs_dir: &Path,
        workflow_file: &str,
        request_id: &str,
    ) -> WorkflowServiceResult<Option<FileWorkflowRunRecord>> {
        let entries = fs::read_dir(runs_dir).map_err(|error| {
            WorkflowServiceError::store(format!("failed to enumerate workflow runs: {error}"))
        })?;
        for entry in entries.take(MAX_RUN_SCAN) {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(record) = self.load_run_path(&path, None) else {
                continue;
            };
            if record.workflow_file == workflow_file
                && record.start_request_id.as_deref() == Some(request_id)
            {
                return Ok(Some(record));
            }
        }
        Ok(None)
    }

    pub fn advance(
        &self,
        options: WorkflowAdvanceOptions,
    ) -> WorkflowServiceResult<WorkflowMutationOutcome> {
        validate_run_id(&options.run_id)?;
        validate_request_id(&options.request_id)?;
        if options.applied_status == WorkflowRunStatus::Running {
            return Err(WorkflowServiceError::invalid(
                "invalid_transition_status",
                "advance status must be completed, failed, or cancelled",
            ));
        }
        let digest = canonical_request_digest(&json!({
            "operation": "advance",
            "run_id": &options.run_id,
            "applied_status": options.applied_status,
            "expected_revision": options.expected_revision,
        }))?;
        self.mutate_run(
            &options.run_id,
            "advance",
            &options.request_id,
            &digest,
            options.expected_revision,
            |record| apply_advance_transition(record, options.applied_status),
        )
    }

    pub fn cancel(
        &self,
        options: WorkflowCancelOptions,
    ) -> WorkflowServiceResult<WorkflowMutationOutcome> {
        validate_run_id(&options.run_id)?;
        validate_request_id(&options.request_id)?;
        let digest = canonical_request_digest(&json!({
            "operation": "cancel",
            "run_id": &options.run_id,
            "expected_revision": options.expected_revision,
        }))?;
        self.mutate_run(
            &options.run_id,
            "cancel",
            &options.request_id,
            &digest,
            options.expected_revision,
            apply_cancel_transition,
        )
    }

    fn mutate_run<F>(
        &self,
        run_id: &str,
        operation: &str,
        request_id: &str,
        request_digest: &str,
        expected_revision: Option<u64>,
        transition: F,
    ) -> WorkflowServiceResult<WorkflowMutationOutcome>
    where
        F: FnOnce(&mut FileWorkflowRunRecord) -> WorkflowServiceResult<()>,
    {
        let runs_dir = secure_project_subdir(&self.workspace, "workflow-runs", false)?
            .ok_or_else(|| WorkflowServiceError::not_found("workflow run", run_id))?;
        let _lock = WorkflowFileLock::acquire(&runs_dir, run_id)?;
        let path = runs_dir.join(format!("{run_id}.json"));
        let mut record = self.load_run_path(&path, Some(run_id))?;
        if record.last_request_id.as_deref() == Some(request_id) {
            if record.last_request_digest.as_deref() == Some(request_digest)
                && record.last_operation.as_deref() == Some(operation)
            {
                return Ok(WorkflowMutationOutcome {
                    record,
                    replayed: true,
                    projection_warning: None,
                });
            }
            return Err(WorkflowServiceError::conflict(
                "idempotency_conflict",
                "request_id was already used with different workflow mutation input",
            ));
        }
        if let Some(expected) = expected_revision {
            if record.revision != expected {
                return Err(WorkflowServiceError::conflict(
                    "revision_conflict",
                    format!(
                        "workflow run revision is {}; expected {expected}",
                        record.revision
                    ),
                ));
            }
        }
        transition(&mut record)?;
        record.workspace_id = self.workspace_id.clone();
        record.revision = record.revision.checked_add(1).ok_or_else(|| {
            WorkflowServiceError::conflict(
                "revision_overflow",
                "workflow run revision cannot be incremented",
            )
        })?;
        record.updated_at = Utc::now().to_rfc3339();
        record.last_request_id = Some(request_id.to_string());
        record.last_request_digest = Some(request_digest.to_string());
        record.last_operation = Some(operation.to_string());
        self.validate_run_record(&record, run_id)?;
        write_run_atomic(&path, &record)?;
        drop(_lock);
        Ok(self.with_projection(record, &path, false))
    }

    pub fn reconcile_task_projection(
        &self,
        run_id: &str,
    ) -> WorkflowServiceResult<FileWorkflowRunRecord> {
        let record = self.load_run(run_id)?;
        let path = secure_project_subdir(&self.workspace, "workflow-runs", false)?
            .ok_or_else(|| WorkflowServiceError::not_found("workflow run", run_id))?
            .join(format!("{run_id}.json"));
        sync_file_workflow_task(&record, &path).map_err(|error| {
            WorkflowServiceError::store(format!("failed to reconcile task projection: {error}"))
        })?;
        Ok(record)
    }

    fn with_projection(
        &self,
        record: FileWorkflowRunRecord,
        path: &Path,
        replayed: bool,
    ) -> WorkflowMutationOutcome {
        // A later mutation may commit after this caller drops its file lock but
        // before it reaches the derived task store. Always project the newest
        // canonical revision so a delayed writer cannot overwrite the task
        // view with stale state.
        let projection_record = self
            .load_run(&record.run_id)
            .unwrap_or_else(|_| record.clone());
        let projection_warning =
            sync_file_workflow_task(&projection_record, path)
                .err()
                .map(|error| {
                    tracing::warn!(
                        run_id = %record.run_id,
                        error = %error,
                        "workflow task projection requires reconciliation"
                    );
                    "task projection requires reconciliation".to_string()
                });
        WorkflowMutationOutcome {
            record,
            replayed,
            projection_warning,
        }
    }
}

fn apply_advance_transition(
    record: &mut FileWorkflowRunRecord,
    applied_status: WorkflowRunStatus,
) -> WorkflowServiceResult<()> {
    if record.status != "running" {
        return Err(WorkflowServiceError::conflict(
            "terminal_workflow_run",
            format!(
                "workflow run is already in terminal status {}",
                record.status
            ),
        ));
    }
    let index = active_step_index(record).ok_or_else(|| {
        WorkflowServiceError::conflict(
            "invalid_workflow_transition",
            "workflow run has no active step",
        )
    })?;
    let now = Utc::now().to_rfc3339();
    record.steps[index].status = applied_status.as_str().to_string();
    record.steps[index].completed_at = Some(now.clone());
    match applied_status {
        WorkflowRunStatus::Completed => {
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
        WorkflowRunStatus::Failed => {
            record.current_step_index = None;
            record.status = "failed".into();
        }
        WorkflowRunStatus::Cancelled => {
            record.current_step_index = None;
            record.status = "cancelled".into();
            for step in &mut record.steps {
                if matches!(step.status.as_str(), "pending" | "running" | "ready") {
                    step.status = "cancelled".into();
                    step.completed_at = Some(now.clone());
                }
            }
        }
        WorkflowRunStatus::Running => {
            return Err(WorkflowServiceError::invalid(
                "invalid_transition_status",
                "advance status must be completed, failed, or cancelled",
            ));
        }
    }
    Ok(())
}

fn apply_cancel_transition(record: &mut FileWorkflowRunRecord) -> WorkflowServiceResult<()> {
    if record.status != "running" {
        return Err(WorkflowServiceError::conflict(
            "terminal_workflow_run",
            format!(
                "workflow run in terminal status {} cannot be cancelled",
                record.status
            ),
        ));
    }
    let now = Utc::now().to_rfc3339();
    for step in &mut record.steps {
        if matches!(step.status.as_str(), "pending" | "running" | "ready") {
            step.status = "cancelled".into();
            step.completed_at = Some(now.clone());
        }
    }
    record.current_step_index = None;
    record.status = "cancelled".into();
    Ok(())
}

fn secure_project_subdir(
    workspace: &Path,
    child: &str,
    create: bool,
) -> WorkflowServiceResult<Option<PathBuf>> {
    let project_dir = workspace.join(".allthecodes");
    if !project_dir.exists() {
        if !create {
            return Ok(None);
        }
        match fs::create_dir(&project_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(WorkflowServiceError::store(format!(
                    "failed to create project runtime directory: {error}"
                )))
            }
        }
    }
    let project_metadata = fs::symlink_metadata(&project_dir).map_err(|error| {
        WorkflowServiceError::store(format!(
            "failed to inspect project runtime directory: {error}"
        ))
    })?;
    if project_metadata.file_type().is_symlink() {
        return Err(WorkflowServiceError::forbidden(
            "workflow_symlink_rejected",
            "project workflow runtime directory must not be a symlink",
        ));
    }
    if !project_metadata.is_dir() {
        return Err(WorkflowServiceError::forbidden(
            "invalid_workflow_directory",
            "project workflow runtime path is not a directory",
        ));
    }
    let canonical_project = fs::canonicalize(&project_dir).map_err(|error| {
        WorkflowServiceError::store(format!(
            "failed to resolve project runtime directory: {error}"
        ))
    })?;
    if !canonical_project.starts_with(workspace) {
        return Err(WorkflowServiceError::forbidden(
            "workflow_path_outside_workspace",
            "project workflow runtime directory is outside the active workspace",
        ));
    }

    let subdir = canonical_project.join(child);
    if !subdir.exists() {
        if !create {
            return Ok(None);
        }
        match fs::create_dir(&subdir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(WorkflowServiceError::store(format!(
                    "failed to create workflow runtime directory: {error}"
                )))
            }
        }
    }
    let metadata = fs::symlink_metadata(&subdir).map_err(|error| {
        WorkflowServiceError::store(format!(
            "failed to inspect workflow runtime directory: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(WorkflowServiceError::forbidden(
            "workflow_symlink_rejected",
            "workflow runtime directory must not be a symlink",
        ));
    }
    if !metadata.is_dir() {
        return Err(WorkflowServiceError::forbidden(
            "invalid_workflow_directory",
            "workflow runtime path is not a directory",
        ));
    }
    let canonical = fs::canonicalize(&subdir).map_err(|error| {
        WorkflowServiceError::store(format!(
            "failed to resolve workflow runtime directory: {error}"
        ))
    })?;
    if !canonical.starts_with(&canonical_project) {
        return Err(WorkflowServiceError::forbidden(
            "workflow_path_outside_workspace",
            "workflow runtime directory is outside the active workspace",
        ));
    }
    Ok(Some(canonical))
}

fn parse_bounded_workflow(path: &Path) -> WorkflowServiceResult<ParsedWorkflow> {
    let metadata = fs::metadata(path).map_err(|error| {
        WorkflowServiceError::store(format!("failed to inspect workflow definition: {error}"))
    })?;
    if metadata.len() > MAX_DEFINITION_BYTES {
        return Err(WorkflowServiceError::too_large(
            "workflow_definition_too_large",
            "workflow definition exceeds the configured size limit",
        ));
    }
    let content = fs::read_to_string(path)
        .map_err(|_| WorkflowServiceError::store("workflow definition could not be read"))?;
    let parsed = match path.extension().and_then(|extension| extension.to_str()) {
        Some("md") => MarkdownWorkflowParser::parse(&content),
        Some("yaml" | "yml") => YamlWorkflowParser::parse(&content),
        _ => {
            return Err(WorkflowServiceError::invalid(
                "invalid_workflow_definition",
                "workflow definition has an unsupported extension",
            ))
        }
    }
    .map_err(|error| {
        WorkflowServiceError::invalid(
            "invalid_workflow_definition",
            format!("workflow definition could not be parsed: {error}"),
        )
    })?;
    validate_parsed_workflow(&parsed)?;
    Ok(parsed)
}

fn validate_parsed_workflow(parsed: &ParsedWorkflow) -> WorkflowServiceResult<()> {
    if parsed.steps.is_empty() {
        return Err(WorkflowServiceError::invalid(
            "empty_workflow_definition",
            "workflow definition has no executable steps",
        ));
    }
    if parsed.steps.len() > MAX_WORKFLOW_STEPS {
        return Err(WorkflowServiceError::too_large(
            "workflow_step_limit_exceeded",
            format!("workflow definition exceeds {MAX_WORKFLOW_STEPS} steps"),
        ));
    }
    for step in &parsed.steps {
        if step.name.trim().is_empty() || step.name.len() > MAX_STEP_PROMPT_BYTES {
            return Err(WorkflowServiceError::too_large(
                "workflow_step_too_large",
                "workflow step name exceeds the configured size limit",
            ));
        }
        if step.prompt.len() > MAX_STEP_PROMPT_BYTES {
            return Err(WorkflowServiceError::too_large(
                "workflow_step_too_large",
                "workflow step prompt exceeds the configured size limit",
            ));
        }
        if step
            .run
            .as_ref()
            .map(|run| run.len() > MAX_STEP_RUN_BYTES)
            .unwrap_or(false)
        {
            return Err(WorkflowServiceError::too_large(
                "workflow_step_too_large",
                "workflow step command hint exceeds the configured size limit",
            ));
        }
    }
    Ok(())
}

fn validate_workflow_identifier(value: &str) -> WorkflowServiceResult<()> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(WorkflowServiceError::invalid(
            "invalid_workflow_identifier",
            "workflow identifier is empty or too long",
        ));
    }
    if Path::new(value).is_absolute()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err(WorkflowServiceError::invalid(
            "invalid_workflow_identifier",
            "workflow identifier must be an exact project-local stem or file name",
        ));
    }
    let (stem, extension) = match value.rsplit_once('.') {
        Some((stem, extension)) => (stem, Some(extension)),
        None => (value, None),
    };
    if stem.is_empty()
        || !stem
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        || extension
            .map(|extension| !WORKFLOW_EXTENSIONS.contains(&extension))
            .unwrap_or(false)
    {
        return Err(WorkflowServiceError::invalid(
            "invalid_workflow_identifier",
            "workflow identifier contains unsupported characters or extension",
        ));
    }
    Ok(())
}

fn is_allowed_workflow_file_name(file: &str) -> bool {
    validate_workflow_identifier(file).is_ok() && file.contains('.')
}

fn validate_run_id(run_id: &str) -> WorkflowServiceResult<()> {
    if run_id.len() > MAX_IDENTIFIER_BYTES {
        return Err(WorkflowServiceError::invalid(
            "invalid_run_id",
            "workflow run identifier is too long",
        ));
    }
    let Some(raw_uuid) = run_id.strip_prefix("workflow-run-") else {
        return Err(WorkflowServiceError::invalid(
            "invalid_run_id",
            "workflow run identifier must use the workflow-run UUID format",
        ));
    };
    let uuid = Uuid::parse_str(raw_uuid).map_err(|_| {
        WorkflowServiceError::invalid(
            "invalid_run_id",
            "workflow run identifier must use the workflow-run UUID format",
        )
    })?;
    if format!("workflow-run-{uuid}") != run_id {
        return Err(WorkflowServiceError::invalid(
            "invalid_run_id",
            "workflow run identifier is not in canonical form",
        ));
    }
    Ok(())
}

fn validate_request_id(request_id: &str) -> WorkflowServiceResult<()> {
    if request_id.is_empty()
        || request_id.len() > MAX_IDENTIFIER_BYTES
        || !request_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        return Err(WorkflowServiceError::invalid(
            "invalid_request_id",
            "request_id is empty, too long, or contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_arguments(args: Option<&Value>) -> WorkflowServiceResult<()> {
    let Some(args) = args else { return Ok(()) };
    if !args.is_object() {
        return Err(WorkflowServiceError::invalid(
            "invalid_workflow_args",
            "workflow args must be a JSON object",
        ));
    }
    let size = serde_json::to_vec(args)
        .map_err(|error| WorkflowServiceError::invalid("invalid_workflow_args", error.to_string()))?
        .len();
    if size > MAX_ARGUMENT_BYTES {
        return Err(WorkflowServiceError::too_large(
            "workflow_args_too_large",
            "workflow args exceed the configured size limit",
        ));
    }
    Ok(())
}

fn canonical_request_digest(value: &Value) -> WorkflowServiceResult<String> {
    let canonical = canonical_json(value);
    let bytes = serde_json::to_vec(&canonical).map_err(|error| {
        WorkflowServiceError::invalid("invalid_workflow_request", error.to_string())
    })?;
    Ok(digest_bytes(&bytes))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let mut canonical = serde_json::Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&object[key]));
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn workflow_timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

fn encode_run_cursor(cursor: &WorkflowRunCursor) -> WorkflowServiceResult<String> {
    let bytes = serde_json::to_vec(cursor).map_err(|error| {
        WorkflowServiceError::store(format!("failed to encode workflow run cursor: {error}"))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_run_cursor(cursor: &str) -> WorkflowServiceResult<WorkflowRunCursor> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| {
        WorkflowServiceError::invalid("invalid_cursor", "workflow run cursor is malformed")
    })?;
    if bytes.len() > 1024 {
        return Err(WorkflowServiceError::invalid(
            "invalid_cursor",
            "workflow run cursor is too large",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        WorkflowServiceError::invalid("invalid_cursor", "workflow run cursor is malformed")
    })
}

fn truncate_diagnostic(message: &str) -> String {
    const MAX_DIAGNOSTIC_CHARS: usize = 240;
    let mut chars = message.chars();
    let truncated = chars
        .by_ref()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn write_run_atomic(path: &Path, record: &FileWorkflowRunRecord) -> WorkflowServiceResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| WorkflowServiceError::store("workflow run path has no parent directory"))?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
        WorkflowServiceError::store(format!("failed to serialize workflow run: {error}"))
    })?;
    let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".workflow-run.tmp.{}.{}.{}",
        std::process::id(),
        sequence,
        Uuid::new_v4()
    ));
    let result = (|| -> WorkflowServiceResult<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|error| {
            WorkflowServiceError::store(format!(
                "failed to create workflow run temporary file: {error}"
            ))
        })?;
        file.write_all(&bytes).map_err(|error| {
            WorkflowServiceError::store(format!("failed to write workflow run: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            WorkflowServiceError::store(format!("failed to sync workflow run: {error}"))
        })?;
        drop(file);
        replace_run_file(&temporary, path).map_err(|error| {
            WorkflowServiceError::store(format!("failed to publish workflow run: {error}"))
        })?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_run_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace_run_file(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let existing = temporary
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let new = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct WorkflowFileLock {
    path: PathBuf,
}

impl WorkflowFileLock {
    fn acquire(runs_dir: &Path, key: &str) -> WorkflowServiceResult<Self> {
        let locks_dir = runs_dir.join(".locks");
        if !locks_dir.exists() {
            match fs::create_dir(&locks_dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(WorkflowServiceError::store(format!(
                        "failed to create workflow lock directory: {error}"
                    )))
                }
            }
        }
        let metadata = fs::symlink_metadata(&locks_dir).map_err(|error| {
            WorkflowServiceError::store(format!(
                "failed to inspect workflow lock directory: {error}"
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WorkflowServiceError::forbidden(
                "workflow_symlink_rejected",
                "workflow lock directory is not a regular project directory",
            ));
        }
        let path = locks_dir.join(format!("{}.lock", digest_bytes(key.as_bytes())));
        for attempt in 0..LOCK_RETRIES {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    let _ = file.sync_all();
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let backoff_ms = (5_u64 << attempt.min(7)).min(250);
                    thread::sleep(Duration::from_millis(backoff_ms));
                }
                Err(error) => {
                    return Err(WorkflowServiceError::store(format!(
                        "failed to acquire workflow lock: {error}"
                    )))
                }
            }
        }
        Err(WorkflowServiceError::conflict(
            "concurrent_workflow_mutation",
            "another process is mutating this workflow run",
        ))
    }
}

impl Drop for WorkflowFileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
    let service = FileWorkflowService::new(cwd).map_err(anyhow::Error::new)?;
    service
        .list_definitions()
        .map_err(anyhow::Error::new)
        .map(|definitions| {
            definitions
                .into_iter()
                .map(|definition| FileWorkflowScriptSummary {
                    name: definition.workflow,
                    path: workflow_scripts_dir(cwd)
                        .join(&definition.workflow_file)
                        .display()
                        .to_string(),
                    file: definition.workflow_file,
                    step_count: definition.step_count,
                    parse_error: definition.parse_error,
                })
                .collect()
        })
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
    let service = FileWorkflowService::new(cwd).map_err(anyhow::Error::new)?;
    let outcome = service
        .start(WorkflowStartOptions {
            workflow: workflow.to_string(),
            args,
            request_id: format!("tool-start-{}", Uuid::new_v4()),
        })
        .map_err(anyhow::Error::new)?;
    legacy_outcome(cwd, outcome)
}

pub fn load_file_workflow_run(
    cwd: &Path,
    run_id: &str,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let service = FileWorkflowService::new(cwd).map_err(anyhow::Error::new)?;
    let record = service.load_run(run_id).map_err(anyhow::Error::new)?;
    Ok((record, workflow_run_file(cwd, run_id)))
}

pub fn advance_file_workflow(
    cwd: &Path,
    run_id: &str,
    applied_status: Option<&str>,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let applied_status = WorkflowRunStatus::parse(applied_status.unwrap_or("completed"))
        .map_err(anyhow::Error::new)?;
    let service = FileWorkflowService::new(cwd).map_err(anyhow::Error::new)?;
    let outcome = service
        .advance(WorkflowAdvanceOptions {
            run_id: run_id.to_string(),
            applied_status,
            expected_revision: None,
            request_id: format!("tool-advance-{}", Uuid::new_v4()),
        })
        .map_err(anyhow::Error::new)?;
    legacy_outcome(cwd, outcome)
}

pub fn cancel_file_workflow(cwd: &Path, run_id: &str) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    let service = FileWorkflowService::new(cwd).map_err(anyhow::Error::new)?;
    let outcome = service
        .cancel(WorkflowCancelOptions {
            run_id: run_id.to_string(),
            expected_revision: None,
            request_id: format!("tool-cancel-{}", Uuid::new_v4()),
        })
        .map_err(anyhow::Error::new)?;
    legacy_outcome(cwd, outcome)
}

fn legacy_outcome(
    cwd: &Path,
    outcome: WorkflowMutationOutcome,
) -> Result<(FileWorkflowRunRecord, PathBuf)> {
    if let Some(warning) = outcome.projection_warning.as_deref() {
        tracing::warn!(
            run_id = %outcome.record.run_id,
            warning,
            "workflow mutation committed with a stale task projection"
        );
    }
    let path = workflow_run_file(cwd, &outcome.record.run_id);
    Ok((outcome.record, path))
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
        "revision": record.revision,
        "workspace_id": &record.workspace_id,
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

fn workflow_run_file(cwd: &Path, run_id: &str) -> PathBuf {
    workflow_runs_dir(cwd).join(format!("{run_id}.json"))
}

fn hidden_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

#[cfg(test)]
mod service_tests {
    use std::ffi::OsString;
    use std::sync::{Arc, Barrier};

    use super::*;

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
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
            if let Some(previous) = self.previous.take() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, EnvGuard) {
        let workspace = tempfile::tempdir().expect("workspace");
        let home = tempfile::tempdir().expect("home");
        let guard = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let definitions = workspace.path().join(".allthecodes/workflows");
        fs::create_dir_all(&definitions).expect("definitions directory");
        fs::write(
            definitions.join("release.md"),
            "- Plan release\n- Verify release\n- Ship release\n",
        )
        .expect("definition");
        (workspace, home, guard)
    }

    fn start(service: &FileWorkflowService, request_id: &str) -> WorkflowMutationOutcome {
        service
            .start(WorkflowStartOptions {
                workflow: "release".to_string(),
                args: Some(json!({"version": "1.0.0"})),
                request_id: request_id.to_string(),
            })
            .expect("start workflow")
    }

    #[test]
    #[serial_test::serial]
    fn service_lists_definitions_and_status_load_is_pure() {
        let (workspace, _home, _guard) = setup();
        let service = FileWorkflowService::new(workspace.path()).expect("service");
        let definitions = service.list_definitions().expect("definitions");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].workflow, "release");
        assert_eq!(definitions[0].step_count, 3);
        assert!(definitions[0].parse_error.is_none());

        let outcome = start(&service, "pure-read-start");
        let run_path =
            workflow_runs_dir(workspace.path()).join(format!("{}.json", outcome.record.run_id));
        let before = fs::read(&run_path).expect("run before read");
        let loaded = service.load_run(&outcome.record.run_id).expect("pure load");
        let after = fs::read(&run_path).expect("run after read");
        assert_eq!(loaded.revision, 1);
        assert_eq!(before, after, "status reads must not rewrite run state");
    }

    #[test]
    #[serial_test::serial]
    fn mutations_are_idempotent_revisioned_and_concurrency_safe() {
        let (workspace, _home, _guard) = setup();
        let service = FileWorkflowService::new(workspace.path()).expect("service");
        let first = start(&service, "start-request");
        let replay = start(&service, "start-request");
        assert!(replay.replayed);
        assert_eq!(first.record.run_id, replay.record.run_id);
        assert_eq!(replay.record.revision, 1);

        let changed = service.start(WorkflowStartOptions {
            workflow: "release".to_string(),
            args: Some(json!({"version": "2.0.0"})),
            request_id: "start-request".to_string(),
        });
        assert_eq!(
            changed.expect_err("changed retry conflicts").code,
            "idempotency_conflict"
        );

        let advanced = service
            .advance(WorkflowAdvanceOptions {
                run_id: first.record.run_id.clone(),
                applied_status: WorkflowRunStatus::Completed,
                expected_revision: Some(1),
                request_id: "advance-first".to_string(),
            })
            .expect("advance");
        assert_eq!(advanced.record.revision, 2);
        let replay = service
            .advance(WorkflowAdvanceOptions {
                run_id: first.record.run_id.clone(),
                applied_status: WorkflowRunStatus::Completed,
                expected_revision: Some(1),
                request_id: "advance-first".to_string(),
            })
            .expect("advance replay");
        assert!(replay.replayed);
        assert_eq!(replay.record.revision, 2);

        let service = Arc::new(service);
        let barrier = Arc::new(Barrier::new(3));
        let handles = ["advance-a", "advance-b"].map(|request_id| {
            let service = service.clone();
            let barrier = barrier.clone();
            let run_id = first.record.run_id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                service.advance(WorkflowAdvanceOptions {
                    run_id,
                    applied_status: WorkflowRunStatus::Completed,
                    expected_revision: Some(2),
                    request_id: request_id.to_string(),
                })
            })
        });
        barrier.wait();
        let results = handles.map(|handle| handle.join().expect("worker"));
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let conflict = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one conflict");
        assert!(matches!(
            conflict.code,
            "revision_conflict" | "concurrent_workflow_mutation"
        ));
        let final_record = service
            .load_run(&first.record.run_id)
            .expect("final record");
        assert_eq!(final_record.revision, 3);
        assert_eq!(final_record.current_step_index, Some(2));
    }

    #[test]
    #[serial_test::serial]
    fn terminal_cancel_rules_and_legacy_revision_migration_hold() {
        let (workspace, _home, _guard) = setup();
        let service = FileWorkflowService::new(workspace.path()).expect("service");
        let running = start(&service, "cancel-start");
        let cancelled = service
            .cancel(WorkflowCancelOptions {
                run_id: running.record.run_id.clone(),
                expected_revision: Some(1),
                request_id: "cancel-request".to_string(),
            })
            .expect("cancel");
        let replay = service
            .cancel(WorkflowCancelOptions {
                run_id: running.record.run_id,
                expected_revision: Some(1),
                request_id: "cancel-request".to_string(),
            })
            .expect("cancel replay");
        assert!(replay.replayed);
        assert_eq!(cancelled.record.revision, replay.record.revision);

        let completed = start(&service, "completed-start");
        let mut revision = completed.record.revision;
        for index in 0..3 {
            let result = service
                .advance(WorkflowAdvanceOptions {
                    run_id: completed.record.run_id.clone(),
                    applied_status: WorkflowRunStatus::Completed,
                    expected_revision: Some(revision),
                    request_id: format!("complete-{index}"),
                })
                .expect("complete step");
            revision = result.record.revision;
        }
        let terminal_cancel = service.cancel(WorkflowCancelOptions {
            run_id: completed.record.run_id.clone(),
            expected_revision: Some(revision),
            request_id: "late-cancel".to_string(),
        });
        assert_eq!(
            terminal_cancel.expect_err("terminal cancel conflicts").code,
            "terminal_workflow_run"
        );

        let legacy = start(&service, "legacy-start");
        let path =
            workflow_runs_dir(workspace.path()).join(format!("{}.json", legacy.record.run_id));
        let mut value: Value =
            serde_json::from_slice(&fs::read(&path).expect("legacy raw")).expect("legacy json");
        value
            .as_object_mut()
            .expect("record object")
            .remove("revision");
        value
            .as_object_mut()
            .expect("record object")
            .remove("workspace_id");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&value).expect("legacy bytes"),
        )
        .expect("legacy file");
        let loaded = service
            .load_run(&legacy.record.run_id)
            .expect("legacy load");
        assert_eq!(loaded.revision, 1);
        assert!(loaded.workspace_id.is_empty());
        let migrated = service
            .advance(WorkflowAdvanceOptions {
                run_id: legacy.record.run_id,
                applied_status: WorkflowRunStatus::Completed,
                expected_revision: Some(1),
                request_id: "legacy-advance".to_string(),
            })
            .expect("legacy mutation");
        assert_eq!(migrated.record.revision, 2);
        assert_eq!(migrated.record.workspace_id, service.workspace_id());
    }

    #[test]
    #[serial_test::serial]
    fn run_pages_are_filter_and_workspace_bound() {
        let (workspace, _home, _guard) = setup();
        let service = FileWorkflowService::new(workspace.path()).expect("service");
        for request_id in ["page-1", "page-2", "page-3"] {
            start(&service, request_id);
        }
        let first = service
            .list_runs(WorkflowRunListOptions {
                limit: Some(1),
                ..Default::default()
            })
            .expect("first page");
        assert_eq!(first.runs.len(), 1);
        assert!(first.truncated);
        let cursor = first.next_cursor.expect("cursor");
        let second = service
            .list_runs(WorkflowRunListOptions {
                cursor: Some(cursor.clone()),
                limit: Some(1),
                ..Default::default()
            })
            .expect("second page");
        assert_eq!(second.runs.len(), 1);
        assert_ne!(first.runs[0].run_id, second.runs[0].run_id);

        let foreign = tempfile::tempdir().expect("foreign workspace");
        let foreign_service = FileWorkflowService::new(foreign.path()).expect("foreign service");
        let error = foreign_service
            .list_runs(WorkflowRunListOptions {
                cursor: Some(cursor),
                limit: Some(1),
                ..Default::default()
            })
            .expect_err("foreign cursor rejected");
        assert_eq!(error.code, "invalid_cursor");

        let running = service
            .list_runs(WorkflowRunListOptions {
                status: Some(WorkflowRunStatus::Running),
                ..Default::default()
            })
            .expect("filtered runs");
        assert_eq!(running.runs.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn definition_symlinks_and_hostile_run_ids_are_rejected() {
        use std::os::unix::fs::symlink;

        let (workspace, _home, _guard) = setup();
        let outside = tempfile::NamedTempFile::new().expect("outside definition");
        fs::write(outside.path(), "- escaped\n").expect("outside content");
        symlink(
            outside.path(),
            workspace.path().join(".allthecodes/workflows/escape.md"),
        )
        .expect("definition symlink");
        let service = FileWorkflowService::new(workspace.path()).expect("service");
        let error = service
            .definition("escape")
            .expect_err("symlink definition rejected");
        assert_eq!(error.code, "workflow_symlink_rejected");

        for run_id in [
            "../workflow-run-12345678-1234-1234-1234-123456789abc",
            "workflow-run-12345678123412341234123456789abc",
            "workflow-run-not-a-uuid",
        ] {
            assert_eq!(
                service.load_run(run_id).expect_err("invalid run id").code,
                "invalid_run_id"
            );
        }
    }
}
