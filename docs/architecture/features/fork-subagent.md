# FORK_SUBAGENT — Fork 子 Agent

> 功能标志：`FEATURE_FORK_SUBAGENT=1`
> 实现状态：完整可用
> 源码文件数：5

## 一、功能概述

Fork 子 Agent 是一个轻量级子引擎调用原语，用于在独立但受限的上下文中执行子任务。与注册到全局 Agent 树的标准 AgentTool 不同，fork 是一个自包含的子 `QueryEngine` 调用：不持久化、不保存会话、不注册到 IPC 树。调用者得到一个结果字符串和一个错误标志。

### 使用场景

- **`/btw` 命令** — 无工具的单轮旁路问答
- **`/simplify` 命令** — 多 Agent 代码审查（当完整 AgentTool 不可用时）
- **`SkillContext::Fork`** — 在受限的子引擎中运行技能提示
- **后台子 Agent** — 通过 BackgroundSupervisor 管理的异步 Agent 任务

## 二、实现架构

### 2.1 ForkParams — Fork 参数

文件：`crates/allthecodes-engine/src/agent/fork.rs:39-67`

```rust
pub struct ForkParams {
    pub prompt: String,                        // 子 Agent 的提示
    pub cwd: String,                           // 工作目录
    pub model: String,                         // 显式模型（失败时回退到 fallback_model）
    pub fallback_model: Option<String>,        // 回退模型（通常是父级的主模型）
    pub tools: Tools,                          // 可用工具集（空 Vec = 无工具）
    pub max_turns: Option<usize>,              // 硬上限，默认为 1（无工具使用）
    pub parent_messages: Option<Vec<Message>>, // 父级历史消息，用于 prompt cache 复用
    pub append_system_prompt: Option<String>,  // 附加到内置 system prompt 的片段
    pub custom_system_prompt: Option<String>,  // 覆盖引擎默认 system prompt
    pub hook_runner: Arc<dyn HookRunner>,      // Hook 运行器
    pub command_dispatcher: Arc<dyn CommandDispatcher>, // 命令调度器
}
```

### 2.2 ForkOutcome — Fork 结果

```rust
pub struct ForkOutcome {
    pub text: String,          // 子引擎输出的文本
    pub had_error: bool,       // 是否以错误结束
    pub duration_ms: u64,      // 执行耗时（毫秒）
    pub agent_id: String,      // 分配的 UUID v4 标识符
}
```

### 2.3 核心调用流程

```rust
pub async fn run_fork(params: ForkParams) -> Result<ForkOutcome>
```

1. 生成新的 `agent_id` 和 `chain_id`（UUID v4）
2. 构建 `QueryEngineConfig`：
   - `persist_session: false` — 不持久化
   - `initial_messages: params.parent_messages` — 种子历史
   - `agent_context` 包含 `agent_type: Some("fork")`
3. 创建子 `QueryEngine`，安装 hook_runner 和 command_dispatcher
4. 调用 `child_engine.submit_message(&params.prompt, QuerySource::Agent(agent_id))`
5. 通过 `collect_stream_result` 收集流结果
6. 返回 `ForkOutcome`

### 2.4 Prompt Cache 复用

当调用者传递 `parent_messages` 时，子引擎复用与父级相同的初始历史。这意味着 fork 可以共享父级 API 请求前缀，最大化 prompt cache 命中率。不传递 `parent_messages` 时，子引擎从空历史开始。

### 2.5 BackgroundSupervisor — 后台 Agent 管理

文件：`crates/allthecodes-engine/src/agent/supervisor.rs`

BackgroundSupervisor 管理后台 Agent 的完整生命周期：

- **注册**：`spawn_background_agent()` 创建任务条目，注册到全局任务存储
- **取消**：`cancel_agent()` 触发 CancellationToken，停止子引擎
- **关闭**：`shutdown_all()` 在程序退出时清理所有后台 Agent
- **Worktree 隔离**：支持 git worktree 隔离，子 Agent 在独立分支工作
- **权限冒泡**：权限请求通过 IPC 通道冒泡到父级终端

#### 权限队列与冒泡

后台 Agent 通过 AgentIPC 通道发送权限事件（`PermissionQueued` / `PermissionResolved`），支持排队与并发管理：

```rust
let queue_position = pending_count.fetch_add(1, Ordering::SeqCst) + 1;
// 发送 PermissionQueued 事件
let decision = callback(request.clone()).await;
// 发送 PermissionResolved 事件
```

### 2.6 Worktree 隔离

后台 Agent 支持 git worktree 隔离，通过环境变量控制：

- `ALLTHECODES_ALLOW_WORKTREE_FALLBACK=true` — 允许在不支持 worktree 时回退
- 子 Agent 在独立分支 `agent-worktree-{short_id}` 上操作
- 完成时检测变更：有变更保留 worktree，无变更自动清理

### 2.7 Agent 定义与工具过滤

`build_child_config()` 根据定义的 `AgentDefinitionEntry` 过滤工具：

- `disallowed_tools` — 不允许使用的工具列表
- `tools` — 白名单（空 = 全部允许）
- 支持通配符匹配和 `Bash(git status)` 语法
- 权限模式从父级继承，支持 `Default` / `Auto` / `Plan` / `Bypass` / `AcceptEdits`

## 三、Agent 树与 IPC 事件

Fork 和后台 Agent 通过 AgentTree 注册到全局树结构：

```rust
pub struct AgentNode {
    pub agent_id: String,
    pub parent_agent_id: Option<String>,
    pub description: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub state: String,           // "running" | "completed" | "error" | "cancelled"
    pub is_background: bool,
    pub depth: usize,
    pub chain_id: String,
    // ...
}
```

事件类型通过 IPC 通道发送：`Spawned`、`Completed`、`Aborted`、`StreamDelta`、`ThinkingDelta`、`ToolUse`、`ToolResult`、`PermissionQueued`、`PermissionResolved`、`TreeSnapshot`。

## 四、关键设计决策

1. **轻量无持久化**：fork 是自包含的子引擎调用，不写入磁盘，不改变父级会话
2. **Prompt Cache 优化**：`parent_messages` 共享上下文字节，最大化缓存命中率
3. **深度限制**：`MAX_AGENT_DEPTH = 5`，防止无限递归
4. **工具过滤**：通过 AgentDefinitionEntry 的 allow/disallow 列表微调子 Agent 工具集
5. **权限继承**：子 Agent 权限模式从父级继承，Plan 模式允许子级回退到 Default

## 五、使用方式

```bash
# Fork 作为旁路问答（无工具）
# 系统内部使用 /btw 命令触发

# 技能 Fork 执行
# Skill 定义中设置 context: fork 触发子引擎执行

# 后台 Agent
# AgentTool 设置 run_in_background: true
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-engine/src/agent/fork.rs` | Fork 核心定义 + 调用实现 |
| `crates/allthecodes-engine/src/agent/supervisor.rs` | 后台 Agent 生命周期管理 |
| `crates/allthecodes-engine/src/agent/dispatch.rs` | Agent 分发（普通 + worktree 模式） |
| `crates/allthecodes-engine/src/agent/mod.rs` | AgentTool 定义 + 工具过滤 |
| `crates/allthecodes-engine/src/agent/tool_impl.rs` | AgentTool 工具实现 |
| `crates/allthecodes-engine/src/agent/worktree.rs` | Worktree 隔离支持 |
| `crates/allthecodes-engine/src/agent/tests.rs` | 测试用例 |
