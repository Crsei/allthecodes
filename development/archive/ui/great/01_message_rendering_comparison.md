# 消息渲染对比：Rust (ratatui) vs TypeScript (Ink)

> **日期**：2026-05-17
> **范围**：`rust/crates/claude-code-rs/src/ui/messages/` 中的所有消息级渲染组件与 `claude-code-bun/src/components/messages/` 中对应的组件。
> **目的**：识别完整性差距，确定移植工作的优先级，并记录架构差异。

---

## 架构概览

### Rust 端 (ratatui + crossterm)

- **渲染模型**：即时模式，逐帧 `Buffer` + `Rect`。每条消息渲染为 `Vec<Line<'a>>`（一组带有样式 span 的 ratatui 文本行）。
- **关键类型**：`ratatui::text::Line`、`ratatui::text::Span`、`ratatui::style::Style`。
- **主调度器**：`render.rs` 第 232 行 `render_single_message_with_context()` 匹配 `Message::User` / `Assistant` / `System` / `Progress` / `Attachment`。
- **虚拟滚动**：`VirtualScroll` 结构体处理视口裁剪；`render_messages()` 在第 134 行仅渲染可见消息。
- **独立辅助文件**：约 31 个独立的 `*_message.rs` 文件，每个导出返回 `String` 的 `render_*_message()` 函数。这些文件**不**被主调度器调用——它们是独立的辅助程序，可能用于非 ratatui 渲染路径（例如，无头/IPC 文本输出）。
- **`_theme: &Theme` 模式**：几乎所有辅助文件都接受 `Theme` 引用但忽略它——它们生成的是无样式的纯文本。

### TypeScript 端 (Ink / React)

- **渲染模型**：声明式、基于组件。每条消息是一个接收 props 并返回 Ink JSX（`<Box>`、`<Text>` 等）的 React 组件。
- **关键原语**：`Box`（flexbox 布局）、`Text`（样式文本）、`Markdown`、`Spinner`、`Color`、`Static`。
- **组件模式**：每个 `*.tsx` 文件导出一个默认的 React 函数式组件，使用 `useContext`、`useMemo`、特性门控和动态导入。
- **特性门控**：`feature()` 来自 `bun:bundle`，支持编译时死代码消除（`KAIROS`、`FORK_SUBAGENT`、`TEAMMEM`、`UDS_INBOX` 等）。
- **丰富的交互性**：通过 `MessageActionsSelectedContext` 实现选择状态、复制到剪贴板（`CtrlO`）、详情展开、微调动画、结构化 diff 渲染。

### 主要架构差异

| 方面 | Rust (ratatui) | TypeScript (Ink) |
|--------|---------------|-----------------|
| 渲染模型 | 即时模式（逐帧绘制） | 声明式（虚拟 DOM diff） |
| 样式 | `Style` 结构体，手动组合 | `Text` props，`Color` 包装器 |
| 布局 | 手动坐标计算 | 通过 `Box` 实现 Flexbox |
| 文本换行 | `wrap.rs` 支持 unicode 宽度感知 | 内置 `Text` 换行 |
| Markdown | `markdown_to_lines()` (ratatui) | `<Markdown>` 组件（丰富） |
| 动画 | 手动光标字符 | `<Spinner>` 组件 |
| 选择 | `MessageRenderContext` | `useContext(MessageActionsSelectedContext)` |
| 特性门控 | cfg 标志 / 运行时检查 | 编译时 `feature()` |
| 复制文本 | `message_copy_text()` 函数 | `useCopy` / `CtrlOToExpand` 包装器 |
| 错误边界 | 无 | `<SentryErrorBoundary>` |

---

## 完整性评级

| 评级 | 含义 |
|--------|---------|
| 5 / 5 | 功能完备：覆盖所有 TS 功能，具有正确的样式 |
| 4 / 5 | 接近完备：少量缺失功能或样式差异 |
| 3 / 5 | 部分实现：核心功能存在，仍有显著差距 |
| 2 / 5 | 基础：有功能，但缺乏样式、交互性和边界情况处理 |
| 1 / 5 | 骨架：仅字符串输出，无样式，无交互性 |

---

## 组件对比

---

### 1. 主调度器：`render.rs`

| 方面 | 详情 |
|--------|--------|
| **Rust 文件** | `messages/render.rs`（1146 行） |
| **TS 对应文件** | 无单一对应文件。通过 `UserTextMessage.tsx` 标签分析 + `AssistantToolUseMessage.tsx` + `SystemTextMessage.tsx` 分发 |
| **完整性** | **4 / 5** |

**Rust 实现（关键函数）：**

```rust
// 主入口 — 将可见消息渲染到 ratatui 缓冲区
pub fn render_messages(messages: &[Message], area: Rect, buf: &mut Buffer,
    theme: &Theme, streaming: bool, scroll: usize,
    vscroll: &VirtualScroll, render_context: &MessageRenderContext) { ... }

// 单条消息调度器 — 路由到 User/Assistant/System/Progress/Attachment
pub fn render_single_message_with_context(msg: &Message, index: usize,
    theme: &Theme, width: usize,
    render_context: &MessageRenderContext) -> Vec<Line<'a>> {
    match msg {
        Message::User(user_msg) => render_user_message(user_msg, theme, index, width, render_context),
        Message::Assistant(assistant_msg) => render_assistant_message(assistant_msg, theme),
        Message::System(system_msg) => render_system_message(system_msg, theme),
        Message::Progress(progress_msg) => render_progress_message(progress_msg, theme),
        Message::Attachment(attachment_msg) => render_attachment_message(attachment_msg, theme),
    }
}
```

**用户消息渲染**（`render_user_message`，第 356 行）：
- 检测 `[Request interrupted by user]` 并渲染警告样式
- 使用 `theme.user_name` 样式渲染 "You: " 前缀
- 将工具结果用户消息路由到 `render_tool_result_user_message()`，该函数会：
  - 检测 Shell 输出并路由到 `render_user_bash_output_message_with_options()`
  - 检测文件编辑工具使用并路由到 `render_file_edit_tool_updated_message()`
  - 回退到 pretty-JSON 或使用 `theme.tool_result` 样式的原始文本
- 处理 `MessageContent::Text` vs `MessageContent::Blocks`

**助手消息渲染**（`render_assistant_message`，第 550 行，约 200 行）：
- 在第一个块上使用 `theme.assistant_name` 样式渲染 "Claude: " 前缀
- `ContentBlock::Text` / `ConnectorText` -> 通过 `markdown_to_lines()` 进行 Markdown 渲染
- `ContentBlock::ToolUse` / `ServerToolUse` -> 带样式的 `[tool_name] input_summary`（暗色）
- `ContentBlock::ToolResult` -> "Result:" 或 "Error:" 前缀，前 5 行 + "`... N more lines`"
- `ContentBlock::Thinking` -> `[thinking]` 头部 + 前 3 行 + "`... N more lines`"
- `ContentBlock::RedactedThinking` -> `[redacted thinking]`
- `ContentBlock::Image` -> `[image: media_type, N chars]`
- 如果 `msg.cost_usd > 0.0` 则显示费用

**系统消息渲染**（`render_system_message`，第 750 行）：
- 子类型路由：CompactBoundary/MicrocompactBoundary（带 token 计数）、ApiError、Informational（Info/Warning/Error）、LocalCommand、Warning
- 每种类型使用主题中的相应样式

**进度消息渲染**（`render_progress_message`，第 800 行）：
- 使用暗色样式显示 `"  ... {message}"`

**附件消息渲染**（`render_attachment_message`，第 823 行）：
- 处理 `EditedTextFile`、`QueuedCommand`、`MaxTurnsReached`、`StructuredOutput`、`HookStoppedContinuation`、`NestedMemory`、`SkillDiscovery`

**选择装饰**（`decorate_selected_message`，第 279 行）：
- 添加 "▶ selected" / "▼ selected" 头部，带复制/详情提示
- 展开的详情行：uuid、时间戳、引用、截断的复制预览

**相对于 TS 缺失的功能：**
- 无组件级错误边界（TS 使用 `SentryErrorBoundary` 包装）
- 未解决的工具使用无动画 `Spinner`
- 无 `ToolUseLoader` 组件
- 无基于 `useContext` 的选择高亮（Rust 使用手动 `MessageRenderContext`）
- 无 `CtrlOToExpand` 键盘快捷键集成
- 无来自 Tool trait 的逐工具 `backgroundColor` / `userFacingToolName`
- 无嵌套工具调用的 `isTransparentWrapper` 检测
- 无工具执行中的进度消息（`HookProgressMessage`、进度跟踪）
- 无分类器检查状态（工具渲染路径中的 "Permission denied" 检测）

---

### 2. 行换行：`wrap.rs`

| 方面 | 详情 |
|--------|--------|
| **Rust 文件** | `messages/wrap.rs`（49 行） |
| **TS 对应文件** | 内置在 Ink `<Text>` 组件中（`wrap` prop） |
| **完整性** | **4 / 5** |

**Rust 实现：**

```rust
pub fn wrap_line_to_width(line: &Line<'_>, width: u16) -> Vec<Line<'static>> {
    // 遍历 spans，跟踪 unicode 字符宽度，
    // 当 current_width + ch_width > max_width 时换行。
    // 在每个换行段保留样式。
    // 返回多个 Line 对象以显示换行后的内容。
}
```

**评估：**
具有样式保留的 Unicode 宽度感知换行。实现扎实，满足了核心需求。Ink 的 `<Text>` 换行是内置的，处理了更多边界情况（单词边界、CJK 换行规则），但这已经很好地覆盖了基本需求。

**缺失功能：**
- 无单词边界感知换行（会在单词中间断开）
- 无连字支持
- 无 ANSI 序列的零宽字符处理（换行前已被剥离）

---

### 3. Bash 输出：`user_bash_output_message.rs`

| 方面 | 详情 |
|--------|--------|
| **Rust 文件** | `messages/user_bash_output_message.rs`（229 行） |
| **TS 对应文件** | `UserBashOutputMessage.tsx` |
| **完整性** | **5 / 5** |

**Rust 实现（最完整的独立渲染器）：**

```rust
pub fn render_user_bash_output_message_with_options(
    command: &str, output: &str, options: ShellOutputRenderOptions
) -> String {
    let output = output.trim();
    if output.is_empty() { return format!("{command} -> (no output)"); }
    let formatted = format_shell_output(output);  // 剥离 ANSI + pretty-print JSON
    let all_lines = formatted.lines().filter(|l| !l.trim().is_empty())...;
    // 折叠到最后 N 行（默认为 5），如果展开则显示全部
    let shown_start = if options.expanded || all_lines.len() <= max_lines { 0 }
                     else { all_lines.len() - max_lines };
    // 将每行截断到指定宽度
    // 构建页脚："+N lines"、经过时间/超时持续时间、文件大小
    // 返回带有 "$ command" 头部的格式化内容
}
```

**覆盖的功能：**
- `ShellOutputRenderOptions`：`width`、`max_lines`、`expanded`、`elapsed_ms`、`timeout_ms`、`total_lines`、`total_bytes`
- 通过 `strip_ansi_escapes` crate 剥离 ANSI 转义序列
- 逐行 JSON pretty-printing（上限为 10K 字符）
- 通过 `UnicodeWidthChar` 实现 Unicode 宽度感知的行截断
- 页脚渲染：折叠行数、经过/超时持续时间、文件大小（B/KB/MB）
- 智能格式化的 `format_duration()`（ms / s / .1s）
- 全面的测试覆盖了折叠、展开、ANSI+JSON+截断场景

**相对于 TS 缺失的功能：**
- 无用于 stdout/stderr 颜色区分的 `<Text>` 颜色
- 无头部中 Shell 命令的语法高亮
- 无内联展开动画（折叠 -> 展开过渡）

**影响：** Rust 实现基本功能完备。仅存在微小的样式差异。

---

### 4. 用户工具结果消息：`user_tool_result_message/`

| 方面 | 详情 |
|--------|--------|
| **Rust 文件** | `messages/user_tool_result_message/`（9 个文件，mod.rs + 8 个子模块） |
| **TS 对应文件** | `UserToolResultMessage/` 目录 |
| **完整性** | **3 / 5** |

**模块结构：**
```
user_tool_result_message/
├── mod.rs                              # 调度器 + 测试
├── utils.rs                            # 常量、ToolResultBlock、UserToolResultLookups
├── user_tool_result_message.rs         # 主调度路由
├── user_tool_canceled_message.rs       # 取消渲染
├── user_tool_error_message.rs          # 错误渲染
├── user_tool_success_message.rs        # 成功渲染
├── user_tool_reject_message.rs         # 拒绝渲染
├── rejected_plan_message.rs            # 计划拒绝
└── rejected_tool_use_message.rs        # 工具使用拒绝
```

**关键实现细节：**

`utils.rs` 常量：
```rust
pub const INTERRUPT_MESSAGE_FOR_TOOL_USE: &str = "[Request interrupted by user for tool use]";
pub const CANCEL_MESSAGE: &str = "The user doesn't want to take this action right now...";
pub const REJECT_MESSAGE: &str = "The user doesn't want to proceed with this tool use...";
pub const PLAN_REJECTION_PREFIX: &str = "The agent proposed a plan that was rejected...";
pub const REJECT_MESSAGE_WITH_REASON_PREFIX: &str = "The user doesn't want to proceed...";
pub const CLASSIFIER_DENIAL_PREFIX: &str = "Permission for this action has been denied. Reason: ";
```

`user_tool_success_message.rs`：
```rust
pub fn render_user_tool_success_message(...) -> Vec<Line<'static>> {
    // 工具名称头部 + 格式化输出（pretty-print JSON）
    // 优雅地支持未知工具（无工具实例）
}
```

`user_tool_error_message.rs`：
```rust
// 处理计划拒绝前缀 -> 渲染拒绝计划头部
// 处理原因前缀 -> 显示包含的原因 + 用户消息
// 处理分类器拒绝 -> 渲染权限拒绝
// 回退到 "Tool failed with error:" 通用前缀
```

**现有的测试：** 通过 `insta::assert_snapshot!()` 进行快照测试，覆盖：
- 已取消、已拒绝工具、已拒绝计划、成功、错误、计划错误
- 路由分发（取消 vs 成功）
- 拒绝原因的保留
- 未知错误
- 中断检测

**相对于 TS 缺失的功能：**
- 所有子模块上均有 `#[allow(dead_code)]`——未接入主渲染路径
- 无上下文中的工具查找（在测试中传递空的 `UserToolResultLookups`）
- 无来自 Tool trait 的 `userFacingToolName` / `backgroundColor`
- 无 `CANCEL_MESSAGE` 内容变体（硬编码字符串，无 i18n）
- 无用于结构化内容块的 `ToolResultBlock::with_content()` 构建器
- 无详细的错误分类（rate_limit、auth_error 等）
- 无可重试错误区分

---

### 5. 独立骨架渲染器（每项 1-2 星）

这约 31 个文件都遵循相同的模式：一个返回 `String` 的 `render_*_message()` 函数，接受 `_theme: &Theme` 但忽略它。它们生成的是无样式、无颜色、无布局的纯文本。

---

#### 5.1 `user_text_message.rs`（12 行）

```rust
pub fn render_user_text_message(text: &str, _theme: &Theme) -> String {
    if text.trim().is_empty() { "You: <empty>".to_string() }
    else { format!("You: {text}") }
}
```
- **TS 对应文件：** `UserTextMessage.tsx`（约 275 行，主调度器）
- **完整性：** 1/5
- **缺失内容：** 基于标签的路由（bash-input、bash-stdout、MEMORY_OVERRIDE 等）、特性门控的消息类型（GitHub webhook、agent 通知、fork 样板、跨会话、频道）、计划消息检测、颜色、布局
- **影响：** 高——这是 TS 中的中央调度器；Rust 版本只是一个桩

---

#### 5.2 `user_prompt_message.rs`（12 行）

```rust
pub fn render_user_prompt_message(prompt: &str, _theme: &Theme) -> String {
    let prompt = prompt.trim();
    if prompt.is_empty() { "Prompt: <empty>".to_string() }
    else { format!("Prompt: {prompt}") }
}
```
- **TS 对应文件：** `UserPromptMessage.tsx`（约 80 行）
- **完整性：** 1/5
- **缺失内容：** 10K 字符截断，带首尾分割（2500/2500）、带 KAIROS 特性门控的简要布局模式、`isBriefOnly`、`viewingAgentTaskId`、上下文相关的背景色、`messageActionsBackground` 选择高亮
- **影响：** 高——TS 版本具有复杂的截断和布局模式

---

#### 5.3 `assistant_text_message.rs`（11 行）

```rust
pub fn render_assistant_text_message(text: &str, _theme: &Theme) -> String {
    if text.trim().is_empty() { "Assistant: <empty text>".to_string() }
    else { format!("Assistant: {text}") }
}
```
- **TS 对应文件：** `AssistantTextMessage.tsx`（约 270 行）
- **完整性：** 1/5
- **缺失内容：** 10+ 种错误状态切换（速率限制、提示过长带升级提示、信用余额、无效 API 密钥带钥匙串锁定检测、组织已禁用、令牌已撤销、API 超时带环境变量提示、Opus 需求、用户中止）、带 1000 字符截断的通用 API 错误前缀 + `CtrlOToExpand`、使用 `BLACK_CIRCLE` 的 Markdown 渲染、通过 `useContext(MessageActionsSelectedContext)` 的选择背景
- **影响：** 关键——TS 版本处理所有 API 错误状态并提供用户指导；Rust 版本完全没有

---

#### 5.4 `assistant_thinking_message.rs`（17 行）

```rust
pub fn render_assistant_thinking_message(thinking: &str, _theme: &Theme) -> String {
    let body = thinking.trim();
    if body.is_empty() { return "Assistant thinking: <empty>".to_string(); }
    let preview_len = body.chars().take(120).collect::<String>();
    if body.len() > preview_len.len() {
        format!("Assistant thinking: {preview_len}...")
    } else {
        format!("Assistant thinking: {body}")
    }
}
```
- **TS 对应文件：** `AssistantThinkingMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 可折叠章节、签名验证状态、暗色/斜体样式、"N more lines" 页脚、时间信息、用于查看完整内容的 `CtrlOToExpand`
- **影响：** 低——`render.rs` 中的主调度器已在 `render_assistant_message()` 中处理了带预览和 "N more lines" 的 thinking 块

---

#### 5.5 `assistant_redacted_thinking_message.rs`（6 行）

```rust
pub fn render_assistant_redacted_thinking_message(_theme: &Theme) -> String {
    "Assistant thinking was redacted by policy".to_string()
}
```
- **TS 对应文件：** `AssistantRedactedThinkingMessage.tsx`
- **完整性：** 2/5
- **缺失内容：** 策略名称显示、带样式的暗色/斜体渲染、锁图标
- **影响：** 低——主调度器将其处理为 `[redacted thinking]` span

---

#### 5.6 `assistant_tool_use_message.rs`（9 行）

```rust
pub fn render_assistant_tool_use_message(tool_name: &str, input: &str, _theme: &Theme) -> String {
    let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Running);
    format!("Assistant used {}", activity.display_call())
}
```
- **TS 对应文件：** `AssistantToolUseMessage.tsx`（约 368 行，最大的单个组件）
- **完整性：** 1/5
- **缺失内容：** 使用 `useMemo` 的工具查找、来自 Tool trait 的 `userFacingToolName` + `backgroundColor`、`isTransparentWrapper`、状态机（`isResolved`、`isQueued`、`isWaitingForPermission`、`isClassifierChecking`）、`renderToolUseProgressMessage()`、`renderToolUseQueuedMessage()`、`ToolUseLoader` spinner 组件、`SentryErrorBoundary` 包装器、`HookProgressMessage` 集成
- **影响：** 关键——TS 版本具有完整的带进度/队列/加载器状态的状态机；Rust 版本只是一行代码

---

#### 5.7 `system_text_message.rs`（19 行）

```rust
pub fn render_system_text_message(tag: &str, message: &str, _theme: &Theme) -> String {
    // format!("System({tag}): {message}") 带空检查
}
```
- **TS 对应文件：** `SystemTextMessage.tsx`（约 827 行，最长的 TS 消息组件）
- **完整性：** 1/5
- **缺失内容：** 子类型路由：`turn_duration`（随机动词、预算跟踪、后台任务）、`memory_saved`（TEAMMEM 特性门控、`FilePathLink` 悬停下划线）、`away_summary`、`agents_killed`、`thinking`、`bridge_status`（`/remote-control` 链接）、`scheduled_task_fire`、`permission_retry`、`stop_hook_summary`（hook 信息/错误/继续、`CtrlOToExpand`）、`api_error` 状态码着色、带基于级别的点/颜色/暗色的通用文本
- **影响：** 关键——TS 有 15+ 种消息子类型；Rust 完全没有

---

#### 5.8 `system_api_error_message.rs`（12 行）

```rust
pub fn render_system_api_error_message(status: u16, detail: &str, _theme: &Theme) -> String {
    format!("API error {status}: {detail}")
}
```
- **TS 对应文件：** `SystemAPIErrorMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 状态码着色（4xx vs 5xx）、重试提示、文档链接、结构化的错误详情
- **影响：** 中——`render.rs` 中的主 `render_assistant_message()` 也没有使用这个

---

#### 5.9 `rate_limit_message.rs`（12 行）

```rust
pub fn render_rate_limit_message(feature: &str, retry_after_ms: u64, _theme: &Theme) -> String {
    format!("Rate limit hit on {reason}; retry in {retry_after_ms}ms")
}
```
- **TS 对应文件：** `RateLimitMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 视觉警告样式、retry-after 格式化（人类可读的持续时间）、速率限制上下文（哪些限制、重置时间）、升级建议
- **影响：** 中

---

#### 5.10 `compact_boundary_message.rs`（11 行）

```rust
pub fn render_compact_boundary_message(before_tokens: usize, after_tokens: usize, _theme: &Theme) -> String {
    format!("Context compacted: before={before_tokens} after={after_tokens}")
}
```
- **TS 对应文件：** `CompactBoundaryMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 暗色样式、令牌节省计算、令牌比率可视化、微压缩详情（已压缩的工具 ID 计数）
- **影响：** 低——`render.rs` 中的主调度器已经处理了带有样式和元数据的压缩边界

---

#### 5.11 `attachment_message.rs`（12 行）

```rust
pub fn render_attachment_message(label: &str, detail: &str, _theme: &Theme) -> String {
    format!("Attachment: {label} -> {detail_line}")
}
```
- **TS 对应文件：** `AttachmentMessage.tsx`（约 536 行）
- **完整性：** 1/5
- **缺失内容：** 25+ 种附件类型特定的渲染器、hook 类型（async_hook_response、hook_blocking_error、hook_non_blocking_error、hook_error_during_execution、hook_success、hook_stopped_continuation、hook_system_message、hook_permission_decision）、带有来自 AppState 的 agent 颜色的队友任务状态、每种类型的结构化布局
- **影响：** 高——TS 版本是一个主要组件，具有广泛的类型特定渲染

---

#### 5.12 `shutdown_message.rs`（11 行）

```rust
pub fn render_shutdown_message(reason: &str, _theme: &Theme) -> String {
    format!("Session shutting down: {reason}")
}
```
- **TS 对应文件：** `ShutdownMessage.tsx`
- **完整性：** 2/5
- **缺失内容：** 警告样式、彩色边框、退出代码显示、重启提示
- **影响：** 低

---

#### 5.13 `user_plan_message.rs`（11 行）

```rust
pub fn render_user_plan_message(plan: &str, _theme: &Theme) -> String {
    format!("User plan: {plan}")
}
```
- **TS 对应文件：** `UserPlanMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 计划步骤编号、审批状态指示器、格式化的检查列表、计划模式横幅
- **影响：** 中

---

#### 5.14 `user_bash_input_message.rs`（11 行）

```rust
pub fn render_user_bash_input_message(command: &str, _theme: &Theme) -> String {
    if command.trim().is_empty() { "You ran a bash command (empty)".to_string() }
    else { format!("> {command}") }
}
```
- **TS 对应文件：** `UserBashInputMessage.tsx`
- **完整性：** 2/5
- **缺失内容：** Shell 提示符样式、语法高亮、工作目录显示、退出代码着色、持续时间显示
- **影响：** 低——这是一个辅助渲染器；Shell 输出是主要显示内容

---

#### 5.15 `user_command_message.rs`（12 行）

```rust
pub fn render_user_command_message(command: &str, cwd: Option<&str>, _theme: &Theme) -> String {
    format!("Command ({cwd}): {command}")
}
```
- **TS 对应文件：** `UserCommandMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 工作目录暗色样式、命令语法信息弹窗、长命令的 `CtrlOToExpand`
- **影响：** 低

---

#### 5.16 `user_image_message.rs`（16 行）

```rust
pub fn render_user_image_message(path: &str, format_hint: &str, _theme: &Theme) -> String {
    format!("Image: {path} ({format_hint})")
}
```
- **TS 对应文件：** `UserImageMessage.tsx`
- **完整性：** 2/5
- **缺失内容：** 图像尺寸显示、缩略图占位符、暗色样式
- **影响：** 低——图像渲染在终端中固有地受限

---

#### 5.17 `user_local_command_output_message.rs`（18 行）

```rust
pub fn render_user_local_command_output_message(command: &str, exit_code: i32, output: &str, _theme: &Theme) -> String {
    format!("Local command [{exit_code}]: {command} -> {body}")
}
```
- **TS 对应文件：** `UserLocalCommandOutputMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 退出代码颜色（非零为红色）、stdout/stderr 分离、带 "N more lines" 页脚的输出截断、持续时间显示
- **影响：** 中

---

#### 5.18 `user_memory_input_message.rs`（16 行）

```rust
pub fn render_user_memory_input_message(key: &str, value: &str, _theme: &Theme) -> String {
    format!("Memory input: {key}={value}")
}
```
- **TS 对应文件：** `UserMemoryInputMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 键值样式（键加粗、值暗色）、TEAMMEM 特性门控集成、多值记忆的结构化格式
- **影响：** 低

---

#### 5.19 `user_teammate_message.rs`（17 行）

```rust
pub fn render_user_teammate_message(teammate: &str, status: &str, _theme: &Theme) -> String {
    format!("Teammate {teammate}: {status}")
}
```
- **TS 对应文件：** `UserTeammateMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 来自 AppState 的 agent 颜色、角色图标、状态进度条、`isAgentSwarmsEnabled` 特性门控
- **影响：** 低——多 agent 协调是一个辅助功能

---

#### 5.20 `user_resource_update_message.rs`（17 行）

```rust
pub fn render_user_resource_update_message(resource: &str, delta: &str, _theme: &Theme) -> String {
    format!("Resource update: {resource} {delta}")
}
```
- **TS 对应文件：** `UserResourceUpdateMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 变化可视化（彩色的 +/-）、资源类型图标、结构化的元数据显示
- **影响：** 低

---

#### 5.21 `user_channel_message.rs`（17 行）

```rust
pub fn render_user_channel_message(channel: &str, message: &str, _theme: &Theme) -> String {
    format!("Channel {channel}: {message}")
}
```
- **TS 对应文件：** `UserChannelMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** KAIROS 特性门控集成、频道名称颜色编码、消息类型指示器、发送者头像/名称、时间戳
- **影响：** 低——KAIROS 是特性门控的

---

#### 5.22 `user_agent_notification_message.rs`（11 行）

```rust
pub fn render_user_agent_notification_message(notification: &str, _theme: &Theme) -> String {
    format!("Notification: {notification}")
}
```
- **TS 对应文件：** `UserAgentNotificationMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 通知级别样式（info/warning/error）、agent 来源显示、关闭操作提示
- **影响：** 低

---

#### 5.23 `task_assignment_message.rs`（12 行）

```rust
pub fn render_task_assignment_message(task: &str, owner: &str, _theme: &Theme) -> String {
    format!("Task '{task}' assigned to {owner}")
}
```
- **TS 对应文件：** `TaskAssignmentMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 负责人颜色编码、任务优先级指示器、状态徽章、完成进度
- **影响：** 低

---

#### 5.24 `plan_approval_message.rs`（8 行）

```rust
pub fn render_plan_approval_message(plan_name: &str, approved: bool, _theme: &Theme) -> String {
    let state = if approved { "approved" } else { "rejected" };
    format!("Plan '{plan_name}' {state}")
}
```
- **TS 对应文件：** `PlanApprovalMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 审批状态样式（绿色对勾 / 红色叉号）、审批时间戳、审批人名称、计划摘要预览
- **影响：** 低

---

#### 5.25 `team_mem_saved.rs`（11 行）

```rust
pub fn render_team_mem_saved(path: &str, _theme: &Theme) -> String {
    format!("Team memory saved to {path}")
}
```
- **TS 对应文件：** `teamMemSaved.ts`
- **完整性：** 1/5
- **缺失内容：** TEAMMEM 特性门控、`FilePathLink` 悬停下划线、内容预览、diff 摘要
- **影响：** 低——特性门控

---

#### 5.26 `team_mem_collapsed.rs`（7 行）

```rust
pub fn render_team_mem_collapsed(team: &str, entries: usize, _theme: &Theme) -> String {
    format!("Team memory for {team} collapsed ({entries} entries)")
}
```
- **TS 对应文件：** `teamMemCollapsed.tsx`
- **完整性：** 1/5
- **缺失内容：** 已折叠条目预览、展开提示、暗色样式
- **影响：** 低——特性门控

---

#### 5.27 `hook_progress_message.rs`（11 行）

```rust
pub fn render_hook_progress_message(hook: &str, stage: &str, _theme: &Theme) -> String {
    format!("Hook [{hook}] {status}")
}
```
- **TS 对应文件：** `HookProgressMessage.tsx`
- **完整性：** 1/5
- **缺失内容：** 进度动画（spinner）、时间显示、hook 类型图标（git hook vs 自定义）、成功/失败样式
- **影响：** 中——hook 是面向用户的主要功能

---

#### 5.28 `collapsed_read_search_content.rs`（15 行）

```rust
pub fn render_collapsed_read_search_content(source: &str, line_count: usize, _theme: &Theme) -> String {
    format!("Read {source}: {line_count} lines (collapsed)")
}
```
- **TS 对应文件：** `CollapsedReadSearchContent.tsx`
- **完整性：** 2/5
- **缺失内容：** 展开按钮 / `CtrlOToExpand`、内容预览（前/后 N 行）、源路径暗色样式、截断可视化
- **影响：** 中

---

#### 5.29 `null_rendering_attachments.rs`（11 行）

```rust
pub fn render_null_rendering_attachments(reason: &str, _theme: &Theme) -> String {
    format!("Skipped attachments: {reason}")
}
```
- **TS 对应文件：** `nullRenderingAttachments.ts`
- **完整性：** 2/5
- **缺失内容：** 暗色样式、附件计数、跳过项目的类别细分
- **影响：** 低

---

#### 5.30 `advisor_message.rs`（20 行）

```rust
pub fn render_advisor_message(model: Option<&str>, prompt: &str, advisory: &str, _theme: &Theme) -> String {
    format!("Advisor[{model}] Prompt: {prompt}\nAdvisory: {advisory}")
}
```
- **TS 对应文件：** `AdvisorMessage.tsx`
- **完整性：** 2/5
- **缺失内容：** 模型名称样式、顾问级别指示器（info/warning）、可折叠提示、结构化输出
- **影响：** 低

---

#### 5.31 `grouped_tool_use_content.rs`（15 行）

```rust
pub fn render_grouped_tool_use_content(tool_names: &[&str], _theme: &Theme) -> String {
    // 通过 ToolActivity::from_tool_use 为每个工具渲染
}
```
- **TS 对应文件：** `GroupedToolUseContent.tsx`
- **完整性：** 2/5
- **缺失内容：** 分组视觉连接器（缩进线）、每个工具的进度指示器、工具计数徽章、折叠 vs 展开状态
- **影响：** 低

---

#### 5.32 `highlighted_thinking_text.rs`（12 行）

```rust
pub fn render_highlighted_thinking_text(thinking: &str, _theme: &Theme) -> String {
    format!("[THINKING] {trimmed}")
}
```
- **TS 对应文件：** `HighlightedThinkingText.tsx`
- **完整性：** 1/5
- **缺失内容：** 高亮/暗色样式、可折叠章节、thinking 模式徽章、时间信息、展开提示
- **影响：** 低——主调度器使用预览处理 thinking

---

### 6. 代码重复：`render.rs` vs `metadata.rs`

两个文件包含相同的函数：

| 函数 | 位于 `render.rs` | 位于 `metadata.rs` |
|----------|---------------|-----------------|
| `message_copy_text` | 第 250 行 | 第 5 行 |
| `message_primary_reference` | 第 270 行 | 第 25 行 |
| `content_block_copy_text` | 第 912 行 | 第 45 行 |
| `content_block_reference` | 第 927 行 | （存在） |
| `image_reference` | 第 940 行 | 第 60 行 |
| `tool_input_summary` | 第 861 行 | （存在） |
| `tool_primary_input` | 第 869 行 | （存在） |
| `message_content_copy_text` | 第 894 行 | 第 34 行 |
| `message_content_reference` | 第 905 行 | （存在） |
| `attachment_copy_text` | 第 948 行 | （存在） |
| `attachment_reference` | 第 963 行 | （存在） |
| `strip_system_reminders` | 第 976 行 | （存在） |
| `tool_result_content_text` | 第 517 行 | （存在） |

这种重复是维护上的隐患。修改必须在两处同时进行。

---

## 汇总表

| # | 组件 | Rust 评级 | TS 行数 | Rust 行数 | 关键程度 | 差距描述 | 计划章节 |
|---|-----------|:-----------:|:--------:|:----------:|:-----------:|-----------------|:--------:|
| 1 | `render.rs`（调度器） | 4/5 | 不适用（分散） | 1146 | 高 | 无错误边界、无 spinner、无工具加载状态机 | — |
| 2 | `wrap.rs`（行换行） | 4/5 | 内置 | 49 | 低 | 无单词边界换行 | — |
| 3 | `user_bash_output_message.rs` | 5/5 | 约 200 | 229 | 无 | 功能完备，支持 ANSI 剥离、JSON 格式化、截断、页脚 | — |
| 4 | `user_tool_result_message/` | 3/5 | 约 400 | 约 300 | 中 | 死代码门控，未接入主渲染路径，无工具查找 | — |
| 5 | `user_text_message.rs` | 1/5 | 275 | 12 | **关键** | TS 调度器路由 15+ 种消息类型；Rust 只是一个简单的 format!() | §4 |
| 6 | `user_prompt_message.rs` | 1/5 | 80 | 12 | 高 | 缺失 10K 截断、简要模式、选择背景 | — |
| 7 | `assistant_text_message.rs` | 1/5 | 270 | 11 | **关键** | 缺失 10+ 种带用户指导的 API 错误状态 | §1 |
| 8 | `assistant_tool_use_message.rs` | 1/5 | 368 | 9 | **关键** | 缺失完整的状态机（进度/队列/加载器/spinner） | §2 |
| 9 | `system_text_message.rs` | 1/5 | 827 | 19 | **关键** | 缺失 15+ 种消息子类型 | §3 |
| 10 | `system_api_error_message.rs` | 1/5 | 约 40 | 12 | 中 | 缺失状态码着色、重试提示 | — |
| 11 | `rate_limit_message.rs` | 1/5 | 约 30 | 12 | 中 | 缺失警告样式、速率限制上下文 | — |
| 12 | `compact_boundary_message.rs` | 1/5 | 约 30 | 11 | 低 | 被 render.rs 的压缩处理重复 | — |
| 13 | `attachment_message.rs` | 1/5 | 536 | 12 | 高 | 缺失 25+ 种类型特定的渲染器 | §5 |
| 14 | `shutdown_message.rs` | 2/5 | 约 20 | 11 | 低 | 缺失警告样式 | — |
| 15 | `user_plan_message.rs` | 1/5 | 约 30 | 11 | 中 | 缺失计划格式化 |
| 16 | `user_bash_input_message.rs` | 2/5 | 约 30 | 11 | 低 | 基本的 Shell 提示符显示 |
| 17 | `user_command_message.rs` | 1/5 | 约 25 | 12 | 低 | 缺失 cwd 样式、CtrlOToExpand |
| 18 | `user_image_message.rs` | 2/5 | 约 25 | 16 | 低 | 终端受限的渲染 |
| 19 | `user_local_command_output_message.rs` | 1/5 | 约 40 | 18 | 中 | 缺失退出代码颜色、输出截断 |
| 20 | `user_memory_input_message.rs` | 1/5 | 约 20 | 16 | 低 | 缺失键值样式 |
| 21 | `user_teammate_message.rs` | 1/5 | 约 30 | 17 | 低 | 特性门控（agent swarms） |
| 22 | `user_resource_update_message.rs` | 1/5 | 约 20 | 17 | 低 | 低优先级 |
| 23 | `user_channel_message.rs` | 1/5 | 约 25 | 17 | 低 | 特性门控（KAIROS） |
| 24 | `user_agent_notification_message.rs` | 1/5 | 约 20 | 11 | 低 | 特性门控（KAIROS） |
| 25 | `task_assignment_message.rs` | 1/5 | 约 20 | 12 | 低 | 低优先级 |
| 26 | `plan_approval_message.rs` | 1/5 | 约 20 | 8 | 低 | 缺失审批样式 |
| 27 | `team_mem_saved.rs` | 1/5 | 约 15 | 11 | 低 | 特性门控（TEAMMEM） |
| 28 | `team_mem_collapsed.rs` | 1/5 | 约 15 | 7 | 低 | 特性门控（TEAMMEM） |
| 29 | `hook_progress_message.rs` | 1/5 | 约 25 | 11 | 中 | 缺失 spinner 动画、时间显示 |
| 30 | `collapsed_read_search_content.rs` | 2/5 | 约 20 | 15 | 中 | 缺失展开交互 |
| 31 | `null_rendering_attachments.rs` | 2/5 | 约 10 | 11 | 低 | 简单的跳过附件消息 |
| 32 | `advisor_message.rs` | 2/5 | 约 20 | 20 | 低 | 低优先级 |
| 33 | `grouped_tool_use_content.rs` | 2/5 | 约 20 | 15 | 低 | 缺失分组可视化 |
| 34 | `highlighted_thinking_text.rs` | 1/5 | 约 15 | 12 | 低 | 被主调度器重复 |
| 35 | `assistant_thinking_message.rs` | 1/5 | 约 30 | 17 | 低 | 被主调度器重复 |
| 36 | `assistant_redacted_thinking_message.rs` | 2/5 | 约 10 | 6 | 低 | 被主调度器重复 |

### 总计

| 类别 | 数量 | 文件 |
|----------|:-----:|-------|
| 功能完备 (5/5) | 1 | `user_bash_output_message.rs` |
| 接近完备 (4/5) | 2 | `render.rs`, `wrap.rs` |
| 部分实现 (3/5) | 1 | `user_tool_result_message/` |
| 基础 (2/5) | 7 | 各种具有微小附加值的骨架文件 |
| 骨架 (1/5) | 25 | 大多数独立的 `*_message.rs` 辅助文件 |
| **死代码** | 8 | 所有 `user_tool_result_message/*` 子模块都有 `#[allow(dead_code)]` |

---

## 关键发现

### 1. 双重渲染路径不匹配
主 `render.rs` 调度器已经在 `render_single_message_with_context()` 中处理了大多数消息类型，具有适当的样式和 "Claude:" / "You:" 前缀模式。然而，独立的 `*_message.rs` 辅助文件代表了一条**单独的、更简单的渲染路径**（返回 `String`，而非 `Vec<Line>`），**不被**主调度器调用。这表明要么：
- 是一个计划中的无头/IPC 文本输出路径
- 是早期架构遗留的骨架代码
- 是编写了但从未集成的代码

主调度器全面处理了用户消息（带工具结果）、助手消息（带所有 ContentBlock 变体）、系统消息（带子类型）、进度消息和附件消息，但没有 TS 组件的丰富交互性。

### 2. 关键差距
- **`assistant_text_message.rs`**：TS 版本处理 10+ 种带用户指导的 API 错误状态（无效密钥、速率限制、信用余额等）。Rust 版本完全没有这些。用户将看到无帮助的错误消息。→ 执行计划 §1
- **`assistant_tool_use_message.rs`**：TS 版本具有完整的状态机。Rust 版本只有一行代码。用户将看不到工具加载/进度状态。→ 执行计划 §2
- **`system_text_message.rs`**：TS 版本有 15+ 种子类型。Rust 版本只有通用格式。
- **`user_text_message.rs`**：TS 版本是通过标签分析路由 15+ 种消息类型的中央调度器。Rust 版本是一个简单的字符串格式化器。
- **`attachment_message.rs`**：TS 版本有 25+ 种附件类型渲染器。Rust 版本只有 7 个简单变体。

### 3. 代码重复
`render.rs` 和 `metadata.rs` 包含了约 13 个工具函数的完全相同副本（`message_copy_text`、`image_reference`、`tool_input_summary` 等）。这是维护上的隐患。

### 4. 死代码
整个 `user_tool_result_message/` 子模块（9 个文件）的每个子模块上都有 `#[allow(dead_code)]`，意味着它已被添加但尚未集成到任何渲染路径中。主 `render.rs` 在 `render_tool_result_user_message()` 和 `render_assistant_message()` 的 `ContentBlock::ToolResult` 分支中内联处理工具结果。

### 5. 无错误边界
TS 将消息组件包装在 `<SentryErrorBoundary>` 中，以防止一条损坏的消息导致整个渲染崩溃。Rust 没有等效机制——任何消息渲染器中的 panic 都会导致 UI 崩溃。

### 6. 无动画
TS 使用 `<Spinner>` 用于未解决的工具使用。Rust 仅在最后一条流式消息上显示静态光标字符（"▌"）。没有工具加载 spinner 或进度动画。

### 7. 无选择交互性
TS 使用 `useContext(MessageActionsSelectedContext)` 进行选择高亮。Rust 使用手动的 `MessageRenderContext` 结构体。Rust 的方法能工作，但缺少 React 状态管理提供的自动重新渲染。

---

## 建议优先级

参考执行计划: `docs/ui/great/plans/plan-01-message-rendering.md`

| 优先级 | 组件 | 工作量 | 影响 | 执行计划章节 |
|----------|-----------|--------|--------|:----------:|
| P0 | `assistant_text_message.rs` -> API 错误状态 | 中 | **关键**——用户会看到错误 | §1 |
| P0 | `assistant_tool_use_message.rs` -> 工具状态机 | 中 | **关键**——用户看不到进度 | §2 |
| P0 | `system_text_message.rs` -> 15+ 种子类型 | 高 | **关键**——系统消息缺失 | §3 |
| P1 | `attachment_message.rs` -> 25+ 种类型渲染器 | 高 | 高——丰富的附件支持 | §5 |
| P1 | `user_text_message.rs` -> 基于标签的路由 | 高 | 高——调度器桩 | §4 |
| P2 | 将 `user_tool_result_message/` 接入主渲染路径 | 低 | 中——已编写，只需要集成 | — |
| P2 | `hook_progress_message.rs` -> spinner + 时间显示 | 低 | 中——hook 面向用户 | §2.7 |
| P3 | 去重 `render.rs`/`metadata.rs` 公共函数 | 低 | 低——维护卫生 | — |
| P3 | 向渲染调度器添加错误边界 | 低 | 低——韧性 | — |
| P3 | 所有其他 1 星骨架文件 | 低 | 低——特性门控或重复 | — |
