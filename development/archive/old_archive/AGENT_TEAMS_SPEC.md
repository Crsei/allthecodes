# Agent Teams / Multi-Agent Swarm 系统实现规格

> 基于 TypeScript 源码分析，用于未来 Rust 实现参考。
> 最后更新: 2026-04-01

## 目录

- [1. 系统概述](#1-系统概述)
- [2. 核心类型定义](#2-核心类型定义)
- [3. Feature Gate](#3-feature-gate)
- [4. Coordinator 模式](#4-coordinator-模式)
- [5. Team 生命周期](#5-team-生命周期)
- [6. Teammate 身份系统](#6-teammate-身份系统)
- [7. Mailbox IPC 协议](#7-mailbox-ipc-协议)
- [8. 执行后端](#8-执行后端)
- [9. In-Process Teammate 执行](#9-in-process-teammate-执行)
- [10. 工具实现](#10-工具实现)
- [11. 权限同步](#11-权限同步)
- [12. 关闭协商协议](#12-关闭协商协议)
- [13. 文件结构映射](#13-文件结构映射)
- [14. 常量定义](#14-常量定义)
- [15. Rust 实现建议](#15-rust-实现建议)

---

## 1. 系统概述

Agent Teams 是一个多代理协调系统，允许一个 "Team Lead" (领导者) 创建和管理多个
"Teammate" (成员) 代理并行执行任务。

**架构核心**:

```
User
  ↓
Team Lead (主 Claude 实例)
  ├── Agent Tool → 创建 Teammate
  ├── SendMessage Tool → 与 Teammate 通信
  ├── TaskCreate/Update → 分配任务
  └── TeamCreate/Delete → 团队生命周期
        ↓
  ┌─────────────────────────────────────────┐
  │              Teammate 执行               │
  │                                         │
  │  后端 1: In-Process (AsyncLocalStorage)  │
  │  后端 2: tmux (终端面板)                  │
  │  后端 3: iTerm2 (macOS 原生分割)          │
  │                                         │
  │  通信: 文件 Mailbox (~/.cc-rust/teams/)    │
  │  权限: 通过 Mailbox 向 Lead 请求审批       │
  │  计划: 可选 Plan Mode 审批门控            │
  └─────────────────────────────────────────┘
```

**关键特性**:
- 一个 Leader 同时只管理一个 Team
- Teammate 通过文件 mailbox 异步通信
- 支持 3 种执行后端: in-process / tmux / iTerm2
- 优雅关闭协商 (Teammate 可拒绝关闭请求)
- 权限流: Teammate 通过 Mailbox 向 Leader 请求工具使用权限
- Plan Mode: 可选的计划审批门控

---

## 2. 核心类型定义

### 2.1 TeamFile — 团队配置 (磁盘持久化)

```
文件位置: ~/.cc-rust/teams/{sanitized_team_name}/config.json
```

```rust
struct TeamFile {
    name: String,
    description: Option<String>,
    created_at: i64,                          // Unix timestamp
    lead_agent_id: String,                    // "team-lead@{team_name}"
    lead_session_id: Option<String>,          // Leader 的 session UUID
    hidden_pane_ids: Vec<String>,             // 隐藏的 pane ID
    team_allowed_paths: Vec<TeamAllowedPath>, // 共享编辑权限
    members: Vec<TeamMember>,
}

struct TeamMember {
    agent_id: String,           // "agent_name@team_name"
    name: String,               // e.g. "researcher"
    agent_type: Option<String>, // e.g. "researcher", "test-runner"
    model: Option<String>,      // 模型覆盖
    prompt: Option<String>,     // 最近发送的 prompt
    color: Option<String>,      // UI 颜色名 (red/blue/green/yellow/purple/orange/pink/cyan)
    plan_mode_required: Option<bool>,
    joined_at: i64,
    tmux_pane_id: String,
    cwd: String,
    worktree_path: Option<String>,
    session_id: Option<String>,
    subscriptions: Vec<String>,             // GitHub PR 订阅
    backend_type: Option<BackendType>,      // "tmux" | "iterm2" | "in-process"
    is_active: Option<bool>,                // false=idle, None/true=active
    mode: Option<PermissionMode>,
}

struct TeamAllowedPath {
    path: String,       // 绝对路径
    tool_name: String,  // "Edit", "Write" 等
    added_by: String,   // 添加者 agent name
    added_at: i64,
}
```

### 2.2 TeamContext — AppState 扩展

```rust
// 需要添加到 AppState
struct TeamContext {
    team_name: String,
    team_file_path: String,
    lead_agent_id: String,
    self_agent_id: Option<String>,
    self_agent_name: Option<String>,
    is_leader: Option<bool>,
    self_agent_color: Option<String>,
    teammates: HashMap<String, TeammateInfo>,  // agent_id → info
}

struct TeammateInfo {
    name: String,
    agent_type: Option<String>,
    color: Option<String>,
    tmux_session_name: String,
    tmux_pane_id: String,
    cwd: String,
    worktree_path: Option<String>,
    spawned_at: i64,
}
```

### 2.3 TeammateMessage — Mailbox 消息

```rust
struct TeammateMessage {
    from: String,          // 发送者 agent name
    text: String,          // 消息内容 (可能是 JSON 结构化消息)
    timestamp: String,     // ISO 8601
    read: bool,            // 是否已读
    color: Option<String>, // 发送者 UI 颜色
    summary: Option<String>, // 5-10 词摘要
}
```

### 2.4 BackendType

```rust
enum BackendType {
    InProcess,
    Tmux,
    ITerm2,
}
```

### 2.5 TeammateSpawnConfig

```rust
struct TeammateSpawnConfig {
    name: String,
    team_name: String,
    color: Option<String>,
    plan_mode_required: bool,
    prompt: String,
    cwd: String,
    model: Option<String>,
    system_prompt: Option<String>,
    system_prompt_mode: Option<String>,  // "default" | "replace" | "append"
    worktree_path: Option<String>,
    parent_session_id: String,
    permissions: Vec<String>,
    allow_permission_prompts: bool,
}
```

### 2.6 TeammateSpawnResult

```rust
struct TeammateSpawnResult {
    success: bool,
    agent_id: String,           // "agent_name@team_name"
    error: Option<String>,
    abort_handle: Option<...>,  // Rust: tokio AbortHandle 或 CancellationToken
    task_id: Option<String>,    // In-process only
    pane_id: Option<String>,    // Pane-based only
}
```

### 2.7 InProcessTeammateTaskState — 进程内 Teammate 任务跟踪

```rust
struct InProcessTeammateTaskState {
    id: String,
    status: TaskStatus,    // Running / Stopped / Completed
    identity: TeammateIdentity,
    prompt: String,
    model: Option<String>,
    abort_handle: Option<tokio::task::AbortHandle>,
    awaiting_plan_approval: bool,
    permission_mode: PermissionMode,
    error: Option<String>,
    pending_user_messages: Vec<String>,
    is_idle: bool,
    shutdown_requested: bool,
    last_reported_tool_count: usize,
    last_reported_token_count: usize,
}

struct TeammateIdentity {
    agent_id: String,
    agent_name: String,
    team_name: String,
    color: Option<String>,
    plan_mode_required: bool,
    parent_session_id: String,
}
```

---

## 3. Feature Gate

### isAgentSwarmsEnabled()

```
逻辑:
1. 如果 USER_TYPE == "ant" → true (内部构建始终启用)
2. 外部用户:
   a. 检查 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS 环境变量 或 --agent-teams CLI 参数
   b. 如果都未设置 → false
   c. 检查 GrowthBook feature gate "tengu_amber_flint" (安全开关, 默认 true)
   d. 返回 gate 值
```

**Rust 简化建议**: 仅检查环境变量 `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`，无需 GrowthBook。

---

## 4. Coordinator 模式

Coordinator 模式是一个独立于 Teams 的高级编排模式。

### 4.1 isCoordinatorMode()

```
条件: feature('COORDINATOR_MODE') AND isEnvTruthy(CLAUDE_CODE_COORDINATOR_MODE)
```

### 4.2 Coordinator 系统提示 (370+ 行)

核心内容:
- **角色定义**: Coordinator 编排 worker，综合结果，与用户沟通
- **工具**: Agent (派生 worker), SendMessage (继续 worker), TaskStop (停止 worker)
- **工作流阶段**:
  1. **Research** — 并行派生 worker 探索代码库
  2. **Synthesis** — Coordinator 综合 worker 发现
  3. **Implementation** — 派生 worker 执行实现
  4. **Verification** — 派生 worker 验证结果
- **并行策略**: 独立任务并发启动
- **Worker Prompt 工程**: 综合发现为具体规格 (包含文件路径、行号)
- **Continue vs Spawn 决策**: 基于上下文重叠度

### 4.3 getCoordinatorUserContext()

```
参数: mcpClients (MCP 服务器列表), scratchpadDir (可选)
返回: { workerToolsContext: 描述 worker 可用工具的文本 }

逻辑:
1. 如果非 coordinator 模式 → 返回空
2. 构建 worker 工具列表:
   - 简单模式: [Bash, Read, Edit]
   - 完整模式: 所有 ASYNC_AGENT_ALLOWED_TOOLS (排除内部 worker 工具)
3. 附加 MCP 服务器名称
4. 如果有 scratchpad → 附加路径和说明
```

---

## 5. Team 生命周期

### 5.1 创建 (TeamCreate)

```
输入: { team_name: String, description?: String, agent_type?: String }
输出: { team_name, team_file_path, lead_agent_id }

步骤:
1. 检查是否已在领导 team (一个 leader 只能管一个 team)
2. 生成唯一 team name (如果已存在则添加 word slug)
3. 生成 lead agent ID: formatAgentId("team-lead", team_name) → "team-lead@{team_name}"
4. 解析 leader 模型 (从 AppState)
5. 构建 TeamFile:
   - name, description, created_at, lead_agent_id, lead_session_id
   - members: [{ agent_id, name: "team-lead", ... }]
6. 写入 ~/.cc-rust/teams/{name}/config.json
7. 注册 session 清理 (防止磁盘泄漏)
8. 初始化任务目录: resetTaskList() + ensureTasksDir()
9. 设置 leader team name → getTaskListId() 返回 team 命名空间
10. 更新 AppState.team_context
11. 日志: tengu_team_created 事件
```

### 5.2 删除 (TeamDelete)

```
输入: {} (无参数)
输出: { success, message, team_name? }

步骤:
1. 从 AppState 获取 team_name
2. 读取 TeamFile，检查活跃成员:
   - 过滤非 lead 成员
   - 分离活跃成员 (is_active !== false)
   - 如果有活跃成员 → 拒绝: "Cannot cleanup team with X active member(s)"
3. cleanupTeamDirectories(team_name):
   - 读取 TeamFile 获取 worktree 路径
   - 销毁所有 worktree (git worktree remove --force, fallback rm -rf)
   - 删除 team 目录 (~/.cc-rust/teams/{name})
   - 删除 tasks 目录 (~/.cc-rust/tasks/{sanitized_name})
4. 取消注册 session 清理
5. 清除颜色分配
6. 清除 leader team name
7. 日志: tengu_team_deleted
8. 清除 AppState.team_context
```

### 5.3 Session 结束清理

```
cleanupSessionTeams():
1. 获取 sessionCreatedTeams 集合
2. 对每个未显式删除的 team:
   a. 杀死孤儿 pane (遍历成员, kill pane)
   b. 清理 team 目录
3. 清空集合
```

---

## 6. Teammate 身份系统

### 6.1 Agent ID 格式

```
格式: "{agent_name}@{team_name}"
示例: "researcher@my-project-team"
特殊: "team-lead@my-project-team" (Leader)

formatAgentId(name, team) → "{name}@{team}"
parseAgentId(id) → { agent_name, team_name }
```

### 6.2 身份解析优先级

TS 使用 3 层优先级链:

```
1. AsyncLocalStorage (进程内 teammate) — 最高优先
2. dynamicTeamContext (tmux/iTerm2 teammate) — 运行时设置
3. AppState / 环境变量 — 最低优先
```

**Rust 对应方案**: 使用 `tokio::task_local!` 替代 AsyncLocalStorage。

### 6.3 身份函数

| 函数 | 返回值 | 逻辑 |
|------|--------|------|
| `get_agent_id()` | `Option<String>` | 优先级链解析 |
| `get_agent_name()` | `Option<String>` | 从 agent_id 提取 @ 前部分 |
| `get_team_name(ctx?)` | `Option<String>` | in-process > dynamic > appState |
| `is_teammate()` | `bool` | 有 agent_id AND team_name |
| `get_teammate_color()` | `Option<String>` | 分配的 UI 颜色 |
| `is_plan_mode_required()` | `bool` | AsyncLocal > dynamic > 环境变量 |
| `is_team_lead(ctx?)` | `bool` | my_agent_id == lead_agent_id |
| `has_active_in_process_teammates(state)` | `bool` | 检查 running 任务 |
| `has_working_in_process_teammates(state)` | `bool` | 检查非 idle 的 running 任务 |
| `wait_for_teammates_to_become_idle(state)` | `Future<()>` | 注册回调，等待所有 idle |

---

## 7. Mailbox IPC 协议

### 7.1 文件格式

```
位置: ~/.cc-rust/teams/{team_name}/inboxes/{agent_name}.json
格式: JSON Array of TeammateMessage
锁文件: {inbox_path}.lock
```

### 7.2 核心操作

| 操作 | 函数 | 锁 | 说明 |
|------|------|-----|------|
| 读全部 | `read_mailbox(name, team)` | 否 | 返回所有消息 |
| 读未读 | `read_unread_messages(name, team)` | 否 | 过滤 `!read` |
| 写入 | `write_to_mailbox(name, msg, team)` | 是 | 锁 → 读最新 → 追加 → 写回 |
| 标记已读 (按索引) | `mark_as_read_by_index(name, team, idx)` | 是 | 锁 → 更新 → 写回 |
| 标记全部已读 | `mark_messages_as_read(name, team)` | 是 | 全部标记 |
| 清空 | `clear_mailbox(name, team)` | 否 | 覆盖为 `[]` |

### 7.3 锁策略

```
机制: 文件锁 (proper-lockfile 在 TS 中)
锁文件: {inbox}.lock
重试: 10 次, 5-100ms 指数退避
目的: 防止多个 Claude 进程并发写入同一 inbox
读操作无锁; 写操作锁后重新读取以捕获并发更新
```

**Rust 对应**: 使用 `fs2::FileExt` (flock) 或 `advisory-lock` crate。

### 7.4 结构化协议消息类型

Mailbox 的 `text` 字段可以是普通文本或 JSON 结构化消息。

#### 关闭请求/响应

```json
// shutdown_request — Leader → Teammate
{
  "type": "shutdown_request",
  "requestId": "shutdown-researcher@team-1719000000",
  "from": "team-lead",
  "reason": "Task completed",
  "timestamp": "2026-04-01T12:00:00Z"
}

// shutdown_approved — Teammate → Leader
{
  "type": "shutdown_approved",
  "requestId": "...",
  "from": "researcher",
  "timestamp": "...",
  "paneId": "%3",
  "backendType": "tmux"
}

// shutdown_rejected — Teammate → Leader
{
  "type": "shutdown_rejected",
  "requestId": "...",
  "from": "researcher",
  "reason": "Still processing important task",
  "timestamp": "..."
}
```

#### 计划审批请求/响应

```json
// plan_approval_request — Teammate → Leader
{
  "type": "plan_approval_request",
  "from": "researcher",
  "timestamp": "...",
  "planFilePath": "/path/to/plan.md",
  "planContent": "# Implementation Plan\n...",
  "requestId": "plan_approval-researcher@team-1719000000"
}

// plan_approval_response — Leader → Teammate
{
  "type": "plan_approval_response",
  "requestId": "...",
  "approved": true,
  "feedback": "Looks good, proceed",
  "timestamp": "...",
  "permissionMode": "default"
}
```

#### 权限请求/响应

```json
// permission_request — Teammate → Leader
{
  "type": "permission_request",
  "request_id": "perm-1234",
  "agent_id": "researcher@team",
  "tool_name": "Bash",
  "tool_use_id": "tu_abc",
  "description": "Run git status",
  "input": { "command": "git status" },
  "permission_suggestions": []
}

// permission_response — Leader → Teammate
{
  "type": "permission_response",
  "request_id": "perm-1234",
  "subtype": "success",           // 或 "error"
  "response": {
    "updated_input": { ... },
    "permission_updates": [...]
  }
}
```

#### Idle 通知

```json
{
  "type": "idle_notification",
  "from": "researcher",
  "timestamp": "...",
  "idleReason": "available",       // "available" | "interrupted" | "failed"
  "summary": "Finished analyzing API endpoints",
  "completedTaskId": "task-123",
  "completedStatus": "resolved"    // "resolved" | "blocked" | "failed"
}
```

#### 其他消息类型

```json
// task_assignment — Leader → Teammate
{ "type": "task_assignment", "taskId": "...", "subject": "...", "description": "...", "assignedBy": "team-lead" }

// team_permission_update — Leader → Teammate (权限规则更新)
{ "type": "team_permission_update", "permissionUpdate": { "type": "addRules", "rules": [...], "behavior": "allow" }, "directoryPath": "...", "toolName": "..." }

// mode_set_request — Leader → Teammate (权限模式切换)
{ "type": "mode_set_request", "mode": "auto", "from": "team-lead" }

// sandbox_permission_request/response — 沙箱网络权限
{ "type": "sandbox_permission_request", "requestId": "...", "workerId": "...", "hostPattern": { "host": "api.example.com" } }
```

### 7.5 协议消息判别

```rust
fn is_structured_protocol_message(text: &str) -> bool {
    // 尝试 JSON 解析, 检查 "type" 字段是否匹配已知协议类型
    let known_types = [
        "permission_request", "permission_response",
        "sandbox_permission_request", "sandbox_permission_response",
        "shutdown_request", "shutdown_approved", "shutdown_rejected",
        "team_permission_update", "mode_set_request",
        "plan_approval_request", "plan_approval_response",
        "idle_notification", "task_assignment",
    ];
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.get("type")?.as_str().map(|t| known_types.contains(&t)))
        .unwrap_or(false)
}
```

### 7.6 消息格式化 (用于注入对话)

```xml
<teammate-message from="researcher" color="blue" timestamp="2026-04-01T12:00:00Z">
Message content here
</teammate-message>
```

---

## 8. 执行后端

### 8.1 TeammateExecutor trait

```rust
#[async_trait]
trait TeammateExecutor: Send + Sync {
    fn backend_type(&self) -> BackendType;

    async fn is_available(&self) -> bool;

    async fn spawn(&self, config: TeammateSpawnConfig) -> TeammateSpawnResult;

    async fn send_message(&self, agent_id: &str, message: TeammateMessage) -> Result<()>;

    /// 优雅终止 (发送关闭请求, Teammate 可拒绝)
    async fn terminate(&self, agent_id: &str, reason: Option<&str>) -> bool;

    /// 强制杀死 (立即停止)
    async fn kill(&self, agent_id: &str) -> bool;

    async fn is_active(&self, agent_id: &str) -> bool;
}
```

### 8.2 PaneBackend trait (Pane 后端专用)

```rust
#[async_trait]
trait PaneBackend: TeammateExecutor {
    fn display_name(&self) -> &str;
    fn supports_hide_show(&self) -> bool;

    async fn is_running_inside(&self) -> bool;

    async fn create_teammate_pane(&self, name: &str, color: &str) -> Result<CreatePaneResult>;
    async fn send_command_to_pane(&self, pane_id: &str, command: &str) -> Result<()>;
    async fn set_pane_border_color(&self, pane_id: &str, color: &str) -> Result<()>;
    async fn set_pane_title(&self, pane_id: &str, name: &str, color: &str) -> Result<()>;
    async fn rebalance_panes(&self, window_target: &str, has_leader: bool) -> Result<()>;
    async fn kill_pane(&self, pane_id: &str) -> bool;
    async fn hide_pane(&self, pane_id: &str) -> bool;
    async fn show_pane(&self, pane_id: &str, target: &str) -> bool;
}
```

### 8.3 In-Process 后端

```
特点:
- 始终可用 (无外部依赖)
- 同一进程内运行, 使用 task_local! 隔离上下文
- 使用 tokio::task::spawn + CancellationToken 管理生命周期
- 通信: 文件 mailbox (与其他后端相同接口)
- 共享 API client、MCP 连接

spawn():
1. 验证 context 已设置
2. 调用 spawn_in_process_teammate() 创建任务
3. 调用 start_in_process_teammate() 启动后台执行
4. 返回 { agent_id, task_id, abort_handle }

terminate():
1. 查找任务 by agent_id
2. 如果已发送过关闭请求 → 返回 true
3. 生成 request_id
4. 创建 shutdown_request 消息
5. 写入 mailbox
6. 标记 shutdown_requested = true

kill():
1. 查找任务
2. 调用 abort_handle.abort()
3. 更新任务状态为 killed

is_active():
1. 查找任务
2. 返回 status == Running AND !aborted
```

### 8.4 Tmux 后端

```
特点:
- 需要 tmux 可用
- 每个 teammate 一个 pane
- 完全进程隔离
- 支持 hide/show

创建策略:
- 在 tmux 内: split 当前窗口, leader 30% / teammates 70%
- 不在 tmux 内: 创建独立 "claude-swarm" session

spawn():
1. 获取 pane 创建锁 (序列化并行 spawn)
2. 判断是否在 tmux 内 → 选择创建方式
3. 创建 pane (split-window 或 new-session)
4. 等待 shell 就绪 (200ms)
5. 发送 claude 命令到 pane (send-keys)
6. 设置 pane 边框颜色和标题
7. 返回 pane_id

terminate():
1. 发送关闭请求到 mailbox (同 in-process)

kill():
1. tmux kill-pane -t {pane_id}

颜色映射:
  red → "red", blue → "blue", green → "green", yellow → "yellow"
  purple → "magenta", orange → "colour208", pink → "colour205", cyan → "cyan"
```

### 8.5 iTerm2 后端

```
特点:
- macOS only
- 使用 iTerm2 原生 split pane API (it2 CLI)
- 类似 tmux 但使用原生终端

(结构与 tmux 类似, 但调用 it2 CLI 而非 tmux)
```

---

## 9. In-Process Teammate 执行

### 9.1 spawn_in_process_teammate()

```
输入: InProcessSpawnConfig, SpawnContext
输出: InProcessSpawnResult { success, agent_id, task_id, abort_handle, teammate_context }

步骤:
1. 生成标识符:
   - agent_id = format!("{}@{}", name, team_name)
   - task_id = generate_task_id("in_process_teammate")

2. 创建 CancellationToken (独立于 parent, teammate 可独立存活)

3. 获取 parent_session_id

4. 创建 TeammateIdentity:
   { agent_id, agent_name, team_name, color, plan_mode_required, parent_session_id }

5. 创建 teammate context (task_local! 或 Context)

6. 创建 InProcessTeammateTaskState:
   - status: Running
   - identity, prompt, model, abort_handle
   - awaiting_plan_approval: false
   - is_idle: false
   - shutdown_requested: false
   - pending_user_messages: []

7. 注册清理处理器 (session 结束时 abort)

8. 注册任务到 AppState.tasks[task_id]

9. 返回结果
```

### 9.2 start_in_process_teammate()

```
输入: InProcessRunnerConfig { identity, task_id, prompt, teammate_context, tool_use_context, ... }

步骤:
1. 保存 agent_id (避免闭包持有完整 config)
2. 异步 fire-and-forget: tokio::spawn(async move { run_in_process_teammate(config) })
3. 附加 catch 日志 (防止 panic 无声)
```

### 9.3 run_in_process_teammate() — 主执行循环

```
步骤:
1. 设置 teammate context (task_local!)
2. 设置 agent context (嵌套 task_local!)
3. 在上下文中运行:
   a. 创建独立 QueryEngine (子代理)
   b. 注入 teammate prompt
   c. 循环:
      - 提交消息到 QueryEngine
      - 等待结果
      - 检查 mailbox (轮询):
        - 处理 shutdown_request → 审批或拒绝
        - 处理 plan_approval_response → 解锁 plan mode
        - 处理 permission_response → 解锁权限请求
        - 处理普通消息 → 注入对话
      - 检查 abort signal → 退出
      - 标记 idle 当等待新消息时
   d. 清理退出

Mailbox 轮询间隔: ~500ms
```

### 9.4 上下文隔离 (Rust 方案)

TS 使用 `AsyncLocalStorage`, Rust 对应方案:

```rust
// 方案 1: tokio task_local! (最接近 AsyncLocalStorage)
tokio::task_local! {
    static TEAMMATE_CONTEXT: TeammateContext;
}

// 在 teammate 执行时设置
TEAMMATE_CONTEXT.scope(ctx, async move {
    // 所有 get_agent_id() 等调用在此作用域内可见
    run_agent_loop().await
}).await;

// 方案 2: 通过 ToolUseContext 传递 (更显式)
// 在 ToolUseContext 中已有 agent_id, agent_type 字段
```

---

## 10. 工具实现

### 10.1 TeamCreate Tool

```rust
struct TeamCreateTool;

// 输入
struct TeamCreateInput {
    team_name: String,
    description: Option<String>,
    agent_type: Option<String>,
}

// 输出
struct TeamCreateOutput {
    team_name: String,
    team_file_path: String,
    lead_agent_id: String,
}

// 验证
fn validate_input(input) -> ValidationResult {
    if input.team_name.trim().is_empty() {
        return Error("team_name is required")
    }
    Ok
}

// 执行
async fn call(input, ctx) -> Result<ToolResult> {
    // 1. 检查是否已领导 team
    // 2. 生成唯一 team name
    // 3. 生成 lead_agent_id = "team-lead@{team_name}"
    // 4. 解析 leader 模型
    // 5. 构建 TeamFile
    // 6. 写入磁盘
    // 7. 注册 session 清理
    // 8. 初始化任务目录
    // 9. 更新 AppState
}
```

### 10.2 TeamDelete Tool

```rust
struct TeamDeleteTool;

// 输入: 无参数
// 输出: { success: bool, message: String, team_name: Option<String> }

async fn call(input, ctx) -> Result<ToolResult> {
    // 1. 获取 team_name from AppState
    // 2. 读取 TeamFile
    // 3. 检查活跃成员 → 如有则拒绝
    // 4. cleanup_team_directories():
    //    - 销毁 worktree (git worktree remove --force / rm -rf fallback)
    //    - 删除 team 目录
    //    - 删除 tasks 目录
    // 5. 取消 session 清理注册
    // 6. 清除颜色分配
    // 7. 清除 AppState.team_context
}
```

### 10.3 SendMessage Tool

```rust
struct SendMessageTool;

// 输入
struct SendMessageInput {
    to: String,               // teammate name, "*", "uds:...", "bridge:..."
    summary: Option<String>,  // 5-10 词摘要
    message: SendMessageContent,
}

enum SendMessageContent {
    Text(String),
    Structured(StructuredMessage),
}

enum StructuredMessage {
    ShutdownRequest { reason: Option<String> },
    ShutdownResponse { request_id: String, approve: bool, reason: Option<String> },
    PlanApprovalResponse { request_id: String, approve: bool, feedback: Option<String> },
}

// 路由逻辑
async fn call(input, ctx) -> Result<ToolResult> {
    match &input.message {
        // 结构化消息路由
        Structured(ShutdownRequest { .. }) => handle_shutdown_request(),
        Structured(ShutdownResponse { approve: true, .. }) => handle_shutdown_approval(),
        Structured(ShutdownResponse { approve: false, .. }) => handle_shutdown_rejection(),
        Structured(PlanApprovalResponse { approve: true, .. }) => handle_plan_approval(),
        Structured(PlanApprovalResponse { approve: false, .. }) => handle_plan_rejection(),
        // 普通消息路由
        Text(_) if input.to == "*" => handle_broadcast(),
        Text(_) => handle_message(),
    }
}

// 单一收件人
async fn handle_message() {
    // 1. 获取 sender name
    // 2. 写入 recipient mailbox
    // 3. 返回 routing 信息
}

// 广播
async fn handle_broadcast() {
    // 1. 读取 TeamFile
    // 2. 遍历成员 (排除自己)
    // 3. 写入每个成员的 mailbox
    // 4. 返回收件人列表
}

// 关闭请求
async fn handle_shutdown_request() {
    // 1. 生成 deterministic request_id
    // 2. 创建 shutdown_request JSON
    // 3. 写入目标 mailbox
}

// 关闭审批
async fn handle_shutdown_approval() {
    // 1. 验证是 team lead
    // 2. In-process: 找到任务 → abort
    // 3. 否则: 发送 shutdown_approved 到 mailbox
}

// 计划审批
async fn handle_plan_approval() {
    // 1. 验证是 team lead
    // 2. 计算权限模式继承 (leader plan → default, 其他 → 继承)
    // 3. 写入 plan_approval_response 到 mailbox
}
```

---

## 11. 权限同步

Teammate 执行工具时可能需要 Leader 审批:

```
流程:
1. Teammate 调用需要权限的工具
2. 权限检查返回 Ask
3. Teammate 创建 permission_request 消息
4. 写入 Leader 的 mailbox
5. Teammate 进入轮询等待 (500ms 间隔)
6. Leader 读取请求 → 展示 UI 审批对话框
7. Leader 创建 permission_response 消息
8. 写入 Teammate 的 mailbox
9. Teammate 读取响应 → 继续或拒绝

超时: 无显式超时 (依赖 abort signal)
```

---

## 12. 关闭协商协议

```
1. Leader 决定关闭 Teammate
   ↓
2. Leader 发送 shutdown_request 到 Teammate mailbox
   ↓
3. Teammate 模型读取消息, 决定:
   ├── 同意 → 发送 shutdown_approved → 调用 graceful_shutdown()
   └── 拒绝 → 发送 shutdown_rejected { reason: "..." }
   ↓
4a. 如果同意:
    - In-process: abort_handle.abort()
    - Tmux: kill-pane
    - 更新 TeamFile: is_active = false
    ↓
4b. 如果拒绝:
    - Leader 展示拒绝原因给用户
    - 用户可决定强制 kill

5. 强制 kill (bypass 协商):
   - In-process: abort_handle.abort() 直接
   - Tmux: tmux kill-pane
   - 不等待 Teammate 响应
```

---

## 13. 文件结构映射

### TypeScript → Rust 映射

| TypeScript | Rust (建议) | 说明 |
|------------|-------------|------|
| `coordinator/coordinatorMode.ts` | `coordinator/mod.rs` | Coordinator 模式 |
| `tools/TeamCreateTool/` | `tools/team_create.rs` | 创建 team |
| `tools/TeamDeleteTool/` | `tools/team_delete.rs` | 删除 team |
| `tools/SendMessageTool/` | `tools/send_message.rs` | 消息路由 |
| `utils/teammate.ts` | `teams/identity.rs` | 身份解析 |
| `utils/teammateContext.ts` | `teams/context.rs` | 上下文隔离 (task_local!) |
| `utils/teammateMailbox.ts` | `teams/mailbox.rs` | 文件 IPC |
| `utils/agentSwarmsEnabled.ts` | `teams/mod.rs` (feature gate) | 功能开关 |
| `utils/swarm/constants.ts` | `teams/constants.rs` | 常量 |
| `utils/swarm/teamHelpers.ts` | `teams/helpers.rs` | TeamFile 管理 |
| `utils/swarm/backends/types.ts` | `teams/backend.rs` | trait 定义 |
| `utils/swarm/backends/InProcessBackend.ts` | `teams/in_process.rs` | 进程内后端 |
| `utils/swarm/backends/TmuxBackend.ts` | `teams/tmux.rs` | tmux 后端 |
| `utils/swarm/inProcessRunner.ts` | `teams/runner.rs` | 执行循环 |
| `state/AppStateStore.ts` (TeamContext) | `types/app_state.rs` 扩展 | 状态字段 |

### 建议目录结构

```
rust/src/teams/
├── mod.rs           # 公共 API + feature gate
├── constants.rs     # 常量 (TEAM_LEAD_NAME 等)
├── types.rs         # TeamFile, TeamMember, TeammateMessage 等
├── identity.rs      # 身份解析 (get_agent_id 等)
├── context.rs       # task_local! 上下文隔离
├── mailbox.rs       # 文件 IPC 协议
├── helpers.rs       # TeamFile CRUD, cleanup
├── backend.rs       # TeammateExecutor trait
├── in_process.rs    # In-process 后端
├── tmux.rs          # Tmux 后端
├── runner.rs        # Teammate 执行循环
└── protocol.rs      # 结构化消息类型 + 解析
```

---

## 14. 常量定义

```rust
pub const TEAM_LEAD_NAME: &str = "team-lead";
pub const SWARM_SESSION_NAME: &str = "claude-swarm";
pub const SWARM_VIEW_WINDOW_NAME: &str = "swarm-view";
pub const TMUX_COMMAND: &str = "tmux";
pub const HIDDEN_SESSION_NAME: &str = "claude-hidden";

pub const TEAMMATE_COMMAND_ENV_VAR: &str = "CLAUDE_CODE_TEAMMATE_COMMAND";
pub const TEAMMATE_COLOR_ENV_VAR: &str = "CLAUDE_CODE_AGENT_COLOR";
pub const PLAN_MODE_REQUIRED_ENV_VAR: &str = "CLAUDE_CODE_PLAN_MODE_REQUIRED";
pub const AGENT_TEAMS_ENV_VAR: &str = "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS";

pub const MAILBOX_LOCK_RETRIES: usize = 10;
pub const MAILBOX_LOCK_MIN_TIMEOUT_MS: u64 = 5;
pub const MAILBOX_LOCK_MAX_TIMEOUT_MS: u64 = 100;

pub const PANE_SHELL_INIT_DELAY_MS: u64 = 200;
pub const MAILBOX_POLL_INTERVAL_MS: u64 = 500;
pub const MAX_TEAMMATE_COLORS: usize = 8;

pub const AGENT_COLORS: &[&str] = &[
    "red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan",
];
```

---

## 15. Rust 实现建议

### 15.1 优先级

```
Phase 1 (核心):
  - types.rs (所有类型定义)
  - constants.rs
  - mailbox.rs (文件 IPC)
  - helpers.rs (TeamFile CRUD)
  - identity.rs (身份解析)

Phase 2 (工具):
  - team_create.rs
  - team_delete.rs
  - send_message.rs

Phase 3 (执行):
  - backend.rs (trait)
  - in_process.rs (最简后端, 无外部依赖)
  - runner.rs (执行循环)
  - context.rs (task_local!)

Phase 4 (可选):
  - tmux.rs (终端面板)
  - coordinator/mod.rs (coordinator 模式)
```

### 15.2 Rust 技术选型

| 需求 | TS 方案 | Rust 方案 |
|------|---------|-----------|
| 上下文隔离 | AsyncLocalStorage | `tokio::task_local!` |
| 取消令牌 | AbortController | `tokio_util::sync::CancellationToken` |
| 文件锁 | proper-lockfile | `fs2::FileExt` (flock) 或 `advisory-lock` |
| JSON 读写 | fs.readFile + JSON.parse | `serde_json` + `tokio::fs` |
| 后台任务 | 未 await 的 Promise | `tokio::task::spawn` |
| 定时轮询 | setInterval | `tokio::time::interval` |

### 15.3 简化建议

1. **跳过 iTerm2 后端** — macOS 专用, 优先实现 in-process + tmux
2. **跳过 GrowthBook** — 环境变量开关即可
3. **跳过 Coordinator 模式** — 可用 Agent Tool 替代, 后续按需添加
4. **Mailbox 锁** — 使用 `fs2::lock_exclusive()` 即可, 无需复杂 retry
5. **权限同步** — 初期可简化为 Leader 自动审批
6. **颜色分配** — 简单 round-robin 即可

### 15.4 估算

```
核心类型 + 常量:     ~500 行
Mailbox IPC:         ~400 行
TeamFile 管理:       ~500 行
身份系统:            ~200 行
TeamCreate 工具:     ~200 行
TeamDelete 工具:     ~150 行
SendMessage 工具:    ~600 行
In-Process 后端:     ~400 行
执行循环:            ~500 行
Tmux 后端:           ~400 行
测试:                ~800 行
─────────────────────────────
总计:                ~4,650 行
```
