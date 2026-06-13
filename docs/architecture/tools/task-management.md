---
title: "任务管理工具 - TaskCreate、TaskUpdate、TaskList、TodoWrite"
description: "从源码角度解析 allthecodes 的任务追踪系统：TaskStore 持久化、任务生命周期、TodoWrite 校验提醒、Plan Workflow 集成和执行链路。"
keywords: ["任务管理", "TaskCreate", "TaskUpdate", "TaskList", "TodoWrite", "TaskStore", "任务追踪"]
---

## 概述

任务管理工具位于 `allthecodes-tools/src/tasks.rs`，使用 `allthecodes-tasks` crate 提供的领域模型和持久化存储。

### 七个核心工具

| 工具 | 名称常量 | 类型 | 只读 |
|------|---------|------|------|
| `TodoWriteTool` | `TODO_WRITE_NAME` | 写入 | 否 |
| `TaskCreateTool` | `TASK_CREATE_NAME` | 写入 | 否 |
| `TaskGetTool` | `TASK_GET_NAME` | 只读 | 是 |
| `TaskUpdateTool` | `TASK_UPDATE_NAME` | 写入 | 否 |
| `TaskListTool` | `TASK_LIST_NAME` | 只读 | 是 |
| `TaskStopTool` | `TASK_STOP_NAME` | 写入 | 否 |
| `TaskOutputTool` | `TASK_OUTPUT_NAME` | 只读 | 是 |

## 领域模型

`allthecodes-tasks/src/domain.rs` 定义了任务的核心类型：

### TaskStatus

```rust
pub enum TaskStatus {
    Pending,       // 等待处理
    InProgress,    // 处理中
    Completed,     // 已完成
    Cancelled,     // 已取消
    Blocked,       // 被阻塞（依赖未满足）
}
```

### TaskEntry

每个任务包含：唯一 ID、主题（subject）、描述（description）、状态、所有者、优先级、阻塞关系（blocked_by/blocks）、自定义字段、创建/更新时间戳等。

## TaskStore：持久化存储

`allthecodes-tasks/src/store.rs` 提供了任务存储的核心抽象：

```rust
pub trait TaskStore: Send + Sync {
    fn try_create(&self, subject: &str, description: &str) -> Result<TaskEntry, TaskError>;
    fn get(&self, id: &str) -> Option<TaskEntry>;
    fn list(&self) -> Vec<TaskEntry>;
    fn try_update_fields(&self, id: &str, updates: TaskFieldUpdates) -> Result<Option<TaskEntry>, TaskError>;
    fn try_delete(&self, id: &str) -> Result<Option<TaskEntry>, TaskError>;
    fn try_stop(&self, id: &str) -> Result<Option<TaskEntry>, TaskError>;
    fn claim_task(&self, id: &str, owner: &str, check_agent_busy: bool) -> Result<TaskEntry, TaskClaimFailure>;
}
```

### 作用域隔离

```rust
fn store_for_context(ctx: &ToolUseContext) -> TaskStore {
    allthecodes_tasks::store_for_task_list_id(&task_list_id_for_context(ctx))
}
```

任务存储按会话 ID 和团队名称隔离——不同的会话、不同的任务列表互不干扰。

## TaskCreate：任务创建

### Plan Workflow 关联

`maybe_link_plan_workflow_task()` 在任务创建时尝试将其关联到 Plan Workflow：

```rust
fn maybe_link_plan_workflow_task(ctx: &ToolUseContext, entry: &TaskEntry)
    -> Result<Option<PlanWorkflowRecord>>
```

通过 `set_app_state` 闭包将任务 ID 和摘要注入到 Plan Workflow 记录中，并持久化到磁盘。

### Hook 触发

```rust
if !configs.is_empty() {
    let payload = json!({"task_id": &entry.id, "subject": ...});
    let _ = crate::hooks::run_event_hooks("TaskCreated", &payload, &configs).await;
}
```

## TaskUpdate：任务状态更新

TaskUpdate 支持多种操作类型：

| 操作 | 说明 |
|------|------|
| `Update` | 常规字段更新（状态、主题、描述、所有者等） |
| `Delete` | 删除任务 |
| `Claim` | 认领任务（设置状态为 InProgress，分配所有者） |

### Claim 机制

认领操作有竞态保护：

```rust
let entry = task_store.claim_task(&request.id, &request.owner, request.check_agent_busy)?;
```

- 如果任务已被其他 agent 认领且 `check_agent_busy` 为 true，返回 `ClaimFailure`
- 认领后可以选择同时更新其他字段（`subject`、`description` 等）

### 完成 Hook

```rust
if request.status == Some(TaskStatus::Completed) && existing.status != TaskStatus::Completed {
    // 仅当状态从未完成变为完成时触发
    let _ = crate::hooks::run_event_hooks("TaskCompleted", &payload, &configs).await;
}
```

## TaskList & TaskGet：查询

- `TaskListTool`：列出当前存储中的所有任务，按创建顺序返回
- `TaskGetTool`：按 ID 获取单个任务的详情

## TaskStop：取消任务

```rust
match task_store.try_stop(&id)? {
    Some(entry) => Ok(ToolResult { data: json!({"task": ..., "message": ...}) }),
    None => Ok(task_error_result(TaskError::not_found(id))),
}
```

## TaskOutput：获取任务输出

支持阻塞和非阻塞两种模式：

- **非阻塞**（`block: false`）：如果任务仍在运行，返回 `retrieval_status: "not_ready"`，否则返回结果
- **阻塞**（`block: true`，默认）：等待任务完成，支持 `timeout_ms` 参数

```rust
let result = if !entry.status.is_active_for_output_wait() {
    task_output_payload(&entry, TaskOutputRetrievalStatus::Success)
} else if !block {
    task_output_payload(&entry, TaskOutputRetrievalStatus::NotReady)
} else {
    wait_for_task_output(task_store, id, timeout_ms, abort_signal).await?
};
```

## TodoWrite：会话待办列表

`TodoWriteTool` 管理当前会话的待办事项列表。

### 校验提醒

```rust
if outcome.verification_nudge_needed {
    data.insert("verification_nudge".to_string(), json!(
        "You completed 3+ todo items without a verification step; consider adding or running verification before claiming completion."
    ));
}
```

当一次完成 3 个以上待办项但没有验证步骤时，系统自动加入验证提醒。

### 数据格式

```json
{
  "todos": [
    { "id": "1", "description": "实现登录功能", "status": "completed" },
    { "id": "2", "description": "编写单元测试", "status": "in_progress" },
    { "id": "3", "description": "更新文档", "status": "pending" }
  ],
  "count": 3,
  "cleared": false,
  "message": "Todo list updated"
}
```

## 工具注册

所有任务工具通过 `tasks.rs` 的 `tools()` 函数统一注册：

```rust
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
```

## Policy 可见性

不同的 `ToolPolicy` 下任务工具的可见性不同：

| Policy | 可见的任务工具 |
|--------|--------------|
| `DefaultAgent` | 全部 |
| `Coordinator` | Task, TaskList, TaskStop |
| `CoordinatorWorker` | TaskList, TaskUpdate |
| `InProcessTeammate` | TaskList, TaskUpdate, TaskOutput |
