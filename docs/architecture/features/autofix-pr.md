# AUTOFIX_PR — 自动 PR 修复

> 功能标志：`REMOTE_TASK_TYPE_AUTOFIX_PR`
> 实现状态：远程任务定义，集成中
> 源码文件数：2

## 一、功能概述

AutoFix PR 是一个远程任务类型，用于自动化 pull request 修复。它作为 `allthecodes-tasks` 任务系统的一部分，定义了一种特殊的远程工作负载——当检测到 PR 问题时，自动创建修复方案并提交为 PR。

### 相关任务类型

```rust
// 远程任务类型枚举
pub const REMOTE_TASK_TYPE_REMOTE_AGENT: &str = "remote-agent";
pub const REMOTE_TASK_TYPE_ULTRAPLAN: &str = "ultraplan";
pub const REMOTE_TASK_TYPE_ULTRAREVIEW: &str = "ultrareview";
pub const REMOTE_TASK_TYPE_AUTOFIX_PR: &str = "autofix-pr";
pub const REMOTE_TASK_TYPE_BACKGROUND_PR: &str = "background-pr";
```

## 二、实现架构

### 2.1 任务域定义

文件：`crates/allthecodes-tasks/src/domain.rs`

`REMOTE_TASK_TYPE_AUTOFIX_PR = "autofix-pr"` 在任务域中定义，作为 `REMOTE_TASK_TYPE_ENUM` 的一部分，用于：

- 任务创建时的类型鉴别
- 任务列表中的类型标识
- 远程工作调度器的路由

### 2.2 PlanWorkflow 集成

文件：`crates/allthecodes/src/plan_workflow.rs`

AutoFix PR 与 Plan Workflow 系统紧密集成，后者提供计划审批工作流：

```rust
pub fn enter_engine_plan_mode(
    engine: &QueryEngine,
    source: &str,
    description: Option<&str>,
    classifier_reason: Option<&str>,
) -> Result<PlanWorkflowRecord>
```

PlanWorkflow 的核心流程：

```
enter_plan_mode_state
    │
    ▼
持久化 PlanWorkflowRecord 到磁盘
    │
    ▼
Agent 执行计划步骤
    │
    ▼
approve / reject 审核
    │
    ├── approve → 执行变更 → 创建 PR
    └── reject → 记录反馈 → 修改计划
```

### 2.3 计划审批状态管理

```rust
pub fn reject_engine_plan(
    engine: &QueryEngine,
    source: &str,
    feedback: Option<String>,
) -> Result<PlanWorkflowRecord>
```

PlanWorkflowRecord 通过 `sync_command_app_state()` 在引擎和命令层之间同步：

```rust
pub fn sync_command_app_state(engine: &QueryEngine, command_state: &AppState) {
    engine.update_app_state(|state| {
        state.tool_permission_context = permission_context;
        state.team_context = team_context;
        state.plan_workflow = plan_workflow;
    });
}
```

### 2.4 任务创建选项

创建 AutoFix PR 任务时使用 `TaskCreateOptions`：

```rust
pub struct TaskCreateOptions {
    pub kind: Option<String>,           // "remote_agent"
    pub parent_id: Option<String>,
    pub depends_on: Vec<String>,
    pub agent_id: Option<String>,
    pub supervisor_id: Option<String>,
    pub isolation: Option<String>,      // "worktree"
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    // ...
}
```

## 三、整体数据流

```
GitHub Webhook / CI 事件
      │
      ▼
Gateway 路由（gateway_routes.rs）
      │
      ▼
PR 活动解析 → 提取问题描述
      │
      ▼
AutoFix PR 任务创建
      │
      ├── type = "autofix-pr"
      ├── supervisor_id = 调度器
      └── isolation = "worktree"（可选）
      │
      ▼
Agent 执行：
  1. 读取 PR diff
  2. 识别问题
  3. 生成修复
  4. 提交到 worktree 分支
      │
      ▼
计划审批工作流（PlanWorkflow）
      │
      ├── 自动审批 → 推送修复分支 → 创建 PR
      └── 人工审批 → 等待用户确认
```

## 四、关键设计决策

1. **远程任务隔离**：AutoFix PR 作为远程任务类型，与本地 Agent 执行分离
2. **Worktree 隔离**：支持在独立 git worktree 中执行修复，不影响主工作区
3. **计划审批集成**：变更前经过 PlanWorkflow 审批，确保安全
4. **幂等性**：`idempotency_key` 确保同一事件不会触发重复修复

## 五、使用方式

```bash
# 自动触发（GitHub Webhook 集成）
# 当检测到 PR 问题时自动创建修复任务

# 手动触发
# 通过任务系统提交 AutoFix PR 请求
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-tasks/src/domain.rs` | 任务类型定义（REMOTE_TASK_TYPE_AUTOFIX_PR） |
| `crates/allthecodes/src/plan_workflow.rs` | PlanWorkflow 适配器（enter_plan_mode / reject / sync） |
| `crates/allthecodes-daemon/src/gateway_bridge.rs` | Gateway 桥接（远程命令分发） |
| `crates/allthecodes-daemon/src/protocol.rs` | 守护进程命令协议（命令排队与分发） |
