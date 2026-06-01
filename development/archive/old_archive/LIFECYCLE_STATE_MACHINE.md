# 生命周期状态机 — Rust 重构参考文档

> 基于 `cc/src/` 源码逆向工程，描述 Claude Code 从进程启动到退出的完整状态转换图。
> 用于指导 Rust 重写中 `async Stream` 状态机的设计。

---

## 目录

1. [全局生命周期总览](#1-全局生命周期总览)
2. [Phase A: 进程启动与快速路径](#2-phase-a-进程启动与快速路径)
3. [Phase B: 完整初始化](#3-phase-b-完整初始化)
4. [Phase C: QueryEngine 会话管理器](#4-phase-c-queryengine-会话管理器)
5. [Phase D: query() 主循环状态机 (核心)](#5-phase-d-query-主循环状态机-核心)
6. [Phase E: 工具执行管线](#6-phase-e-工具执行管线)
7. [Phase F: 权限决策状态机](#7-phase-f-权限决策状态机)
8. [Phase G: 上下文管理管线](#8-phase-g-上下文管理管线)
9. [Phase H: 停止钩子与后处理](#9-phase-h-停止钩子与后处理)
10. [Phase I: 关闭与清理](#10-phase-i-关闭与清理)
11. [Rust 映射方案](#11-rust-映射方案)
12. [附录: 完整类型定义](#12-附录-完整类型定义)

---

## 1. 全局生命周期总览

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         PROCESS LIFECYCLE                                │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  [A] cli.tsx ──fast-path?──→ 立即退出 (--version, --daemon-worker, ...)  │
│       │                                                                  │
│       │ 标准路径                                                          │
│       ▼                                                                  │
│  [B] main.tsx ── 完整初始化 (auth, settings, MCP, plugins, state)        │
│       │                                                                  │
│       ▼                                                                  │
│  ┌─── REPL 循环 ─────────────────────────────────────────────────┐       │
│  │                                                                │       │
│  │  用户输入 → [C] QueryEngine.submitMessage()                    │       │
│  │                    │                                            │       │
│  │                    ▼                                            │       │
│  │              [D] query() 主循环 (while true)                    │       │
│  │              ┌───────────────────────────────┐                  │       │
│  │              │ Setup → Context → API Call    │                  │       │
│  │              │    → Decision → Tools         │                  │       │
│  │              │    → Attachments → Continue    │                  │       │
│  │              └──────────┬────────────────────┘                  │       │
│  │                         │ Terminal                               │       │
│  │                         ▼                                       │       │
│  │              [H] Stop Hooks → Result                            │       │
│  │                         │                                       │       │
│  │                         ▼                                       │       │
│  │              等待下一次用户输入                                    │       │
│  │                                                                │       │
│  └────────────────────────────────────────────────────────────────┘       │
│                                                                          │
│  [I] 关闭 (Ctrl+C / 正常退出)                                            │
│       → MCP cleanup → abort signals → cursor reset → exit                │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Phase A: 进程启动与快速路径

**源文件**: `src/entrypoints/cli.tsx`

进程启动后立即进入快速路径检测。目标: 为简单操作避免加载数百个模块。

```
process.argv
    │
    ├─ --version / -v / -V ──────→ print(MACRO.VERSION) → exit(0)
    │   [零模块加载]
    │
    ├─ --dump-system-prompt ─────→ 加载 config + model → 渲染 prompt → exit(0)
    │   [仅 Ant 内部构建]
    │
    ├─ --claude-in-chrome-mcp ──→ runClaudeInChromeMcpServer() → exit
    │
    ├─ --chrome-native-host ────→ runChromeNativeHost() → exit
    │
    ├─ --computer-use-mcp ──────→ runComputerUseMcpServer() → exit
    │   [feature: CHICAGO_MCP]
    │
    ├─ --daemon-worker ─────────→ runDaemonWorker(kind) → exit
    │   [feature: DAEMON; 精简路径, 无 config/analytics]
    │
    ├─ remote-control/rc/remote/sync/bridge
    │   ├─ enableConfigs()
    │   ├─ auth 验证
    │   ├─ GrowthBook gate 检查
    │   └─ bridgeMain() → exit
    │   [feature: BRIDGE_MODE]
    │
    └─ (其他) ──────────────────→ 进入标准路径 → main.tsx
```

**Rust 映射**: 简单的 `match` on `args`，快速路径直接 `return`。

---

## 3. Phase B: 完整初始化

**源文件**: `src/main.tsx` (585-834 行)

```
main.tsx entry
    │
    │ ═══ 并行启动 (性能关键) ═══
    ├──┬── startMdmRawRead()        [MDM 子进程]
    │  ├── startKeychainPrefetch()   [系统密钥链读取]
    │  └── profileCheckpoint()       [性能采样]
    │
    │ ═══ 进程级安全 ═══
    ├── 设置 NoDefaultCurrentDirectoryInExePath (Windows)
    ├── 注册 SIGINT handler (非 -p 模式)
    └── 注册 exit handler (光标重置)
    │
    │ ═══ 模式检测与 argv 重写 ═══
    ├── cc:// / cc+unix:// URL → 重写为 open 子命令
    ├── --handle-uri → 深度链接处理 → exit
    ├── assistant [sessionId] → 暂存 sessionId
    ├── ssh <host> [dir] → 提取 SSH flags
    │
    │ ═══ 交互性检测 ═══
    ├── hasPrintFlag (-p/--print) ?
    ├── hasInitOnlyFlag (--init-only) ?
    ├── hasSdkUrl (--sdk-url) ?
    ├── isTTY ?
    └── → isInteractive = !isNonInteractive
    │
    │ ═══ 客户端类型识别 ═══
    │  github-action / sdk-typescript / sdk-python / sdk-cli
    │  claude-vscode / local-agent / claude-desktop / remote / cli (默认)
    │
    │ ═══ 初始化序列 ═══
    ├── ensureKeychainPrefetchCompleted()    [等待并行任务]
    ├── ensureMdmSettingsLoaded()            [等待并行任务]
    ├── initializeGrowthBook()               [特性开关]
    ├── initializeAnalyticsGates()           [遥测]
    ├── loadPolicyLimits()                   [策略]
    ├── initializeTelemetryAfterTrust()      [OTel]
    ├── loadRemoteManagedSettings()          [远程设置]
    │
    │ ═══ 功能初始化 ═══
    ├── initBundledSkills()       → 注册 15+ 内置技能
    ├── initBuiltinPlugins()      → 加载内部插件
    ├── getMcpToolsCommandsAndResources() → MCP 发现
    │
    │ ═══ AppState 创建 ═══
    ├── getDefaultAppState()      → 初始不可变状态
    ├── createStore(state, onChangeAppState)
    │
    │ ═══ React 渲染树挂载 ═══
    └── <App> → <AppStateProvider> → <REPL>
             → launchRepl()
```

### Rust 初始化顺序 (简化)

```rust
async fn main() -> Result<()> {
    // Phase A: 快速路径
    let args = Args::parse();
    if let Some(fast) = args.fast_path() {
        return fast.run().await;
    }

    // Phase B: 完整初始化
    let (settings, permissions) = tokio::join!(
        Settings::load(),
        Permissions::load(),
    );

    let app_state = AppState::new(settings, permissions);
    let store = Store::new(app_state);

    // 工具注册 (无网络)
    let tools = ToolRegistry::new();

    // 进入 REPL 循环
    let mut repl = Repl::new(store, tools);
    repl.run().await
}
```

---

## 4. Phase C: QueryEngine 会话管理器

**源文件**: `src/QueryEngine.ts` (~1295 行)
**详细文档**: [`QUERY_ENGINE_SESSION_LIFECYCLE.md`](QUERY_ENGINE_SESSION_LIFECYCLE.md) — 本节为概述；消息分发 (§2)、持久化管线 (§4)、预算检查 (§5)、Result 生成 (§6)、Rust 差距分析 (§7) 等细节见该文档。当两者描述有分歧时以 SESSION 文档为准。

QueryEngine 是**会话级**状态容器。一个实例 = 一个对话 (多轮次)。

### 状态结构

```
QueryEngine {
    // 跨轮次持久状态
    mutableMessages: Message[]           // 完整对话历史
    abortController: AbortController     // 取消信号
    permissionDenials: SDKPermissionDenial[]  // 权限拒绝记录
    readFileState: FileStateCache        // 文件内容 LRU 缓存
    totalUsage: NonNullableUsage         // 累计 API 使用量
}
```

### submitMessage() 生命周期

```
submitMessage(prompt, options) → AsyncGenerator<SDKMessage>
    │
    ├── 1. 初始化 ToolUseContext
    │   ├── 清除本轮技能发现跟踪
    │   ├── 设置工作目录
    │   ├── 包装 canUseTool (跟踪权限拒绝)
    │   └── 确定 thinking 配置
    │
    ├── 2. 获取系统提示词
    │   ├── fetchSystemPromptParts() [并行]
    │   ├── loadMemoryPrompt() [并行]
    │   └── 组装 systemPrompt
    │
    ├── 3. processUserInput(prompt)
    │   ├── 解析斜杠命令 (/compact, /clear, ...)
    │   ├── 处理附件 (文件, 图片, URL)
    │   └── 推送 UserMessage 到 mutableMessages
    │
    ├── 4. 委托给 query() 生成器
    │   ├── 构建 QueryParams
    │   └── yield* query(params)
    │
    ├── 5. 处理 query() 产出
    │   ├── assistant → 记录 + 转发 SDK
    │   ├── user (tool_result) → 记录
    │   ├── system (compact_boundary) → 释放旧消息 GC
    │   ├── stream_event → 累计 usage + 可选转发
    │   └── attachment → 处理结构化输出
    │
    ├── 6. 预算检查
    │   └── maxBudgetUsd 超限 → 终止
    │
    └── 7. 返回最终 result 消息
```

### Rust 映射

```rust
struct QueryEngine {
    messages: Vec<Message>,
    abort: CancellationToken,
    permission_denials: Vec<PermissionDenial>,
    file_cache: FileStateCache,
    total_usage: Usage,
}

impl QueryEngine {
    fn submit_message(&mut self, prompt: &str) -> impl Stream<Item = SdkMessage> {
        // ...
    }
}
```

---

## 5. Phase D: query() 主循环状态机 (核心)

**源文件**: `src/query.ts` (1729 行) — **整个系统的心脏**

### 5.1 状态定义

```typescript
type State = {
    messages: Message[]                          // 当前上下文 (每次迭代增长)
    toolUseContext: ToolUseContext               // 工具执行环境 (可变)
    autoCompactTracking: AutoCompactTrackingState | undefined  // 压缩跟踪
    maxOutputTokensRecoveryCount: number         // 输出截断恢复计数 (0-3)
    hasAttemptedReactiveCompact: boolean          // 响应式压缩已尝试标记
    maxOutputTokensOverride: number | undefined   // 输出 token 上限覆盖
    pendingToolUseSummary: Promise<ToolUseSummaryMessage | null> | undefined
    stopHookActive: boolean | undefined           // 停止钩子激活标记
    turnCount: number                            // 当前轮次 (从 1 开始)
    transition: Continue | undefined             // 上一次 continue 的原因
}
```

### 5.2 转换类型

```typescript
// 终止原因 — query() 的返回值
type Terminal = { reason: TerminalReason; error?: Error; turnCount?: number }

type TerminalReason =
    | 'completed'            // 正常完成 (无工具调用 / 预算用尽)
    | 'aborted_streaming'    // 流式阶段用户中断 (Ctrl+C)
    | 'aborted_tools'        // 工具执行阶段用户中断
    | 'blocking_limit'       // 达到 token 硬上限 (auto-compact OFF)
    | 'prompt_too_long'      // prompt 过长, 所有恢复手段耗尽
    | 'image_error'          // 图片/PDF 大小错误
    | 'model_error'          // API 调用异常 (携带 error 对象)
    | 'hook_stopped'         // 工具 hook 阻止继续
    | 'stop_hook_prevented'  // stop hook preventContinuation
    | 'max_turns'            // 达到最大轮次 (携带 turnCount)

// 继续原因 — 循环内部 state 转换标签
type Continue = { reason: ContinueReason; [key: string]: unknown }

type ContinueReason =
    | 'next_turn'                    // 工具执行完毕 → 带结果回模型
    | 'collapse_drain_retry'         // context collapse 排空后重试
    | 'reactive_compact_retry'       // 响应式压缩后重试
    | 'max_output_tokens_escalate'   // 8k → 64k 升级重试
    | 'max_output_tokens_recovery'   // 注入续写消息重试 (最多 3 次)
    | 'stop_hook_blocking'           // stop hook 错误 → 回模型
    | 'token_budget_continuation'    // token 预算 < 90% → 注入续写
```

### 5.3 完整状态转换图

```
                          ┌──────────────────────────┐
                          │    LOOP ENTRY (while)     │
                          │                          │
                          │  解构 state              │
                          │  技能发现预取             │
                          │  查询链跟踪递增           │
                          └────────────┬─────────────┘
                                       │
                          ┌────────────▼─────────────┐
                    ┌─────│ 1. CONTEXT MANAGEMENT    │
                    │     │                          │
                    │     │ applyToolResultBudget    │
                    │     │ snipCompact              │
                    │     │ microcompact             │
                    │     │ contextCollapse          │
                    │     │ autoCompact              │
                    │     └────────────┬─────────────┘
                    │                  │
                    │     ┌────────────▼─────────────┐
                    │     │ 2. BLOCKING LIMIT CHECK  │
                    │     │                          │
                    │     │ tokenCount > limit ?     │
                    │     └──┬──────────────┬────────┘
                    │        │ YES          │ NO
                    │        ▼              │
                    │  return Terminal      │
                    │  {blocking_limit}     │
                    │                       │
                    │     ┌────────────────▼──────────┐
                    │     │ 3. API CALL (streaming)   │
                    │     │                           │
                    │     │ for await (msg of API) {  │
                    │     │   收集 assistantMessages   │
                    │     │   收集 toolUseBlocks       │
                    │     │   流式工具执行              │
                    │     │   扣留可恢复错误            │
                    │     │ }                          │
                    │     └──┬────────────────────────┘
                    │        │
                    │        ├── FallbackTriggeredError
                    │        │   → 切换 fallbackModel
                    │        │   → 清空 assistant/tool 状态
                    │        │   → retry (attemptWithFallback)
                    │        │
                    │        ├── ImageSizeError / ImageResizeError
                    │        │   → return Terminal {image_error}
                    │        │
                    │        └── 其他异常
                    │            → yield 错误消息
                    │            → return Terminal {model_error, error}
                    │
                    │     ┌──────────────▼───────────────┐
                    │     │ 4. POST-STREAMING             │
                    │     │                               │
                    │     │ executePostSamplingHooks()     │
                    │     │ abort 检查                     │
                    │     └──┬──────────────┬─────────────┘
                    │        │ aborted      │ normal
                    │        ▼              │
                    │  return Terminal      │
                    │  {aborted_streaming}  │
                    │                       │
                    │     ┌────────────────▼──────────────────────────┐
                    │     │ 5. DECISION POINT                         │
                    │     │   needsFollowUp == false                  │
                    │     │                                           │
                    │     │ ┌─ isWithheld413 ?                       │
                    │     │ │  ├─ contextCollapse.recoverFromOverflow │
                    │     │ │  │  → continue {collapse_drain_retry}   │
                    │     │ │  ├─ reactiveCompact.tryReactiveCompact  │
                    │     │ │  │  → continue {reactive_compact_retry} │
                    │     │ │  └─ 不可恢复                             │
                    │     │ │     → return {prompt_too_long}           │
                    │     │ │                                          │
                    │     │ ├─ isWithheldMaxOutputTokens ?             │
                    │     │ │  ├─ 未覆盖 + capEnabled                  │
                    │     │ │  │  → continue {max_output_tokens_escalate}
                    │     │ │  ├─ recoveryCount < 3                    │
                    │     │ │  │  → continue {max_output_tokens_recovery}
                    │     │ │  └─ 恢复耗尽                              │
                    │     │ │     → yield 错误 → fall through           │
                    │     │ │                                          │
                    │     │ ├─ isApiErrorMessage ?                     │
                    │     │ │  → return {completed}  [跳过 stop hooks] │
                    │     │ │                                          │
                    │     │ ├─ handleStopHooks()                       │
                    │     │ │  ├─ preventContinuation                  │
                    │     │ │  │  → return {stop_hook_prevented}       │
                    │     │ │  ├─ blockingErrors                       │
                    │     │ │  │  → continue {stop_hook_blocking}      │
                    │     │ │  └─ pass                                  │
                    │     │ │                                          │
                    │     │ ├─ checkTokenBudget()                      │
                    │     │ │  ├─ continue                             │
                    │     │ │  │  → continue {token_budget_continuation}│
                    │     │ │  └─ stop                                  │
                    │     │ │     → fall through                        │
                    │     │ │                                          │
                    │     │ └─ return {completed}                       │
                    │     └──────────────────────────────────────────┘
                    │                       │
                    │                       │ needsFollowUp == true
                    │                       ▼
                    │     ┌────────────────────────────────────────┐
                    │     │ 6. TOOL EXECUTION                      │
                    │     │                                        │
                    │     │ StreamingToolExecutor.getRemainingResults()
                    │     │   OR runTools(blocks, canUseTool, ctx) │
                    │     │                                        │
                    │     │ for await (update of toolUpdates) {    │
                    │     │   yield update.message                 │
                    │     │   收集 toolResults                      │
                    │     │   检查 hook_stopped_continuation        │
                    │     │ }                                      │
                    │     └──┬──────────────┬──────────┬──────────┘
                    │        │ aborted      │ hook     │ normal
                    │        ▼              │ stopped  │
                    │  return Terminal      ▼          │
                    │  {aborted_tools}  return Terminal│
                    │                  {hook_stopped}  │
                    │                                  │
                    │     ┌────────────────────────────▼───────────┐
                    │     │ 7. ATTACHMENTS                         │
                    │     │                                        │
                    │     │ getAttachmentMessages() [文件变更]      │
                    │     │ pendingMemoryPrefetch → 消费            │
                    │     │ pendingSkillPrefetch → 消费             │
                    │     │ queuedCommands → 消费                   │
                    │     └────────────────────────────┬───────────┘
                    │                                  │
                    │     ┌────────────────────────────▼───────────┐
                    │     │ 8. CONTINUE PREPARATION                │
                    │     │                                        │
                    │     │ refreshTools() [MCP 热加载]             │
                    │     │ maxTurns 检查                           │
                    │     │   → 超限: return {max_turns, turnCount} │
                    │     │ taskSummary 生成 (后台)                  │
                    │     │                                        │
                    │     │ state = State {                         │
                    │     │   messages: [...old, ...assistant,      │
                    │     │              ...toolResults],           │
                    │     │   turnCount: turnCount + 1,             │
                    │     │   transition: {next_turn},              │
                    │     │   ...reset counters                     │
                    │     │ }                                       │
                    │     │                                        │
                    │     │ → continue (回到 LOOP ENTRY)            │
                    │     └────────────────────────────────────────┘
                    │
                    └─ (回到 LOOP ENTRY)
```

### 5.4 Continue 转换的状态保持/重置规则

| continue 原因 | messages | recovery count | reactive compact | turnCount | maxOutputOverride |
|---|---|---|---|---|---|
| `next_turn` | old + assistant + tools | **重置 0** | **重置 false** | +1 | **重置 undefined** |
| `collapse_drain_retry` | drained | 保持 | 保持 | 保持 | 重置 |
| `reactive_compact_retry` | postCompact | 保持 | **设为 true** | 保持 | 重置 |
| `max_output_tokens_escalate` | 保持 | 保持 | 保持 | 保持 | **设为 64k** |
| `max_output_tokens_recovery` | old + assistant + recovery_msg | **+1** | 保持 | 保持 | 重置 |
| `stop_hook_blocking` | old + assistant + errors | 重置 0 | **保持** | 保持 | 重置 |
| `token_budget_continuation` | old + assistant + nudge_msg | 重置 0 | 重置 false | 保持 | 重置 |

---

## 6. Phase E: 工具执行管线

**源文件**: `src/services/tools/toolExecution.ts`, `StreamingToolExecutor.ts`, `toolOrchestration.ts`

### 6.1 单工具执行管线

```
runToolUse(block, assistantMsg, canUseTool, context)
    │
    ├── 1. 工具查找
    │   ├── 在 tools[] 中按名称匹配
    │   ├── 回退到基础工具集按别名匹配
    │   └── 未找到 → 返回错误 "No such tool available"
    │
    ├── 2. 输入验证
    │   ├── Zod schema 解析 (tool.inputSchema.safeParse)
    │   ├── tool.validateInput() 自定义验证
    │   └── 失败 → 返回错误 + "schema not sent" 提示
    │
    ├── 3. Pre-Tool Hooks
    │   ├── runPreToolUseHooks() [async iterator]
    │   ├── 可产出: 消息, 权限覆盖, 修改后的输入, 停止信号
    │   └── 收集 hook 耗时
    │
    ├── 4. 权限检查
    │   ├── resolveHookPermissionDecision() [处理 hook 覆盖]
    │   ├── canUseTool() → hasPermissionsToUseTool()
    │   ├── 行为 = 'allow' → 继续
    │   ├── 行为 = 'deny' → 返回拒绝
    │   └── 行为 = 'ask' → 交互式提示 (非 headless)
    │
    ├── 5. 工具调用
    │   └── tool.call(input, context, canUseTool, parentMsg, onProgress)
    │
    ├── 6. Post-Tool Hooks
    │   ├── 成功: runPostToolUseHooks()
    │   └── 失败: runPostToolUseFailureHooks()
    │
    └── 7. 结果处理
        ├── mapToolResultToToolResultBlockParam()
        ├── 大结果持久化到磁盘 (> maxResultSizeChars)
        └── 返回消息
```

### 6.2 并发编排

```
partitionToolCalls(blocks)
    │
    ├── concurrencySafe = true → 并行批次 (最多 10 并发)
    │   └── tokio::JoinSet / FuturesUnordered
    │
    └── concurrencySafe = false → 严格串行
        └── 逐个执行, 中间可 yield

StreamingToolExecutor 状态:
    queued → executing → completed → yielded

    并发安全工具: 立即启动执行
    非并发安全工具: 等待队列清空后独占执行

    结果顺序: FIFO (即使后提交的先完成)
```

### 6.3 Rust 映射

```rust
enum ToolExecutionState {
    Queued,
    Executing,
    Completed(ToolResult),
    Yielded,
}

trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self, input: &Value) -> String;
    fn input_schema(&self) -> &JsonSchema;

    async fn call(
        &self,
        input: Value,
        ctx: &ToolUseContext,
        on_progress: impl Fn(Progress),
    ) -> Result<ToolResult>;

    fn is_concurrency_safe(&self, input: &Value) -> bool;
    fn is_read_only(&self, input: &Value) -> bool;
    fn check_permissions(&self, input: &Value, ctx: &PermissionContext) -> PermissionResult;
}
```

---

## 7. Phase F: 权限决策状态机

**源文件**: `src/utils/permissions/permissions.ts`

### 7.1 权限模式

```rust
enum PermissionMode {
    Default,           // 每次需要提示
    Plan,              // 暂停规划状态
    AcceptEdits,       // 自动接受文件编辑
    BypassPermissions, // 全部自动允许
    DontAsk,           // 全部静默拒绝
    // 以下仅内部:
    // Auto,           // ML 分类器 (仅 Ant)
    // Bubble,         // 叠加权限模式
}
```

### 7.2 权限决策流程

```
hasPermissionsToUseTool(tool, input, context)
    │
    ├── Phase 1a: 无条件规则匹配 (仅工具名)
    │   ├── toolAlwaysAllowedRule() → allow
    │   ├── getDenyRuleForTool() → deny
    │   └── getAskRuleForTool() → ask
    │
    ├── Phase 1b: 模式匹配规则 (工具名 + 参数)
    │   ├── preparePermissionMatcher(input) → Matcher
    │   │   例: Bash(prefix:git) → 匹配 "git status"
    │   └── 检查 allow/deny/ask 规则集
    │
    ├── Phase 2: Hook 拦截
    │   ├── executePermissionRequestHooks()
    │   └── 可返回: allow / deny / 修改的权限上下文
    │
    └── Phase 3: 模式检查
        ├── BypassPermissions → allow
        ├── DontAsk → deny (静默)
        ├── Default/Plan → 交互式提示
        └── Auto → ML 分类器 (side_query)

权限规则源 (优先级递减):
    1. policySettings (企业管理)
    2. projectSettings (.cc-rust/settings.json)
    3. userSettings (~/.cc-rust/settings.json)
    4. localSettings (仓库特定)
    5. cliArg (命令行参数)
    6. session (会话内授权)

拒绝跟踪 (仅 Auto 模式):
    consecutiveDenials >= 3 → 回退到交互式提示
    totalDenials >= 20 → 强制回退
```

### 7.3 Rust 映射

```rust
enum PermissionBehavior {
    Allow,
    Deny,
    Ask,
}

struct PermissionResult {
    behavior: PermissionBehavior,
    updated_input: Option<Value>,
    message: Option<String>,
    reason: PermissionDecisionReason,
}

enum PermissionDecisionReason {
    Rule { source: RuleSource, pattern: String },
    Hook { hook_name: String },
    Mode { mode: PermissionMode },
}
```

---

## 8. Phase G: 上下文管理管线

**源文件**: `src/services/compact/`, `src/query.ts` 307-549 行

### 8.1 管线顺序 (每次循环迭代)

```
messagesForQuery
    │
    ├── 1. applyToolResultBudget
    │   │  按工具结果大小预算裁剪
    │   │  大结果 → 持久化到磁盘, 替换为占位符
    │   └── 幂等, 持久化仅在 agentId/repl_main_thread 时
    │
    ├── 2. snipCompact [feature: HISTORY_SNIP]
    │   │  历史裁剪 (保留最近 N 条)
    │   └── 返回 { messages, tokensFreed, boundaryMessage? }
    │
    ├── 3. microcompact
    │   │  轻量压缩: 删除已缓存的工具结果
    │   └── [feature: CACHED_MICROCOMPACT] 延迟边界消息
    │
    ├── 4. contextCollapse [feature: CONTEXT_COLLAPSE]
    │   │  上下文折叠: 将旧区段替换为摘要
    │   │  读时投影 — 折叠存储独立于消息数组
    │   └── 跑在 autoCompact 之前 (如果折叠够用则跳过 autoCompact)
    │
    └── 5. autoCompact
        │  完整自动压缩:
        │  ├── 判断是否需要 (token 阈值)
        │  ├── 调用模型生成摘要
        │  ├── buildPostCompactMessages()
        │  │   ├── 摘要消息
        │  │   ├── 恢复最近 5 个文件 (各 5K token 上限)
        │  │   ├── 重注入技能指令 (25K token 预算, 5 技能, 各 5K)
        │  │   └── hook 结果
        │  └── 更新 tracking { compacted: true, turnId, turnCounter: 0 }
        │
        └── messagesForQuery = 处理后的消息
```

### 8.2 压缩恢复路径

```
API 返回 prompt_too_long (413):
    │
    ├── 1. contextCollapse.recoverFromOverflow()
    │   │  排空暂存的折叠队列
    │   └── 成功 (committed > 0) → continue {collapse_drain_retry}
    │
    ├── 2. reactiveCompact.tryReactiveCompact()
    │   │  紧急压缩 (已扣留的 413 错误)
    │   └── 成功 → continue {reactive_compact_retry}
    │
    └── 3. 不可恢复 → return {prompt_too_long}

API 返回 max_output_tokens:
    │
    ├── 1. OTK 升级 (8k → 64k)
    │   │  条件: capEnabled + 未手动覆盖
    │   └── → continue {max_output_tokens_escalate}
    │
    ├── 2. 续写恢复 (注入 "Resume directly" 消息)
    │   │  最多 3 次
    │   └── → continue {max_output_tokens_recovery}
    │
    └── 3. 恢复耗尽 → yield 错误, fall through to return {completed}
```

---

## 9. Phase H: 停止钩子与后处理

**源文件**: `src/query/stopHooks.ts`

```
handleStopHooks(messages, assistant, systemPrompt, ..., toolUseContext)
    │
    ├── 1. 后台任务 (fire-and-forget)
    │   ├── 作业分类 [feature: TEMPLATES]
    │   ├── 提示建议生成
    │   ├── 记忆提取 [自动记忆]
    │   └── 自动沉思 (conversational insights)
    │
    ├── 2. Computer Use 清理 [feature: CHICAGO_MCP]
    │   ├── 自动取消隐藏
    │   └── 锁释放
    │
    ├── 3. 用户定义的 Stop Hooks
    │   ├── executeStopHooks() [async generator]
    │   ├── 收集阻塞错误
    │   ├── 收集非阻塞输出
    │   └── 跟踪 hook 计数
    │
    ├── 4. Hook 摘要消息
    │   └── hookCount > 0 → yield createStopHookSummaryMessage()
    │
    ├── 5. 团队特定 Hooks [isTeammate()]
    │   ├── executeTaskCompletedHooks() → in-progress tasks
    │   └── executeTeammateIdleHooks()
    │
    └── 返回 { blockingErrors, preventContinuation }
        │
        ├── preventContinuation=true → return {stop_hook_prevented}
        ├── blockingErrors.length > 0 → continue {stop_hook_blocking}
        └── pass → fall through to token budget / return {completed}
```

---

## 10. Phase I: 关闭与清理

```
进程退出触发:
    │
    ├── gracefulShutdown() [async]
    │   ├── MCP 服务器传输关闭
    │   ├── 创建的 team 清理
    │   ├── sandbox 进程终止
    │   └── 文件监视器停止
    │
    ├── gracefulShutdownSync() [同步]
    │   └── 光标重置, 定时器清除
    │
    ├── AbortController.abort()
    │   └── 传播到所有工具执行
    │
    └── 会话持久化
        ├── 写入对话记录 (transcript)
        └── 刷新 sessionStorage
```

---

## 11. Rust 映射方案

### 11.1 核心状态机

```rust
use tokio_stream::Stream;
use tokio::sync::watch;

/// query() 主循环的 Rust 实现
pub fn query(params: QueryParams) -> impl Stream<Item = StreamYield> {
    async_stream::stream! {
        let mut state = QueryState::new(params);

        loop {
            // 1. Context management
            state.apply_context_management().await;

            // 2. Blocking limit check
            if state.is_at_blocking_limit() {
                yield StreamYield::Terminal(Terminal::BlockingLimit);
                return;
            }

            // 3. API call
            let api_result = match state.call_api().await {
                Ok(result) => result,
                Err(ApiError::ImageSize(e)) => {
                    yield StreamYield::Terminal(Terminal::ImageError);
                    return;
                }
                Err(e) => {
                    yield StreamYield::Terminal(Terminal::ModelError(e));
                    return;
                }
            };

            // 4. Abort check
            if state.is_aborted() {
                yield StreamYield::Terminal(Terminal::AbortedStreaming);
                return;
            }

            // 5. Decision point
            if !api_result.needs_follow_up {
                match state.handle_no_follow_up(&api_result).await {
                    Decision::Continue(reason) => {
                        state.transition(reason);
                        continue;
                    }
                    Decision::Return(terminal) => {
                        yield StreamYield::Terminal(terminal);
                        return;
                    }
                }
            }

            // 6. Tool execution
            match state.execute_tools(&api_result).await {
                ToolOutcome::Aborted => {
                    yield StreamYield::Terminal(Terminal::AbortedTools);
                    return;
                }
                ToolOutcome::HookStopped => {
                    yield StreamYield::Terminal(Terminal::HookStopped);
                    return;
                }
                ToolOutcome::Continue(results) => {
                    // 7. Attachments
                    state.gather_attachments(&results).await;

                    // 8. Max turns check
                    if state.exceeds_max_turns() {
                        yield StreamYield::Terminal(Terminal::MaxTurns(state.turn_count));
                        return;
                    }

                    state.prepare_next_turn(results);
                }
            }
        }
    }
}
```

### 11.2 状态转换枚举

```rust
/// 终止原因
#[derive(Debug, Clone)]
pub enum Terminal {
    Completed,
    AbortedStreaming,
    AbortedTools,
    BlockingLimit,
    PromptTooLong,
    ImageError,
    ModelError(Box<dyn std::error::Error + Send>),
    HookStopped,
    StopHookPrevented,
    MaxTurns(usize),
}

/// 继续原因
#[derive(Debug, Clone)]
pub enum ContinueReason {
    NextTurn,
    CollapseDrainRetry { committed: usize },
    ReactiveCompactRetry,
    MaxOutputTokensEscalate,
    MaxOutputTokensRecovery { attempt: u32 },
    StopHookBlocking,
    TokenBudgetContinuation,
}

/// 循环状态
pub struct QueryState {
    messages: Vec<Message>,
    tool_use_context: ToolUseContext,
    auto_compact_tracking: Option<AutoCompactTracking>,
    max_output_tokens_recovery_count: u32,
    has_attempted_reactive_compact: bool,
    max_output_tokens_override: Option<u32>,
    turn_count: usize,
    transition: Option<ContinueReason>,
    abort: CancellationToken,
}
```

### 11.3 关键类型映射表

| TypeScript | Rust | 说明 |
|---|---|---|
| `AsyncGenerator<T, R>` | `impl Stream<Item=T>` + 返回 `R` | `async_stream::stream!` 或手写 `Poll` |
| `while (true) { ... continue }` | `loop { ... continue }` | 直接对应 |
| `state = next; continue` | `self.state = next; continue` | `&mut self` 模式 |
| `yield message` | `yield StreamYield::Message(msg)` | `async_stream` 宏 |
| `return { reason }` | `yield StreamYield::Terminal(t); return` | 流结束 |
| `AbortController` | `tokio_util::sync::CancellationToken` | 取消传播 |
| `Promise<T>` | `tokio::task::JoinHandle<T>` | 后台任务 |
| `pMap(items, fn, {concurrency})` | `FuturesUnordered` + `Semaphore` | 有界并发 |
| `DeepImmutable<AppState>` | `Arc<AppState>` | Copy-on-write via `Arc::make_mut` |
| `feature('FLAG')` | `#[cfg(feature = "flag")]` | 编译时消除 |
| `z.object({...})` | `#[derive(Deserialize, Validate)]` | serde + validator |

---

## 12. 附录: 完整类型定义

### A. Message 枚举

```rust
#[derive(Debug, Clone)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    StreamEvent(StreamEvent),
    Attachment(AttachmentMessage),
    Tombstone(TombstoneMessage),
    ToolUseSummary(ToolUseSummaryMessage),
}

#[derive(Debug, Clone)]
pub struct UserMessage {
    pub uuid: Uuid,
    pub timestamp: DateTime<Utc>,
    pub content: Vec<ContentBlock>,
    pub is_meta: bool,
    pub tool_use_result: Option<String>,
    pub source_tool_assistant_uuid: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct AssistantMessage {
    pub uuid: Uuid,
    pub timestamp: DateTime<Utc>,
    pub content: Vec<ContentBlock>,
    pub usage: Usage,
    pub stop_reason: Option<StopReason>,
    pub is_api_error_message: bool,
    pub api_error: Option<String>,
    pub cost_usd: f64,
}
```

### B. AppState (简化版, Phase 0-8 所需)

```rust
pub struct AppState {
    // 设置
    pub settings: Settings,
    pub verbose: bool,
    pub main_loop_model: Option<String>,

    // 权限
    pub permission_context: ToolPermissionContext,
    pub permission_mode: PermissionMode,

    // 工具
    pub tools: Vec<Arc<dyn Tool>>,

    // 任务
    pub tasks: HashMap<String, TaskState>,

    // 会话
    pub thinking_enabled: Option<bool>,
    pub fast_mode: Option<bool>,

    // UI 状态
    pub expanded_view: ExpandedView,
    pub footer_selection: Option<FooterItem>,
}
```

### C. ToolUseContext

```rust
pub struct ToolUseContext {
    pub abort: CancellationToken,
    pub file_cache: Arc<RwLock<FileStateCache>>,
    pub app_state: Arc<RwLock<AppState>>,
    pub messages: Vec<Message>,
    pub options: ToolOptions,
    pub agent_id: Option<String>,
    pub query_tracking: Option<QueryTracking>,
}

pub struct ToolOptions {
    pub tools: Vec<Arc<dyn Tool>>,
    pub commands: Vec<Command>,
    pub main_loop_model: String,
    pub thinking_config: Option<ThinkingConfig>,
    pub is_non_interactive_session: bool,
}
```

---

## 文件索引

| 文档章节 | 源文件 | 行数 |
|---|---|---|
| Phase A | `src/entrypoints/cli.tsx` | 1-200 |
| Phase B | `src/main.tsx` | 585-834 |
| Phase C | `src/QueryEngine.ts` | 全文 ~1295 |
| Phase D | `src/query.ts` | 全文 ~1729 |
| Phase E | `src/services/tools/toolExecution.ts` | 全文 |
| Phase E | `src/services/tools/StreamingToolExecutor.ts` | 全文 |
| Phase E | `src/services/tools/toolOrchestration.ts` | 全文 |
| Phase F | `src/utils/permissions/permissions.ts` | 全文 |
| Phase G | `src/services/compact/*.ts` | 多文件 |
| Phase H | `src/query/stopHooks.ts` | 全文 |
| Phase I | `src/utils/process.ts` | gracefulShutdown |
| 状态存储 | `src/state/AppStateStore.ts` | 89-569 |
| 全局状态 | `src/bootstrap/state.ts` | 全文 ~1758 |
