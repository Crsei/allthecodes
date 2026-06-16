//! Tool adapters for the `cc-tasks` task domain.

use std::sync::Arc;

pub mod specs;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_tasks::{
    parse_task_create, parse_task_id, parse_task_output_limit_bytes, parse_task_output_timeout_ms,
    parse_task_update, replace_todos_for_key, task_list_id_from_parts, task_output_payload,
    task_output_payload_with_events, task_to_json_from_store, todo_owner_key, wait_for_task_output,
    TaskEntry, TaskError, TaskListScope, TaskOutputRetrievalStatus, TaskOutputWaitResult,
    TaskStatus, TaskStore, TaskUpdateAction,
};
use allthecodes_types::message::AssistantMessage;

fn task_error_result(error: TaskError) -> ToolResult {
    ToolResult {
        data: json!({
            "error": error.message,
            "task_error": error.to_json(),
        }),
        new_messages: vec![],
        ..Default::default()
    }
}

fn task_list_id_for_context(ctx: &ToolUseContext) -> String {
    let app_state = (ctx.get_app_state)();
    task_list_id_from_parts(TaskListScope {
        explicit_task_list_id: None,
        scoped_team_name: None,
        app_team_name: app_state
            .team_context
            .as_ref()
            .map(|team| team.team_name.clone()),
        session_id: Some(ctx.session_id.clone()),
    })
}

fn store_for_context(ctx: &ToolUseContext) -> TaskStore {
    allthecodes_tasks::store_for_task_list_id(&task_list_id_for_context(ctx))
}

fn default_update_owner(input: &Value, ctx: &ToolUseContext) -> String {
    input
        .get("owner")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            ctx.agent_id
                .as_deref()
                .map(str::trim)
                .filter(|owner| !owner.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| ctx.session_id.clone())
}

fn plan_workflow_cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| {
        let fallback = std::env::temp_dir().join("allthecodes-plan-workflow");
        let _ = std::fs::create_dir_all(fallback.join(".allthecodes"));
        fallback
    })
}

fn maybe_link_plan_workflow_task(
    ctx: &ToolUseContext,
    entry: &TaskEntry,
) -> Result<Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>> {
    let cwd = plan_workflow_cwd();
    let existing = match crate::workflow::plan::load(&cwd) {
        Ok(record) => record,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "failed to load plan workflow for task link; continuing without link"
            );
            None
        }
    };
    let persist_cwd = cwd.clone();
    let task_id = entry.id.clone();
    let summary = Some(entry.subject.clone());
    let slot: Arc<
        parking_lot::Mutex<Option<Option<allthecodes_types::plan_workflow::PlanWorkflowRecord>>>,
    > = Arc::new(parking_lot::Mutex::new(None));
    let slot_for_update = Arc::clone(&slot);

    (ctx.set_app_state)(Box::new(move |mut state| {
        let linked = crate::workflow::plan::maybe_link_implementation_task_state(
            &mut state,
            &cwd,
            existing,
            "main",
            "task_create",
            task_id,
            summary,
        );
        *slot_for_update.lock() = Some(linked);
        state
    }));

    let record = slot.lock().clone().unwrap_or(None);
    if let Some(record) = &record {
        crate::workflow::plan::persist(&persist_cwd, record)?;
    }
    Ok(record)
}

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TODO_WRITE_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Replace the current session todo list with the provided todos array.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::todo_write_schema()
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match allthecodes_tasks::parse_todo_items(input) {
            Ok(_) => ValidationResult::Ok,
            Err(message) => ValidationResult::Error {
                message,
                error_code: 1,
            },
        }
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let todos = match allthecodes_tasks::parse_todo_items(&input) {
            Ok(todos) => todos,
            Err(message) => {
                return Ok(ToolResult {
                    data: json!({ "error": message }),
                    new_messages: vec![],
                    ..Default::default()
                });
            }
        };
        let key = todo_owner_key(&ctx.session_id, ctx.agent_id.as_deref());
        let outcome = replace_todos_for_key(&key, todos);
        let mut data = json!({
            "todos": outcome.todos,
            "count": outcome.todos.len(),
            "cleared": outcome.cleared,
            "message": if outcome.cleared {
                "Todo list cleared because all items are completed"
            } else {
                "Todo list updated"
            },
        });

        if outcome.verification_nudge_needed {
            if let Some(map) = data.as_object_mut() {
                map.insert(
                    "verification_nudge".to_string(),
                    json!(
                        "You completed 3+ todo items without a verification step; consider adding or running verification before claiming completion."
                    ),
                );
            }
        }

        Ok(ToolResult {
            data,
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Track progress by replacing the current todo list with a complete todos array. Use pending, in_progress, and completed statuses.".to_string()
    }
}

pub struct TaskCreateTool;

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_CREATE_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Create a new task to track work progress.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_create_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let request = parse_task_create(&input);
        let task_store = store_for_context(ctx);
        let entry = if request.has_options {
            task_store.try_create_with_options(
                &request.subject,
                &request.description,
                request.options,
            )
        } else {
            task_store.try_create(&request.subject, &request.description)
        }?;

        let linked_plan_workflow = maybe_link_plan_workflow_task(ctx, &entry)?;
        let app_state = (ctx.get_app_state)();
        let configs = allthecodes_types::hooks::load_hook_configs(&app_state.hooks, "TaskCreated");
        if !configs.is_empty() {
            let payload = json!({
                "task_id": &entry.id,
                "subject": &entry.subject,
                "description": &entry.description,
            });
            let _ = crate::hooks::run_event_hooks("TaskCreated", &payload, &configs).await;
        }

        let mut data = json!({
            "task": task_to_json_from_store(&task_store, &entry),
            "message": format!("Created task: {}", entry.subject)
        });
        if let Some(record) = linked_plan_workflow {
            if let Some(map) = data.as_object_mut() {
                map.insert("plan_workflow".to_string(), json!(record));
            }
        }

        Ok(ToolResult {
            data,
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "Create tasks to track your progress on complex work.".to_string()
    }
}

pub struct TaskGetTool;

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_GET_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Get details of a task by ID.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_get_schema()
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let id = match parse_task_id(&input) {
            Ok(id) => id,
            Err(error) => return Ok(task_error_result(error)),
        };

        let task_store = store_for_context(ctx);
        match task_store.get(&id) {
            Some(entry) => Ok(ToolResult {
                data: json!({ "task": task_to_json_from_store(&task_store, &entry) }),
                new_messages: vec![],
                ..Default::default()
            }),
            None => Ok(task_error_result(TaskError::not_found(id))),
        }
    }

    async fn prompt(&self) -> String {
        "Get the current status of a task.".to_string()
    }
}

pub struct TaskUpdateTool;

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_UPDATE_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Update a task's status.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_update_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let request = match parse_task_update(&input, default_update_owner(&input, ctx)) {
            Ok(request) => request,
            Err(error) => return Ok(task_error_result(error)),
        };

        let task_store = store_for_context(ctx);
        let Some(existing) = task_store.get(&request.id) else {
            return Ok(task_error_result(TaskError::not_found(request.id)));
        };

        if request.action == TaskUpdateAction::Delete {
            let deleted = task_store.try_delete(&request.id)?.is_some();
            return Ok(ToolResult {
                data: json!({
                    "success": deleted,
                    "task_id": request.id,
                    "updated_fields": if deleted { vec!["deleted"] } else { Vec::<&str>::new() },
                    "status_change": if deleted {
                        json!({ "from": existing.status.as_str(), "to": "deleted" })
                    } else {
                        Value::Null
                    },
                    "message": if deleted {
                        format!("Task '{}' deleted", existing.subject)
                    } else {
                        format!("Task not found: {}", request.id)
                    },
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        if request.action == TaskUpdateAction::Claim {
            let entry = match task_store.claim_task(
                &request.id,
                &request.owner,
                request.check_agent_busy,
            ) {
                Ok(entry) => entry,
                Err(failure) => {
                    let error = TaskError::claim_failed(failure.reason.as_str());
                    return Ok(ToolResult {
                        data: json!({
                            "error": error.message,
                            "task_error": error.to_json(),
                            "claim": failure.to_json(),
                        }),
                        new_messages: vec![],
                        ..Default::default()
                    });
                }
            };
            let mut updates = request.fields.clone();
            updates.status = None;
            updates.owner = None;
            let entry = if updates.subject.is_some()
                || updates.description.is_some()
                || updates.active_form.is_some()
                || updates.metadata_patch.is_some()
                || !updates.add_blocks.is_empty()
                || !updates.add_blocked_by.is_empty()
            {
                task_store
                    .try_update_fields(&request.id, updates)?
                    .unwrap_or(entry)
            } else {
                entry
            };
            return Ok(ToolResult {
                data: json!({
                    "task": task_to_json_from_store(&task_store, &entry),
                    "updated_fields": ["status", "owner"],
                    "status_change": { "from": existing.status.as_str(), "to": TaskStatus::InProgress.as_str() },
                    "message": format!("Task '{}' claimed by {}", entry.subject, request.owner)
                }),
                new_messages: vec![],
                ..Default::default()
            });
        }

        let updates = request.fields.clone();
        let updated_fields = request.updated_fields;

        match task_store.try_update_fields(&request.id, updates)? {
            Some(entry) => {
                if request.status == Some(TaskStatus::Completed)
                    && existing.status != TaskStatus::Completed
                {
                    let app_state = (ctx.get_app_state)();
                    let configs = allthecodes_types::hooks::load_hook_configs(
                        &app_state.hooks,
                        "TaskCompleted",
                    );
                    if !configs.is_empty() {
                        let payload = json!({
                            "task_id": &entry.id,
                            "subject": &entry.subject,
                            "status": entry.status.as_str(),
                        });
                        let _ = crate::hooks::run_event_hooks("TaskCompleted", &payload, &configs)
                            .await;
                    }
                }

                Ok(ToolResult {
                    data: json!({
                        "task": task_to_json_from_store(&task_store, &entry),
                        "updated_fields": updated_fields,
                        "status_change": request.status.map(|new_status| json!({
                            "from": existing.status.as_str(),
                            "to": new_status.as_str()
                        })),
                        "message": if let Some(status) = request.status {
                            format!("Task '{}' updated to {}", entry.subject, status.as_str())
                        } else {
                            format!("Task '{}' updated", entry.subject)
                        }
                    }),
                    new_messages: vec![],
                    ..Default::default()
                })
            }
            None => Ok(task_error_result(TaskError::not_found(request.id))),
        }
    }

    async fn prompt(&self) -> String {
        "Update task status to track progress.".to_string()
    }
}

pub struct TaskListTool;

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_LIST_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "List all tasks and their statuses.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_list_schema()
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let task_store = store_for_context(ctx);
        let entries = task_store.list();
        let tasks: Vec<Value> = entries
            .iter()
            .map(|entry| task_to_json_from_store(&task_store, entry))
            .collect();

        Ok(ToolResult {
            data: json!({
                "tasks": tasks,
                "count": tasks.len()
            }),
            new_messages: vec![],
            ..Default::default()
        })
    }

    async fn prompt(&self) -> String {
        "List all tasks to see current progress.".to_string()
    }
}

pub struct TaskStopTool;

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_STOP_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Cancel a running task.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_stop_schema()
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let id = match parse_task_id(&input) {
            Ok(id) => id,
            Err(error) => return Ok(task_error_result(error)),
        };

        let task_store = store_for_context(ctx);
        match task_store.try_stop(&id)? {
            Some(entry) => Ok(ToolResult {
                data: json!({
                    "task": task_to_json_from_store(&task_store, &entry),
                    "message": format!("Task '{}' cancelled", entry.subject)
                }),
                new_messages: vec![],
                ..Default::default()
            }),
            None => Ok(task_error_result(TaskError::not_found(id))),
        }
    }

    async fn prompt(&self) -> String {
        "Cancel a running task.".to_string()
    }
}

pub struct TaskOutputTool;

fn task_output_payload_for_tool(
    task_store: &TaskStore,
    entry: &TaskEntry,
    retrieval_status: TaskOutputRetrievalStatus,
    after_seq: Option<u64>,
    limit_bytes: usize,
) -> Result<Value> {
    if after_seq.is_some() {
        let Some(events) = task_store.read_output_events(&entry.id, after_seq, limit_bytes)? else {
            return Ok(task_output_payload(entry, retrieval_status));
        };
        Ok(task_output_payload_with_events(
            entry,
            retrieval_status,
            Some(events),
        ))
    } else {
        Ok(task_output_payload(entry, retrieval_status))
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        crate::tasks::specs::TASK_OUTPUT_NAME
    }

    async fn description(&self, _: &Value) -> String {
        "Get the retained output/log of a task.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        crate::tasks::specs::task_output_schema()
    }

    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        _p: &AssistantMessage,
        _: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        let block = input.get("block").and_then(|v| v.as_bool()).unwrap_or(true);
        let timeout_ms = parse_task_output_timeout_ms(&input)?;
        let after_seq = input
            .get("after_seq")
            .or_else(|| input.get("afterSeq"))
            .and_then(|value| value.as_u64());
        let limit_bytes = parse_task_output_limit_bytes(&input)?;

        let task_store = store_for_context(ctx);
        let initial = task_store.get(id);
        match initial {
            Some(entry) => Ok(ToolResult {
                data: if !block {
                    let retrieval_status = if entry.status.is_active_for_output_wait() {
                        TaskOutputRetrievalStatus::NotReady
                    } else {
                        TaskOutputRetrievalStatus::Success
                    };
                    task_output_payload_for_tool(
                        &task_store,
                        &entry,
                        retrieval_status,
                        after_seq,
                        limit_bytes,
                    )?
                } else if !entry.status.is_active_for_output_wait() {
                    task_output_payload_for_tool(
                        &task_store,
                        &entry,
                        TaskOutputRetrievalStatus::Success,
                        after_seq,
                        limit_bytes,
                    )?
                } else {
                    match wait_for_task_output(
                        task_store.clone(),
                        id,
                        timeout_ms,
                        ctx.abort_signal.clone(),
                    )
                    .await?
                    {
                        TaskOutputWaitResult::Ready(entry) => task_output_payload_for_tool(
                            &task_store,
                            &entry,
                            TaskOutputRetrievalStatus::Success,
                            after_seq,
                            limit_bytes,
                        )?,
                        TaskOutputWaitResult::TimedOut(Some(entry)) => {
                            task_output_payload_for_tool(
                                &task_store,
                                &entry,
                                TaskOutputRetrievalStatus::Timeout,
                                after_seq,
                                limit_bytes,
                            )?
                        }
                        TaskOutputWaitResult::TimedOut(None) => json!({
                            "retrieval_status": TaskOutputRetrievalStatus::Timeout.as_str(),
                            "task": null,
                            "error": format!("Task not found: {}", id),
                        }),
                    }
                },
                new_messages: vec![],
                ..Default::default()
            }),
            None => Ok(ToolResult {
                data: json!({ "error": format!("Task not found: {}", id) }),
                new_messages: vec![],
                ..Default::default()
            }),
        }
    }

    async fn prompt(&self) -> String {
        "Get the retained output or logs from a task.".to_string()
    }
}

pub fn tools() -> Tools {
    vec![
        Arc::new(TodoWriteTool),
        Arc::new(TaskCreateTool),
        Arc::new(TaskGetTool),
        Arc::new(TaskUpdateTool),
        Arc::new(TaskListTool),
        Arc::new(TaskStopTool),
        Arc::new(TaskOutputTool),
    ]
}
