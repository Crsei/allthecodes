//! Tool-input parsing and validation for task tools.

use serde_json::Value;

use crate::{TaskCreateOptions, TaskError, TaskStatus, TaskUpdateFields};

#[derive(Debug, Clone)]
pub struct TaskCreateRequest {
    pub subject: String,
    pub description: String,
    pub options: TaskCreateOptions,
    pub has_options: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskUpdateAction {
    Delete,
    Claim,
    Update,
}

#[derive(Debug, Clone)]
pub struct TaskUpdateRequest {
    pub id: String,
    pub status: Option<TaskStatus>,
    pub status_value: Option<String>,
    pub action: TaskUpdateAction,
    pub fields: TaskUpdateFields,
    pub updated_fields: Vec<&'static str>,
    pub owner: String,
    pub check_agent_busy: bool,
}

pub fn parse_task_create(input: &Value) -> TaskCreateRequest {
    let subject = input
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let description = input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let options = TaskCreateOptions {
        kind: string_field(input, "kind"),
        parent_id: string_field(input, "parent_id"),
        depends_on: dependency_ids_from_input(input),
        owner: string_field(input, "owner"),
        active_form: string_field(input, "activeForm"),
        metadata: input.get("metadata").filter(|v| v.is_object()).cloned(),
        tool_use_id: string_field(input, "tool_use_id"),
        agent_id: string_field(input, "agent_id"),
        supervisor_id: string_field(input, "supervisor_id"),
        isolation: string_field(input, "isolation"),
        worktree_path: string_field(input, "worktree_path"),
        worktree_branch: string_field(input, "worktree_branch"),
        remote_task_type: string_field(input, "remote_task_type"),
        remote_session_id: string_field(input, "remote_session_id"),
        remote_task_metadata: input
            .get("remote_task_metadata")
            .filter(|v| v.is_object())
            .cloned(),
        poll_started_at: input.get("poll_started_at").and_then(|v| v.as_i64()),
    };
    let has_options = options.kind.is_some()
        || options.parent_id.is_some()
        || !options.depends_on.is_empty()
        || options.owner.is_some()
        || options.active_form.is_some()
        || options.metadata.is_some()
        || options.tool_use_id.is_some()
        || options.agent_id.is_some()
        || options.supervisor_id.is_some()
        || options.isolation.is_some()
        || options.worktree_path.is_some()
        || options.worktree_branch.is_some()
        || options.remote_task_type.is_some()
        || options.remote_session_id.is_some()
        || options.remote_task_metadata.is_some()
        || options.poll_started_at.is_some();

    TaskCreateRequest {
        subject,
        description,
        options,
        has_options,
    }
}

pub fn parse_task_id(input: &Value) -> Result<String, TaskError> {
    let id = task_id_from_input(input).trim();
    if id.is_empty() {
        Err(TaskError::missing_field("task_id", "Missing task_id"))
    } else {
        Ok(id.to_string())
    }
}

pub fn parse_task_update(
    input: &Value,
    default_owner: impl Into<String>,
) -> Result<TaskUpdateRequest, TaskError> {
    let id = parse_task_id(input)?;
    let status_value = input.get("status").and_then(|v| v.as_str());
    let status = match status_value {
        Some("deleted") | None => None,
        Some(status_str) => TaskStatus::from_str(status_str)
            .ok_or_else(|| TaskError::invalid_field("status", status_str))
            .map(Some)?,
    };
    let action = match (status_value, status) {
        (Some("deleted"), _) => TaskUpdateAction::Delete,
        (_, Some(TaskStatus::InProgress)) => TaskUpdateAction::Claim,
        _ => TaskUpdateAction::Update,
    };

    let owner = input
        .get("owner")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| default_owner.into());

    let mut fields = task_update_fields_from_input(input);
    fields.status = status;
    let updated_fields = task_updated_fields_from_input(input, status_value);

    Ok(TaskUpdateRequest {
        id,
        status,
        status_value: status_value.map(ToString::to_string),
        action,
        fields,
        updated_fields,
        owner,
        check_agent_busy: bool_alias(input, "check_agent_busy", "checkAgentBusy"),
    })
}

pub fn task_id_from_input(input: &Value) -> &str {
    input
        .get("task_id")
        .or_else(|| input.get("taskId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

pub fn task_update_fields_from_input(input: &Value) -> TaskUpdateFields {
    TaskUpdateFields {
        subject: string_field(input, "subject"),
        description: string_field(input, "description"),
        active_form: input
            .get("activeForm")
            .map(|value| normalize_optional_string(value.as_str().map(ToString::to_string))),
        owner: input
            .get("owner")
            .map(|value| normalize_optional_string(value.as_str().map(ToString::to_string))),
        metadata_patch: input
            .get("metadata")
            .filter(|value| value.is_object())
            .cloned(),
        status: None,
        add_blocks: string_array_field(input, "addBlocks"),
        add_blocked_by: string_array_field(input, "addBlockedBy"),
    }
}

pub fn task_updated_fields_from_input(
    input: &Value,
    status_value: Option<&str>,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    for (input_key, field_name) in [
        ("subject", "subject"),
        ("description", "description"),
        ("activeForm", "activeForm"),
        ("owner", "owner"),
        ("metadata", "metadata"),
        ("addBlocks", "blocks"),
        ("addBlockedBy", "blockedBy"),
    ] {
        if input.get(input_key).is_some() {
            fields.push(field_name);
        }
    }
    if status_value.is_some() {
        fields.push("status");
    }
    fields
}

pub fn dependency_ids_from_input(input: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for field in ["depends_on", "blocked_by", "blockedBy"] {
        ids.extend(string_array_field(input, field));
    }
    normalize_dependencies(ids)
}

pub fn string_array_field(input: &Value, field: &str) -> Vec<String> {
    input
        .get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn normalize_dependencies(depends_on: Vec<String>) -> Vec<String> {
    let mut deps = Vec::new();
    for dep in depends_on {
        let dep = dep.trim();
        if dep.is_empty() || deps.iter().any(|existing: &String| existing == dep) {
            continue;
        }
        deps.push(dep.to_string());
    }
    deps
}

pub fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn string_field(input: &Value, field: &str) -> Option<String> {
    input
        .get(field)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn bool_alias(input: &Value, snake: &str, camel: &str) -> bool {
    input
        .get(snake)
        .or_else(|| input.get(camel))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn create_accepts_dependency_aliases() {
        let request = parse_task_create(&json!({
            "subject": "s",
            "description": "d",
            "depends_on": [" a "],
            "blockedBy": ["a", "b"]
        }));

        assert_eq!(request.options.depends_on, vec!["a", "b"]);
        assert!(request.has_options);
    }

    #[test]
    fn update_returns_structured_invalid_status() {
        let error = parse_task_update(
            &json!({"task_id": "1", "status": "wat"}),
            "agent-a".to_string(),
        )
        .unwrap_err();

        assert_eq!(error.code, crate::TaskErrorCode::InvalidField);
        assert_eq!(error.field, Some("status"));
    }

    #[test]
    fn update_classifies_claim_action() {
        let request = parse_task_update(
            &json!({"taskId": "1", "status": "in_progress", "checkAgentBusy": true}),
            "agent-a".to_string(),
        )
        .unwrap();

        assert_eq!(request.id, "1");
        assert_eq!(request.action, TaskUpdateAction::Claim);
        assert_eq!(request.owner, "agent-a");
        assert!(request.check_agent_busy);
    }
}
