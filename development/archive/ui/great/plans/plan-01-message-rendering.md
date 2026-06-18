# 执行计划：消息渲染系统补齐 — 5 个 P0/P1 渲染器

> **生成日期**: 2026-05-18
> **来源**: `docs/ui/great/06_missing_features_summary.md` §1
> **参考实现**:
> - TS: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/components/messages/`
> - Rust: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/messages/`
> - TS 错误常量: `claude-code-bun/src/services/api/errors.ts`

---

## 当前状态（2026-05-18 修正）

本计划已进入部分实施状态，不能再按下方“当前 Rust 实现（11 行/9 行/12 行）”
描述理解现状。当前已补齐一批 helper 级消息渲染逻辑，并完成以下审计修正：

- API 错误展示遵循 `06_missing_features_summary.md` 约定：只有 API/第三方 API
  调用错误路径使用 `Error occurred: <具体错误信息>`，保留第三方 API/OAuth 返回的
  原始错误详情；普通 assistant 文本不走该前缀，也不把第三方错误强行改写成
  Anthropic billing、登录或封禁文案。
- `render.rs` 主运行时渲染路径已接入 helper：
  assistant API error 走 `assistant_text_message` 的错误格式化；
  assistant tool-use 走 `assistant_tool_use_message`；
  system API error 走 `system_text_message`；
  text user message 走 `user_text_message` 路由；
  当前 `Attachment` enum 可映射的 runtime 附件走 `attachment_message`。
- JSON 解析得到的 attachment/system 字段使用 owned `String` 保存，避免从临时
  `serde_json::Value` 返回悬垂 `&str`。
- `render.rs` 中 API error span 使用 owned 文本，避免返回借用局部 `String` 的
  `Line<'a>`。
- 普通 user prompt helper fallback 保持 `You: <content>`，避免变成
  `[prompt]: <content>`。
- resolved transparent wrapper tool helper 保留显式完成标记，不返回空字符串吞掉状态。
- 聚焦验证：`cargo test -p claude-code-rs ui::messages -- --nocapture` 已通过
  `100 passed`，覆盖 assistant/system API error 主渲染路径。

后续继续执行本计划时，应基于当前代码重新审计缺口，而不是从 stub 状态重新开始。

## 总览

| # | 文件 | Rust 行数 | TS 参考 | TS 行数 | 当前评分 | 目标评分 | 估算工作量 |
|---|------|:---------:|:---------:|:--------:|:--------:|:--------:|:----------:|
| 1 | `assistant_text_message.rs` | 11 | `AssistantTextMessage.tsx` + `errors.ts` | 197 + 1211 | 1/5 | 4/5 | ~200 行 |
| 2 | `assistant_tool_use_message.rs` | 9 | `AssistantToolUseMessage.tsx` | 278 | 1/5 | 4/5 | ~250 行 |
| 3 | `system_text_message.rs` | 19 | `SystemTextMessage.tsx` | 427 | 1/5 | 4/5 | ~350 行 |
| 4 | `user_text_message.rs` | 12 | `UserTextMessage.tsx` | 173 | 1/5 | 4/5 | ~150 行 |
| 5 | `attachment_message.rs` | 12 | `AttachmentMessage.tsx` | 508 | 1/5 | 4/5 | ~400 行 |

**总工作量估算**: ~1350 行新 Rust 代码（不含测试）

**关键设计原则**:
- 这些渲染器目前返回 `String`（纯文本），独立于主 `render.rs` 调度器路径（返回 `Vec<Line<'a>>`）。补齐时保持它们为 `String` 返回的辅助路径，但大幅扩展其内容。
- 主调度器 `render.rs` 中对应的类型分发逻辑也需要同步更新，以确保辅助路径和主路径都不再是 stub。
- TS 的路由/调度逻辑在 `UserTextMessage.tsx` 和 `render.rs` 中各自独立存在 — Rust 端保留当前 `render.rs` 的分发方式，仅扩展辅助文件的内容。
- API 错误文案按照 `06_missing_features_summary.md` §"第三方 API / OAuth 错误展示约定"的约定，使用 `Error occurred: <具体错误信息>` 前缀格式。

---

## 1. `assistant_text_message.rs` — API 错误状态（P0）

### TS 参考

**文件 + 行号**:
| 组件 | 文件 | 行号 |
|------|------|:----:|
| 主组件 | `AssistantTextMessage.tsx` | 55-196 |
| 速率限制检测 | `AssistantTextMessage.tsx` | 69-71 |
| switch 分支 | `AssistantTextMessage.tsx` | 73-173 |
| 通用 API 错误回退 | `AssistantTextMessage.tsx` | 157-173 |
| 错误常量定义 | `services/api/errors.ts` | 54-198 |
| `getAssistantMessageFromError` | `services/api/errors.ts` | 425-934 |
| 速率限制消息组件 | `RateLimitMessage.tsx` | 全部 112 行 |

### 当前 Rust 实现（11 行）

```rust
pub fn render_assistant_text_message(text: &str, _theme: &Theme) -> String {
    if text.trim().is_empty() {
        "Assistant: <empty text>".to_string()
    } else {
        format!("Assistant: {text}")
    }
}
```

### 分步实施

#### Phase 1.1: 错误类型枚举（~40 行）

在 `assistant_text_message.rs` 中（或新建 `assistant_api_error.rs`）添加错误类型枚举：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantApiError<'a> {
    /// 速率限制（精确匹配 or `isRateLimitErrorMessage`）
    RateLimit(&'a str),
    /// Prompt 超过上下文窗口上限
    PromptTooLong,
    /// 信用余额不足
    CreditBalanceTooLow,
    /// 无效 API Key（OAuth 登录态）
    InvalidApiKey,
    /// 无效 API Key（外部来源，如 ANTHROPIC_API_KEY env）
    InvalidApiKeyExternal,
    /// 组织被禁用（env key）
    OrgDisabledEnvKey,
    /// 组织被禁用（env key + OAuth 共存）
    OrgDisabledEnvKeyWithOAuth,
    /// OAuth Token 被撤销
    TokenRevoked,
    /// API 超时
    ApiTimeout(Option<u64>),  // Option<API_TIMEOUT_MS>
    /// Opus 容量不足
    CustomOffSwitch,
    /// 用户中止
    UserAbort,
    /// 通用 API 错误（`startsWithApiErrorPrefix`）
    ApiErrorPrefix(&'a str),
}
```

TS 参考: `AssistantTextMessage.tsx:73-173` + `errors.ts:54-69`

#### Phase 1.2: 错误分类函数（~30 行）

实现 `classify_assistant_text()` 函数，通过精确字符串匹配将文本分类到上述枚举：

```rust
pub fn classify_assistant_text(text: &str) -> Option<AssistantApiError> {
    // 优先级顺序与 TS switch 一致
    // 1. 空文本 → None（走默认）
    // 2. 速率限制 → RateLimit
    // 3. 精确匹配各常量
    // 4. startsWithApiErrorPrefix → ApiErrorPrefix
    // 5. 默认 → None（走普通文本）
}
```

TS 参考: `AssistantTextMessage.tsx:69-71` + `errors.ts:156-158`（`startsWithApiErrorPrefix`）

#### Phase 1.3: 每种错误类型的用户可见文案（~80 行）

每个枚举变体输出格式化的错误文案:

| 变体 | 输出格式 | TS 参考行 |
|------|---------|:---------:|
| `RateLimit(msg)` | `"Error occurred: {msg}"` | 69-71 |
| `PromptTooLong` | `"Error occurred: Context limit reached · /compact or /clear to continue {upgrade_hint}"` | 79-89 |
| `CreditBalanceTooLow` | `"Error occurred: Credit balance too low · Add funds: https://platform.claude.com/settings/billing"` | 91-98 |
| `InvalidApiKey` | `"Error occurred: Not logged in · Please run /login"` | 100-108 |
| `InvalidApiKeyExternal` | `"Error occurred: Invalid API key · Fix external API key"` | 103-108 |
| `OrgDisabledEnvKey` | `"Error occurred: {text}"` | 110-116 |
| `OrgDisabledEnvKeyWithOAuth` | `"Error occurred: {text}"` | 110-116 |
| `TokenRevoked` | `"Error occurred: OAuth token revoked · Please run /login"` | 118-123 |
| `ApiTimeout(ms)` | `"Error occurred: Request timed out"` + ms hint | 125-133 |
| `CustomOffSwitch` | `"Error occurred: We are experiencing high demand for Opus 4. ..."` | 135-146 |
| `UserAbort` | `"[Request interrupted by user]"` | 149-154 |
| `ApiErrorPrefix(s)` | `"Error occurred: {s}"`（截断至 1000 字符） | 157-173 |

TS 错误常量定义参考: `errors.ts:54-69`、`errors.ts:155-198`

**关键文案**:
- `INVALID_API_KEY_ERROR_MESSAGE` = `'Not logged in · Please run /login'` (line 155)
- `CREDIT_BALANCE_TOO_LOW_ERROR_MESSAGE` = `'Credit balance is too low'` (line 154)
- `API_TIMEOUT_ERROR_MESSAGE` = `'Request timed out'` (line 169)
- `TOKEN_REVOKED_ERROR_MESSAGE` = `'OAuth token revoked · Please run /login'` (lines 162-163)
- `API_ERROR_MESSAGE_PREFIX` = `'API Error'` (line 54)

#### Phase 1.4: 集成到 `render_assistant_text_message()`（~20 行）

```rust
pub fn render_assistant_text_message(text: &str, theme: &Theme) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "Assistant: <empty text>".to_string();
    }
    // 分类并渲染
    if let Some(error) = classify_assistant_text(trimmed) {
        return render_api_error(&error);
    }
    // 默认：普通文本渲染
    format!("Assistant: {trimmed}")
}
```

#### Phase 1.5: 测试（~30 行）

覆盖所有 10+ 枚举变体的输入，以及空文本、普通文本、超长错误截断等边界情况。

### 依赖

- 需要从缓存常量文件中导出错误消息字符串常量（可新建 `src/ui/messages/api_error_constants.rs`）
- `RateLimitMessage` 本身不需要在 Rust 中复制 — 只需显示错误字符串
- 不需要 keychain 检测逻辑（环境检测）

---

## 2. `assistant_tool_use_message.rs` — 工具状态机（P0）

### TS 参考

**文件 + 行号**:
| 组件 | 文件 | 行号 |
|------|------|:----:|
| 主组件 | `AssistantToolUseMessage.tsx` | 36-201 |
| Tool 查找 + 记忆化解析 | `AssistantToolUseMessage.tsx` | 72-91 |
| 状态计算 | `AssistantToolUseMessage.tsx` | 95-97 |
| TransparentWrapper | `AssistantToolUseMessage.tsx` | 99-114 |
| 空白名称处理 | `AssistantToolUseMessage.tsx` | 116-118 |
| Dot vs Loader | `AssistantToolUseMessage.tsx` | 138-151 |
| 工具名渲染 | `AssistantToolUseMessage.tsx` | 152-161 |
| 输入摘要渲染 | `AssistantToolUseMessage.tsx` | 162-166 |
| Classifier 检查状态 | `AssistantToolUseMessage.tsx` | 173-178 |
| 等待权限状态 | `AssistantToolUseMessage.tsx` | 179-182 |
| 进度消息 | `AssistantToolUseMessage.tsx` | 183-196 |
| 队列消息 | `AssistantToolUseMessage.tsx` | 197-198 |
| `renderToolUseMessage()` | `AssistantToolUseMessage.tsx` | 204-219 |
| `renderToolUseProgressMessage()` | `AssistantToolUseMessage.tsx` | 221-268 |
| `renderToolUseQueuedMessage()` | `AssistantToolUseMessage.tsx` | 270-277 |
| `ToolUseLoader` | `ToolUseLoader.tsx` | (推断约 50 行) |
| `HookProgressMessage` | `HookProgressMessage.tsx` | (推断约 25 行) |

### 当前 Rust 实现（9 行）

```rust
pub fn render_assistant_tool_use_message(tool_name: &str, input: &str, _theme: &Theme) -> String {
    let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Running);
    format!("Assistant used {}", activity.display_call())
}
```

### 分步实施

#### Phase 2.1: 工具状态枚举 + 输入数据结构（~30 行）

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolUseState {
    /// 正在进行
    InProgress,
    /// 已解析（完成）
    Resolved,
    /// 在队列中
    Queued,
    /// 等待用户权限
    WaitingForPermission,
    /// 分类器检查中
    ClassifierChecking,
    /// 发生错误
    Error,
}
```

更新函数签名以接收状态信息：

```rust
pub fn render_assistant_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
    has_hook_progress: bool,
    _theme: &Theme,
) -> String {
```

TS 参考: `AssistantToolUseMessage.tsx:95-97`（状态计算）、`AssistantToolUseMessage.tsx:65`（classifier）

#### Phase 2.2: 队列状态渲染（~30 行）

```rust
fn render_tool_use_queued_message(tool_name: &str) -> String {
    // "● {tool_name}" (dim)
    format!("● {tool_name}")
}
```

TS 参考: `AssistantToolUseMessage.tsx:138-140`（queued dot）

#### Phase 2.3: 进度消息渲染（~60 行）

```rust
fn render_tool_use_progress_message(
    tool_name: &str,
    input_summary: &str,
    has_hook_progress: bool,
) -> String {
    let mut parts = Vec::new();
    // "● {tool_name}({input_summary})" (当前运行)
    if has_hook_progress {
        parts.push("[hook running]");
    }
    // 组装
}
```

TS 参考: `AssistantToolUseMessage.tsx:221-268`（progress message）

#### Phase 2.4: 存在错误状态渲染（~30 行）

```rust
fn render_tool_use_error_state(tool_name: &str, input_summary: &str) -> String {
    // "● {tool_name}({input_summary}) [error]"
}
```

TS 参考: `AssistantToolUseMessage.tsx:149`（isError check in ToolUseLoader）

#### Phase 2.5: Classifier + 权限等待状态（~30 行）

```rust
fn render_classifier_checking(tool_name: &str) -> String {
    // "● {tool_name} (auto classifier checking...)"
}
fn render_waiting_for_permission(tool_name: &str) -> String {
    // "● {tool_name} (waiting for permission...)"
}
```

TS 参考: `AssistantToolUseMessage.tsx:173-182`

#### Phase 2.6: 透明包装工具处理（~20 行）

```rust
fn is_transparent_wrapper_tool(tool_name: &str) -> bool {
    matches!(tool_name, "Bash" | "Write" | "Edit" | ...)
}
fn render_transparent_wrapper(tool_name: &str) -> Option<String> {
    // 仅 in-progress 时显示
    // 其他状态返回 None
}
```

TS 参考: `AssistantToolUseMessage.tsx:99-114`

#### Phase 2.7: 集成到主函数 + HookProgress 集成（~40 行）

```rust
pub fn render_assistant_tool_use_message(
    tool_name: &str,
    input: &str,
    state: ToolUseState,
    has_hook_progress: bool,
    _theme: &Theme,
) -> String {
    match state {
        ToolUseState::Queued => render_tool_use_queued_message(tool_name),
        ToolUseState::InProgress => {
            let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Running);
            let summary = activity.display_call();
            let mut lines = Vec::new();
            // 进度 + hook 行
            if has_hook_progress {
                lines.push(format!("● {tool_name}({summary}) [hook running]"));
            } else {
                lines.push(format!("● {tool_name}({summary})"));
            }
            lines.join("\n")
        }
        ToolUseState::Error => {
            let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Error);
            format!("● {}({}) [error]", ...)
        }
        ToolUseState::WaitingForPermission => {
            format!("● {tool_name} (waiting for permission...)")
        }
        ToolUseState::ClassifierChecking => {
            format!("● {tool_name} (classifier checking...)")
        }
        ToolUseState::Resolved => {
            let activity = ToolActivity::from_tool_use(tool_name, input, ToolState::Done);
            format!("● {}({})", ...)
        }
    }
}
```

#### Phase 2.8: 更新调用点（~20 行）

在主调度器 `render.rs` 中更新对 `render_assistant_tool_use_message()` 的调用，传入正确的状态信息。

### 依赖

- 需要 `ToolUseState` 枚举与主调度器共享
- `ToolActivity` 已有，但需要扩展以支持完整状态
- TS 中 `ToolUseLoader` 组件的 spinner 动画在 ratatui 中无法直接移植 — 使用静态字符回退

---

## 3. `system_text_message.rs` — 15+ 种子类型（P0）

### TS 参考

**文件 + 行号**:
| 子类型 | `SystemTextMessage.tsx` 行号 | 渲染函数 | 行号 |
|--------|:--------------------------:|----------|:----:|
| `turn_duration` | 47-49 | `TurnDurationMessage` | 288-339 |
| `memory_saved` | 51-53 | `MemorySavedMessage` | 341-384 |
| `away_summary` | 55-64 | inline | — |
| `agents_killed` | 67-76 | inline | — |
| `thinking` | 79-84 | `ThinkingMessage` | 386-402 |
| `bridge_status` | 86-88 | `BridgeStatusMessage` | 404-426 |
| `scheduled_task_fire` | 90-98 | inline | — |
| `permission_retry` | 100-108 | inline | — |
| `api_error` | 117-119 | `SystemAPIErrorMessage` | — |
| `stop_hook_summary` | 121-130 | `StopHookSummaryMessage` | 151-252 |
| generic (info/warning/error) | 138-148 | `SystemTextMessageInner` | 254-286 |

SV 引用: `SystemAPIErrorMessage.tsx` (55 行)

### 当前 Rust 实现（19 行）

```rust
pub fn render_system_text_message(tag: &str, message: &str, _theme: &Theme) -> String {
    let tag = tag.trim();
    let message = message.trim();
    if message.is_empty() {
        if tag.is_empty() {
            "System message".to_string()
        } else {
            format!("System({tag})")
        }
    } else if tag.is_empty() {
        message.to_string()
    } else {
        format!("System({tag}): {message}")
    }
}
```

### 分步实施

#### Phase 3.1: 系统消息子类型枚举（~30 行）

```rust
#[derive(Debug, Clone)]
pub enum SystemSubtype<'a> {
    TurnDuration {
        duration_ms: u64,
        budget_limit: Option<u64>,
        budget_tokens: Option<u64>,
        budget_nudges: Option<u32>,
    },
    MemorySaved {
        written_paths: Vec<&'a str>,
        verb: Option<&'a str>,
    },
    AwaySummary(&'a str),
    AgentsKilled,
    Thinking(&'a str),
    BridgeStatus {
        url: &'a str,
        upgrade_nudge: Option<&'a str>,
    },
    ScheduledTaskFire(&'a str),
    PermissionRetry(Vec<&'a str>),
    ApiError {
        retry_attempt: u32,
        error: &'a str,
        retry_in_ms: u64,
        max_retries: u32,
    },
    StopHookSummary {
        hook_count: usize,
        hook_infos: Vec<HookInfo>,
        hook_errors: Vec<String>,
        prevented_continuation: bool,
        stop_reason: Option<String>,
        total_duration_ms: u64,
    },
    Generic {
        content: &'a str,
        level: &'a str,  // "info", "warning", "error"
    },
}
```

TS 参考: `SystemTextMessage.tsx:44-149`（subtype dispatch），`SystemTextMessage.tsx:151-252`（StopHookSummary）

#### Phase 3.2: 子类型分流函数（~40 行）

```rust
pub fn classify_system_message(tag: &str, message: &str) -> SystemSubtype {
    match tag {
        "turn_duration" => parse_turn_duration(message),
        "memory_saved" => parse_memory_saved(message),
        "away_summary" => SystemSubtype::AwaySummary(message),
        "agents_killed" => SystemSubtype::AgentsKilled,
        "thinking" => SystemSubtype::Thinking(message),
        "bridge_status" => parse_bridge_status(message),
        "scheduled_task_fire" => SystemSubtype::ScheduledTaskFire(message),
        "permission_retry" => parse_permission_retry(message),
        "api_error" => parse_api_error(message),
        "stop_hook_summary" => parse_stop_hook_summary(message),
        _ => SystemSubtype::Generic {
            content: message,
            level: tag,
        },
    }
}
```

TS 参考: `SystemTextMessage.tsx:44-149`

#### Phase 3.3: `TurnDurationMessage` 渲染（~40 行）

```rust
fn render_turn_duration(msg: &SystemSubtype) -> String {
    // TS: SystemTextMessage.tsx 288-339
    // "∗ Worked for {duration}" (dim)
    // 如果有 budget: " · {used} / {limit} ({pct}%)"
    // 如果有 nudges: " · N nudges"
    // 如果有后台任务: " · {summary} still running"
}
```

TS 参考: `SystemTextMessage.tsx:288-339`（TurnDurationMessage）

#### Phase 3.4: `MemorySavedMessage` 渲染（~30 行）

```rust
fn render_memory_saved(msg: &SystemSubtype) -> String {
    // TS: 341-384
    // "● Saved N memories"
    // 对每条 path: "  {basename}"
}
```

TS 参考: `SystemTextMessage.tsx:341-384`（MemorySavedMessage）

#### Phase 3.5: `StopHookSummaryMessage` 渲染（~60 行）

```rust
fn render_stop_hook_summary(msg: &SystemSubtype) -> String {
    // TS: 151-252
    // "● Ran N hooks ({duration})"
    //   "⎿ {hook_command}" (per hook)
    //   "⎿ {stop_reason}" (if prevented)
    //   "⎿ hook error: {err}" (per error)
}
```

TS 参考: `SystemTextMessage.tsx:151-252`

#### Phase 3.6: 其他子类型渲染（~80 行）

每个独立子类型的简短渲染：

- `AwaySummary`: `"※ {content}"` (dim) — TS: 56-63
- `AgentsKilled`: `"● All background agents stopped"` — TS: 68-76
- `Thinking`: `"∗ {content}"` (dim) — TS: 79-84
- `BridgeStatus`: `/remote-control is active. Code in CLI or at {url}` — TS: 404-426
- `ScheduledTaskFire`: `"⁭ {content}"` (dim) — TS: 90-98
- `PermissionRetry`: `"⁭ Allowed: {cmd1}, {cmd2}, ..."` — TS: 100-108
- `ApiError`: `"{error}" (truncated to 1000 chars) / "Retrying in N seconds... (attempt X/Y)"` — SystemAPIErrorMessage.tsx: 42-53
- `Generic(info)`: `"{content}"` (dim) — TS: 138-148
- `Generic(warning)`: `"● {content}"` (warning color) — TS: 138-148

#### Phase 3.7: 集成到主函数（~30 行）

```rust
pub fn render_system_text_message(tag: &str, message: &str, theme: &Theme) -> String {
    let subtype = classify_system_message(tag, message);
    match subtype {
        SystemSubtype::TurnDuration { .. } => render_turn_duration(&subtype),
        SystemSubtype::MemorySaved { .. } => render_memory_saved(&subtype),
        SystemSubtype::StopHookSummary { .. } => render_stop_hook_summary(&subtype),
        // ... etc
        SystemSubtype::Generic { .. } => {
            // Fallback to current behavior
            // ... (improved with level-based styling)
        }
    }
}
```

#### Phase 3.8: SystemAPIErrorMessage 集成（~20 行）

新建 `system_api_error_message.rs` 扩展或集成到子类型渲染中。TS 参考: `SystemAPIErrorMessage.tsx:17-55`

#### Phase 3.9: 测试（~40 行）

覆盖所有 11 种子类型 + 边界情况（空内容、无 tag、未知 subtype 等）。

---

## 4. `user_text_message.rs` — 标签路由（P1）

### TS 参考

**文件 + 行号**:
| 路由 | `UserTextMessage.tsx` 行号 | 目标组件 | 条件 |
|------|:------------------------:|----------|------|
| 空内容 | 45-47 | null | `text.trim() === NO_CONTENT_MESSAGE` |
| Plan 模式 | 50-52 | `UserPlanMessage` | `planContent` prop |
| Tick 标签 | 54-56 | null | `extractTag(text, TICK_TAG)` |
| LocalCommandCaveat | 59-61 | null | `text.includes('<local-command-caveat>')` |
| Bash 输出 | 64-66 | `UserBashOutputMessage` | `text.startsWith('<bash-stdout')` or `<bash-stderr` |
| 本地命令输出 | 69-71 | `UserLocalCommandOutputMessage` | `text.startsWith('<local-command-stdout')` or `<local-command-stderr` |
| 中断消息 | 74-80 | `InterruptedByUser` | 精确匹配 `INTERRUPT_MESSAGE` |
| GitHub webhook | 88-96 | `UserGitHubWebhookMessage` | 特性门控 KAIROS_GITHUB_WEBHOOKS |
| Bash 输入 | 99-101 | `UserBashInputMessage` | `text.includes('<bash-input>')` |
| 命令消息 | 104-106 | `UserCommandMessage` | `text.includes('<command-message>')` |
| 记忆输入 | 108-110 | `UserMemoryInputMessage` | `text.includes('<user-memory-input>')` |
| 队友消息 | 113-115 | `UserTeammateMessage` | 特性门控 agent swarms |
| 任务通知 | 118-120 | `UserAgentNotificationMessage` | `text.includes('<task-notification>')` |
| MCP 资源更新 | 123-125 | `UserResourceUpdateMessage` | `text.includes('<mcp-resource-update')` |
| Fork 样板 | 130-143 | `UserForkBoilerplateMessage` | `text.includes('<fork-boilerplate>')` |
| 跨会话消息 | 148-156 | `UserCrossSessionMessage` | 特性门控 UDS_INBOX |
| 频道消息 | 159-165 | `UserChannelMessage` | 特性门控 KAIROS |
| 默认 | 169-171 | `UserPromptMessage` | fallback |

### 当前 Rust 实现（12 行）

```rust
pub fn render_user_text_message(text: &str, _theme: &Theme) -> String {
    if text.trim().is_empty() {
        "You: <empty>".to_string()
    } else {
        format!("You: {text}")
    }
}
```

### 分步实施

#### Phase 4.1: 标签分发路由函数（~60 行）

```rust
#[derive(Debug, Clone)]
pub enum UserTextRendered {
    Rendered(String),
    // 值为 None 表示该消息应当被隐藏（不渲染）
    Hidden,
    // 需要委托给其他渲染器
    Delegated(&'static str, String),
}

pub fn route_user_text(text: &str) -> UserTextRendered {
    let trimmed = text.trim();

    // 1. 空内容检查
    if trimmed.is_empty() || trimmed == NO_CONTENT_MESSAGE {
        return UserTextRendered::Hidden;
    }

    // 2. Bash 输出
    if trimmed.starts_with("<bash-stdout") || trimmed.starts_with("<bash-stderr") {
        return UserTextRendered::Delegated("bash_output", trimmed.to_string());
    }

    // 3. 本地命令输出
    if trimmed.starts_with("<local-command-stdout") || trimmed.starts_with("<local-command-stderr") {
        return UserTextRendered::Delegated("local_command_output", trimmed.to_string());
    }

    // 4. 中断消息
    if trimmed == INTERRUPT_MESSAGE || trimmed == INTERRUPT_MESSAGE_FOR_TOOL_USE {
        return UserTextRendered::Rendered("[Request interrupted by user]".to_string());
    }

    // 5. Bash 输入
    if trimmed.contains("<bash-input>") {
        return UserTextRendered::Delegated("bash_input", trimmed.to_string());
    }

    // 6. 命令消息
    if trimmed.contains("<command-message>") {
        return UserTextRendered::Delegated("command", trimmed.to_string());
    }

    // 7. 用户记忆输入
    if trimmed.contains("<user-memory-input>") {
        return UserTextRendered::Delegated("memory_input", trimmed.to_string());
    }

    // 8. 任务通知
    if trimmed.contains("<task-notification>") {
        return UserTextRendered::Delegated("agent_notification", trimmed.to_string());
    }

    // 9. MCP 资源更新
    if trimmed.contains("<mcp-resource-update") || trimmed.contains("<mcp-polling-update") {
        return UserTextRendered::Delegated("resource_update", trimmed.to_string());
    }

    // 10. 默认 → UserPromptMessage
    UserTextRendered::Delegated("prompt", trimmed.to_string())
}
```

TS 参考: `UserTextMessage.tsx:37-172`

#### Phase 4.2: 委托渲染器集成（~40 行）

确保 `route_user_text()` 返回的 `Delegated` 类型能被主调度器 `render.rs` 正确处理。主调度器中已有以下渲染函数，但需要验证它们是否被正确调用：

- `render_user_bash_output_message_with_options()` — 已实现（完整）
- `render_user_local_command_output_message()` — 需改进
- `render_user_bash_input_message()` — 需改进
- `render_user_command_message()` — 需改进
- `render_user_memory_input_message()` — 需改进
- `render_user_agent_notification_message()` — 需改进
- `render_user_resource_update_message()` — 需改进
- `render_user_prompt_message()` — 需改进

#### Phase 4.3: 更新主函数（~20 行）

```rust
pub fn render_user_text_message(text: &str, theme: &Theme) -> String {
    match route_user_text(text) {
        UserTextRendered::Hidden => String::new(),
        UserTextRendered::Rendered(s) => s,
        UserTextRendered::Delegated(kind, content) => {
            // 默认：显示委托标签 + 内容
            format!("[{kind}]: {content}")
        }
    }
}
```

#### Phase 4.4: 更新主调度器（~30 行）

在 `render.rs` 的 `render_user_message()` 中，当 `UserTextRendered::Delegated` 时，路由到正确的子渲染器。

#### Phase 4.5: 测试（~20 行）

覆盖所有 15+ 路由分支、空内容、常规文本等。

---

## 5. `attachment_message.rs` — 25+ 附件类型渲染器（P1）

### TS 参考

**文件 + 行号**:
| 附件类型 | `AttachmentMessage.tsx` 行号 | 渲染内容 |
|---------|:--------------------------:|---------|
| `teammate_mailbox` | 45-115 | teammate 消息列表、task assignment、plan approval |
| `skill_discovery` | 120-139 | skills 名称 + 反馈提示（EXPERIMENTAL_SKILL_SEARCH） |
| `tool_discovery` | 143-154 | 发现的 tools 列表 |
| `directory` | 158-162 | "Listed directory {path}/" |
| `file` / `already_read_file` | 163-188 | 行数、notebook cells、unchanged |
| `compact_file_reference` | 189-192 | "Referenced file {path}" |
| `pdf_reference` | 194-198 | "Referenced PDF {path} (N pages)" |
| `selected_lines_in_ide` | 200-205 | "Selected N lines from {path} in {ide}" |
| `nested_memory` | 207-211 | "Loaded {path}" |
| `relevant_memories` | 213-252 | 记忆计数 + CtrlOToExpand + 展开时路径列表 |
| `dynamic_skill` | 253-263 | "Loaded N skills from {path}" |
| `skill_listing` | 265-273 | "N skills available" |
| `agent_listing_delta` | 275-284 | "N agent types available" |
| `queued_command` | 286-299 | 嵌套 UserTextMessage + UserImageMessage |
| `plan_file_reference` | 301-302 | "Plan file referenced ({path})" |
| `invoked_skills` | 303-308 | "Skills restored ({names})" |
| `diagnostics` | 310-311 | DiagnosticsDisplay |
| `mcp_resource` | 312-316 | "Read MCP resource {name} from {server}" |
| `command_permissions` | 318-320 | null |
| `async_hook_response` | 322-335 | "Async hook {event} completed" |
| `hook_blocking_error` | 337-349 | "{name} hook returned blocking error" |
| `hook_non_blocking_error` | 351-358 | "{name} hook error" |
| `hook_error_during_execution` | 359-365 | "{name} hook warning" |
| `hook_success` | 367-368 | null |
| `hook_stopped_continuation` | 369-378 | "{name} hook stopped continuation: {msg}" |
| `hook_system_message` | 379-384 | "{name} says: {content}" |
| `hook_permission_decision` | 385-392 | "Allowed/Denied by {event} hook" |
| `task_status` | 393-394, 424-485 | GenericTaskStatus / TeammateTaskStatus |
| `teammate_shutdown_batch` | 395-403 | "N teammates shut down gracefully" |

### 当前 Rust 实现（12 行）

```rust
pub fn render_attachment_message(label: &str, detail: &str, _theme: &Theme) -> String {
    let detail_line = if detail.trim().is_empty() {
        "no details".to_string()
    } else {
        detail.to_string()
    };
    format!("Attachment: {label} -> {detail_line}")
}
```

### 分步实施

#### Phase 5.1: 附件类型枚举（~40 行）

将当前 `Attachment::type`（或新建）映射为完整的枚举：

```rust
#[derive(Debug, Clone)]
pub enum AttachmentKind<'a> {
    /// 文件（读/未变更）
    File { display_path: &'a str, lines: Option<usize>, truncated: bool },
    /// 已读文件（未变更）
    AlreadyReadFile { display_path: &'a str, unchanged: bool },
    /// 压缩文件引用
    CompactFileReference { display_path: &'a str },
    /// PDF 引用
    PdfReference { display_path: &'a str, page_count: u32 },
    /// 目录列表
    Directory { display_path: &'a str },
    /// IDE 选择行
    SelectedLinesInIde { display_path: &'a str, line_start: u32, line_end: u32, ide_name: &'a str },
    /// 嵌套记忆
    NestedMemory { display_path: &'a str },
    /// 相关记忆（CollapsedReadSearchGroup）
    RelevantMemories { count: usize, paths: Vec<&'a str> },
    /// 动态技能
    DynamicSkill { count: usize, display_path: &'a str },
    /// 技能列表
    SkillListing { count: usize, is_initial: bool },
    /// Agent 列表增量
    AgentListingDelta { count: usize, is_initial: bool },
    /// 队列命令
    QueuedCommand { prompt: &'a str },
    /// 计划文件引用
    PlanFileReference { path: &'a str },
    /// 调用（已恢复）技能
    InvokedSkills { names: Vec<&'a str> },
    /// 诊断
    Diagnostics,
    /// MCP 资源
    McpResource { name: &'a str, server: &'a str },
    /// 命令权限
    CommandPermissions,
    /// Hook: 异步响应
    AsyncHookResponse { event: &'a str, verbose: bool },
    /// Hook: 阻塞错误
    HookBlockingError { name: &'a str, hook_event: &'a str, stderr: Option<&'a str> },
    /// Hook: 非阻塞错误
    HookNonBlockingError { name: &'a str, hook_event: &'a str },
    /// Hook: 执行中错误
    HookErrorDuringExecution { name: &'a str, hook_event: &'a str },
    /// Hook: 成功
    HookSuccess,
    /// Hook: 停止继续
    HookStoppedContinuation { name: &'a str, hook_event: &'a str, message: &'a str },
    /// Hook: 系统消息
    HookSystemMessage { name: &'a str, content: &'a str },
    /// Hook: 权限决策
    HookPermissionDecision { name: &'a str, hook_event: &'a str, decision: &'a str },
    /// 任务状态
    TaskStatus { description: &'a str, status: &'a str },
    /// 队友关机批次
    TeammateShutdownBatch { count: u32 },
    /// 队友邮箱
    TeammateMailbox { messages: usize },
    /// 技能发现
    SkillDiscovery { skills: Vec<(&'a str, Option<&'a str>)> },
    /// 工具发现
    ToolDiscovery { tools: Vec<&'a str> },
}
```

TS 参考: `AttachmentMessage.tsx:157-421`（switch 语句）

#### Phase 5.2: 分类函数（~30 行）

```rust
pub fn classify_attachment(label: &str, detail: &str) -> AttachmentKind {
    // 根据 label + detail 解析附件类型
    match label {
        "file" => AttachmentKind::File { ... },
        "already_read_file" => AttachmentKind::AlreadyReadFile { ... },
        "directory" => AttachmentKind::Directory { ... },
        // ... etc
        _ => {
            // 尝试从 detail JSON 中解析更多信息
            // 回退到通用格式
        }
    }
}
```

#### Phase 5.3: Phase 1 — 核心文件/引用类型（~80 行）

实现最基本的类型渲染，这些类型最常用：

| 类型 | 输出格式 |
|------|---------|
| `File` | `"Read {path} (N lines)"` / `"(N cells)"` / `"(unchanged)"` |
| `Directory` | `"Listed directory {path}/"` |
| `CompactFileReference` | `"Referenced file {path}"` |
| `PdfReference` | `"Referenced PDF {path} (N pages)"` |
| `SelectedLinesInIde` | `"Selected N lines from {path} in {ide}"` |
| `NestedMemory` | `"Loaded {path}"` |
| `RelevantMemories` | `"Recalled N memories"` |

TS 参考: `AttachmentMessage.tsx:159-212`

#### Phase 5.4: Phase 2 — Hook 类型（~100 行）

所有 9 种 Hook 类型渲染。很多 hook 类型共享 `hookEvent` 过滤逻辑（Stop/SubagentStop 时返回 null）。

| 类型 | 输出格式 | 特殊逻辑 |
|------|---------|---------|
| `AsyncHookResponse` | `"Async hook {event} completed"` | 仅 verbose/mode 时显示 |
| `HookBlockingError` | `"{name} hook returned blocking error: {stderr}"` | 跳过 Stop/SubagentStop |
| `HookNonBlockingError` | `"{name} hook error"` | 跳过 Stop/SubagentStop |
| `HookErrorDuringExecution` | `"{name} hook warning"` | 跳过 Stop/SubagentStop |
| `HookSuccess` | (null) | 总为 null |
| `HookStoppedContinuation` | `"{name} hook stopped continuation: {msg}"` | 跳过 Stop/SubagentStop |
| `HookSystemMessage` | `"{name} says: {content}"` | — |
| `HookPermissionDecision` | `"Allowed/Denied by {event} hook"` | — |

TS 参考: `AttachmentMessage.tsx:322-392`

#### Phase 5.5: Phase 3 — 复合类型 + 特性门控类型（~100 行）

| 类型 | 输出格式 | 门控 |
|------|---------|:----:|
| `SkillDiscovery` | `"N relevant skills: {names}"` + 反馈提示 | EXPERIMENTAL_SKILL_SEARCH |
| `ToolDiscovery` | `"Discovered tools: {names}"` | EXPERIMENTAL_SEARCH_EXTRA_TOOLS |
| `QueuedCommand` | 嵌套 `UserTextMessage` + `UserImageMessage` | — |
| `TeammateMailbox` | teammate messages 列表 | isAgentSwarmsEnabled |
| `TaskStatus` | `"Task '{description}' {status}"` | — |
| `TeammateShutdownBatch` | `"N teammates shut down gracefully"` | — |
| `DynamicSkill` | `"Loaded N skills from {path}"` | — |
| `SkillListing` | `"N skills available"` / null (initial) | — |
| `AgentListingDelta` | `"N agent types available"` / null | — |
| `InvokedSkills` | `"Skills restored ({names})"` | — |
| `Diagnostics` | 诊断显示 | — |
| `McpResource` | `"Read MCP resource {name} from {server}"` | — |
| `CommandPermissions` | (null) | — |
| `PlanFileReference` | `"Plan file referenced ({path})"` | — |

TS 参考: `AttachmentMessage.tsx:45-155`（mailbox/discovery）+ `AttachmentMessage.tsx:253-421`（其余类型）

#### Phase 5.6: 集成到主函数（~30 行）

```rust
pub fn render_attachment_message(label: &str, detail: &str, theme: &Theme) -> String {
    let kind = classify_attachment(label, detail);
    render_attachment_kind(&kind)
}
```

#### Phase 5.7: 测试（~50 行）

覆盖 25+ 种附件类型、边界情况（空 detail、特性门控关闭时的行为等）。

---

## 集成到主调度器

所有 5 个渲染器都需要在主调度器 `render.rs` 中使用。当前 `render_single_message_with_context()`（第 232 行）调度到辅助渲染器。补齐后需要：

1. `assistant_text_message.rs` — `render_assistant_message()` 在连续文本块上已调用 Markdown 渲染；引入错误分类后，在 Markdown 渲染前插入错误检测步骤
2. `assistant_tool_use_message.rs` — 扩展调用签名，传入 `ToolUseState` 枚举
3. `system_text_message.rs` — 扩展调用以传入子类型参数（或解析后的枚举）
4. `user_text_message.rs` — 扩展调用以处理委托结果
5. `attachment_message.rs` — 扩展调用以传入结构化附件数据而非 `(label, detail)` 元组

### 调度器修改估算

| 文件 | 新增行 | 修改行 |
|------|:------:|:------:|
| `render.rs` | ~30 | ~20 |
| `assistant_text_message.rs` | ~150 | ~10 |
| `assistant_tool_use_message.rs` | ~200 | ~10 |
| `system_text_message.rs` | ~280 | ~20 |
| `user_text_message.rs` | ~120 | ~10 |
| `attachment_message.rs` | ~330 | ~10 |
| **总计** | **~1,110** | **~80** |

---

## 依赖项和注意事项

### 共享枚举
- `ToolUseState`、`SystemSubtype`、`AttachmentKind` 和错误类型枚举可能需要在 `src/ui/messages/types.rs` 或 `src/types/` 中集中定义，以避免模块间循环依赖

### 特性门控
- TS 中使用 `feature()` 编译时消除的代码（KAIROS、TEAMMEM、FORK_SUBAGENT、UDS_INBOX 等）在 Rust 中使用 `cfg(feature = "...")` 或运行时检查
- `AttachmentMessage` 中的 `skill_discovery` 和 `tool_discovery` 类型仅在对应特性启用时渲染

### 字符串 vs `Vec<Line>`
- 目前所有辅助渲染器返回 `String`。部分类型（如 `RelevantMemories` 的多行展示）更适合返回 `Vec<Line>`。考虑逐步迁移关键渲染器到 `Vec<Line>` 返回类型
- 如果迁移，需要在主调度器中处理两种返回类型

### 不移植的内容
- `<SentryErrorBoundary>` — 错误边界是 React 特有概念
- `<Spinner>` 动画 — ratatui 不支持基于帧的动画；使用静态字符回退
- `useMemo` / `useContext` — React 特定优化
- `CtrlOToExpand` — Rust 端使用 `render.rs` 中已存在的 `decorate_selected_message()` 机制
- `MacOS keychain lock detection` — 平台特定，在 `InvalidApiKey` 中忽略

---

## 验收标准

1. **`assistant_text_message.rs`**: 10 种 API 错误分类 + 精确文案 + 通用 API 回退
2. **`assistant_tool_use_message.rs`**: 7 种工具状态（InProgress/Resolved/Queued/WaitingForPermission/ClassifierChecking/Error + HookProgress 集成）
3. **`system_text_message.rs`**: 11 种子类型路由（turn_duration/memory_saved/away_summary/agents_killed/thinking/bridge_status/scheduled_task_fire/permission_retry/api_error/stop_hook_summary/generic）
4. **`user_text_message.rs`**: 15+ 种标签路由 + 正确的 null/委托/渲染决策
5. **`attachment_message.rs`**: 25+ 种附件类型渲染器 + 特性门控处理
