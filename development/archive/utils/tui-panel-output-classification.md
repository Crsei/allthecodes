# TUI Panel Output Content Classification

TUI 采用单消息流架构（非多面板），消息经过 7 层分类后渲染到统一视图中。

---

## 1. 核心消息类型 (`Message` 枚举)

定义位置：`crates/cc-types/src/message.rs`

```rust
pub enum Message {
    User(UserMessage),        // 用户输入（文本/工具结果/图像）
    Assistant(AssistantMessage), // AI 回复（文本/工具调用/思考/图像）
    System(SystemMessage),    // 系统消息（压缩边界/API错误/本地命令/警告）
    Progress(ProgressMessage),   // 进度指示（运行中的工具摘要）
    Attachment(AttachmentMessage), // 附件元数据（文件编辑/队列命令/max_turns 等）
}
```

## 2. ContentBlock 枚举（助理/用户消息内容块）

```rust
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id, name, input },
    ServerToolUse { id, name, input },
    ToolResult { tool_use_id, content, is_error },
    Thinking { thinking, signature },
    RedactedThinking { data },
    ConnectorText { connector_text, signature },
    Image { source: ImageSource },
}
```

## 3. SystemSubtype 枚举（系统消息子类型）

```rust
pub enum SystemSubtype {
    CompactBoundary { compact_metadata },
    MicrocompactBoundary { microcompact_metadata },
    ApiError { retry_attempt, max_retries, retry_in_ms, error },
    Informational { level: InfoLevel },  // Info, Warning, Error
    LocalCommand { content },
    Warning,
}
```

## 4. 系统文本标记分类

定义位置：`src/ui/messages/system_text_message.rs`

`classify_system_message(tag, message)` 解析 `(tag, message)` 为 `SystemTagKind`：

```rust
pub enum SystemTagKind {
    TurnDuration { duration_ms, budget_limit, budget_tokens, budget_nudges },
    MemorySaved { written_paths, verb },
    AwaySummary(String),
    AgentsKilled,
    Thinking(String),
    BridgeStatus { url, upgrade_nudge },
    ScheduledTaskFire(String),
    PermissionRetry(Vec<String>),
    ApiError { retry_attempt, error, retry_in_ms, max_retries },
    StopHookSummary { hook_count, hook_infos, hook_errors, ... },
    Generic { content, level },
}
```

## 5. 用户文本路由（XML 标签驱动）

定义位置：`src/ui/messages/user_text_message.rs`

`route_user_text(text)` 通过 XML 标签将纯用户文本路由到不同渲染器：

```rust
pub enum UserTextRendered {
    Rendered(String),              // 完全渲染的字符串
    Hidden,                        // 隐藏（无内容/滴答信号/本地命令提示）
    Delegated(&'static str, String), // 委托给另一个渲染器
}
```

路由键：

| 标签 | 路由键 | 说明 |
|---|---|---|
| `<bash-stdout>` / `<bash-stderr>` | `bash_output` | Bash 命令输出 |
| `<local-command-stdout>` / `<local-command-stderr>` | `local_command_output` | 本地命令输出 |
| `<bash-input>` | `bash_input` | Bash 输入 |
| `<command-message>` | `command` | 斜杠命令消息 |
| `<user-memory-input>` | `memory_input` | 记忆输入 |
| `<task-notification>` | `agent_notification` | 代理任务通知 |
| `<mcp-resource-update>` / `<mcp-polling-update>` | `resource_update` | MCP 资源更新 |
| `<fork-boilerplate>` | `fork_boilerplate` | 分支模板 |
| 默认 | `prompt` | 用户提示文本 |

隐藏路由：空内容、`[NO_CONTENT]`、`<local-command-caveat>`、`<tick>`

## 6. 助理文本 API 错误分类

定义位置：`src/ui/messages/assistant_text_message.rs`

```rust
pub enum AssistantApiError<'a> {
    RateLimit(&'a str),
    PromptTooLong(&'a str),
    CreditBalanceTooLow(&'a str),
    InvalidApiKey(&'a str),
    InvalidApiKeyExternal(&'a str),
    OrgDisabledEnvKey(&'a str),
    OrgDisabledEnvKeyWithOAuth(&'a str),
    TokenRevoked(&'a str),
    ApiTimeout(&'a str),
    CustomOffSwitch(&'a str),
    UserAbort,
    ApiErrorPrefix(&'a str),
}
```

## 7. RenderableMessage 枚举（预处理后的渲染视图）

定义位置：`src/ui/messages/render/context.rs`

```rust
pub(crate) enum RenderableMessage {
    Message { message: Message, source_index: usize },
    GroupedToolUse(GroupedToolUseRenderRecord),
    CollapsedReadSearch(CollapsedReadSearchRenderRecord),
}
```

预处理管道（`prepare_renderable_messages()`，按顺序应用）：

1. `normalize_messages_for_render` — 拆分为单 ContentBlock 的 RenderableMessage
2. `filter_compact_boundary` — 仅保留最新 compact 边界之后的消息
3. `should_show_renderable_message` — 隐藏进度消息、空附件等
4. `reorder_messages_in_ui` — 连接 ToolResult 及其匹配的 ToolUse
5. `filter_brief_messages` — 摘要过滤（当前为 pass-through）
6. `truncate_transcript_messages` — 转录模式限制为 30 条
7. `apply_grouping` — 同类工具调用合并（Task、Agent、Read、Grep、Glob）
8. `collapse_read_search_groups` — 连续 Read/Search/List 折叠为摘要行

### 可折叠工具类型

定义位置：`src/ui/messages/render/grouping.rs`

```rust
enum CollapsibleKind {
    Read,   // "Read"
    Search, // "Grep", "Glob", "WebSearch"
    List,   // "LS", "List"
}
```

## 8. 渲染调度

定义位置：`src/ui/messages/render/mod.rs`

### 助理消息渲染

- `ContentBlock::Text` → Markdown 渲染，API 错误文本特殊样式
- `ContentBlock::ToolUse` → 工具名 + 状态指示（`ToolUseState`）
- `ContentBlock::Thinking` → 扩展思考块（verbose/transcript 模式可见）
- `ContentBlock::Image` → `[image: type, size]` 占位

### 用户消息渲染

- Bash 输出 → 可展开/折叠
- ToolResult → bash 输出或文件差异预览
- 普通文本 → 用户消息背景色渲染

### 系统消息渲染

- `CompactBoundary` → "会话已压缩"
- `ApiError` → api_error 标签样式
- `Informational` → 按 InfoLevel 分级样式
- `LocalCommand` → "$ " 前缀

### 工具使用状态机

定义位置：`src/ui/messages/assistant_tool_use_message.rs`

```rust
pub enum ToolUseState {
    Queued,
    InProgress,
    Resolved,
    Error,
    WaitingForPermission,
    ClassifierChecking,
}
```

## 9. 功能面板标签系统

定义位置：`src/ui/components/feature_panels.rs`

```rust
pub enum FeaturePanelKind {
    Mcp,       // MCP 服务器
    Agents,    // 自定义代理
    Teams,     // 团队成员
    Lsp,       // LSP 集成
    Settings,  // 配置
    Sandbox,   // 沙盒环境
    Plugins,   // 插件
    Skills,    // Slash 命令技能
    Tasks,     // 计划任务
}
```

## 10. 视觉样式（Theme）

定义位置：`src/ui/rendering/theme.rs`

| 样式字段 | 用途 |
|---|---|
| `assistant_name` | 助理文本标签 |
| `user_name` | 用户文本标签 |
| `system_name` | 系统消息 |
| `tool_name` | 工具使用块、分组工具调用 |
| `tool_result` | 工具结果、bash 输出 |
| `error` | 错误消息、API 错误、工具错误 |
| `warning` | 警告、中断消息 |
| `info` | 信息消息 |
| `dim` | 次要文本、压缩边界、折叠摘要 |
| `thinking` | 扩展思考块 |
| `code`, `code_bg` | Markdown 代码块 |
| `heading`, `bold`, `italic`, `link` | Markdown 格式 |
| `syntax_*` | 语法高亮令牌 |

## 关键文件索引

| 文件 | 用途 |
|---|---|
| `crates/cc-types/src/message.rs` | 核心 Message、ContentBlock、SystemSubtype |
| `src/ui/messages/render/context.rs` | RenderableMessage、预处理管道 |
| `src/ui/messages/render/mod.rs` | 渲染调度主入口 |
| `src/ui/messages/render/render_assistant.rs` | 助理/系统/进度/附件消息渲染 |
| `src/ui/messages/render/render_user.rs` | 用户消息渲染 |
| `src/ui/messages/render/preprocessing.rs` | 规范化、过滤、重排 |
| `src/ui/messages/render/grouping.rs` | 工具使用分组与折叠 |
| `src/ui/messages/system_text_message.rs` | 系统消息标签分类 |
| `src/ui/messages/user_text_message.rs` | 用户文本路由 |
| `src/ui/messages/assistant_text_message.rs` | 助理 API 错误分类 |
| `src/ui/messages/assistant_tool_use_message.rs` | 工具使用状态机 |
| `src/ui/components/feature_panels.rs` | 功能面板标签系统 |
| `src/ui/rendering/theme.rs` | 视觉样式定义 |
