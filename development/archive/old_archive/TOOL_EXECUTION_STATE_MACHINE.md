# Tool Execution 状态机 — Rust 重构文档

> 基于 `cc/src` TypeScript 源码分析，面向 `cc/rust/` Rust 实现

---

## 目录

1. [概述：Tool Execution 在系统中的位置](#1-概述)
2. [顶层状态机：Query Loop](#2-顶层状态机query-loop)
3. [单个工具执行状态机](#3-单个工具执行状态机)
4. [并发编排状态机](#4-并发编排状态机)
5. [权限检查状态机](#5-权限检查状态机)
6. [Hook 系统交互](#6-hook-系统交互)
7. [Denial Tracking 状态机](#7-denial-tracking-状态机)
8. [Rust 类型映射](#8-rust-类型映射)
9. [Rust 实现设计](#9-rust-实现设计)
10. [关键实现注意事项](#10-关键实现注意事项)

---

## 1. 概述

### 在整体架构中的位置

```
QueryEngine.submitMessage()
  └─ query() generator loop          ← 顶层状态机
       ├─ API streaming              ← 收集 tool_use blocks
       ├─ StreamingToolExecutor      ← 并发编排状态机
       │    └─ runToolUse()          ← 单工具执行状态机
       │         ├─ validateInput    ← 输入验证
       │         ├─ PreToolUse hooks ← Hook 系统
       │         ├─ canUseTool       ← 权限检查状态机
       │         ├─ tool.call()      ← 实际执行
       │         └─ PostToolUse hooks
       └─ transition → next state    ← 循环继续/终止决策
```

### 源文件映射

| TypeScript 源文件 | 职责 | Rust 目标模块 |
|---|---|---|
| `query.ts` (~1729行) | 顶层 query loop 状态机 | `crate::query::loop` |
| `services/tools/toolExecution.ts` (~1600行) | 单工具执行流水线 | `crate::tool::execution` |
| `services/tools/StreamingToolExecutor.ts` (~531行) | 流式并发工具编排 | `crate::tool::executor` |
| `services/tools/toolOrchestration.ts` (~189行) | 批量并发工具编排（旧路径） | `crate::tool::orchestration` |
| `utils/permissions/permissions.ts` | 权限检查管线 | `crate::permission` |
| `utils/permissions/denialTracking.ts` | 拒绝计数追踪 | `crate::permission::denial` |
| `hooks/useCanUseTool.tsx` | 权限 UI 协调 | `crate::permission::interactive` |
| `Tool.ts` (~793行) | Tool trait 定义 | `crate::tool::types` |

---

## 2. 顶层状态机：Query Loop

### 2.1 状态定义

```
TS: src/query.ts:202-217

type State = {
  messages: Message[]
  toolUseContext: ToolUseContext
  autoCompactTracking: AutoCompactTrackingState | undefined
  maxOutputTokensRecoveryCount: number
  hasAttemptedReactiveCompact: boolean
  maxOutputTokensOverride: number | undefined
  pendingToolUseSummary: Promise<ToolUseSummaryMessage | null> | undefined
  stopHookActive: boolean | undefined
  turnCount: number
  transition: Continue | undefined
}
```

### 2.2 状态转换图

```
                     ┌──────────────┐
                     │  LOOP ENTRY  │
                     │  (初始化)     │
                     └──────┬───────┘
                            │
                     ┌──────▼───────┐
                     │  SETUP       │
                     │  解构 state  │
                     │  prefetch    │
                     │  queryChain  │
                     └──────┬───────┘
                            │
                     ┌──────▼───────────┐
                     │ CONTEXT MGMT     │
                     │ toolResultBudget │
                     │ snipCompact      │
                     │ microcompact     │
                     │ contextCollapse  │
                     │ autoCompact      │
                     └──────┬───────────┘
                            │
                     ┌──────▼───────────────┐
                     │  API STREAMING        │
                     │  callModel()          │
                     │  收集 assistant msgs  │
                     │  收集 tool_use blocks │
                     │  StreamingToolExec    │
                     └──────┬───────────────┘
                            │
              ┌─────────────┼─────────────┐
              │             │             │
         [aborted]    [no tool_use]   [has tool_use]
              │             │             │
              ▼             ▼             ▼
        ┌──────────┐  ┌──────────┐  ┌───────────────┐
        │ TERMINAL │  │ RECOVERY │  │ TOOL EXECUTION│
        │ aborted_ │  │ CHECKS   │  │ runTools /    │
        │ streaming│  │          │  │ getRemainingR │
        └──────────┘  └────┬─────┘  └───────┬───────┘
                           │                │
              ┌────────────┼────────┐       │
              │            │        │       │
     [withhold413]  [maxOutput] [normal]    │
              │            │        │       │
              ▼            ▼        ▼       │
     ┌──────────────┐  ┌────────┐  │       │
     │collapse_drain│  │escalate│  │       │
     │reactive_comp │  │recovery│  │       │
     │ → CONTINUE   │  │→ CONT  │  │       │
     └──────────────┘  └────────┘  │       │
                                   │       │
                           ┌───────▼───┐   │
                           │STOP HOOKS │   │
                           └─────┬─────┘   │
                                 │         │
                    ┌────────────┼─────┐   │
                    │            │     │   │
              [prevented]  [blocking] [ok] │
                    │            │     │   │
                    ▼            ▼     ▼   │
              ┌────────┐  ┌───────┐  ┌──┐ │
              │TERMINAL│  │CONTIN │  │  │ │
              │hook_   │  │stop_  │  │  │ │
              │stopped │  │hook_  │  │  │ │
              └────────┘  │block  │  │  │ │
                          └───────┘  │  │ │
                                     │  │ │
                    ┌────────────────┬┘  │ │
                    │ TOKEN BUDGET   │   │ │
                    │ (if enabled)   │   │ │
                    └──────┬─────────┘   │ │
                           │             │ │
                  ┌────────┼───────┐     │ │
                  │        │       │     │ │
              [continue] [stop]  [n/a]   │ │
                  │        │       │     │ │
                  ▼        ▼       ▼     │ │
           ┌─────────┐ ┌──────┐   │     │ │
           │CONTINUE │ │TERM  │   │     │ │
           │token_   │ │compl │   │     │ │
           │budget   │ │eted  │   │     │ │
           └─────────┘ └──────┘   │     │ │
                                  └──┬──┘ │
                              ┌──────▼────▼──────┐
                              │ POST-TOOL        │
                              │ attachments      │
                              │ memory prefetch  │
                              │ skill discovery  │
                              │ maxTurns check   │
                              └──────┬───────────┘
                                     │
                              ┌──────▼───────┐
                              │  CONTINUE    │
                              │  next_turn   │
                              │  turnCount++ │
                              └──────────────┘
```

### 2.3 Terminal Reasons (终止原因)

从 query.ts 提取的所有 `return` 点:

| reason | 触发条件 | 行号 |
|---|---|---|
| `blocking_limit` | token 已达阻塞上限 | ~647 |
| `image_error` | 图片大小/格式错误 | ~977 |
| `model_error` | API 调用异常 | ~996 |
| `aborted_streaming` | 流式阶段中断 | ~1051 |
| `prompt_too_long` | PTL 恢复失败 | ~1175 |
| `completed` | 正常完成（无 tool_use） | ~1357 |
| `stop_hook_prevented` | 停止钩子阻止 | ~1279 |
| `hook_stopped` | 工具钩子阻止 | ~1521 |
| `aborted_tools` | 工具执行阶段中断 | ~1515 |
| `max_turns` | 达到最大轮次限制 | ~1711 |

### 2.4 Continue Reasons (继续原因)

从 query.ts 提取的所有 `state = {...}; continue` 点:

| reason | 触发条件 |
|---|---|
| `collapse_drain_retry` | 上下文折叠排空后重试 |
| `reactive_compact_retry` | 响应式压缩后重试 |
| `max_output_tokens_escalate` | 输出 token 上限提升重试 |
| `max_output_tokens_recovery` | 注入恢复消息后继续 |
| `stop_hook_blocking` | 停止钩子阻塞错误后重试 |
| `token_budget_continuation` | token 预算允许继续 |
| `next_turn` | 正常的工具执行后下一轮 |

---

## 3. 单个工具执行状态机

### 3.1 执行流水线

```
TS: services/tools/toolExecution.ts

runToolUse(toolUse, assistantMsg, canUseTool, ctx)
  │
  ├─ 1. TOOL LOOKUP
  │    ├─ findToolByName(available tools)
  │    ├─ fallback: findToolByName(all tools) via alias
  │    └─ not found → yield error result, return
  │
  ├─ 2. ABORT CHECK
  │    └─ signal.aborted → yield CANCEL_MESSAGE, return
  │
  └─ 3. streamedCheckPermissionsAndCallTool()
       │
       ├─ 3a. INPUT VALIDATION (Zod schema)
       │    └─ parsedInput.success === false → yield InputValidationError
       │
       ├─ 3b. INPUT SEMANTIC VALIDATION
       │    └─ tool.validateInput?() → false → yield error
       │
       ├─ 3c. SPECULATIVE CLASSIFIER (Bash only)
       │    └─ startSpeculativeClassifierCheck()
       │
       ├─ 3d. INPUT PROCESSING
       │    ├─ strip _simulatedSedEdit (defense-in-depth)
       │    └─ backfillObservableInput (clone for hooks)
       │
       ├─ 3e. PRE-TOOL HOOKS
       │    ├─ runPreToolUseHooks() → yields:
       │    │   ├─ 'message' → progress / attachment
       │    │   ├─ 'hookPermissionResult' → override permission
       │    │   ├─ 'hookUpdatedInput' → update input
       │    │   ├─ 'preventContinuation' → flag
       │    │   ├─ 'stopReason' → flag
       │    │   ├─ 'additionalContext' → extra msg
       │    │   └─ 'stop' → yield error, RETURN EARLY
       │    └─ emit hook timing summary (if >500ms)
       │
       ├─ 3f. PERMISSION CHECK
       │    ├─ resolveHookPermissionDecision()
       │    │   ├─ hookPermissionResult exists? → use it
       │    │   └─ otherwise → canUseTool() → full permission pipeline
       │    │
       │    ├─ behavior === 'allow' → proceed to 3g
       │    ├─ behavior === 'deny' / 'ask':
       │    │   ├─ yield error tool_result
       │    │   ├─ if classifier denial → PermissionDenied hooks
       │    │   │   └─ hook says retry? → yield retry hint
       │    │   └─ RETURN
       │    └─ updatedInput → apply
       │
       ├─ 3g. TOOL EXECUTION
       │    ├─ resolve callInput (backfill vs hook vs original)
       │    ├─ tool.call(callInput, ctx, canUseTool, msg, onProgress)
       │    ├─ → ToolResult { data, newMessages?, contextModifier?, mcpMeta? }
       │    ├─ mapToolResultToToolResultBlockParam → API format
       │    └─ processToolResultBlock → size budget enforcement
       │
       ├─ 3h. POST-TOOL HOOKS
       │    ├─ runPostToolUseHooks() → yields:
       │    │   ├─ updatedMCPToolOutput (MCP only)
       │    │   ├─ attachment messages
       │    │   └─ hook_stopped_continuation
       │    └─ PostToolUse failure hooks (on error)
       │
       └─ 3i. RESULT ASSEMBLY
            ├─ tool_result block (+ acceptFeedback, + contentBlocks)
            ├─ newMessages from tool
            ├─ contextModifier
            └─ return MessageUpdateLazy[]
```

### 3.2 工具执行状态枚举

```
Single Tool States:
  ┌─────────┐
  │ LOOKUP  │──not found──→ ERROR (unknown tool)
  └────┬────┘
       │ found
  ┌────▼──────┐
  │ ABORT_CHK │──aborted──→ CANCELLED
  └────┬──────┘
       │ alive
  ┌────▼───────────┐
  │ INPUT_VALIDATE │──invalid──→ ERROR (validation)
  └────┬───────────┘
       │ valid
  ┌────▼──────────┐
  │ SEMANTIC_VALID│──invalid──→ ERROR (semantic)
  └────┬──────────┘
       │ valid
  ┌────▼──────────┐
  │ PRE_HOOKS    │──stop──→ STOPPED (by hook)
  └────┬──────────┘
       │ continue
  ┌────▼──────────────┐
  │ PERMISSION_CHECK  │──deny/ask──→ DENIED
  └────┬──────────────┘
       │ allow
  ┌────▼──────────┐
  │ EXECUTING    │──error──→ ERROR (runtime)
  └────┬──────────┘
       │ success
  ┌────▼──────────┐
  │ POST_HOOKS   │
  └────┬──────────┘
       │
  ┌────▼──────────┐
  │ COMPLETED    │
  └───────────────┘
```

---

## 4. 并发编排状态机

### 4.1 StreamingToolExecutor (新路径)

```
TS: services/tools/StreamingToolExecutor.ts

TrackedTool 状态: 'queued' | 'executing' | 'completed' | 'yielded'
```

**并发规则**:
- `isConcurrencySafe === true` 的工具可以与其他 concurrent-safe 工具并行
- `isConcurrencySafe === false` 的工具必须独占执行
- 结果按接收顺序缓冲并发出

```
状态转换:

  addTool(block)
  ┌──────────┐
  │ QUEUED   │─── tool not found ──→ COMPLETED (error result)
  └────┬─────┘
       │ canExecuteTool() === true
       │
  ┌────▼──────────┐
  │ EXECUTING     │
  │               │
  │  检查 abort:  │
  │  ├─ sibling_error → synthetic error → COMPLETED
  │  ├─ user_interrupted → synthetic error → COMPLETED
  │  ├─ streaming_fallback → synthetic error → COMPLETED
  │  │
  │  runToolUse():│
  │  ├─ progress → pendingProgress (即时通知)
  │  ├─ result → messages buffer
  │  ├─ Bash error → hasErrored = true
  │  │              siblingAbort.abort('sibling_error')
  │  └─ contextModifier → 排队应用 (非并发工具)
  │                │
  └────────────────┘
       │ promise resolved
       │
  ┌────▼──────────┐
  │ COMPLETED     │
  └────┬──────────┘
       │ getCompletedResults() / getRemainingResults()
       │
  ┌────▼──────────┐
  │ YIELDED       │
  └───────────────┘
```

**Sibling Abort 机制**:
- 仅 Bash 工具错误触发 sibling abort
- 其他工具 (Read/WebFetch/etc) 错误不影响兄弟工具
- `siblingAbortController` 是 `toolUseContext.abortController` 的子控制器

### 4.2 toolOrchestration (旧路径 / fallback)

```
TS: services/tools/toolOrchestration.ts

partitionToolCalls(blocks) → Batch[]

Batch { isConcurrencySafe: boolean, blocks: ToolUseBlock[] }

连续的 concurrent-safe 工具合并为一个批次
非 concurrent-safe 工具单独成批

执行:
  for each batch:
    if concurrent → runToolsConcurrently(blocks) // Promise.all + 限流
    else          → runToolsSerially(blocks)     // 逐个执行

  最大并发度: CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY || 10
```

---

## 5. 权限检查状态机

### 5.1 权限检查管线

```
TS: utils/permissions/permissions.ts

hasPermissionsToUseTool()
  └─ hasPermissionsToUseToolInner()
       │
       ├─ PHASE 1: IMMEDIATE CHECKS (fail-fast)
       │   ├─ 1a. 整个工具被 deny rule 拒绝 → DENY
       │   ├─ 1b. 整个工具有 ask rule → ASK (除非沙箱 bash 自动允许)
       │   ├─ 1c. tool.checkPermissions() → tool-specific check
       │   ├─ 1d. tool 实现拒绝 → DENY
       │   ├─ 1e. tool 需要用户交互 → ASK (即使 bypass 模式)
       │   ├─ 1f. 内容级 ask rule → ASK
       │   └─ 1g. 安全检查 → ASK (bypass-immune)
       │
       ├─ PHASE 2: BYPASS & ALLOW
       │   ├─ 2a. bypassPermissions / (plan + bypass可用) → ALLOW
       │   └─ 2b. 整个工具在 alwaysAllowRules → ALLOW
       │
       └─ PHASE 3: MODE TRANSFORM (外层)
            │
            ├─ behavior === 'ask':
            │   ├─ mode === 'dontAsk' → DENY
            │   ├─ mode === 'auto' → 运行分类器
            │   │   ├─ shouldBlock → DENY (记录 denial)
            │   │   ├─ !shouldBlock → ALLOW (记录 success)
            │   │   ├─ unavailable + iron_gate → DENY (fail-closed)
            │   │   ├─ unavailable + !gate → ASK (fail-open)
            │   │   └─ denial limit exceeded → ASK (回退到提示)
            │   └─ shouldAvoidPermissionPrompts →
            │       ├─ run hooks → ALLOW/DENY
            │       └─ null → DENY
            │
            └─ behavior !== 'ask' → 直接返回
```

### 5.2 PermissionResult 类型

```
TS: types/permissions.ts

PermissionDecision =
  | { behavior: 'allow', updatedInput, userModified?, acceptFeedback?,
      contentBlocks?, decisionReason? }
  | { behavior: 'ask', message, suggestions?, blockedPath?,
      pendingClassifierCheck?, contentBlocks? }
  | { behavior: 'deny', message, decisionReason }

PermissionResult = PermissionDecision
  | { behavior: 'passthrough', message }  // internal only, converted to 'ask'

DecisionReason =
  | { type: 'rule', rule }
  | { type: 'mode', mode }
  | { type: 'hook', hookName, hookSource?, reason? }
  | { type: 'classifier', classifier, reason }
  | { type: 'asyncAgent', reason }
  | { type: 'sandboxOverride', reason }
  | { type: 'safetyCheck', reason, classifierApprovable }
  | { type: 'workingDir', reason }
  | { type: 'subcommandResults', reasons }
  | { type: 'permissionPromptTool', permissionPromptToolName }
  | { type: 'other', reason }
```

### 5.3 Permission Mode 状态机

```
Available Modes:
  default       ── 交互式提示
  bypassPermissions ── 自动允许 (除 bypass-immune 检查外)
  acceptEdits   ── 编辑操作快速通道
  dontAsk       ── ask → deny 转换
  plan          ── 默认行为 + 可选 bypass 提升
  auto          ── 分类器代替交互 (需要 TRANSCRIPT_CLASSIFIER feature)

Mode 对 'ask' 结果的影响:
  default     → 显示权限对话框
  bypass      → (ask 不会到达这里，phase 2 已 allow)
  acceptEdits → 编辑工具自动允许
  dontAsk     → 转换为 deny
  plan        → 与 default 相同 (或提升为 bypass)
  auto        → 运行分类器 → allow/deny/fallback-to-ask
```

---

## 6. Hook 系统交互

### 6.1 工具执行相关 Hook 事件

| Hook 事件 | 时机 | 可能结果 |
|---|---|---|
| `PreToolUse` | 工具执行前 | stop, preventContinuation, updateInput, permissionDecision |
| `PostToolUse` | 工具执行后 | updatedMCPToolOutput, hook_stopped_continuation |
| `PostToolUseFailure` | 工具执行失败后 | 通知性 |
| `PermissionRequest` | 权限检查时 | allow, deny |
| `PermissionDenied` | 自动模式拒绝后 | retry hint |
| `Stop` | 轮次结束时 | blockingErrors, preventContinuation |
| `StopFailure` | API 错误终止时 | 通知性 |
| `PostSampling` | 模型响应完成后 | 通知性 |

### 6.2 交互式权限处理器 — 5路竞争

```
TS: hooks/toolPermission/handlers/interactiveHandler.ts

5 个异步源竞争 (ResolveOnce 保证 exactly-one 语义):

  1. User Dialog     ── onAllow / onReject / onAbort
  2. Hooks           ── PermissionRequest 异步执行
  3. Bash Classifier ── bash 允许/拒绝分类
  4. Bridge          ── 远程 (claude.ai CCR) 批准
  5. Channel         ── 远程 (Telegram/iMessage) 批准

  ResolveOnce { claim() → boolean }
    ├─ 第一个调用 claim() 的源获胜
    ├─ 后续调用者检查 isResolved() 并提前返回
    └─ 保证原子性: check + mark in one operation
```

---

## 7. Denial Tracking 状态机

```
TS: utils/permissions/denialTracking.ts

State:
  { consecutiveDenials: number, totalDenials: number }

Initial: { 0, 0 }

Transitions:
  recordDenial()  → { consecutive + 1, total + 1 }
  recordSuccess() → { consecutive = 0, total (unchanged) }

Limits:
  maxConsecutive: 3   ── 连续 3 次分类器拒绝后阻塞
  maxTotal: 20        ── 总计 20 次拒绝后阻塞

shouldFallbackToPrompting():
  consecutive >= 3 || total >= 20

Limit Exceeded:
  interactive mode → 回退到用户提示
  headless mode    → throw AbortError (终止 agent)

Persistence:
  main agent     → appState.denialTracking
  async subagent → context.localDenialTracking (mutable in-place)
```

---

## 8. Rust 类型映射

### 8.1 核心 Trait

```rust
/// 对应 TS Tool<Input, Output, P> — src/Tool.ts:362-695
#[async_trait]
pub trait Tool: Send + Sync {
    /// 工具名
    fn name(&self) -> &str;

    /// 别名 (向后兼容重命名)
    fn aliases(&self) -> &[&str] { &[] }

    /// 是否启用
    fn is_enabled(&self) -> bool { true }

    /// 是否并发安全
    fn is_concurrency_safe(&self, input: &ToolInput) -> bool { false }

    /// 是否只读
    fn is_read_only(&self, input: &ToolInput) -> bool { false }

    /// 是否破坏性
    fn is_destructive(&self, input: &ToolInput) -> bool { false }

    /// 中断行为
    fn interrupt_behavior(&self) -> InterruptBehavior { InterruptBehavior::Block }

    /// 结果最大字符数
    fn max_result_size_chars(&self) -> usize;

    /// 验证输入
    async fn validate_input(&self, input: &ToolInput, ctx: &ToolUseContext)
        -> ValidationResult { ValidationResult::Ok }

    /// 检查权限 (工具特定逻辑)
    async fn check_permissions(&self, input: &ToolInput, ctx: &ToolUseContext)
        -> PermissionResult;

    /// 执行工具
    async fn call(
        &self,
        input: ToolInput,
        ctx: &mut ToolUseContext,
        progress: &dyn ProgressSender,
    ) -> Result<ToolResult, ToolError>;

    /// 将结果映射为 API 格式
    fn map_result_to_block(&self, data: &ToolOutput, tool_use_id: &str)
        -> ToolResultBlockParam;

    /// 用户可见名称
    fn user_facing_name(&self, input: Option<&ToolInput>) -> String {
        self.name().to_string()
    }
}
```

### 8.2 核心枚举

```rust
/// 工具执行状态 — 对应 StreamingToolExecutor 的 ToolStatus
#[derive(Debug, Clone, PartialEq)]
pub enum ToolStatus {
    Queued,
    Executing,
    Completed,
    Yielded,
}

/// 中断行为 — 对应 Tool.interruptBehavior
#[derive(Debug, Clone, PartialEq)]
pub enum InterruptBehavior {
    Cancel,
    Block,
}

/// 取消原因 — 对应 StreamingToolExecutor.getAbortReason
#[derive(Debug, Clone)]
pub enum AbortReason {
    SiblingError,
    UserInterrupted,
    StreamingFallback,
}

/// 权限决策 — 对应 PermissionDecision
#[derive(Debug, Clone)]
pub enum PermissionDecision {
    Allow {
        updated_input: Option<ToolInput>,
        user_modified: bool,
        accept_feedback: Option<String>,
        decision_reason: Option<DecisionReason>,
    },
    Ask {
        message: String,
        pending_classifier_check: Option<ClassifierFuture>,
    },
    Deny {
        message: String,
        decision_reason: Option<DecisionReason>,
    },
}

/// 权限决策原因 — 对应 PermissionDecisionReason
#[derive(Debug, Clone)]
pub enum DecisionReason {
    Rule { rule: PermissionRule },
    Mode { mode: PermissionMode },
    Hook { hook_name: String, reason: Option<String> },
    Classifier { classifier: String, reason: String },
    AsyncAgent { reason: String },
    SandboxOverride { reason: String },
    SafetyCheck { reason: String, classifier_approvable: bool },
    WorkingDir { reason: String },
    Other { reason: String },
}

/// 权限模式 — 对应 INTERNAL_PERMISSION_MODES
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionMode {
    Default,
    BypassPermissions,
    AcceptEdits,
    DontAsk,
    Plan,
    Auto,
}

/// Query Loop 终止原因 — 对应 Terminal
#[derive(Debug)]
pub enum TerminalReason {
    Completed,
    BlockingLimit,
    ImageError,
    ModelError { error: Box<dyn std::error::Error + Send> },
    AbortedStreaming,
    AbortedTools,
    PromptTooLong,
    StopHookPrevented,
    HookStopped,
    MaxTurns { turn_count: usize },
}

/// Query Loop 继续原因 — 对应 Continue
#[derive(Debug)]
pub enum ContinueReason {
    NextTurn,
    CollapseDrainRetry { committed: usize },
    ReactiveCompactRetry,
    MaxOutputTokensEscalate,
    MaxOutputTokensRecovery { attempt: usize },
    StopHookBlocking,
    TokenBudgetContinuation,
}

/// 输入验证结果 — 对应 ValidationResult
pub enum ValidationResult {
    Ok,
    Error { message: String, error_code: i32 },
}

/// 工具执行结果 — 对应 ToolResult<T>
pub struct ToolResult {
    pub data: ToolOutput,
    pub new_messages: Vec<Message>,
    pub context_modifier: Option<Box<dyn FnOnce(&mut ToolUseContext)>>,
}
```

### 8.3 Query Loop State

```rust
/// 对应 query.ts 的 State struct
pub struct QueryLoopState {
    pub messages: Vec<Message>,
    pub tool_use_context: ToolUseContext,
    pub auto_compact_tracking: Option<AutoCompactTracking>,
    pub max_output_tokens_recovery_count: u32,
    pub has_attempted_reactive_compact: bool,
    pub max_output_tokens_override: Option<u32>,
    pub stop_hook_active: Option<bool>,
    pub turn_count: u32,
    pub transition: Option<ContinueReason>,
}
```

### 8.4 Denial Tracking

```rust
/// 对应 DenialTrackingState
#[derive(Debug, Clone)]
pub struct DenialTracking {
    pub consecutive_denials: u32,
    pub total_denials: u32,
}

impl DenialTracking {
    const MAX_CONSECUTIVE: u32 = 3;
    const MAX_TOTAL: u32 = 20;

    pub fn new() -> Self {
        Self { consecutive_denials: 0, total_denials: 0 }
    }

    pub fn record_denial(&mut self) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
    }

    pub fn record_success(&mut self) {
        self.consecutive_denials = 0;
        // total_denials is NOT reset
    }

    pub fn should_fallback_to_prompting(&self) -> bool {
        self.consecutive_denials >= Self::MAX_CONSECUTIVE
            || self.total_denials >= Self::MAX_TOTAL
    }
}
```

---

## 9. Rust 实现设计

### 9.1 Generator → Stream 映射

TS 使用 `AsyncGenerator` 作为核心模式。Rust 映射为 `Stream`:

```rust
use tokio_stream::Stream;
use pin_project_lite::pin_project;

/// 对应 query() 生成器
pub fn query(params: QueryParams) -> impl Stream<Item = QueryEvent> {
    try_stream! {
        let mut state = QueryLoopState::new(params);
        loop {
            // SETUP
            let setup = state.setup()?;

            // CONTEXT MANAGEMENT
            state.manage_context().await?;

            // API STREAMING
            let streaming_result = state.stream_api_call().await?;

            // TOOL EXECUTION
            match streaming_result {
                StreamResult::NoToolUse => {
                    let terminal = state.handle_no_tool_use().await?;
                    match terminal {
                        LoopDecision::Continue(reason) => {
                            state.transition = Some(reason);
                            continue;
                        }
                        LoopDecision::Terminal(reason) => {
                            return Ok(reason);
                        }
                    }
                }
                StreamResult::HasToolUse(blocks) => {
                    state.execute_tools(blocks).await?;

                    // POST-TOOL
                    if state.check_abort()? { return ...; }
                    if state.check_hooks()? { return ...; }
                    if state.check_max_turns()? { return ...; }

                    state.transition = Some(ContinueReason::NextTurn);
                    state.turn_count += 1;
                }
                StreamResult::Aborted => {
                    return Ok(TerminalReason::AbortedStreaming);
                }
            }
        }
    }
}
```

### 9.2 StreamingToolExecutor 设计

```rust
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinSet;

pub struct StreamingToolExecutor {
    tools: Vec<TrackedTool>,
    tool_definitions: Arc<ToolRegistry>,
    context: ToolUseContext,
    has_errored: AtomicBool,
    sibling_cancel: CancellationToken,
    discarded: AtomicBool,
    progress_notify: Arc<Notify>,
}

struct TrackedTool {
    id: String,
    block: ToolUseBlock,
    assistant_message: AssistantMessage,
    status: Mutex<ToolStatus>,
    is_concurrency_safe: bool,
    handle: Option<JoinHandle<Vec<Message>>>,
    pending_progress: Mutex<Vec<Message>>,
}

impl StreamingToolExecutor {
    /// 添加工具到执行队列
    pub fn add_tool(&mut self, block: ToolUseBlock, msg: AssistantMessage) {
        // ... 入队 + 尝试处理队列
    }

    /// 获取已完成的结果 (非阻塞)
    pub fn get_completed_results(&mut self) -> impl Iterator<Item = MessageUpdate> {
        // ... 按顺序 yield 已完成的结果
    }

    /// 等待剩余结果 (异步)
    pub fn get_remaining_results(&mut self) -> impl Stream<Item = MessageUpdate> {
        // ... 等待所有工具完成
    }

    /// 并发安全检查
    fn can_execute_tool(&self, is_concurrency_safe: bool) -> bool {
        let executing: Vec<_> = self.tools.iter()
            .filter(|t| *t.status.lock() == ToolStatus::Executing)
            .collect();

        executing.is_empty()
            || (is_concurrency_safe && executing.iter().all(|t| t.is_concurrency_safe))
    }
}
```

### 9.3 权限检查模块

```rust
/// 对应 hasPermissionsToUseTool
pub async fn check_tool_permission(
    tool: &dyn Tool,
    input: &ToolInput,
    ctx: &ToolUseContext,
    app_state: &AppState,
) -> PermissionDecision {
    // Phase 1: Immediate checks
    if let Some(deny) = get_deny_rule(app_state, tool) {
        return PermissionDecision::Deny { ... };
    }

    if let Some(ask) = get_ask_rule(app_state, tool) {
        // sandbox bash auto-allow exception
        ...
    }

    let tool_result = tool.check_permissions(input, ctx).await;
    // ... Phase 1c-1g

    // Phase 2: Bypass & Allow
    if should_bypass(app_state) {
        return PermissionDecision::Allow { ... };
    }

    if let Some(allow) = get_allow_rule(app_state, tool) {
        return PermissionDecision::Allow { ... };
    }

    // Phase 3: Mode transforms
    match app_state.permission_mode {
        PermissionMode::DontAsk => PermissionDecision::Deny { ... },
        PermissionMode::Auto => run_classifier(tool, input, ctx).await,
        _ => PermissionDecision::Ask { ... }, // → interactive handler
    }
}
```

### 9.4 CancellationToken 层次

```rust
/// 对应 TypeScript 的 AbortController 嵌套
///
/// query.abortController (顶层, 用户中断)
///   └─ executor.siblingAbortController (sibling 错误)
///        └─ tool.toolAbortController (单工具)
///             └─ tool abort 冒泡到 query (除了 sibling_error)

use tokio_util::sync::CancellationToken;

pub struct AbortHierarchy {
    pub query_token: CancellationToken,
    pub sibling_token: CancellationToken,  // child of query_token
}

impl AbortHierarchy {
    pub fn new(query_token: CancellationToken) -> Self {
        Self {
            sibling_token: query_token.child_token(),
            query_token,
        }
    }

    pub fn tool_token(&self) -> CancellationToken {
        self.sibling_token.child_token()
    }
}
```

---

## 10. 关键实现注意事项

### 10.1 执行顺序保证

- **结果必须按工具接收顺序发出**，即使并发执行。StreamingToolExecutor 通过 tools Vec 的顺序遍历保证这一点。
- 非并发安全工具遇到时，必须停止检查后续工具（即使后续是并发安全的）。
- Rust 实现中使用 `Vec<TrackedTool>` + 顺序遍历即可保持此语义。

### 10.2 Bash 错误级联

- **仅** Bash 工具错误会取消兄弟工具。Read/WebFetch/Glob 等工具的错误不会。
- 原因：Bash 命令通常有隐式依赖链（mkdir 失败 → 后续命令无意义）。
- Rust：在 `TrackedTool` 完成处理时，检查 `tool_name == "Bash"` 再决定是否 cancel siblings。

### 10.3 ToolAbortController 冒泡

- 权限对话框拒绝时会 abort 单工具的 controller
- 该 abort 必须冒泡到 query controller（否则 ExitPlanMode 等场景会回退到发送 REJECT_MESSAGE 而非正确中止）
- 但 `sibling_error` 原因的 abort **不应**冒泡到 query controller
- Rust：在 tool_token 的 cancel callback 中检查原因

### 10.4 Passthrough → Ask 转换

- 工具的 `checkPermissions()` 返回 `passthrough` 表示"无意见"
- 在外层自动转换为 `ask` + 生成适当的提示消息
- Rust：直接在 `PermissionResult` 枚举中不暴露 passthrough，在内部处理

### 10.5 Context Modifier 限制

- Context modifier 目前仅支持非并发工具
- 并发工具的 modifier 被排队但不应用（当前 TS 代码注释明确说明）
- Rust：在 `StreamingToolExecutor` 中仅对 `is_concurrency_safe == false` 应用 modifier

### 10.6 本地优先实现顺序

Phase 3（本文档对应的实现阶段）应按以下顺序实现：

1. **Tool trait + ToolRegistry** — 核心类型定义
2. **ToolInput validation** — Zod → serde + jsonschema
3. **PermissionDecision / PermissionMode** — 权限枚举（无网络依赖）
4. **DenialTracking** — 纯状态机，无依赖
5. **单工具执行管线** — `runToolUse` 等价物
6. **StreamingToolExecutor** — 并发编排器
7. **权限检查管线** — `hasPermissionsToUseTool`（分类器部分跳过，属于网络阶段）
8. **Hook 系统** — PreToolUse / PostToolUse（本地 hook 执行器）
9. **集成到 query loop** — 将工具执行接入主循环

### 10.7 可跳过 / 延后的部分

| 组件 | 原因 |
|---|---|
| 分类器 (transcript classifier) | 需要网络 API 调用 |
| Bridge / Channel 权限处理器 | 需要远程连接 |
| MCP 工具 | 需要 MCP 协议栈 |
| ToolSearch / 延迟加载 | 优化特性，非核心 |
| 工具使用摘要 (Haiku) | 需要 API |
| OTel / 遥测 | 可观测性，非功能性 |
| Speculative classifier | 优化，依赖分类器 |

---

## 附录 A: 工具并发安全性参考

从源码提取的各工具 `isConcurrencySafe` 默认/实现：

| 工具 | 并发安全 | 原因 |
|---|---|---|
| `Bash` | 条件性 | 只读命令 (grep/find/cat 等) 安全 |
| `FileRead` (Read) | true | 纯读取 |
| `FileEdit` (Edit) | false | 写入文件 |
| `FileWrite` (Write) | false | 写入文件 |
| `Glob` | true | 纯搜索 |
| `Grep` | true | 纯搜索 |
| `WebFetch` | true | 无本地副作用 |
| `WebSearch` | true | 无本地副作用 |
| `Agent` | false | 启动子 agent |
| `Skill` | false | 可能修改状态 |
| `LSP` | 条件性 | 取决于操作类型 |
| `NotebookEdit` | false | 写入文件 |
| `AskUserQuestion` | false | 需要用户交互 |
| 默认 (buildTool) | false | 安全默认 |

## 附录 B: Message 类型在工具执行中的角色

```
工具执行产生的 Message 类型:

user (tool_result):
  ├─ 成功结果:   { type: 'tool_result', content: <result>, is_error: false }
  ├─ 错误结果:   { type: 'tool_result', content: '<tool_use_error>...', is_error: true }
  ├─ 权限拒绝:   { type: 'tool_result', content: <deny_msg>, is_error: true }
  ├─ 取消:       { type: 'tool_result', content: CANCEL_MESSAGE, is_error: true }
  └─ 兄弟取消:   { type: 'tool_result', content: 'Cancelled: ...', is_error: true }

progress:
  └─ tool-specific progress data

attachment:
  ├─ hook_permission_decision
  ├─ hook_stopped_continuation
  ├─ edited_text_file (file change tracking)
  ├─ max_turns_reached
  └─ structured_output

system:
  └─ 模型降级通知
```
