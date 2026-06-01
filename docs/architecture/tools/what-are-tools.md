---
title: "工具系统设计 - Tool 抽象与注册机制"
description: "深入理解 allthecodes 的 Tool trait 设计：从类型定义、注册机制、调用链路到权限检查，揭示内置工具如何通过统一的 Tool 接口协同工作。"
keywords: ["工具系统", "Tool trait", "rust 工具", "输入 schema", "权限检查", "get_all_tools"]
---

## AI 为什么需要工具

大语言模型本质上只能做一件事：**根据输入文本，生成输出文本**。

它不能读文件、不能执行命令、不能搜索代码。要让 AI 真正"动手"，需要一个桥梁——这就是 **Tool**（工具）。

工具是 AI 的双手。AI 说"我想读这个文件"，工具系统替它真正去读；AI 说"我想执行这条命令"，工具系统替它真正去跑。

## Tool trait：Rust 中的统一工具接口

所有工具都实现 `crates/allthecodes-tools/src/tool.rs` 中定义的 `Tool` trait。这不是一个具体的类型，而是一个包含 15+ 方法的 trait，任何满足该接口的 struct 就是一个工具：

### 核心方法

| 方法 | 类型 | 说明 |
|------|------|------|
| `name()` | `fn(&self) -> &str` | 唯一标识（如 `Read`、`Bash`、`Edit`） |
| `description()` | `async fn(&self, input) -> String` | **动态描述**——根据输入参数返回不同描述 |
| `input_json_schema()` | `fn(&self) -> Value` | JSON Schema 定义参数类型和校验规则 |
| `call()` | `async fn(&self, input, ctx, parent_message, on_progress) -> Result<ToolResult>` | 执行函数 |

### 安全与权限

| 方法 | 说明 |
|------|------|
| `validate_input()` | 输入校验（在权限检查之前），返回 `ValidationResult` |
| `check_permissions()` | 权限检查（在校验之后），返回 `PermissionResult` |
| `is_read_only()` | 是否只读操作（影响权限模式） |
| `is_destructive()` | 是否不可逆操作（删除、覆盖、发送） |
| `is_concurrency_safe()` | 相同输入是否可以并行执行 |
| `interrupt_behavior()` | 用户中断时的行为：`Cancel` 或 `Block` |

### 输出与元数据

| 方法 | 说明 |
|------|------|
| `max_result_size_chars()` | 结果字符上限（默认 `100_000`） |
| `user_facing_name()` | 对外显示的名称（如 `Bash(git)`） |
| `get_path()` | 提取操作的文件路径（用于权限匹配和 UI 显示） |
| `backfill_observable_input()` | 回填可观察字段（如文件路径） |
| `to_auto_classifier_input()` | 为自动分类器提供结构化输入 |

### Prompt 与上下文

| 方法 | 说明 |
|------|------|
| `prompt()` | 返回该工具的详细使用说明，注入到 System Prompt |
| `mcp_server_name()` | 如果是 MCP 工具，返回所属服务器名称 |

## 核心数据结构

### ToolResult

```rust
pub struct ToolResult {
    pub data: Value,                          // 主要返回数据
    pub model_content: Option<ToolResultContent>,  // 模型看到的精简内容
    pub display_preview: Option<String>,            // UI 展示预览
    pub new_messages: Vec<Message>,                 // 注入到对话的新消息
}
```

### ToolUseContext

工具执行时接收的上下文，包含了引擎状态的所有必要信息：

- `options: ToolUseOptions` — 调试模式、模型名称、自定义 prompt 等
- `abort_signal` — 中断信号（`tokio::sync::watch`）
- `read_file_state: FileStateCache` — 文件读取缓存
- `get_app_state` / `set_app_state` — 应用状态读写
- `session_id` — 当前会话 ID
- `agent_id` / `agent_type` — 当前 agent 标识
- `permission_callback` — 权限回调
- `ask_user_callback` — 用户询问回调
- `hook_runner` — Hook 执行器
- `command_dispatcher` — 命令分发器

### 校验与权限结果

```rust
pub enum ValidationResult {
    Ok,
    Error { message: String, error_code: i32 },
}

pub enum PermissionResult {
    Allow { updated_input: Value },
    Deny { message: String },
    Ask { message: String },
}
```

## 工具注册：分层组装

`crates/allthecodes-tools/src/registry.rs` 负责工具的注册和组装：

### 基础工具（始终可用）

`allthecodes_tools_base_tools()` 注册了以下内置工具：

- **文件操作**：Read, Write, Edit, Glob, Grep（来自 `crate::fs::tools()`）
- **任务管理**：TodoWrite, TaskCreate, TaskGet, TaskUpdate, TaskList, TaskStop, TaskOutput（来自 `crate::tasks::tools()`）
- **执行工具**：Bash（由 `exec` 模块提供）
- **对话工具**：AskUserQuestion, SendUserMessage
- **Web 工具**：WebFetch, WebSearch
- **规划工具**：EnterPlanMode, ExitPlanMode
- **其他**：Sleep, ConfigTool, StructuredOutput, Brief, SystemStatus, ToolSearch

### 外部工具提供者

通过 `ToolRegistryProviders` 注册跨 crate 的工具：

```rust
pub type ToolProvider = Arc<dyn Fn() -> Tools + Send + Sync + 'static>;

pub struct ToolRegistryProviders {
    pub base_tool_providers: Vec<ToolProvider>,    // 内置工具
    pub runtime_tool_providers: Vec<ToolProvider>, // 运行时插件
}
```

- `base_tool_providers`：由 `cc-engine`、`cc-teams`、`cc-worktree` 等 crate 注册
- `runtime_tool_providers`：运行时追加的工具，有重复名称保护

### Policy 过滤

`ToolPolicy` 枚举定义了不同运行时的可见工具白名单：

| Policy | 可见工具 | 用途 |
|--------|---------|------|
| `DefaultAgent` | 全部 | 交互式会话 |
| `Coordinator` | Agent, Task, SendMessage, TaskList, TaskStop, PR 订阅 | 协调器 lead |
| `CoordinatorWorker` | Glob, Grep, Read, Bash, Edit, Write, TodoWrite, TaskList, TaskUpdate, SendMessage | 协调器 worker |
| `InProcessTeammate` | Glob, Grep, Read, Bash, Edit, Write, TodoWrite, TaskList, TaskUpdate, TaskOutput, SendMessage | 进程内队友 |

## 工具调用的完整链路

从 API 返回 `tool_use` 到结果回传，经过以下步骤：

```
1. API 返回 tool_use block（包含 name + input）
   ↓
2. findToolByName() 查找工具
   ↓
3. validateInput() — 输入校验
   ↓ 失败 → 返回错误 ToolResult
4. checkPermissions() — 规则匹配
   ↓ 拒绝 → 返回拒绝 ToolResult
5. call() — 执行实际操作
   ↓ onProgress() 回调实时更新 UI
6. 返回 ToolResult
   ↓
7. 新消息追加到对话 → 进入下一轮迭代
```

## 编译时特性控制

工具模块通过 Cargo feature flags 控制编译：

```rust
// lib.rs
#[cfg(feature = "full")]
pub mod exec;       // Bash, PowerShell 等
#[cfg(feature = "full")]
pub mod fs;         // 文件操作工具
#[cfg(feature = "full")]
pub mod hooks;      // Hook 系统
#[cfg(feature = "full")]
pub mod tasks;      // 任务管理工具
#[cfg(feature = "full")]
pub mod web_search; // 搜索工具
```

这让 `allthecodes-tools` crate 可以在不依赖完整引擎的情况下，提供工具契约和注册辅助函数。

## 文件状态缓存（FileStateCache）

Read/Write/Edit 工具共享一个文件状态缓存，用于：

- 记录文件读取的时间戳和内容哈希
- 在编辑时校验文件未被外部修改（防覆写）
- 支持多个路径键（原始路径、解析路径、规范路径）

```rust
pub struct FileStateCache {
    pub entries: Arc<RwLock<HashMap<String, FileCacheEntry>>>,
}

pub struct FileCacheEntry {
    pub content_hash: u64,
    pub last_read_timestamp: i64,
}
```

缓存通过 `hash_content()` 使用 Rust 默认哈希器计算内容指纹，确保原子性编辑的安全校验。
