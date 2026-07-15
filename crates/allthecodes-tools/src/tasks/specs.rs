use allthecodes_tasks::{REMOTE_TASK_TYPE_ENUM, TASK_CREATE_KIND_ENUM};
use serde_json::{json, Value};

pub const TODO_WRITE_NAME: &str = "TodoWrite";
pub const TASK_CREATE_NAME: &str = "TaskCreate";
pub const TASK_GET_NAME: &str = "TaskGet";
pub const TASK_UPDATE_NAME: &str = "TaskUpdate";
pub const TASK_LIST_NAME: &str = "TaskList";
pub const TASK_STOP_NAME: &str = "TaskStop";
pub const TASK_OUTPUT_NAME: &str = "TaskOutput";
pub const DELEGATE_TASK_NAME: &str = "DelegateTask";

pub const TASK_OUTPUT_DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const TASK_OUTPUT_MAX_TIMEOUT_MS: u64 = 600_000;

pub fn todo_write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "todos": {
                "type": "array",
                "description": "Complete replacement todo list for the current session or agent",
                "items": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string", "description": "Todo item text" },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"],
                            "description": "Todo status"
                        },
                        "activeForm": {
                            "type": "string",
                            "description": "Optional in-progress wording for UI display"
                        }
                    },
                    "required": ["content", "status"]
                }
            }
        },
        "required": ["todos"]
    })
}

pub fn task_create_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subject": { "type": "string", "description": "A brief title for the task" },
            "description": { "type": "string", "description": "What needs to be done" },
            "activeForm": {
                "type": "string",
                "description": "Present continuous form shown while the task is in progress"
            },
            "metadata": {
                "type": "object",
                "description": "Arbitrary metadata to attach to the task"
            },
            "kind": {
                "type": "string",
                "description": "Stable task type for persisted records; legacy aliases are accepted and normalized",
                "enum": TASK_CREATE_KIND_ENUM
            },
            "parent_id": { "type": "string", "description": "Optional parent task ID" },
            "depends_on": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Task IDs that should complete before this task"
            },
            "blocked_by": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Bun-compatible alias for depends_on"
            },
            "blockedBy": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Bun-compatible camelCase alias for depends_on"
            },
            "owner": {
                "type": "string",
                "description": "Optional owner that has claimed this task"
            },
            "tool_use_id": {
                "type": "string",
                "description": "Optional upstream tool use ID associated with this task"
            },
            "agent_id": {
                "type": "string",
                "description": "Optional agent ID associated with this task"
            },
            "supervisor_id": {
                "type": "string",
                "description": "Optional runtime supervisor ID for this task"
            },
            "isolation": {
                "type": "string",
                "description": "Optional runtime isolation label, such as worktree"
            },
            "worktree_path": {
                "type": "string",
                "description": "Optional worktree path for isolated task execution"
            },
            "worktree_branch": {
                "type": "string",
                "description": "Optional worktree branch for isolated task execution"
            },
            "remote_task_type": {
                "type": "string",
                "enum": REMOTE_TASK_TYPE_ENUM,
                "description": "Optional remote task subtype used by remote-agent supervisors"
            },
            "remote_session_id": {
                "type": "string",
                "description": "Optional remote session ID used to restore or poll a remote task"
            },
            "remote_task_metadata": {
                "type": "object",
                "description": "Optional remote task metadata, such as repository or pull request identifiers"
            },
            "poll_started_at": {
                "type": "integer",
                "description": "Optional remote poll start timestamp in milliseconds since epoch"
            }
        },
        "required": ["subject", "description"]
    })
}

pub fn task_get_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string", "description": "The task ID to look up" }
        },
        "anyOf": [
            { "required": ["task_id"] },
            { "required": ["taskId"] }
        ]
    })
}

pub fn task_update_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string", "description": "The task ID to update" },
            "taskId": {
                "type": "string",
                "description": "Bun-compatible alias for task_id"
            },
            "subject": { "type": "string", "description": "New subject for the task" },
            "description": { "type": "string", "description": "New description for the task" },
            "activeForm": {
                "type": "string",
                "description": "Present continuous form shown while the task is in progress"
            },
            "status": {
                "type": "string",
                "enum": ["pending", "in_progress", "completed", "failed", "cancelled", "recoverable", "interrupted", "deleted"],
                "description": "New status for the task"
            },
            "addBlocks": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Task IDs that this task blocks"
            },
            "addBlockedBy": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Task IDs that block this task"
            },
            "owner": {
                "type": "string",
                "description": "Agent or session owner for this task"
            },
            "metadata": {
                "type": "object",
                "description": "Metadata keys to merge into the task; null values delete keys"
            },
            "check_agent_busy": {
                "type": "boolean",
                "description": "When claiming, fail if the same owner already has another unfinished task"
            },
            "checkAgentBusy": {
                "type": "boolean",
                "description": "Bun-compatible camelCase alias for check_agent_busy"
            }
        },
        "required": ["task_id"]
    })
}

pub fn task_list_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

pub fn task_stop_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": { "type": "string", "description": "The task ID to cancel" }
        },
        "required": ["task_id"]
    })
}

pub fn task_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "task_id": {
                "type": "string",
                "description": "The task ID whose output to retrieve"
            },
            "block": {
                "type": "boolean",
                "description": "Whether to wait for the task to leave pending/running state",
                "default": true
            },
            "timeout": {
                "type": "integer",
                "minimum": 0,
                "maximum": TASK_OUTPUT_MAX_TIMEOUT_MS,
                "description": "Maximum wait time in milliseconds when block=true",
                "default": TASK_OUTPUT_DEFAULT_TIMEOUT_MS
            },
            "after_seq": {
                "type": "integer",
                "minimum": 0,
                "description": "When set, return incremental output_events after this sequence"
            },
            "limit_bytes": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1048576,
                "description": "Maximum retained output event bytes to return when after_seq is set",
                "default": 1048576
            }
        },
        "required": ["task_id"]
    })
}

pub fn delegate_task_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "role": {
                "type": "string",
                "description": "Agent role or subagent type to delegate to"
            },
            "prompt": {
                "type": "string",
                "description": "Task prompt for the child agent"
            },
            "model": {
                "type": "string",
                "description": "Optional model override for the delegated child agent"
            },
            "cwd": {
                "type": "string",
                "description": "Optional working directory for the delegated task"
            },
            "worktree": {
                "type": "string",
                "description": "Optional worktree slug. When present, the child task is marked for worktree isolation."
            },
            "max_turns": {
                "type": "integer",
                "minimum": 1,
                "description": "Optional maximum child-agent turns"
            },
            "verification_policy": {
                "type": "string",
                "description": "Optional verification expectation for the child task"
            }
        },
        "required": ["role", "prompt"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_schemas_have_object_properties() {
        for schema in [
            todo_write_schema(),
            task_create_schema(),
            task_get_schema(),
            task_update_schema(),
            task_list_schema(),
            task_stop_schema(),
            task_output_schema(),
            delegate_task_schema(),
        ] {
            assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
            assert!(schema.get("properties").is_some());
        }
    }
}
