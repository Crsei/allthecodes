//! cc-tasks — task-domain types, parsing, errors, and persistence shapes.
//!
//! This crate owns the task domain and durable task store. Tool adapters and
//! UI rendering stay in their respective runtime crates.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::{sleep, Duration, Instant};
use tokio_util::sync::CancellationToken;

pub mod commands;
pub mod domain;
pub mod errors;
pub mod json;
pub mod lifecycle;
pub mod lists;
pub mod output;
mod repository;
pub mod store;
pub mod todo;
pub mod tool_requests;
pub mod types;

pub use commands::{parse_tasks_command, TasksCommand};
pub use domain::{
    DEFAULT_TASK_LIST_ID, REMOTE_TASK_TYPE_AUTOFIX_PR, REMOTE_TASK_TYPE_BACKGROUND_PR,
    REMOTE_TASK_TYPE_ENUM, REMOTE_TASK_TYPE_REMOTE_AGENT, REMOTE_TASK_TYPE_ULTRAPLAN,
    REMOTE_TASK_TYPE_ULTRAREVIEW, TASK_CREATE_KIND_ENUM, TASK_KIND_DREAM,
    TASK_KIND_IN_PROCESS_TEAMMATE, TASK_KIND_LOCAL_AGENT, TASK_KIND_LOCAL_BASH,
    TASK_KIND_LOCAL_WORKFLOW, TASK_KIND_MONITOR_MCP, TASK_KIND_REMOTE_AGENT, TASK_KIND_TOOL,
};
pub use errors::{TaskError, TaskErrorCode};
pub use json::task_to_json_from_store;
pub use lifecycle::{
    is_remote_recoverable_task, is_remote_review_task, normalize_loaded_status,
    normalize_new_status, recover_task_after_restart, remote_review_timed_out,
    REMOTE_REVIEW_TIMEOUT_MS,
};
pub use lists::{
    global_store, sanitize_task_list_id, store_for_task_list_id, task_list_dir,
    task_list_id_from_parts, unassign_teammate_tasks, TaskListScope,
};
pub use output::{
    parse_task_output_timeout_ms, task_output_payload, wait_for_task_output,
    DEFAULT_TASK_OUTPUT_TIMEOUT_MS, MAX_TASK_OUTPUT_TIMEOUT_MS,
};
pub use store::TaskStore;
pub use todo::{parse_todo_items, replace_todos_for_key, todo_owner_key};
pub use tool_requests::{
    dependency_ids_from_input, normalize_dependencies, normalize_optional_string,
    parse_task_create, parse_task_id, parse_task_update, string_array_field, task_id_from_input,
    task_update_fields_from_input, task_updated_fields_from_input, TaskCreateRequest,
    TaskUpdateAction, TaskUpdateRequest,
};
pub use types::{
    default_task_kind, PersistedTaskFile, PersistedTaskRecord, TaskClaimFailure,
    TaskClaimFailureReason, TaskCreateOptions, TaskEntry, TaskOutputRetrievalStatus,
    TaskOutputWaitResult, TaskRuntimeHandle, TaskStatus, TaskUpdateFields, TeammateTaskExitReason,
    TodoItem, TodoWriteOutcome, UnassignTeammateTasksResult, UnassignedTaskSummary,
};

const TASK_SCHEMA_VERSION: u32 = 5;
const DEFAULT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024;
const OUTPUT_SUMMARY_MAX_CHARS: usize = 2_000;
const TASK_OUTPUT_POLL_INTERVAL_MS: u64 = 100;
const TASK_HIGHWATERMARK_FILE: &str = ".highwatermark";
const TASK_HIGHWATERMARK_LOCK_FILE: &str = ".highwatermark.lock";
const TASK_HIGHWATERMARK_LOCK_RETRIES: usize = 30;
const TASK_LIST_LOCK_FILE: &str = ".lock";
#[cfg(not(test))]
const TASK_LIST_LOCK_RETRIES: usize = 30;
#[cfg(test)]
const TASK_LIST_LOCK_RETRIES: usize = 3;
const ALLTHECODES_TASK_LIST_ID_ENV: &str = "ALLTHECODES_TASK_LIST_ID";
const CC_RUST_TASK_LIST_ID_ENV: &str = "CC_RUST_TASK_LIST_ID";
const ALLTHECODES_TEAM_NAME_ENV: &str = "ALLTHECODES_TEAM_NAME";
const CLAUDE_CODE_TASK_LIST_ID_ENV: &str = "CLAUDE_CODE_TASK_LIST_ID";
const CLAUDE_CODE_TEAM_NAME_ENV: &str = "CLAUDE_CODE_TEAM_NAME";

fn refresh_output_metadata(entry: &mut TaskEntry) {
    entry.output_bytes = entry.output.len();
    entry.output_summary = summarize_output(&entry.output);
}

fn summarize_output(output: &str) -> String {
    if output.chars().count() <= OUTPUT_SUMMARY_MAX_CHARS {
        return output.to_string();
    }
    let tail: String = output
        .chars()
        .rev()
        .take(OUTPUT_SUMMARY_MAX_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    tail
}

fn trim_output_to_limit(output: &str, limit: usize) -> (String, bool) {
    if output.len() <= limit {
        return (output.to_string(), false);
    }

    let mut start = output.len().saturating_sub(limit);
    while start < output.len() && !output.is_char_boundary(start) {
        start += 1;
    }
    (output[start..].to_string(), true)
}

fn blocked_dependencies_with_tasks(
    tasks: &HashMap<String, TaskEntry>,
    entry: &TaskEntry,
) -> Vec<String> {
    entry
        .depends_on
        .iter()
        .filter(|id| {
            tasks
                .get(*id)
                .map(|dep| !dep.status.is_success())
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

fn highest_numeric_task_id(tasks: &HashMap<String, TaskEntry>) -> u64 {
    tasks
        .keys()
        .filter_map(|id| id.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn fallback_task_id(tasks: &HashMap<String, TaskEntry>) -> String {
    loop {
        let id = uuid::Uuid::new_v4().to_string();
        if !tasks.contains_key(&id) {
            return id;
        }
    }
}

fn merge_metadata(existing: Option<Value>, patch: &Value) -> Option<Value> {
    let Some(patch_map) = patch.as_object() else {
        return existing;
    };
    let mut merged = existing
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    for (key, value) in patch_map {
        if value.is_null() {
            merged.remove(key);
        } else {
            merged.insert(key.clone(), value.clone());
        }
    }
    Some(Value::Object(merged))
}

fn normalize_remote_task_type(value: Option<String>) -> Option<String> {
    let value = normalize_optional_string(value)?;
    let normalized = value.to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        REMOTE_TASK_TYPE_REMOTE_AGENT => Some(REMOTE_TASK_TYPE_REMOTE_AGENT.to_string()),
        REMOTE_TASK_TYPE_ULTRAPLAN => Some(REMOTE_TASK_TYPE_ULTRAPLAN.to_string()),
        REMOTE_TASK_TYPE_ULTRAREVIEW => Some(REMOTE_TASK_TYPE_ULTRAREVIEW.to_string()),
        REMOTE_TASK_TYPE_AUTOFIX_PR => Some(REMOTE_TASK_TYPE_AUTOFIX_PR.to_string()),
        REMOTE_TASK_TYPE_BACKGROUND_PR => Some(REMOTE_TASK_TYPE_BACKGROUND_PR.to_string()),
        _ => Some(value),
    }
}

fn sanitize_kind(kind: &str) -> String {
    let trimmed = kind.trim();
    if trimmed.is_empty() {
        return default_task_kind();
    }
    let sanitized: String = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    match sanitized.to_ascii_lowercase().as_str() {
        "local_shell" | "local-bash" | "bash" => TASK_KIND_LOCAL_BASH.to_string(),
        "local-agent" => TASK_KIND_LOCAL_AGENT.to_string(),
        "remote-agent" => TASK_KIND_REMOTE_AGENT.to_string(),
        "teammate" | "team" | "in-process-teammate" => TASK_KIND_IN_PROCESS_TEAMMATE.to_string(),
        "workflow" | "local-workflow" => TASK_KIND_LOCAL_WORKFLOW.to_string(),
        "monitor-mcp" => TASK_KIND_MONITOR_MCP.to_string(),
        "dream" => TASK_KIND_DREAM.to_string(),
        "tool" => TASK_KIND_TOOL.to_string(),
        _ => sanitized,
    }
}

fn safe_file_stem(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn output_file_name(id: &str) -> String {
    format!("{}.output.log", safe_file_stem(id))
}

fn write_text_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file"),
        uuid::Uuid::new_v4()
    ));
    fs::write(&tmp, contents).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", path.display())),
    }
}
