# submit_message.rs 拆分计划

> 原文件: `crates/cc-engine/src/lifecycle/submit_message.rs`
> 原始行数: 1668
> 目标: 拆分为 5 个子模块，每个子模块 <= 400 行

## 当前结构分析

该文件实现了 `QueryEngine::submit_message` -- 整个对话轮次的主流水线。文件包含一个单一的 `impl QueryEngine` 块（第699-1599行）以及围绕它的大量辅助函数和结构体。代码按照模块注释中描述的五个阶段组织：

- **Phase A-pre**: UserPromptSubmit hook 触发（L757-813）
- **Phase A**: 输入处理 -- 用户输入处理、命令调度、技能调用、bash 模式（L818-926）
- **Phase B**: 系统提示词构建（L992-1004）
- **Phase C**: 预查询设置 -- SystemInit 消息、本地命令快速路径（L932-988）
- **Phase D**: 查询循环 -- 流式消息分发，处理9种 QueryYield 变体 + 预算检查（L1009-1511）
- **Phase E**: 结果生成 -- SdkResult 构建（L1516-1597）

辅助代码可分为四类独立的功能域：
1. **记忆召回**（L35-178, L490-559, L180-190）: 确定性记忆上下文 + 模型辅助记忆召回
2. **命令处理**（L192-379, L586-643）: 本地命令调度、会话切换/清除、技能调用、bash 模式
3. **系统提示词构建**（L381-484, L561-596, L1602-1653）: 提示词组装、hooks、消息内容提取
4. **遥测**（L645-697）: submit/hook 遥测 span 管理

当前仅有的测试（L1655-1669）是 `model_assisted_memory_recall_tests`，仅测 `is_truthy_model_assisted_memory_recall_value`，应随记忆召回代码一起迁移。

## 拆分方案

### 子模块 1: `memory_recall.rs` (~250 行)

- **职责**: 记忆召回策略 -- 包括确定性记忆上下文构建、模型辅助记忆召回（API 调用排序记忆候选项）、记忆上下文覆盖解析
- **迁移内容**:
  - `fn model_assisted_memory_recall_enabled()` (L35-40) -- 读取环境变量判断是否启用模型辅助记忆召回
  - `fn is_truthy_model_assisted_memory_recall_value()` (L42-47) -- 真值字符串解析
  - `fn model_assisted_memory_recall_timeout()` (L49-56) -- 超时配置读取
  - `fn deterministic_memory_context()` (L58-82) -- 基于关键词的确定性记忆匹配
  - `fn build_model_assisted_memory_context()` (L84-178) -- 通过 API 调用让模型对记忆候选项排序
  - `fn assistant_message_text()` (L180-190) -- 从 AssistantMessage 提取纯文本（仅在 memory_recall 中使用）
  - `fn resolve_memory_context_override()` (L490-559) -- 选择确定性 vs 模型辅助记忆策略
  - `mod model_assisted_memory_recall_tests` (L1655-1669) -- 对应测试
- **依赖**:
  - `cc_session::memdir` -- 记忆目录 API
  - `cc_api::api::client::ApiClient` / `MessagesRequest` -- 模型辅助召回的 API 调用
  - `cc_types::sdk::AssistantMessage` -- 消息类型
  - `tracing::debug` -- 日志
  - `tokio::time::timeout` -- 超时控制
- **被依赖**: `system_prompt_build.rs`（通过 `resolve_memory_context_override` 在 `build_submit_system_prompt` 中调用）

**关键设计决策**: `resolve_memory_context_override` 是 `build_submit_system_prompt` 的内部依赖，但其逻辑完全围绕记忆策略选择，放入 memory_recall 保持了单一职责。`system_prompt_build.rs` 通过 `pub(super) fn` 调用它。

---

### 子模块 2: `command_handling.rs` (~230 行)

- **职责**: 本地命令处理 -- 解析后的命令分发执行、会话管理（切换/清除）、技能调用辅助、bash 模式命令执行
- **迁移内容**:
  - `struct LocalCommandOutcome` + `impl` (L216-228) -- 命令执行结果
  - `fn handle_parsed_command()` (L234-321) -- 核心命令分发函数，处理 Output/Query/SwitchSession/Clear/Exit/None/Error 七种结果
  - `fn apply_command_state()` (L323-330) -- 将命令上下文应用到引擎状态
  - `fn switch_command_session()` (L332-346) -- 命令触发的会话切换
  - `fn clear_command_session()` (L348-379) -- 命令触发的会话清除
  - `fn skill_args_from_prompt()` (L586-596) -- 从提示词中提取技能参数
  - `fn bash_mode_result_message()` (L598-643) -- 执行 bash 命令并构造结果消息
- **依赖**:
  - `crate::command_runtime::{CommandContext, CommandResult}` -- 命令运行时
  - `crate::input_processing::ProcessedInput` -- 输入处理结果
  - `crate::types::config::QueryEngineConfig` -- 配置
  - `crate::bootstrap::SessionId` / `PROCESS_STATE` -- 会话标识
  - `super::QueryEngineState` / `super::types::UsageTrackingExt` -- 引擎状态
  - `cc_types::commands::CommandDispatcher` -- 命令分发器
  - `parking_lot::RwLock` -- 锁
  - `tokio::process::Command` / `tokio::time::timeout` -- bash 模式执行
  - `uuid::Uuid` / `chrono` -- 消息标识
- **被依赖**: `mod.rs`（在 `submit_message` Phase A 中调用 `handle_parsed_command` 和 `skill_args_from_prompt`）

**关键设计决策**: `LocalCommandOutcome` 是 `handle_parsed_command` 的返回类型，两者紧密耦合，应放在同一模块。`bash_mode_result_message` 和 `skill_args_from_prompt` 是 Phase A 输入处理的辅助函数，虽然不直接处理命令，但它们是"输入分流"逻辑的一部分。

---

### 子模块 3: `system_prompt_build.rs` (~250 行)

- **职责**: 系统提示词构建流水线 -- 从状态读取配置、调用记忆召回、组装提示词、触发 hooks、以及消息内容提取辅助函数
- **迁移内容**:
  - `struct SubmitSystemPrompt` (L381-385) -- 系统提示词构建结果
  - `fn build_submit_system_prompt()` (L391-484) -- 核心提示词组装函数，协调记忆上下文和提示词生成
  - `fn fire_instructions_loaded_hook()` (L561-584) -- 触发 InstructionsLoaded hook
  - `fn latest_user_query_text()` (L1602-1612) -- 从消息历史提取最近用户查询文本
  - `fn user_message_text()` (L1614-1629) -- 从 UserMessage 内容提取文本
  - `fn recent_tool_names()` (L1631-1653) -- 从消息历史提取最近使用的工具名
- **依赖**:
  - `super::memory_recall::resolve_memory_context_override` -- 记忆上下文解析
  - `crate::system_prompt::build_system_prompt_with_memory_contexts` -- 底层提示词构建
  - `crate::types::config::QueryEngineConfig` -- 配置
  - `super::QueryEngineState` -- 引擎状态
  - `cc_types::hooks::HookRunner` -- hook 执行
  - `crate::types::tool::Tools` -- 工具快照
  - `crate::types::message::*` -- 消息类型
- **被依赖**: `mod.rs`（在 Phase B 中调用 `build_submit_system_prompt`）

**关键设计决策**: `latest_user_query_text`、`user_message_text`、`recent_tool_names` 三个消息内容提取函数仅在 `build_submit_system_prompt` 中使用（L429-430），因此归属同一模块。`fire_instructions_loaded_hook` 是提示词构建流水线的最后一步，也应在此。

---

### 子模块 4: `stream_handler.rs` (~400 行)

- **职责**: Phase D 查询循环的流式事件处理 -- 处理 `inner_stream` 产出的每种 `QueryYield` 变体（9 种类型），维护提交状态，执行预算检查
- **迁移内容**:
  - `enum StreamAction` (新增) -- 流处理动作枚举:
    ```rust
    pub(super) enum StreamAction {
        /// Yield an SdkMessage to the caller.
        Yield(SdkMessage),
        /// Yield an SdkResult and terminate the stream.
        Terminate(SdkResult),
        /// No output; continue the loop.
        Continue,
    }
    ```
  - `struct StreamContext` (新增) -- 流处理上下文，封装循环中可变状态
  - `fn process_stream_item()` (新增) -- 主分发函数，根据 QueryYield 变体调用对应 handler
  - `fn handle_assistant_message()` (L1134-1166 抽取) -- 处理 Assistant 消息
  - `fn handle_user_message()` (L1171-1205 抽取) -- 处理 User 消息
  - `fn handle_progress_message()` (L1210-1217 抽取) -- 处理 Progress 消息
  - `fn handle_system_message()` (L1222-1268 抽取) -- 处理 System 消息
  - `fn handle_attachment_message()` (L1273-1364 抽取) -- 处理 Attachment 消息
  - `fn handle_stream_event()` (L1369-1393 抽取) -- 处理 StreamEvent
  - `fn handle_request_start()` (L1398-1400 抽取) -- 处理 RequestStart 信号
  - `fn handle_tombstone()` (L1405-1435 抽取) -- 处理 Tombstone（模型回退重试）
  - `fn handle_tool_use_summary()` (L1440-1449 抽取) -- 处理 ToolUseSummary
  - `fn check_budget()` (L1454-1510 抽取) -- 每次迭代后的预算检查
- **依赖**:
  - `cc_engine::query::loop_impl::QueryYield` -- 查询循环产出类型
  - `crate::types::message::*` -- 消息类型
  - `cc_types::sdk::*` -- SDK 消息类型
  - `super::types::{AbortReason, UsageTrackingExt}` -- 终止原因、用量追踪
  - `super::SubmitTurnState` -- 提交状态
  - `crate::session::transcript` -- 转录记录
  - `crate::session::storage` -- 会话保存
  - `crate::services::langfuse` -- Langfuse 追踪
  - `super::telemetry::*` -- 遥测函数
- **被依赖**: `mod.rs`（在 Phase D 循环中调用 `process_stream_item`）

**关键设计决策**: 这是拆分中最关键的部分。原始的 Phase D 循环是一个 ~380 行的 `while let Some(item) = inner_stream.next().await { match item { ... } }` 块，每种 QueryYield 变体都有独立的 match arm。由于 `async_stream::stream!` 宏的 `yield` 语义，无法简单地将 match arm 提取为普通函数（被调用函数不能 yield）。

解决方案是引入 `StreamAction` 枚举和 `StreamContext` 结构体。每个 handler 函数返回 `Vec<StreamAction>`（通常为 0-2 个动作），主循环遍历这些动作并执行 yield/return。这样既保持了代码的可测试性（handler 可单独测试），又不改变运行时行为。

**主循环重构示例**（mod.rs 中，约 20 行替代原 ~380 行）:
```rust
while let Some(item) = inner_stream.next().await {
    let mut ctx = StreamContext { /* ... */ };
    let actions = process_stream_item(item, &mut ctx);
    let mut terminated = false;
    for action in actions {
        match action {
            StreamAction::Yield(msg) => yield msg,
            StreamAction::Terminate(result) => {
                yield result;
                terminated = true;
                break;
            }
            StreamAction::Continue => {}
        }
    }
    if terminated { return; }
    if let Some(result) = check_budget(&ctx) {
        yield result;
        return;
    }
}
```

---

### 子模块 5: `mod.rs` (~460 行)

- **职责**: 模块入口、重导出、共享类型定义、`submit_message` 主流水线编排（Phase A-pre/A/B/C/E + Phase D 协调）
- **迁移内容**:
  - 模块声明和重导出
  - `struct SubmitTurnState` + `impl` (L192-214) -- 提交轮次状态
  - 遥测辅助函数 (L645-697) -- `start_submit_telemetry`, `finish_submit_telemetry`, `start_hook_telemetry`, `finish_hook_telemetry`（~53 行，与 submit_message 流程紧耦合，保留在 mod.rs 中避免循环依赖）
  - `impl QueryEngine { pub fn submit_message() }` (L699-1599, 重构后约 200 行):
    - Phase A-pre: UserPromptSubmit hook
    - Phase A: 调用 `command_handling::handle_parsed_command`、技能处理、bash 模式
    - Phase C: SystemInit 消息、本地命令快速路径
    - Phase B: 调用 `system_prompt_build::build_submit_system_prompt`
    - Phase D setup: QueryParams 构建、API client 创建、QueryEngineDeps 组装
    - Phase D loop: 调用 `stream_handler::process_stream_item` + `check_budget`（~20 行）
    - Phase E: 结果生成

## 拆分后目录结构

```
submit_message/
├── mod.rs                 (~460 行) — 模块入口，SubmitTurnState，遥测，submit_message 主流水线
├── memory_recall.rs       (~250 行) — 记忆召回策略（确定性 + 模型辅助）
├── command_handling.rs    (~230 行) — 本地命令调度，技能调用，bash 模式
├── system_prompt_build.rs (~250 行) — 系统提示词构建，消息内容提取
└── stream_handler.rs      (~400 行) — Phase D 流式事件处理，预算检查
```

原始 1668 行拆分为 5 个模块，最大模块 ~460 行（mod.rs，作为编排中心不可避免），其余均在 230-400 行范围内。

## 迁移步骤

### 步骤 1: 创建 `submit_message/` 目录和 `mod.rs`

将 `submit_message.rs` 重命名为 `submit_message/mod.rs`（整个目录从单文件变为模块目录）。此时代码不变，只是文件路径了。验证编译通过。

```bash
mkdir crates/cc-engine/src/lifecycle/submit_message/
mv crates/cc-engine/src/lifecycle/submit_message.rs \
   crates/cc-engine/src/lifecycle/submit_message/mod.rs
```

父模块 `lifecycle/mod.rs` 中的 `mod submit_message;` 无需改动 -- Rust 编译器会自动识别目录形式的模块。

### 步骤 2: 提取 `memory_recall.rs`

提取记忆召回相关函数。这是最独立的功能域，仅被 `system_prompt_build.rs` 和测试引用。

从 `mod.rs` 中移除:
- L35-190: 6 个函数（`model_assisted_memory_recall_enabled` 等）
- L490-559: `resolve_memory_context_override` 函数
- L1655-1669: 测试模块

创建 `memory_recall.rs`，包含:
- 所需的 use 语句（`tracing::debug`, `cc_session::memdir`, `cc_api`, `tokio::time`, `std::time::Duration`）
- 上述 7 个函数 + 测试
- 所有函数标记为 `pub(super)`

在 `mod.rs` 中添加 `mod memory_recall;`，并更新 `build_submit_system_prompt` 中对 `resolve_memory_context_override` 的调用。

验证 `cargo check -p cc-engine` 编译通过。

### 步骤 3: 提取 `command_handling.rs`

提取命令处理相关函数。这些函数仅在 `submit_message` 的 Phase A 中被调用。

从 `mod.rs` 中移除:
- L216-379: `LocalCommandOutcome`, `handle_parsed_command`, `apply_command_state`, `switch_command_session`, `clear_command_session`
- L586-643: `skill_args_from_prompt`, `bash_mode_result_message`

创建 `command_handling.rs`，包含:
- 所需的 use 语句
- 上述结构体和函数
- 关键函数标记为 `pub(super)`: `handle_parsed_command`, `skill_args_from_prompt`, `bash_mode_result_message`

验证编译通过。

### 步骤 4: 提取 `system_prompt_build.rs`

提取系统提示词构建相关函数。

从 `mod.rs` 中移除:
- L381-385: `SubmitSystemPrompt` 结构体
- L391-484: `build_submit_system_prompt`
- L561-584: `fire_instructions_loaded_hook`
- L1602-1653: `latest_user_query_text`, `user_message_text`, `recent_tool_names`

创建 `system_prompt_build.rs`，包含上述结构体和函数。

验证编译通过。

### 步骤 5: 提取 `stream_handler.rs`

这是最大也最关键的提取。需要引入 `StreamAction` 枚举和 `StreamContext` 结构体。

从 `mod.rs` 中移除:
- L1129-1511: Phase D 循环的全部 match arm 逻辑

创建 `stream_handler.rs`，包含:
- `pub(super) enum StreamAction` -- 动作枚举
- `pub(super) struct StreamContext<'a>` -- 循环上下文
- `pub(super) fn process_stream_item()` -- 主分发
- 9 个 handler 函数（每个 QueryYield 变体一个）
- `pub(super) fn check_budget()` -- 预算检查

将原 Phase D 循环替换为调用 `stream_handler::process_stream_item` 的 ~20 行编排代码。

验证编译通过。

### 步骤 6: 清理和验证

- 确认 `mod.rs` 中的 use 语句已清理
- 运行 `cargo check -p cc-engine` 确认编译
- 运行 `cargo test -p cc-engine` 确认所有测试通过
- 验证每个文件的行数在目标范围内

## 模块依赖关系图

```
mod.rs (submit_message)
├── uses → memory_recall::resolve_memory_context_override (via system_prompt_build)
├── uses → command_handling::{handle_parsed_command, skill_args_from_prompt, bash_mode_result_message}
├── uses → system_prompt_build::{build_submit_system_prompt, SubmitSystemPrompt}
├── uses → stream_handler::{process_stream_item, check_budget, StreamAction, StreamContext}
│
├── memory_recall.rs
│   └── (standalone, depends on cc_session::memdir, cc_api)
│
├── command_handling.rs
│   └── depends on: super::QueryEngineState, super::types::UsageTrackingExt,
│       crate::command_runtime, crate::input_processing, crate::bootstrap
│
├── system_prompt_build.rs
│   └── depends on: memory_recall::resolve_memory_context_override,
│       crate::system_prompt, super::QueryEngineState
│
└── stream_handler.rs
    └── depends on: super::SubmitTurnState, super::types::{AbortReason, UsageTrackingExt},
        crate::session::{transcript, storage}, crate::services::langfuse,
        super::telemetry::{finish_submit_telemetry, ...}
```

## 风险与注意事项

1. **StreamAction 模式引入的复杂度**: `stream_handler.rs` 中引入 `StreamAction` 枚举是本次拆分的核心技巧。每个 handler 函数返回 `Vec<StreamAction>` 而非直接 yield。主循环需要正确处理 `Terminate` 动作的 early return 语义。建议在步骤 5 完成后重点测试所有 early-return 路径（hook blocked、local command、API error、max turns、max budget）。

2. **遥测函数的位置**: `start_submit_telemetry`、`finish_submit_telemetry` 等函数（L645-697）有 `#[cfg(feature = "telemetry")]` 和 `#[cfg(not(feature = "telemetry"))]` 两种变体。保留在 `mod.rs` 中，`stream_handler.rs` 通过 `super::` 引用它们。

3. **`#[expect(clippy::too_many_arguments)]` 警告**: `handle_parsed_command`（L230-233）和 `build_submit_system_prompt`（L387-389）都有此属性。迁移后应保留在新文件中。

4. **`UsageTracking` 的两种来源**: `UsageTracking` 来自 `cc_types::sdk::*`（L29 的 glob import），而 `UsageTrackingExt` 来自 `super::types`（L32）。迁移时每个新文件需要显式导入所需的类型。

5. **`async_stream::stream!` 宏**: `submit_message` 方法的整个函数体被包在 `async_stream::stream!` 宏中。任何在此宏作用域内的函数调用都能正常工作，但被调用的函数本身不能包含 `yield`。`StreamAction` 模式正是为了绕过这个限制。

6. **`StreamContext` 的生命周期**: `StreamContext<'a>` 包含对多个可变引用的借用，这些引用在主循环的每次迭代中都是活跃的。`process_stream_item` 函数必须接受 `&mut StreamContext<'a>` 而非拆分的独立参数，以避免借用冲突。

7. **测试覆盖**: 当前文件仅有一个简单的测试。拆分后建议为 `stream_handler.rs` 中的各 handler 函数补充单元测试。
