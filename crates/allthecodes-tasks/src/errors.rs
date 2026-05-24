//! Structured task-domain errors exposed by tool and command adapters.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskErrorCode {
    MissingField,
    InvalidField,
    NotFound,
    ClaimFailed,
}

impl TaskErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskErrorCode::MissingField => "missing_field",
            TaskErrorCode::InvalidField => "invalid_field",
            TaskErrorCode::NotFound => "not_found",
            TaskErrorCode::ClaimFailed => "claim_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskError {
    pub code: TaskErrorCode,
    pub message: String,
    pub field: Option<&'static str>,
    pub task_id: Option<String>,
}

impl TaskError {
    pub fn missing_field(field: &'static str, usage: impl Into<String>) -> Self {
        Self {
            code: TaskErrorCode::MissingField,
            message: usage.into(),
            field: Some(field),
            task_id: None,
        }
    }

    pub fn invalid_field(field: &'static str, value: impl AsRef<str>) -> Self {
        let value = value.as_ref();
        Self {
            code: TaskErrorCode::InvalidField,
            message: format!("Invalid {field}: {value}"),
            field: Some(field),
            task_id: None,
        }
    }

    pub fn not_found(task_id: impl Into<String>) -> Self {
        let task_id = task_id.into();
        Self {
            code: TaskErrorCode::NotFound,
            message: format!("Task not found: {task_id}"),
            field: Some("task_id"),
            task_id: Some(task_id),
        }
    }

    pub fn claim_failed(reason: impl AsRef<str>) -> Self {
        let reason = reason.as_ref();
        Self {
            code: TaskErrorCode::ClaimFailed,
            message: format!("Task claim failed: {reason}"),
            field: None,
            task_id: None,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code.as_str(),
            "message": self.message,
            "field": self.field,
            "task_id": self.task_id,
        })
    }
}
