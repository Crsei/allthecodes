# QueryEngine Session 生命周期 — Rust 重构文档

## 概述

QueryEngine 是 Claude Code 的**会话控制器**。一个 QueryEngine 实例 = 一个对话 (conversation)。每次 `submitMessage()` 调用 = 一个对话轮次 (turn)。它是 query loop 状态机的上层包装，负责：

1. 用户输入处理 (斜杠命令、附件、图片)
2. 系统提示词组装
3. 将处理后的消息委托给 query loop
4. 拦截 query loop 产出的每条消息，分发到：持久化、SDK 输出、使用量跟踪、预算检查
5. 生成最终的 result 消息

---

## 1. Session 生命周期状态图

```
                     ┌─────────────────────┐
                     │  QueryEngine::new()  │
                     │  ─ session_id 生成    │
                     │  ─ 初始消息加载       │
                     │  ─ readFileCache 克隆 │
                     │  ─ abort 控制器创建   │
                     └──────────┬──────────┘
                                │
              ┌─────────────────▼─────────────────┐
              │       submitMessage(prompt)         │
              │  ┌─────────────────────────────┐   │
              │  │ Phase A: Input Processing   │   │
              │  │  ─ discoveredSkillNames 清空 │   │
              │  │  ─ setCwd()                 │   │
              │  │  ─ processUserInput()       │   │
              │  │  ─ 消息推入 mutableMessages  │   │
              │  │  ─ 会话持久化 (transcript)   │   │
              │  └────────────┬────────────────┘   │
              │               │                    │
              │     shouldQuery == false?           │
              │        ├── YES: 本地命令结果 ──────────▶ yield result(success)
              │        └── NO ↓                    │
              │  ┌────────────▼────────────────┐   │
              │  │ Phase B: System Prompt Build │   │
              │  │  ─ fetchSystemPromptParts()  │   │
              │  │  ─ userContext 合并           │   │
              │  │  ─ memoryMechanics 注入      │   │
              │  │  ─ custom + append 组装       │   │
              │  └────────────┬────────────────┘   │
              │               │                    │
              │  ┌────────────▼────────────────┐   │
              │  │ Phase C: Pre-Query Setup     │   │
              │  │  ─ canUseTool 包装 (denial)  │   │
              │  │  ─ fileHistory 快照           │   │
              │  │  ─ skills/plugins 加载        │   │
              │  │  ─ yield systemInitMessage   │   │
              │  │  ─ toolPermission 更新        │   │
              │  └────────────┬────────────────┘   │
              │               │                    │
              │  ┌────────────▼────────────────┐   │
              │  │ Phase D: Query Loop          │   │
              │  │  ─ for await (msg of query)  │   │
              │  │  ─ 消息分发 (见下方详图)      │   │
              │  │  ─ 预算检查 (每条消息后)      │   │
              │  │  ─ 结构化输出重试限制         │   │
              │  └────────────┬────────────────┘   │
              │               │                    │
              │  ┌────────────▼────────────────┐   │
              │  │ Phase E: Result Generation   │   │
              │  │  ─ flush transcript          │   │
              │  │  ─ isResultSuccessful() 检查  │   │
              │  │  ─ 提取 textResult           │   │
              │  │  ─ yield result(success/err) │   │
              │  └─────────────────────────────┘   │
              └────────────────────────────────────┘
                                │
                     ┌──────────▼──────────┐
                     │   下一次 submit      │
                     │   (同一实例复用)      │
                     └─────────────────────┘
```

---

## 2. Phase D: 消息分发 (Message Dispatch) 详图

QueryEngine 从 query loop 接收的每条消息，按 `message.type` 分发处理：

```
query loop yield ──▶ message.type?
  │
  ├─ assistant ──────────▶ ① mutableMessages.push
  │                       ② 提取 stop_reason
  │                       ③ yield* normalizeMessage → SDK
  │                       ④ persistSession? → fire-and-forget recordTranscript
  │
  ├─ user ───────────────▶ ① mutableMessages.push
  │                       ② turnCount++
  │                       ③ yield* normalizeMessage → SDK
  │                       ④ persistSession? → await recordTranscript
  │
  ├─ progress ───────────▶ ① mutableMessages.push
  │                       ② yield* normalizeMessage → SDK
  │                       ③ persistSession? → fire-and-forget recordTranscript
  │
  ├─ stream_event ───────▶ ① message_start  → 重置 currentMessageUsage
  │                       ② message_delta  → 累加 usage, 捕获 stop_reason
  │                       ③ message_stop   → accumulateUsage → totalUsage
  │                       ④ includePartialMessages? → yield stream_event → SDK
  │
  ├─ attachment ─────────▶ ① mutableMessages.push
  │                       ② persistSession? → fire-and-forget recordTranscript
  │                       ③ attachment.type?
  │                       │  ├─ structured_output → 捕获 structuredOutputFromTool
  │                       │  ├─ max_turns_reached → yield result(error_max_turns) → return
  │                       │  └─ queued_command   → replayUserMessages? → yield SDKUserReplay
  │
  ├─ system ─────────────▶ ① snipReplay? → 裁剪 mutableMessages (snip boundary)
  │                       ② compact_boundary?
  │                       │  ├─ YES → splice(0, boundaryIdx) 释放旧消息
  │                       │  │      → yield compact_boundary → SDK
  │                       │  └─ NO  → api_error? → yield api_retry → SDK
  │                       ③ 其他系统消息: 不 yield (headless 静默)
  │
  ├─ tombstone ──────────▶ skip (模型降级重试时的消息作废信号)
  │
  ├─ tool_use_summary ───▶ yield tool_use_summary → SDK
  │
  └─ stream_request_start▶ skip (不转发)

  ── 每条消息处理后 ──▶ 检查 maxBudgetUsd → 超限? yield result(error_max_budget)
                       检查 structuredOutput 重试次数 → 超限? yield result(error)
```

---

## 3. 跨轮次持久状态 (Cross-Turn Persistent State)

QueryEngine 实例在多次 `submitMessage()` 之间保持的状态：

| 状态 | TypeScript 字段 | 生命周期 | 说明 |
|------|----------------|----------|------|
| **消息历史** | `mutableMessages: Message[]` | 会话级 | 所有已产出的消息，含 tool_result |
| **abort 控制器** | `abortController: AbortController` | 会话级 | 每次 submit 复用 (非重建) |
| **权限拒绝** | `permissionDenials: SDKPermissionDenial[]` | 会话级 | 累积所有轮次的拒绝 |
| **总使用量** | `totalUsage: NonNullableUsage` | 会话级 | 跨轮次累加 |
| **文件缓存** | `readFileState: FileStateCache` | 会话级 | LRU，工具已读/已写文件 |
| **嵌套内存路径** | `loadedNestedMemoryPaths: Set<string>` | 会话级 | 去重 CLAUDE.md 注入 |
| **孤儿权限** | `hasHandledOrphanedPermission: boolean` | 会话级 | 只处理一次 |

轮次级 (Turn-Scoped) 状态，在每次 `submitMessage` 开始时重置：

| 状态 | 重置时机 | 说明 |
|------|---------|------|
| `discoveredSkillNames` | `.clear()` | 技能发现追踪 |
| `currentMessageUsage` | 每次 `message_start` | 当前 API 响应的 token 消耗 |
| `turnCount` | 初始化 `= 1` | 当前 submit 内的轮次 |
| `lastStopReason` | 初始化 `= null` | 最后一个 stop_reason |
| `structuredOutputFromTool` | 初始化 `= undefined` | 结构化输出捕获 |

---

## 4. 会话持久化管线 (Session Persistence Pipeline)

```
                   submitMessage()
                        │
        ┌───────────────▼───────────────┐
        │   messagesFromUserInput 产出   │
        │   persistSession?              │
        │   ├─ bareMode → fire-and-forget│
        │   └─ 非 bare → await + flush?  │
        └───────────────┬───────────────┘
                        │
        ┌───────────────▼───────────────┐
        │     query loop 每条消息:       │
        │                               │
        │  assistant → void recordTranscript(messages)    [fire-and-forget]
        │  user      → await recordTranscript(messages)   [同步等待]
        │  progress  → void recordTranscript(messages)    [fire-and-forget]
        │  attachment→ void recordTranscript(messages)    [fire-and-forget]
        │  compact_boundary:                              │
        │    ├─ 先 flush preservedSegment tail            │
        │    └─ 然后 await recordTranscript(messages)     │
        └───────────────┬───────────────┘
                        │
        ┌───────────────▼───────────────┐
        │     result 产出前:             │
        │  EAGER_FLUSH / IS_COWORK?      │
        │  → await flushSessionStorage() │
        └───────────────────────────────┘
```

**关键设计决策**:
- assistant 消息用 fire-and-forget 写入，因为 claude.ts 流式 yield 后还会 mutate `usage`/`stop_reason`
- user/compact_boundary 消息同步等待，确保 resume 时数据完整
- Desktop/Cowork 环境需要 eager flush (进程可能被立即杀死)

---

## 5. 预算与限制检查管线

```
每条消息处理后:
  │
  ├──▶ maxBudgetUsd 检查
  │    getTotalCost() >= maxBudgetUsd?
  │    ├─ YES → flush → yield result(error_max_budget_usd) → return
  │    └─ NO  → 继续
  │
  ├──▶ structuredOutput 重试检查 (仅当 jsonSchema 存在)
  │    countToolCalls(SYNTHETIC_OUTPUT) >= maxRetries(5)?
  │    ├─ YES → flush → yield result(error_max_structured_output_retries) → return
  │    └─ NO  → 继续
  │
  └──▶ query loop 内部的限制 (由 query.ts 处理):
       ├─ maxTurns → yield attachment(max_turns_reached) → QueryEngine 捕获 → yield result
       ├─ tokenBudget → query loop 自动 continue/stop
       └─ blocking_limit → query loop 返回 Terminal
```

---

## 6. Result 生成逻辑

```
query loop 结束后:
  │
  ├──▶ 查找最后一条 assistant 或 user 消息
  │
  ├──▶ isResultSuccessful(result, lastStopReason)?
  │    成功条件:
  │    ├─ assistant + 最后内容块是 text/thinking/redacted_thinking
  │    ├─ user + 所有内容块都是 tool_result
  │    └─ stop_reason == 'end_turn'
  │
  │    ├─ NO  → yield result(error_during_execution)
  │    │        errors[] = [ede_diagnostic, ...turn-scoped logErrors]
  │    │
  │    └─ YES → 提取 textResult (最后 text 块)
  │            yield result(success)
  │            包含: duration_ms, num_turns, total_cost_usd,
  │                  usage, permission_denials, structured_output,
  │                  stop_reason, session_id
```

---

## 7. TypeScript → Rust 映射表

### 7.1 QueryEngine 本体

| TypeScript | Rust (当前 engine.rs) | 差距分析 |
|-----------|----------------------|---------|
| `class QueryEngine` | `struct QueryEngine` | ✅ 已有 |
| `mutableMessages: Message[]` | `Arc<RwLock<Vec<Message>>>` | ✅ 已有 |
| `abortController: AbortController` | `Arc<AtomicBool>` | ⚠️ 简化了 — TS 版支持 `.reason` (interrupt vs abort) |
| `permissionDenials: SDKPermissionDenial[]` | `Arc<Mutex<Vec<PermissionDenial>>>` | ✅ 已有 |
| `totalUsage: NonNullableUsage` | `Arc<Mutex<UsageTracking>>` | ✅ 已有 |
| `readFileState: FileStateCache` | 缺失 | ❌ 需要添加，跨轮次复用 |
| `discoveredSkillNames: Set<string>` | 缺失 | ❌ 需要添加 (轮次级) |
| `loadedNestedMemoryPaths: Set<string>` | 缺失 | ❌ 需要添加 (会话级去重) |
| `hasHandledOrphanedPermission: boolean` | 缺失 | ❌ 需要添加 |
| `config: QueryEngineConfig` | `config: QueryEngineConfig` | ⚠️ 缺少多个字段 |

### 7.2 submitMessage() 完整管线

| 阶段 | TypeScript 实现 | Rust 状态 | 优先级 |
|------|----------------|-----------|--------|
| **A1** discoveredSkillNames.clear() | 每次 submit 开始 | ❌ 未实现 | P2 |
| **A2** setCwd(cwd) | 设置工作目录 | ❌ 未调用 | P1 |
| **A3** processUserInput() | 斜杠命令、附件处理 | ❌ 未实现 (直接拼字符串) | P1 |
| **A4** 持久化 user message | recordTranscript | ❌ 未调用 | P1 |
| **A5** 消息确认 (replay) | replayUserMessages | ❌ 未实现 | P2 |
| **B1** fetchSystemPromptParts() | 工具+模型 → 系统提示 | ⚠️ 简化版 build_system_prompt | P1 |
| **B2** memoryMechanics 注入 | loadMemoryPrompt() | ❌ 未实现 | P3 |
| **B3** coordinatorUserContext | 多代理协调上下文 | ❌ 未实现 | P3 |
| **C1** canUseTool 包装 | 拦截记录 denial | ❌ 未实现 (denial 不追踪) | P1 |
| **C2** fileHistory 快照 | fileHistoryMakeSnapshot | ❌ 未实现 | P2 |
| **C3** skills/plugins 加载 | getSlashCommandToolSkills | ❌ 未实现 | P3 |
| **C4** yield systemInitMessage | 系统初始化消息 | ❌ 未实现 | P1 |
| **C5** structuredOutput enforcement | registerStructuredOutputEnforcement | ❌ 未实现 | P3 |
| **D1** query loop 消费 | for await of query() | ✅ 已有 (stream 包装) |  |
| **D2** 消息分发 switch | 7 种消息类型处理 | ⚠️ 只处理 Message/RequestStart | P1 |
| **D3** stream_event 处理 | usage 累加, stop_reason 捕获 | ❌ 未实现 | P1 |
| **D4** compact_boundary GC | splice 释放旧消息 | ❌ 未实现 | P1 |
| **D5** snipReplay | snip boundary 裁剪 | ❌ 未实现 | P3 |
| **D6** maxBudgetUsd 检查 | 每消息后检查 | ❌ 未实现 | P1 |
| **D7** structuredOutput 重试限制 | countToolCalls 检查 | ❌ 未实现 | P3 |
| **E1** flush transcript | 结果前持久化 | ❌ 未实现 | P1 |
| **E2** isResultSuccessful() | 结果验证 | ❌ 未实现 | P1 |
| **E3** textResult 提取 | 最后 text 块 | ❌ 未实现 | P1 |
| **E4** yield result | SDK result 消息 | ❌ 未实现 (stream 直接透传) | P1 |

### 7.3 QueryEngineConfig 字段对照

| TypeScript 字段 | Rust 当前 | 需要添加? |
|----------------|-----------|----------|
| `cwd: string` | ✅ `cwd: String` | — |
| `tools: Tools` | ✅ `tools: Tools` | — |
| `commands: Command[]` | ❌ | 是 |
| `mcpClients: MCPServerConnection[]` | ❌ | 是 (Phase 11) |
| `agents: AgentDefinition[]` | ❌ | 是 (Phase 8) |
| `canUseTool: CanUseToolFn` | ❌ | 是 — P1 |
| `getAppState / setAppState` | ❌ (内建 Arc) | 不需要 (Rust 自管理) |
| `initialMessages?: Message[]` | ✅ | — |
| `readFileCache: FileStateCache` | ❌ | 是 — P1 |
| `customSystemPrompt?` | ✅ | — |
| `appendSystemPrompt?` | ✅ | — |
| `userSpecifiedModel?` | ✅ | — |
| `fallbackModel?` | ✅ | — |
| `thinkingConfig?` | ❌ | 是 |
| `maxTurns?` | ✅ | — |
| `maxBudgetUsd?` | ✅ | — |
| `taskBudget?` | ✅ | — |
| `jsonSchema?` | ❌ | 是 (结构化输出) |
| `verbose?` | ✅ | — |
| `replayUserMessages?` | ❌ | 是 — P2 |
| `handleElicitation?` | ❌ | 是 (MCP) |
| `includePartialMessages?` | ❌ | 是 — P2 |
| `setSDKStatus?` | ❌ | 是 — P2 |
| `abortController?` | ✅ (AtomicBool) | — |
| `orphanedPermission?` | ❌ | 是 — P2 |
| `snipReplay?` | ❌ | 是 — P3 |

---

## 8. Rust 重构路线图

### 8.1 P1 — 核心管线补全 (使 submitMessage 产出完整 SDK 消息)

```rust
// 需要新增的类型

/// SDK 输出消息 (对应 TypeScript SDKMessage)
pub enum SdkMessage {
    /// 系统初始化消息
    SystemInit { tools: Vec<String>, model: String, session_id: String },
    /// 助手消息
    Assistant(SdkAssistantMessage),
    /// 用户消息回放
    UserReplay(SdkUserReplay),
    /// 流事件
    StreamEvent(StreamEvent),
    /// 压缩边界
    CompactBoundary { session_id: String, uuid: Uuid, metadata: CompactMetadata },
    /// API 重试
    ApiRetry { attempt: u32, max_retries: u32, delay_ms: u64, error: String },
    /// 工具摘要
    ToolUseSummary { summary: String, tool_use_ids: Vec<String> },
    /// 最终结果
    Result(SdkResult),
}

pub struct SdkResult {
    pub subtype: ResultSubtype,      // success, error_during_execution, error_max_turns, ...
    pub is_error: bool,
    pub duration_ms: u64,
    pub num_turns: usize,
    pub result: String,              // 最后文本
    pub stop_reason: Option<String>,
    pub session_id: String,
    pub total_cost_usd: f64,
    pub usage: UsageTracking,
    pub permission_denials: Vec<PermissionDenial>,
}

pub enum ResultSubtype {
    Success,
    ErrorDuringExecution,
    ErrorMaxTurns,
    ErrorMaxBudgetUsd,
    ErrorMaxStructuredOutputRetries,
}
```

**重构步骤**:

1. **扩展 QueryEngineConfig**: 添加 `read_file_cache`, `can_use_tool`, `json_schema`, `include_partial_messages`, `replay_user_messages` 字段

2. **实现完整消息分发**: `submit_message()` 的 stream 包装器中，从直接透传改为 match 所有 7 种 QueryYield variant，执行 TypeScript 原版的全部逻辑

3. **实现 isResultSuccessful()**: 检查最后消息类型和 stop_reason

4. **实现 SdkResult 生成**: query loop 结束后产出 Result 消息

5. **实现 stream_event 处理**: usage 累加 + stop_reason 捕获

6. **接入 session persistence**: 在消息分发中调用 `crate::session::transcript::record_transcript()`

### 8.2 P2 — 输入处理与增强功能

1. **processUserInput()**: 实现斜杠命令解析 + 附件处理
2. **canUseTool 包装**: denial tracking → permission_denials
3. **fileHistory 快照**: 提交前文件状态快照
4. **systemInitMessage**: 工具列表+模型信息的初始化消息
5. **compact_boundary GC**: splice 释放旧消息防内存泄漏
6. **orphanedPermission**: 会话恢复时的孤儿权限处理

### 8.3 P3 — 高级特性

1. **snipReplay**: HISTORY_SNIP 边界处理
2. **memoryMechanics**: CLAUDE.md 自动内存注入
3. **skills/plugins**: 技能发现 + 插件加载
4. **structuredOutput enforcement**: JSON Schema 验证重试
5. **coordinatorUserContext**: 多代理协调上下文

---

## 9. abort 控制重构

TypeScript 的 AbortController 支持 `.reason` 属性区分中断类型：

```typescript
// TypeScript
abortController.abort('interrupt')  // 用户提交新消息打断
abortController.abort()              // 用户 Ctrl+C
// 检查:
signal.reason !== 'interrupt'  → 显示 "[Request interrupted]"
signal.reason === 'interrupt'  → 静默 (新消息自带上下文)
```

当前 Rust 只用 `AtomicBool`，需要升级：

```rust
pub struct AbortController {
    aborted: AtomicBool,
    reason: Mutex<Option<AbortReason>>,
}

pub enum AbortReason {
    /// 用户提交了新消息 (submit-interrupt): 不显示中断消息
    Interrupt,
    /// 用户按 Ctrl+C 或调用 abort(): 显示中断消息
    UserAbort,
    /// 预算耗尽等系统中断
    System(String),
}

impl AbortController {
    pub fn abort(&self, reason: AbortReason) { ... }
    pub fn is_aborted(&self) -> bool { ... }
    pub fn reason(&self) -> Option<AbortReason> { ... }
    pub fn should_show_interruption_message(&self) -> bool {
        self.reason().map_or(true, |r| !matches!(r, AbortReason::Interrupt))
    }
}
```

---

## 10. 并发模型对照

| 关注点 | TypeScript | Rust |
|--------|-----------|------|
| **消息历史共享** | 直接 `this.mutableMessages` (单线程) | `Arc<RwLock<Vec<Message>>>` |
| **usage 更新** | 直接 `this.totalUsage =` (单线程) | `Arc<Mutex<UsageTracking>>` |
| **transcript 写入** | `void recordTranscript()` (fire-and-forget promise) | `tokio::spawn(async { ... })` |
| **stream 消费** | `for await (const msg of query())` | `while let Some(item) = stream.next().await` |
| **abort 传播** | `AbortSignal` 传入 API fetch | `CancellationToken` + `tokio::select!` |
| **GC 释放** | `splice(0, idx)` 丢弃引用 | `Vec::drain(..idx)` 或 `truncate` |

**关键差异**: TypeScript 是单线程+协作式异步，`mutableMessages` 不需要锁。Rust 中如果 query loop 和 UI 在不同 tokio task，需要 `Arc<RwLock<>>` 或 channel 通信。但如果保持单 task 消费 stream (如当前实现)，可以用 `&mut Vec<Message>` 避免锁开销。

---

## 11. ask() 便捷函数

TypeScript 有一个 `ask()` 函数作为 QueryEngine 的一次性包装：

```typescript
async function* ask({ prompt, tools, ... }): AsyncGenerator<SDKMessage> {
    const engine = new QueryEngine({ ... })
    try {
        yield* engine.submitMessage(prompt)
    } finally {
        setReadFileCache(engine.getReadFileState())  // 回写文件缓存
    }
}
```

Rust 对应：

```rust
pub async fn ask(
    prompt: &str,
    config: QueryEngineConfig,
) -> impl Stream<Item = SdkMessage> {
    let engine = QueryEngine::new(config);
    engine.submit_message(prompt, QuerySource::Sdk)
    // 注意: 文件缓存回写在 engine Drop 时处理，或通过返回 engine 让调用方访问
}
```
