# Structured Brief Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 KAIROS Brief 从“工具骨架 + UI 占位”补成端到端结构化输出链路：模型调用 `Brief` 或 `SendUserMessage` 后，生成统一 payload，保留 tool result 给模型上下文，同时向 SDK、IPC、daemon SSE、Rust TUI、每日记忆日志输出结构化 `brief_message`。

**Architecture:** `Brief`/`SendUserMessage` 工具返回 canonical JSON；engine 在工具执行结果中提取 `BriefMessagePayload`；query loop 在写入 tool_result user message 的同时额外产出 `SdkMessage::BriefMessage`；daemon 映射为 SSE `brief_message`；headless/IPC 映射为 `BackendMessage::BriefMessage`；Rust TUI 按上游规则过滤普通 assistant 文本并显示 Brief 工具结果；每日 memory log 记录 Brief 内容。

**Tech Stack:** Rust, serde, allthecodes-types, allthecodes-tools, allthecodes-engine query loop, allthecodes-ipc-protocol, allthecodes-daemon SSE, Rust TUI renderer, Cargo workspace tests.

## Global Constraints

- 进入 Full Build 阶段后按上游完整行为对齐，不以 Lite 范围缩减功能。
- 上游参考使用 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/commands/brief.ts` 和 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/components/Messages.tsx`。
- 持久化路径继续使用 allthecodes 隔离路径，所有日志和状态写入 `~/.allthecodes/` 或 `ALLTHECODES_HOME` 下的等价目录。
- `FEATURE_KAIROS_BRIEF` 仍依赖 `FEATURE_KAIROS`，测试必须覆盖 gate 未开启和开启两种路径。
- Rust TUI 只改 `crates/allthecodes/src/ui/` 下的 UI 代码。
- 修改后使用仓库要求的 Rust 环境变量运行验证：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## Current State

- `crates/allthecodes-config/src/features.rs` 已有 `Feature::KairosBrief` 和 `FEATURE_KAIROS_BRIEF` gate。
- `crates/allthecodes-engine/src/types/app_state.rs` 与 `crates/allthecodes-tools/src/tool.rs` 已有 `is_brief_only`。
- `crates/allthecodes-commands/src/brief.rs` 已能切换 `ctx.app_state.is_brief_only` 并清 prompt cache，但没有把 tool opt-in 变化和下一轮 system reminder 串起来。
- `crates/allthecodes-tools/src/runtime/brief.rs` 已有 `Brief` 工具，返回 `is_brief_message/message/status/attachments`。
- `crates/allthecodes-tools/src/interaction/send_user_message.rs` 已有 `SendUserMessage` 工具，但返回 schema 与 Brief 不统一，也没有 `is_brief_message` 标记。
- `crates/allthecodes-ipc-protocol/src/protocol/mod.rs` 已声明 `BackendMessage::BriefMessage { message, status, attachments }`，`normalized.rs` 已映射为 `brief_message`，但仓库内没有生产该消息的路径。
- `crates/allthecodes/src/ui/messages/render/preprocessing.rs` 的 `filter_brief_messages` 目前直接返回原始 messages。
- `crates/allthecodes-types/src/sdk.rs` 没有 `SdkMessage::BriefMessage`，daemon 的 `sdk_message_to_sse` 因而无法自然广播 Brief。

## Implementation Tasks

### 1. 定义共享 Brief payload

- [ ] 新增 `crates/allthecodes-types/src/brief.rs`，定义共享结构和工具名常量：

```rust
pub const BRIEF_TOOL_NAME: &str = "Brief";
pub const SEND_USER_MESSAGE_TOOL_NAME: &str = "SendUserMessage";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefMessageStatus {
    Normal,
    Proactive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefMessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefMessagePayload {
    pub message: String,
    pub status: BriefMessageStatus,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<BriefMessageLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}
```

- [ ] 在同文件实现：
  - `is_brief_tool_name(tool_name: &str) -> bool`
  - `brief_payload_from_tool_result(tool_name: &str, tool_use_id: &str, session_id: &str, data: &serde_json::Value) -> Option<BriefMessagePayload>`
  - `brief_payload_to_json(payload: &BriefMessagePayload) -> serde_json::Value`
- [ ] `Brief` 工具映射规则：
  - 仅当 `data.is_brief_message == true` 且 `message` 非空时提取。
  - `status` 缺省为 `normal`，仅接受 `normal` 和 `proactive`。
  - `attachments` 缺省为空数组。
- [ ] `SendUserMessage` 映射规则：
  - 接受 `message` 非空的结果。
  - `status` 固定为 `normal`。
  - `level` 缺省为 `info`，仅接受 `info/warning/error`。
  - `attachments` 固定为空数组。
- [ ] 在 `crates/allthecodes-types/src/lib.rs` 导出 `pub mod brief;`。

#### Tests

- [ ] 在 `brief.rs` 内加入单元测试：
  - `Brief` payload 提取成功。
  - `Brief` bad status 返回 `None`。
  - `SendUserMessage` level 正常映射。
  - 空 message 返回 `None`。

### 2. 统一工具输出数据

- [ ] 修改 `crates/allthecodes-tools/src/runtime/brief.rs`：
  - 使用 `allthecodes_types::brief::BRIEF_TOOL_NAME` 作为 name 常量来源。
  - `ToolResult.data` 保持现有字段并确保 `is_brief_message: true`。
  - 设置 `display_preview = Some(message.clone())`，让普通 tool result 渲染不再显示整段 JSON。
- [ ] 修改 `crates/allthecodes-tools/src/interaction/send_user_message.rs`：
  - 使用 `SEND_USER_MESSAGE_TOOL_NAME` 作为 name 常量来源。
  - `ToolResult.data` 增加 `is_brief_message: true` 和 `status: "normal"`：

```json
{
  "is_brief_message": true,
  "message": "...",
  "status": "normal",
  "level": "info"
}
```

  - 设置 `display_preview = Some(message.clone())`。
- [ ] 保持两个工具 read-only、concurrency-safe 行为不变。

#### Tests

- [ ] 更新 `crates/allthecodes-tools/src/runtime/brief.rs` 测试，断言 `call()` 的 `display_preview` 和 `is_brief_message`。
- [ ] 更新 `crates/allthecodes-tools/src/interaction/send_user_message.rs` 测试，断言 `call()` 的 unified fields。

### 3. 将 Brief payload 带过工具执行边界

- [ ] 修改 `crates/allthecodes-engine/src/query/deps.rs`：
  - 为 `ToolExecResult` 添加字段：

```rust
pub brief_message: Option<allthecodes_types::brief::BriefMessagePayload>,
```

  - 更新所有 `ToolExecResult` 构造点，非 Brief 路径填 `None`。
- [ ] 修改 `crates/allthecodes-engine/src/lifecycle/deps/execute.rs`：
  - 在 `tool_exec_result(...)` 内调用 `brief_payload_from_tool_result(...)`。
  - 成功提取后把 `source_tool_name/tool_use_id/session_id/timestamp` 填完整。
  - 如果 payload 存在且 `ToolResult.display_preview` 为空，把 preview 设为 payload message。
- [ ] 修改 `crates/allthecodes-engine/src/lifecycle/deps/tool_pipeline.rs`：
  - `record_and_audit` 仍先写入 permission feedback。
  - 结果大小裁剪不能破坏 Brief payload。实现顺序为：先从原始 `result.data` 提取 payload，再对 `result.data` 做 `enforce_result_size`。
- [ ] 修改 `crates/allthecodes-engine/src/query/loop_helpers.rs`：
  - 保持 `make_tool_result_user_message(...)` 对 Brief tool result 的常规转换，确保模型下一轮仍能看到工具调用成功。
  - 对 Brief payload 的 user-visible 事件不在这里丢弃 tool result。

#### Tests

- [ ] 在 `crates/allthecodes-engine/src/lifecycle/deps/execute.rs` 或现有 lifecycle 测试中加入测试：Brief tool result 生成 `ToolExecResult.brief_message`。
- [ ] 在 `crates/allthecodes-engine/src/query/loop_helpers.rs` 加测试：Brief tool result 仍生成 `ContentBlock::ToolResult`，`tool_use_result` 为 message preview。

### 4. 新增 SDK Brief 消息并从 query loop 发出

- [ ] 修改 `crates/allthecodes-types/src/sdk.rs`：
  - 新增 `SdkMessage::BriefMessage(SdkBriefMessage)`。
  - 新增 DTO：

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SdkBriefMessage {
    pub payload: crate::brief::BriefMessagePayload,
    pub session_id: String,
    pub uuid: Uuid,
}
```

  - `event_name()` 返回 `"brief_message"`。
- [ ] 修改 `crates/allthecodes-engine/src/query/loop_impl.rs`：
  - 在 `crates/allthecodes-types/src/message.rs` 扩展 `QueryYield::BriefMessage(crate::brief::BriefMessagePayload)`。
  - 在工具执行完成后、生成并 yield tool_result user message 的同一循环内，对每个 `exec_result.brief_message` 额外 `yield QueryYield::BriefMessage(...)`。
  - 在 query-to-SDK 转换处把 `QueryYield::BriefMessage` 映射为 `SdkMessage::BriefMessage`。
  - Brief SDK 消息必须在对应 tool result user replay 后发出，使前端先有 tool lookup，再收到结构化展示事件。

#### Tests

- [ ] 在 `crates/allthecodes-types/src/sdk.rs` 测试 `SdkMessage::BriefMessage` 序列化 type tag 为 `brief_message`。
- [ ] 在 engine query loop 测试中模拟一个返回 Brief payload 的工具，断言 submit stream 同时包含：
  - `assistant` tool_use；
  - `user_replay` tool_result；
  - `brief_message`；
  - final `result`。

### 5. IPC 与 daemon SSE 输出

- [ ] 修改 `crates/allthecodes-ipc-protocol/src/protocol/mod.rs`：
  - 保留现有 wire-compatible 字段 `message/status/attachments`。
  - 给 `BackendMessage::BriefMessage` 增加可选字段：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
level: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
source_tool_name: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
tool_use_id: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
session_id: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
timestamp: Option<i64>,
```

- [ ] 在 `crates/allthecodes-ipc-protocol/src/normalized.rs` 保持 `brief_message` event name，并补充 payload 字段测试。
- [ ] 修改 `crates/allthecodes-daemon/src/routes.rs`：
  - 在 `sdk_message_to_sse(...)` 中新增 `SdkMessage::BriefMessage` 分支。
  - SSE `event_type` 为 `"brief_message"`。
  - `data` 顶层包含 `message_id/message/status/attachments/level/source_tool_name/tool_use_id/session_id/timestamp`。
  - 继续通过 `DaemonState::broadcast(...)` 获得 event id 和 replay 能力。
- [ ] 修改 `crates/allthecodes-daemon/src/memory_log.rs`：
  - 新增 `append_brief_message(payload: &BriefMessagePayload)`，写入形如 `brief[normal/info]: message` 的条目。
  - 路径继续走 `process_state::daily_log_path(...)`。
- [ ] 修改 `crates/allthecodes-daemon/src/routes.rs` 或 worker 消费 SDK stream 的位置：
  - 收到 `SdkMessage::BriefMessage` 时调用 `memory_log::append_brief_message(...)`。
  - 不把普通 assistant text 写成 Brief。

#### Tests

- [ ] 在 `crates/allthecodes-daemon/src/routes.rs` 增加测试：`sdk_message_to_sse(SdkMessage::BriefMessage)` 生成 `event_type == "brief_message"` 且 data 字段完整。
- [ ] 在 `crates/allthecodes-daemon/src/memory_log.rs` 增加测试，使用临时 `ALLTHECODES_HOME` 验证 Brief log 写到 daily log。
- [ ] 在 `crates/allthecodes-ipc-protocol/src/protocol/mod.rs` 或 `normalized.rs` 增加序列化兼容测试：旧三字段 BriefMessage 仍可反序列化。

### 6. `/brief` 开关对齐上游行为

- [ ] 修改 `crates/allthecodes-commands/src/lib.rs` 的 `CommandResult`：
  - 新增 variant：

```rust
OutputWithMeta {
    text: String,
    meta_messages: Vec<Message>,
}
```

  - `meta_messages` 仅加入 transcript/model context，不显示给用户。
- [ ] 修改 `crates/allthecodes-commands/src/brief.rs`：
  - `on/enable`、`off/disable`、空参数 toggle 都返回 `OutputWithMeta`。
  - 开启时 meta 内容为：

```xml
<system-reminder>
Brief mode is now enabled. Use the Brief tool for all user-facing output; plain text outside it is hidden from the user's view.
</system-reminder>
```

  - 关闭时 meta 内容为：

```xml
<system-reminder>
Brief mode is now disabled. The Brief tool is no longer available; reply with plain text.
</system-reminder>
```

  - 当 `ctx.app_state.kairos_active == true` 时不注入 meta reminder，遵循上游 `brief.ts` 的 Kairos active shortcut。
  - 继续调用 `clear_prompt_cache()`。
- [ ] 修改 `crates/allthecodes-engine/src/lifecycle/submit_message/command_handling.rs`：
  - 处理 `CommandResult::OutputWithMeta`。
  - 应用 app state。
  - 把 `meta_messages` 追加到 engine transcript，消息使用 `Message::User(UserMessage { is_meta: true, ... })`。
  - 设置 `processed.should_query = false`，避免 `/brief on` 这条 slash command 本身触发一次模型查询。
  - `processed.result_text = Some(text)`，本地用户仍看到系统提示。
- [ ] 修改 `crates/allthecodes-daemon/src/routes.rs` command endpoint：
  - 处理 `OutputWithMeta`，广播 `system_info`，并把 meta messages 写入 engine transcript 或通过同一 command handling helper 复用逻辑。

#### Tests

- [ ] 更新 `crates/allthecodes-commands/src/brief.rs` 测试：
  - feature gate 未开仍返回原错误。
  - feature gate 开启时 `/brief on` 设置 `is_brief_only` 并返回一个 meta system reminder。
  - `kairos_active` 时 `/brief on` 不返回 meta reminder。
- [ ] 更新 `crates/allthecodes-engine/src/lifecycle/submit_message/command_handling.rs` 测试：`OutputWithMeta` 会进入后续 query context。

### 7. Runtime-aware Brief prompt 与工具可见性

- [ ] 修改 `crates/allthecodes-engine/src/system_prompt/mod.rs`：
  - 给 `build_system_prompt_with_memory_contexts(...)` 增加 `app_state: Option<&AppState>` 或更窄的 `BriefPromptMode` 参数。
  - `build_submit_system_prompt(...)` 从 `state_ref.read().app_state` 传入当前 runtime state。
- [ ] 修改 `crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs`：
  - 把 `kairos_brief_section()` 改为接收 runtime state。
  - 当 `is_brief_only == true` 时使用严格文案：所有用户可见输出必须走 `Brief`，普通 assistant text 会被 UI 隐藏。
  - 当 `kairos_active == true` 且 Brief feature 可用时使用 KAIROS 文案：状态、提问、最终摘要、主动通知走 Brief/SendUserMessage。
  - 当两者都不是 active 时不注入 Brief 段落。
- [ ] 修改工具列表过滤入口：
  - 优先在 `crates/allthecodes-tools/src/registry.rs` 新增显式 helper，或在 engine submit 的 `prompt_tools_snapshot` 生成处过滤。
  - `Brief` 和 `SendUserMessage` 在以下条件之一满足时可见：
    - `Feature::KairosBrief` 且 `app_state.is_brief_only == true`；
    - `Feature::Kairos` 且 `app_state.kairos_active == true`。
  - `/brief off` 后下一次 prompt 不再暴露 `Brief`。

#### Tests

- [ ] 更新 `crates/allthecodes-engine/src/system_prompt/tests.rs`：
  - Brief-only true 时包含严格 Brief 文案。
  - Brief feature 开但 runtime 未开启时不包含 Brief 文案。
  - Kairos active 时包含 KAIROS Brief 文案。
- [ ] 增加工具过滤测试：Brief off 不出现在 SystemInit tools，Brief on 出现。

### 8. Rust TUI 渲染与过滤

- [ ] 修改 `crates/allthecodes/src/ui/messages/render/context.rs`：
  - 给 `MessageRenderOptions` 增加 `is_brief_only: bool`。
  - `render_context_cache_key(...)` 加入 brief flag，避免切换模式后复用旧 render cache。
- [ ] 修改 `crates/allthecodes/src/ui/app/render.rs`：
  - 普通 chat render options 填 `is_brief_only: self.app_state.is_brief_only` 或现有 app state 等价字段。
  - transcript render options 设置 `is_transcript_mode: true`，并让 transcript 绕过 brief-only 过滤。
- [ ] 修改 `crates/allthecodes/src/ui/messages/render/preprocessing.rs` 的 `filter_brief_messages(...)`：
  - transcript mode 直接返回原 messages。
  - brief-only mode：
    - 保留 `Message::System`，但隐藏 api metrics 类调试噪声。
    - 保留真实用户输入。
    - 保留 `Brief` 和 `SendUserMessage` 的 assistant tool_use blocks。
    - 保留这些 brief tool_use 对应的 user tool_result blocks。
    - 保留 assistant API error messages。
    - 隐藏 assistant text、thinking、redacted thinking、非 Brief tool_use、非 Brief tool_result。
  - 普通模式：
    - 如果一个 turn 调用了 `Brief` 或 `SendUserMessage`，只删除该 turn 的 assistant text blocks。
    - 如果该 turn 没调用 Brief，assistant text 正常显示。
    - tool_use/tool_result 保持可见。
- [ ] 修改 `crates/allthecodes/src/ui/tui/engine_events.rs`：
  - 处理 `SdkMessage::BriefMessage`，转为 `BackendMessage::BriefMessage` 或直接追加一个轻量 user-visible message。
  - brief-only mode 下忽略 streaming `text_delta` 和 `thinking_delta` 的可见追加；final assistant message 仍进入 transcript，由 render filter 决定可见性。
- [ ] 修改 `crates/allthecodes/src/ui/tui/subsystem_events.rs`：
  - `handle_backend_messages(...)` 处理 `BackendMessage::BriefMessage`。
  - 按 `level/status` 转成 notification tone。
  - 不把 Brief payload 渲染成 SystemInfo，避免和普通系统消息混淆。

#### Tests

- [ ] 在 `crates/allthecodes/src/ui/messages/render/preprocessing.rs` 或 `render/mod.rs` 增加测试：
  - brief-only 只显示 Brief tool_use/tool_result 和真实用户输入。
  - brief-only 保留 API error assistant message。
  - transcript mode 不过滤。
  - 普通模式只删除调用 Brief 的 turn 中的 assistant text。
- [ ] 在 `crates/allthecodes/src/ui/tui/tests.rs` 增加测试：
  - `SdkMessage::BriefMessage` 变为 visible message 或 notification。
  - brief-only streaming text delta 不创建可见 assistant text。

### 9. Headless / IPC 适配

- [ ] 修改 headless SDK-to-IPC adapter：
  - 先用 `rg "SdkMessage::" crates/allthecodes-ipc crates/allthecodes-engine crates/allthecodes/src -n` 找到集中转换位置。
  - 对 `SdkMessage::BriefMessage` 输出 `BackendMessage::BriefMessage`。
  - 字段使用 `BriefMessagePayload` 展开，保持 `message/status/attachments` 顶层字段。
- [ ] 修改 `crates/allthecodes/src/ui/tui.rs` 或实际接收 `BackendMessage` 的 TUI loop：
  - 增加 `BackendMessage::BriefMessage` 分支。
  - 分支复用 `subsystem_events` 中的 Brief 添加函数，避免 headless 和 daemon 两条路径显示不一致。

#### Tests

- [ ] 增加 headless adapter 单元测试：SDK Brief 输入得到 IPC Brief 输出。
- [ ] 增加 normalized event 测试：IPC Brief 输出 `event_type == "brief_message"`。

### 10. End-to-end smoke

- [ ] 加一个集成测试或 scripted smoke，使用 fake model 返回：
  - assistant text；
  - `Brief` tool_use；
  - final result。
- [ ] 验证：
  - model context 保留 `tool_result`；
  - SDK stream 有 `brief_message`；
  - daemon SSE 有 `brief_message`；
  - TUI brief-only render 不显示 assistant text；
  - transcript mode 能看到完整 assistant/tool transcript。
- [ ] 使用 feature env 运行目标测试：

```bash
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes-types brief
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes-tools brief
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes-engine brief
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes-ipc-protocol brief
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes-daemon brief
FEATURE_KAIROS=1 FEATURE_KAIROS_BRIEF=1 cargo test -p allthecodes brief
```

- [ ] 最终验证：

```bash
cargo check --workspace
cargo build --workspace --release
```

## Acceptance Criteria

- `/brief on` 会切换 runtime state、刷新 prompt/tool list，并在非 Kairos active 时给下一轮模型注入明确 system reminder。
- `Brief` 和 `SendUserMessage` 两个工具都生成统一 `BriefMessagePayload`。
- Brief tool result 保留在模型上下文中，不因为用户可见事件而丢失。
- SDK stream、IPC protocol、daemon SSE 都有结构化 `brief_message`。
- daemon SSE replay 可以重放 Brief 事件。
- 每日 memory log 会记录 Brief 内容，路径位于 allthecodes 隔离数据根下。
- Rust TUI brief-only mode 隐藏普通 assistant text/thinking 和非 Brief tool result，transcript mode 不过滤。
- 普通模式只在实际调用 Brief 的 turn 隐藏重复 assistant text。
- 所有新增测试通过，workspace release build 无 warning。

## Execution Order

1. 共享 payload 和工具输出。
2. engine 工具执行边界和 query loop SDK 事件。
3. IPC/daemon SSE/memory log。
4. `/brief` command meta reminder 与 runtime-aware prompt/tool list。
5. Rust TUI render filter 与 streaming visibility。
6. 集成 smoke 与 workspace 验证。
