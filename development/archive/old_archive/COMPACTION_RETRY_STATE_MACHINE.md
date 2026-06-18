# 压缩-重试状态机 — Rust 重构文档

> 基于 `cc/src/services/compact/` 和 `cc/src/query.ts` TypeScript 源码分析，面向 `cc/rust/` Rust 实现

---

## 目录

1. [概述：压缩层级架构](#1-概述压缩层级架构)
2. [Query Loop 集成：压缩-重试交互图](#2-query-loop-集成)
3. [Token 阈值体系](#3-token-阈值体系)
4. [AutoCompact 状态机](#4-autocompact-状态机)
5. [Full Compaction 管线](#5-full-compaction-管线)
6. [Session Memory Compaction](#6-session-memory-compaction)
7. [Microcompact 状态机](#7-microcompact-状态机)
8. [Snip Compact](#8-snip-compact)
9. [Reactive Compact (413 恢复)](#9-reactive-compact-413-恢复)
10. [Context Collapse](#10-context-collapse)
11. [Tool Result Budget](#11-tool-result-budget)
12. [Post-Compact 清理与恢复](#12-post-compact-清理与恢复)
13. [消息分组：API Round Grouping](#13-消息分组)
14. [Rust 类型映射](#14-rust-类型映射)
15. [Rust 实现设计](#15-rust-实现设计)
16. [实现优先级与可跳过项](#16-实现优先级)

---

## 1. 概述：压缩层级架构

系统实现了 **6 层**上下文管理机制，按执行优先级排列：

```
Query Loop 每次迭代:

  ┌──────────────────────────────────────────────────────────────────┐
  │ Layer 0: Tool Result Budget                                      │
  │   applyToolResultBudget() — 逐消息大小限制，替换超大结果            │
  │   位置: 最先执行，在所有压缩之前                                    │
  ├──────────────────────────────────────────────────────────────────┤
  │ Layer 1: Snip Compact (HISTORY_SNIP feature)                     │
  │   snipCompactIfNeeded() — 直接删除旧消息段，释放 token              │
  │   位置: tool result budget 之后                                   │
  ├──────────────────────────────────────────────────────────────────┤
  │ Layer 2: Microcompact                                            │
  │   microcompactMessages() — 清除旧工具结果内容                      │
  │   三条路径: 时间触发 → 缓存编辑 → 无操作                           │
  │   位置: snip 之后                                                │
  ├──────────────────────────────────────────────────────────────────┤
  │ Layer 3: Context Collapse (CONTEXT_COLLAPSE feature)             │
  │   applyCollapsesIfNeeded() — 渐进式折叠                           │
  │   位置: microcompact 之后，autocompact 之前                       │
  │   互斥: 启用时压制 autocompact                                    │
  ├──────────────────────────────────────────────────────────────────┤
  │ Layer 4: AutoCompact (主动)                                      │
  │   autoCompactIfNeeded() — 全量摘要压缩                            │
  │   位置: context collapse 之后，API 调用之前                        │
  │   含电路断路器 (max 3 consecutive failures)                       │
  ├──────────────────────────────────────────────────────────────────┤
  │ Layer 5: Reactive Compact / Recovery (被动)                      │
  │   仅在 API 返回 413/max_output_tokens 后触发                     │
  │   位置: 流式响应完成后的恢复分支                                   │
  │   包含: collapse_drain → reactive_compact → escalate → recovery  │
  └──────────────────────────────────────────────────────────────────┘
```

### 源文件映射

| TypeScript 源文件 | 职责 | Rust 目标模块 |
|---|---|---|
| `services/compact/autoCompact.ts` (~352行) | 自动压缩触发 + 电路断路器 | `crate::compact::auto_compact` |
| `services/compact/compact.ts` (~1000+行) | 全量/部分压缩实现 | `crate::compact::compaction` |
| `services/compact/microCompact.ts` (~450行) | 微压缩 (时间/缓存编辑) | `crate::compact::micro` |
| `services/compact/sessionMemoryCompact.ts` | Session Memory 压缩 | `crate::compact::session_memory` |
| `services/compact/grouping.ts` (~64行) | API Round 消息分组 | `crate::compact::grouping` |
| `services/compact/postCompactCleanup.ts` (~78行) | 压缩后状态清理 | `crate::compact::cleanup` |
| `services/compact/prompt.ts` | 压缩提示词 | `crate::compact::prompt` |
| `services/compact/timeBasedMCConfig.ts` (~44行) | 时间触发配置 | `crate::compact::config` |
| `utils/toolResultStorage.ts` (~1041行) | 工具结果持久化 + 大小预算 | `crate::tool::result_storage` |
| `query.ts` (压缩集成段) | 压缩-重试在主循环中的编排 | `crate::query` |

---

## 2. Query Loop 集成

### 2.1 每次迭代的压缩-重试完整流程

```
TS: query.ts — while(true) 循环体

┌─────────────────────────────────────────────────────────────────────┐
│ PHASE A: PRE-API CONTEXT MANAGEMENT                                 │
│                                                                     │
│  messagesForQuery = getMessagesAfterCompactBoundary(messages)       │
│                                                                     │
│  ① applyToolResultBudget(messagesForQuery, contentReplacementState) │
│     → 替换超大工具结果为磁盘引用                                      │
│                                                                     │
│  ② snipCompactIfNeeded(messagesForQuery)  [HISTORY_SNIP]           │
│     → snipTokensFreed, 可能 yield boundaryMessage                   │
│                                                                     │
│  ③ microcompactMessages(messagesForQuery, ctx, querySource)         │
│     → 时间触发清除 / 缓存编辑 / 无操作                               │
│     → 可能产生 pendingCacheEdits                                     │
│                                                                     │
│  ④ applyCollapsesIfNeeded(messagesForQuery, ctx, querySource)       │
│     [CONTEXT_COLLAPSE]                                              │
│     → 应用暂存的折叠                                                 │
│                                                                     │
│  ⑤ autoCompactIfNeeded(messagesForQuery, ctx, ...)                  │
│     → 如果 tokenCount >= threshold:                                  │
│       ├─ 先试 sessionMemoryCompaction                               │
│       └─ 再试 compactConversation (全量摘要)                         │
│     → 成功: yield postCompactMessages, messagesForQuery 替换         │
│     → 失败: consecutiveFailures++, 电路断路器                        │
│                                                                     │
│  ⑥ calculateTokenWarningState → blockingLimit 检查                  │
│     → 达到硬上限 → return Terminal::BlockingLimit                    │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│ PHASE B: API STREAMING + TOOL EXECUTION                             │
│  (见 TOOL_EXECUTION_STATE_MACHINE.md)                               │
├─────────────────────────────────────────────────────────────────────┤
│ PHASE C: POST-STREAMING RECOVERY (needsFollowUp == false)           │
│                                                                     │
│  检查最后一条 assistant message:                                     │
│                                                                     │
│  ⑦ isWithheld413 (prompt_too_long 被扣留):                         │
│     ├─ C1: collapse_drain_retry                                     │
│     │   contextCollapse.recoverFromOverflow()                       │
│     │   if committed > 0 → continue(collapse_drain_retry)           │
│     │                                                               │
│     ├─ C2: reactive_compact_retry                                   │
│     │   reactiveCompact.tryReactiveCompact()                        │
│     │   if compacted → continue(reactive_compact_retry)             │
│     │                                                               │
│     └─ C3: 不可恢复 → yield error, return Terminal::PromptTooLong   │
│                                                                     │
│  ⑧ isWithheldMaxOutputTokens:                                      │
│     ├─ C4: max_output_tokens_escalate (8k→64k, 一次性)              │
│     │   → continue(max_output_tokens_escalate)                      │
│     │                                                               │
│     ├─ C5: max_output_tokens_recovery (注入续写消息, max 3次)        │
│     │   → continue(max_output_tokens_recovery)                      │
│     │                                                               │
│     └─ C6: 恢复耗尽 → yield error                                   │
│                                                                     │
│  ⑨ stopHooks / tokenBudget / completed 检查                        │
│     (见 TOOL_EXECUTION_STATE_MACHINE.md)                            │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│ PHASE D: TOOL EXECUTION → NEXT TURN                                │
│  → continue(next_turn)                                              │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 压缩-重试 Continue 路径汇总

| Continue Reason | 触发源 | 重要细节 |
|---|---|---|
| `collapse_drain_retry` | 413 + context collapse 排空 | 一次性尝试，不循环 |
| `reactive_compact_retry` | 413 / media error + reactive compact | `hasAttemptedReactiveCompact = true` 防循环 |
| `max_output_tokens_escalate` | max_output_tokens + 未覆盖 | 8k→64k，一次性 |
| `max_output_tokens_recovery` | max_output_tokens + count < 3 | 注入续写指令，最多 3 次 |
| `next_turn` | 正常工具执行完成 | `hasAttemptedReactiveCompact = false` 重置 |

### 2.3 扣留 (Withholding) 机制

```
TS: query.ts ~800-825 — 流式循环内

API 错误消息在流式循环中被**扣留** (不立即 yield):
  - prompt_too_long → 由 contextCollapse 或 reactiveCompact 判断
  - max_output_tokens → 由 isWithheldMaxOutputTokens 判断
  - media_size_error → 由 reactiveCompact 判断 (需 mediaRecoveryEnabled)

扣留的消息仍 push 到 assistantMessages，但不 yield 到消费者。
后续恢复路径检查这些消息，决定:
  - 恢复成功 → 消息被丢弃，continue 新的迭代
  - 恢复失败 → yield 被扣留的消息，return Terminal
```

---

## 3. Token 阈值体系

### 3.1 常量定义

```
TS: services/compact/autoCompact.ts

MAX_OUTPUT_TOKENS_FOR_SUMMARY = 20,000     // 压缩输出预留
AUTOCOMPACT_BUFFER_TOKENS     = 13,000     // 自动压缩缓冲
WARNING_THRESHOLD_BUFFER_TOKENS = 20,000   // 警告阈值缓冲
ERROR_THRESHOLD_BUFFER_TOKENS   = 20,000   // 错误阈值缓冲
MANUAL_COMPACT_BUFFER_TOKENS    = 3,000    // 手动压缩预留（硬上限）
```

### 3.2 阈值计算

```
contextWindow = getContextWindowForModel(model)  // 如 200k
  ↓ cap by CLAUDE_CODE_AUTO_COMPACT_WINDOW env (可选)

effectiveContextWindow = contextWindow - min(maxOutputTokens, 20k)
  ↓ 例: 200k - 20k = 180k

autoCompactThreshold = effectiveContextWindow - 13k
  ↓ 例: 180k - 13k = 167k
  ↓ cap by CLAUDE_AUTOCOMPACT_PCT_OVERRIDE env (可选百分比)

warningThreshold = threshold - 20k        例: 147k
errorThreshold   = threshold - 20k        例: 147k
blockingLimit    = effectiveContextWindow - 3k   例: 177k
  ↓ override by CLAUDE_CODE_BLOCKING_LIMIT_OVERRIDE env
```

### 3.3 阈值判定函数

```
calculateTokenWarningState(tokenUsage, model) → {
  percentLeft: max(0, round(((threshold - usage) / threshold) * 100)),
  isAboveWarningThreshold: usage >= warningThreshold,
  isAboveErrorThreshold:   usage >= errorThreshold,
  isAboveAutoCompactThreshold: isAutoCompactEnabled() && usage >= autoCompactThreshold,
  isAtBlockingLimit:       usage >= blockingLimit,
}

其中 threshold = isAutoCompactEnabled() ? autoCompactThreshold : effectiveContextWindow
```

### 3.4 视觉阈值图

```
Token 使用量 →

0%                                            ~82%        ~93%      ~98%  100%
├─────────────────────────────────────────────┼───────────┼─────────┼────┤
│               正常区域                       │  警告区域  │ 压缩区域│硬限│
│                                             │           │         │    │
│                                             warning   autocompact │    │
│                                             threshold  threshold  │blocking
│                                                                   │ limit
│                                                   effectiveContextWindow
```

---

## 4. AutoCompact 状态机

### 4.1 跟踪状态

```
TS: autoCompact.ts:51-60

AutoCompactTrackingState {
  compacted: boolean           // 本轮是否已压缩
  turnCounter: number          // 上次压缩后的轮次计数
  turnId: string               // 唯一轮次 ID
  consecutiveFailures?: number // 连续失败计数 (电路断路器)
}
```

### 4.2 决策树

```
shouldAutoCompact(messages, model, querySource, snipTokensFreed):

  ├─ querySource === 'session_memory' | 'compact' → false (递归保护)
  ├─ querySource === 'marble_origami' + CONTEXT_COLLAPSE → false
  ├─ !isAutoCompactEnabled() → false
  │   ├─ DISABLE_COMPACT env → false
  │   ├─ DISABLE_AUTO_COMPACT env → false
  │   └─ !globalConfig.autoCompactEnabled → false
  ├─ REACTIVE_COMPACT + tengu_cobalt_raccoon → false (reactive-only 模式)
  ├─ CONTEXT_COLLAPSE + isContextCollapseEnabled() → false (collapse 拥有上下文管理)
  └─ tokenCount = tokenCountWithEstimation(messages) - snipTokensFreed
     └─ return tokenCount >= autoCompactThreshold
```

### 4.3 电路断路器

```
autoCompactIfNeeded():

  ┌─────────────────────┐
  │ 初始状态             │
  │ failures = 0        │
  └──────────┬──────────┘
             │
  ┌──────────▼──────────┐
  │ DISABLE_COMPACT?    │─── yes ──→ return {wasCompacted: false}
  └──────────┬──────────┘
             │ no
  ┌──────────▼──────────────────┐
  │ failures >= 3 (MAX)?       │─── yes ──→ return {wasCompacted: false}
  │ (电路断路器开路)            │           (永久跳过本 session)
  └──────────┬──────────────────┘
             │ no
  ┌──────────▼──────────┐
  │ shouldAutoCompact()?│─── false ──→ return {wasCompacted: false}
  └──────────┬──────────┘
             │ true
  ┌──────────▼───────────────────┐
  │ trySessionMemoryCompaction() │
  └──────────┬───────────────────┘
             │
      ┌──────┼──────┐
      │success│ null │
      │      │      │
      ▼      │      ▼
  return {   │  ┌────────────────────┐
  wasCompacted│ │ compactConversation()│
  = true}    │  └──────────┬─────────┘
             │             │
             │    ┌────────┼────────┐
             │    │success │ error  │
             │    │        │        │
             │    ▼        ▼        │
             │  return {  failures++│
             │  wasCompacted: true, │
             │  failures: 0}       │
             │             │        │
             │             ▼        │
             │    if >= MAX_FAILURES│
             │    → log "circuit    │
             │      breaker tripped"│
             │             │        │
             │             ▼        │
             │    return {          │
             │    wasCompacted:false│
             │    failures: N}     │
             └─────────────────────┘

注意: failures 由 query loop 在 AutoCompactTrackingState 中跨轮次传递
```

---

## 5. Full Compaction 管线

### 5.1 CompactionResult 结构

```
TS: compact.ts:299-310

CompactionResult {
  boundaryMarker: SystemMessage         // 标记压缩边界
  summaryMessages: UserMessage[]        // 生成的摘要
  attachments: AttachmentMessage[]      // 压缩后上下文恢复附件
  hookResults: HookResultMessage[]      // session_start hook 输出
  messagesToKeep?: Message[]            // 保留的消息后缀 (部分/session memory)
  userDisplayMessage?: string           // 用户可见状态信息
  preCompactTokenCount?: number
  postCompactTokenCount?: number        // 压缩 API 调用总用量 (非结果大小)
  truePostCompactTokenCount?: number    // 结果上下文的真实估算大小
  compactionUsage?: TokenUsage          // API token 使用详情
}
```

### 5.2 buildPostCompactMessages 顺序

```
TS: compact.ts:330-338

[boundaryMarker, ...summaryMessages, ...messagesToKeep, ...attachments, ...hookResults]

这个顺序是所有压缩路径的标准输出:
  1. 边界标记 (系统消息)
  2. 摘要内容 (用户消息)
  3. 保留的原始消息 (可选, session memory / partial)
  4. 恢复附件 (文件内容、计划、技能、工具列表)
  5. Hook 产出 (session_start hooks)
```

### 5.3 compactConversation 8 阶段流水线

```
TS: compact.ts:387-763

Phase 1: INITIALIZATION
  ├─ messages.length === 0 → throw "Not enough messages"
  ├─ preCompactTokenCount = tokenCountWithEstimation(messages)
  ├─ executePreCompactHooks({trigger: auto|manual})
  └─ mergeHookInstructions(custom, hook)

Phase 2: STREAMING + PTL RETRY LOOP
  ├─ summaryRequest = getCompactPrompt(instructions)
  ├─ for (;;):
  │   ├─ summaryResponse = streamCompactSummary({messages, ...})
  │   ├─ summary = getAssistantMessageText(summaryResponse)
  │   ├─ if !summary.startsWith(PROMPT_TOO_LONG) → break (成功)
  │   │
  │   ├─ PTL 重试:
  │   │   ptlAttempts++
  │   │   if ptlAttempts > MAX_PTL_RETRIES (3) → throw PROMPT_TOO_LONG
  │   │   truncated = truncateHeadForPTLRetry(messages, response)
  │   │   if !truncated → throw PROMPT_TOO_LONG
  │   │   messagesToSummarize = truncated
  │   │   continue (重试)
  │   │
  │   └─ 其他错误:
  │       if !summary → throw "no summary"
  │       if startsWithApiErrorPrefix → throw summary
  │
  └─ streamCompactSummary 内部:
      ├─ Path 1: runForkedAgent() — prompt cache 共享 (默认启用)
      │   └─ 失败 → fallback 到 Path 2
      ├─ Path 2: queryModelWithStreaming() — 直接流式调用
      │   └─ maxAttempts = tengu_compact_streaming_retry ? 2 : 1
      │   └─ 失败 → throw "Compaction interrupted"
      └─ 30s session activity keepalive

Phase 3: CACHE INVALIDATION
  ├─ preCompactReadFileState = snapshot(readFileState)
  ├─ readFileState.clear()
  └─ loadedNestedMemoryPaths.clear()

Phase 4: POST-COMPACT ATTACHMENTS (并行)
  ├─ Promise.all([
  │   createPostCompactFileAttachments(state, ctx, maxFiles=5),
  │   createAsyncAgentAttachmentsIfNeeded(ctx),
  │ ])
  ├─ createPlanAttachmentIfNeeded()
  ├─ createPlanModeAttachmentIfNeeded()
  ├─ createSkillAttachmentIfNeeded()
  ├─ getDeferredToolsDeltaAttachment()
  ├─ getAgentListingDeltaAttachment()
  └─ getMcpInstructionsDeltaAttachment()

Phase 5: BOUNDARY + SUMMARY CREATION
  ├─ boundaryMarker = createCompactBoundaryMessage(auto|manual, tokenCount)
  ├─ 携带 preCompactDiscoveredTools (延迟工具跟踪)
  ├─ summaryMessages = [createUserMessage(summary)]
  └─ processSessionStartHooks('compact')

Phase 6: TELEMETRY
  ├─ truePostCompactTokenCount = 消息估算
  ├─ willRetriggerNextTurn = true if still above threshold
  ├─ logEvent('tengu_compact', {...})
  └─ notifyCompaction() (prompt cache break detection)

Phase 7: SESSION METADATA + TRANSCRIPT
  ├─ reAppendSessionMetadata()  // 保持 session title 在 16KB tail 内
  └─ writeSessionTranscriptSegment(messages) [KAIROS, fire-and-forget]

Phase 8: POST-COMPACT HOOKS
  ├─ executePostCompactHooks({trigger, compactSummary})
  └─ return CompactionResult
```

### 5.4 PTL 重试：truncateHeadForPTLRetry

```
TS: compact.ts:243-291

truncateHeadForPTLRetry(messages, ptlResponse):
  1. 清除前次重试标记 (PTL_RETRY_MARKER)
  2. groups = groupMessagesByApiRound(messages)
  3. if groups.length < 2 → return null (无法再删)

  4. 计算 dropCount:
     ├─ tokenGap 可解析 → 累积删除组直到覆盖 gap
     └─ tokenGap 不可解析 → 删除 20% 的组

  5. dropCount = min(dropCount, groups.length - 1)  // 至少保留 1 组
  6. if dropCount < 1 → return null

  7. sliced = groups.slice(dropCount).flat()
  8. if sliced[0].type === 'assistant' → 前插 PTL_RETRY_MARKER
  9. return sliced
```

### 5.5 部分压缩

```
TS: compact.ts:772+

partialCompactConversation(allMessages, pivotIndex, ctx, params, feedback?, direction='from')

  direction === 'from':
    messagesToSummarize = allMessages.slice(pivotIndex)    // 后半
    messagesToKeep = allMessages.slice(0, pivotIndex)      // 前半 (保留)
    → prompt cache 对 kept 消息有效

  direction === 'up_to':
    messagesToSummarize = allMessages.slice(0, pivotIndex) // 前半
    messagesToKeep = allMessages.slice(pivotIndex)          // 后半 (保留)
    → prompt cache 失效 (摘要在 kept 前面)
    → 清除旧 compact boundary (防止 backward scan 错误)

  后续阶段与 full compaction 相同
  boundaryMarker 附加 preservedSegment 元数据:
    { headUuid, anchorUuid, tailUuid }
```

---

## 6. Session Memory Compaction

### 6.1 配置

```
TS: sessionMemoryCompact.ts:47-61

SessionMemoryCompactConfig {
  minTokens: 10,000          // 保留最少 token
  minTextBlockMessages: 5    // 保留最少含文本消息数
  maxTokens: 40,000          // 保留最多 token (硬上限)
}
```

### 6.2 决策树

```
trySessionMemoryCompaction(messages, agentId, autoCompactThreshold?):

  Gate 1: Feature Checks
    ├─ shouldUseSessionMemoryCompaction()
    │   ├─ ENABLE_CLAUDE_CODE_SM_COMPACT env override
    │   ├─ DISABLE_CLAUDE_CODE_SM_COMPACT env override
    │   └─ tengu_session_memory + tengu_sm_compact 特性标志
    └─ 任何 gate 失败 → return null (回退到 legacy compaction)

  Gate 2: Session Memory 可用性
    ├─ waitForSessionMemoryExtraction() with timeout
    ├─ session memory 文件不存在 → return null
    ├─ session memory 为空模板 → return null
    └─ continue

  计算保留消息范围:
    ├─ 找到 lastSummarizedMessageId 在消息中的位置
    ├─ calculateMessagesToKeepIndex():
    │   ├─ 从 lastSummarizedIndex + 1 开始
    │   ├─ 向后扩展直到满足:
    │   │   ├─ >= minTokens
    │   │   ├─ >= minTextBlockMessages
    │   │   └─ <= maxTokens (硬上限)
    │   ├─ 不超过最后一个 compactBoundary (磁盘链完整性)
    │   └─ adjustIndexToPreserveAPIInvariants():
    │       ├─ tool_use/tool_result 配对完整
    │       └─ thinking blocks 不被孤立
    │
    └─ messagesToKeep = messages.slice(startIndex).filter(!boundary)

  阈值检查:
    ├─ postCompactTokenCount = estimateMessageTokens(result)
    ├─ if postCompactTokenCount >= autoCompactThreshold
    │   → return null (太大，交给 legacy)
    └─ continue

  返回 CompactionResult:
    ├─ summary = session memory 内容
    ├─ messagesToKeep = 计算的范围
    ├─ boundaryMarker + preservedSegment 注解
    └─ hookResults = processSessionStartHooks()
```

---

## 7. Microcompact 状态机

### 7.1 三路分支

```
TS: microCompact.ts:253-293

microcompactMessages(messages, ctx?, querySource?):

  ┌─────────────────────────┐
  │ 清除 compactWarning     │
  └──────────┬──────────────┘
             │
  ┌──────────▼──────────────────────┐
  │ Path 1: 时间触发微压缩           │
  │ maybeTimeBasedMicrocompact()    │
  │                                 │
  │ 条件:                           │
  │   config.enabled &&             │
  │   querySource 是主线程 &&        │
  │   存在 assistant 消息 &&         │
  │   gap >= gapThresholdMinutes    │
  │   (默认 60 分钟)                │
  │                                 │
  │ 动作:                           │
  │   收集 COMPACTABLE_TOOLS 的 ID  │
  │   保留最近 keepRecent 个         │
  │   清除其余: content → "[Old...] │
  │   resetMicrocompactState()      │
  │   返回 {messages: 已修改}       │
  └──────────┬──────────────────────┘
             │ null (未触发)
  ┌──────────▼──────────────────────────┐
  │ Path 2: 缓存编辑微压缩              │
  │ [CACHED_MICROCOMPACT feature]       │
  │                                     │
  │ 条件:                               │
  │   isCachedMicrocompactEnabled() &&  │
  │   isModelSupportedForCacheEditing()│
  │   && isMainThreadSource()          │
  │                                     │
  │ 动作:                               │
  │   注册新 tool_result 到全局状态      │
  │   getToolResultsToDelete(state)     │
  │   if deletable.length > 0:          │
  │     createCacheEditsBlock()         │
  │     pendingCacheEdits = block       │
  │     → API 层在下次调用时发送        │
  │   返回 {messages, compactionInfo}   │
  └──────────┬──────────────────────────┘
             │ (未启用/无变化)
  ┌──────────▼──────────────┐
  │ Path 3: 无操作           │
  │ return {messages}       │
  └─────────────────────────┘
```

### 7.2 可压缩工具集

```
COMPACTABLE_TOOLS = {
  Read, Bash/PowerShell, Grep, Glob,
  WebSearch, WebFetch, Edit, Write
}
```

### 7.3 缓存编辑状态 (模块级)

```
CachedMCState {
  registeredTools: Set<tool_use_id>      // 已注册的工具结果
  toolOrder: [tool_use_id]               // 注册顺序
  deletedRefs: Set<tool_use_id>          // 已删除的引用
  pinnedEdits: [{userMessageIndex, block}] // 固定的编辑位置
}

状态转换:
  registerToolResult(id) → registeredTools.add(id), toolOrder.push(id)
  getToolResultsToDelete() → 超出阈值的旧工具
  createCacheEditsBlock() → deletedRefs.add(ids)
  markToolsSentToAPI() → 快照 sent 状态
  resetCachedMCState() → 全部清空 (compact/时间触发后)
```

---

## 8. Snip Compact

```
[HISTORY_SNIP feature — 在外部构建中不存在]

snipCompactIfNeeded(messages):
  → { messages: Message[], tokensFreed: number, boundaryMessage?: Message }

核心语义:
  - 直接删除旧消息段 (不生成摘要)
  - tokensFreed 传递给 autoCompact 的 snipTokensFreed 参数
  - 因为存活的 assistant 的 usage 仍反映裁剪前的上下文
    → tokenCountWithEstimation 看不到节省
    → 需要手动减去 snipTokensFreed

在 query loop 中的位置:
  snip → microcompact → contextCollapse → autoCompact
  snip 的 tokensFreed 影响 autoCompact 阈值判定
```

---

## 9. Reactive Compact (413 恢复)

```
[REACTIVE_COMPACT feature — 在外部构建中可能不存在]

isReactiveCompactEnabled(): boolean
  → GrowthBook 特性标志检查

isWithheldPromptTooLong(message): boolean
  → 判断消息是否为 prompt_too_long 错误
  → 用于流式循环中的扣留决策

isWithheldMediaSizeError(message): boolean
  → 判断消息是否为 media_size 错误
  → 需要 mediaRecoveryEnabled gate

tryReactiveCompact({hasAttempted, querySource, aborted, messages, cacheSafeParams}):
  ├─ hasAttempted === true → return null (防循环)
  ├─ aborted → return null
  ├─ 调用 compactConversation (全量摘要)
  ├─ 成功 → return CompactionResult
  └─ 失败 → return null (错误由上层处理)

在 query loop 中:
  1. 流式循环扣留 413 消息
  2. needsFollowUp === false 时检查
  3. 先尝试 collapse_drain (如果 CONTEXT_COLLAPSE 启用)
  4. 再尝试 reactive_compact
  5. 都失败 → yield 被扣留的错误, return Terminal
```

---

## 10. Context Collapse

```
[CONTEXT_COLLAPSE feature — 计划中，集成点已实现]

isContextCollapseEnabled(): boolean
  → feature 和 env 检查

applyCollapsesIfNeeded(messages, ctx, querySource):
  → Promise<{messages}>
  → 在 microcompact 之后、autoCompact 之前运行
  → 可能提交暂存的折叠

recoverFromOverflow(messages, querySource):
  → {messages, committed}
  → 排空所有暂存折叠
  → committed > 0 时触发 collapse_drain_retry

resetContextCollapse():
  → 由 postCompactCleanup 调用

互斥关系:
  - 启用时: 压制 shouldAutoCompact (返回 false)
  - 压制 blockingLimit 预检 (让 413 触发恢复)
  - 不压制 reactiveCompact (作为后备)
  - 不压制 isAutoCompactEnabled() (其他地方可查询)

阈值设计:
  ~90% effectiveContext → collapse commit 开始
  ~93% effectiveContext → autoCompact (被压制)
  ~95% effectiveContext → blocking spawn
```

---

## 11. Tool Result Budget

### 11.1 ContentReplacementState

```
TS: utils/toolResultStorage.ts

ContentReplacementState {
  seenIds: Set<string>                    // 已遇到的 tool_use_id
  replacements: Map<string, string>       // id → 替换内容
}
```

### 11.2 applyToolResultBudget

```
applyToolResultBudget(messages, state?, writeToTranscript?, skipToolNames?):
  ├─ state 为空 → 直接返回 messages (特性未启用)
  ├─ enforceToolResultBudget(messages, state, skipToolNames)
  │   ├─ 遍历消息中的 tool_result blocks
  │   ├─ 超大结果 → 替换为磁盘引用 + preview
  │   └─ 记录 newlyReplaced
  ├─ if newlyReplaced.length > 0:
  │   └─ writeToTranscript(records) — 持久化替换记录
  └─ return result.messages
```

### 11.3 工具结果持久化

```
persistToolResult(content, toolUseId):
  ├─ 写入 projectDir/sessionId/tool-results/{id}.{txt|json}
  ├─ flag='wx' (独占创建，防竞争)
  ├─ EEXIST → 已持久化 (幂等)
  └─ 返回 {filepath, originalSize, preview, hasMore}

getPersistenceThreshold(toolName, declaredMax):
  ├─ Infinity → Infinity (如 Read 工具, 自限大小)
  ├─ GrowthBook override (tengu_satin_quoll) → 使用覆盖值
  └─ else → min(declaredMax, DEFAULT_MAX = 50k chars)
```

---

## 12. Post-Compact 清理与恢复

### 12.1 runPostCompactCleanup

```
TS: postCompactCleanup.ts:31-77

runPostCompactCleanup(querySource?):

  主线程清理 (isMainThread = !querySource || startsWith('repl_main_thread') || 'sdk'):
    ├─ resetContextCollapse() [CONTEXT_COLLAPSE]
    ├─ getUserContext.cache.clear()
    └─ resetGetMemoryFilesCache('compact')

  所有线程清理:
    ├─ resetMicrocompactState()
    ├─ clearSystemPromptSections()
    ├─ clearClassifierApprovals()
    ├─ clearSpeculativeChecks()
    ├─ clearBetaTracingState()
    ├─ sweepFileContentCache() [COMMIT_ATTRIBUTION, async]
    └─ clearSessionMessagesCache()

  **不清理** (跨压缩保留):
    ├─ invoked skill content (~4K tokens)
    ├─ sentSkillNames
    └─ file state (post-compact 附件需要它)
```

### 12.2 Post-Compact 附件常量

```
POST_COMPACT_MAX_FILES_TO_RESTORE = 5      // 恢复最多 5 个文件
POST_COMPACT_TOKEN_BUDGET = 50,000         // 文件恢复总 token 预算
POST_COMPACT_MAX_TOKENS_PER_FILE = 5,000   // 单文件 token 上限
POST_COMPACT_MAX_TOKENS_PER_SKILL = 5,000  // 单技能 token 上限
POST_COMPACT_SKILLS_TOKEN_BUDGET = 25,000  // 技能恢复总 token 预算
```

---

## 13. 消息分组

### groupMessagesByApiRound

```
TS: grouping.ts:22-63

算法:
  for each message:
    if message.type === 'assistant'
       && message.id !== lastAssistantId
       && current.length > 0:
      → 开始新组 (边界触发)
    else:
      → 加入当前组

  if message.type === 'assistant':
    lastAssistantId = message.id

用途:
  - truncateHeadForPTLRetry: 按组删除最旧消息
  - reactive compact: 按组分割消息
  - 保证 tool_use/tool_result 不被拆分 (API 约束)

优势 (vs 旧的人类轮次分组):
  - 适用于单提示代理会话 (SDK/CCR/eval)
  - 更细粒度 (每个 API round 一组)
  - streaming 的多个 assistant 块共享 id → 同一组
```

---

## 14. Rust 类型映射

### 14.1 核心状态类型

```rust
/// 对应 AutoCompactTrackingState
#[derive(Debug, Clone)]
pub struct AutoCompactTracking {
    pub compacted: bool,
    pub turn_counter: u32,
    pub turn_id: String,
    pub consecutive_failures: u32,
}

impl AutoCompactTracking {
    pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;

    pub fn new() -> Self {
        Self {
            compacted: false,
            turn_counter: 0,
            turn_id: String::new(),
            consecutive_failures: 0,
        }
    }

    pub fn is_circuit_broken(&self) -> bool {
        self.consecutive_failures >= Self::MAX_CONSECUTIVE_FAILURES
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }
}
```

### 14.2 Token 阈值

```rust
/// 对应 autoCompact.ts 常量
pub mod thresholds {
    pub const MAX_OUTPUT_TOKENS_FOR_SUMMARY: u32 = 20_000;
    pub const AUTOCOMPACT_BUFFER_TOKENS: u32 = 13_000;
    pub const WARNING_THRESHOLD_BUFFER: u32 = 20_000;
    pub const ERROR_THRESHOLD_BUFFER: u32 = 20_000;
    pub const MANUAL_COMPACT_BUFFER: u32 = 3_000;
}

/// 对应 calculateTokenWarningState 返回值
#[derive(Debug, Clone)]
pub struct TokenWarningState {
    pub percent_left: u32,
    pub is_above_warning: bool,
    pub is_above_error: bool,
    pub is_above_auto_compact: bool,
    pub is_at_blocking_limit: bool,
}

pub fn effective_context_window(model: &str) -> u32 {
    let context_window = get_context_window_for_model(model);
    let max_output = get_max_output_tokens(model);
    let reserved = max_output.min(thresholds::MAX_OUTPUT_TOKENS_FOR_SUMMARY);
    context_window - reserved
}

pub fn auto_compact_threshold(model: &str) -> u32 {
    effective_context_window(model) - thresholds::AUTOCOMPACT_BUFFER_TOKENS
}

pub fn blocking_limit(model: &str) -> u32 {
    effective_context_window(model) - thresholds::MANUAL_COMPACT_BUFFER
}
```

### 14.3 CompactionResult

```rust
/// 对应 CompactionResult interface
pub struct CompactionResult {
    pub boundary_marker: SystemMessage,
    pub summary_messages: Vec<UserMessage>,
    pub attachments: Vec<AttachmentMessage>,
    pub hook_results: Vec<HookResultMessage>,
    pub messages_to_keep: Option<Vec<Message>>,
    pub user_display_message: Option<String>,
    pub pre_compact_token_count: Option<u32>,
    pub post_compact_token_count: Option<u32>,
    pub true_post_compact_token_count: Option<u32>,
    pub compaction_usage: Option<TokenUsage>,
}

impl CompactionResult {
    /// 对应 buildPostCompactMessages
    pub fn build_post_compact_messages(&self) -> Vec<Message> {
        let mut result = vec![self.boundary_marker.clone().into()];
        result.extend(self.summary_messages.iter().cloned().map(Into::into));
        if let Some(keep) = &self.messages_to_keep {
            result.extend(keep.iter().cloned());
        }
        result.extend(self.attachments.iter().cloned().map(Into::into));
        result.extend(self.hook_results.iter().cloned().map(Into::into));
        result
    }
}
```

### 14.4 Microcompact

```rust
/// 可压缩工具集
pub static COMPACTABLE_TOOLS: Lazy<HashSet<&str>> = Lazy::new(|| {
    [
        "Read", "Bash", "PowerShell", "Grep", "Glob",
        "WebSearch", "WebFetch", "Edit", "Write",
    ].into_iter().collect()
});

/// 对应 MicrocompactResult
pub struct MicrocompactResult {
    pub messages: Vec<Message>,
    pub compaction_info: Option<CompactionInfo>,
}

pub struct CompactionInfo {
    pub pending_cache_edits: Option<PendingCacheEdits>,
}

pub struct PendingCacheEdits {
    pub trigger: String,  // "auto"
    pub deleted_tool_ids: Vec<String>,
    pub baseline_cache_deleted_tokens: u32,
}

/// 对应 TimeBasedMCConfig
pub struct TimeBasedMCConfig {
    pub enabled: bool,
    pub gap_threshold_minutes: u32,  // 默认 60
    pub keep_recent: usize,          // 默认 5
}

/// 对应 SessionMemoryCompactConfig
pub struct SessionMemoryCompactConfig {
    pub min_tokens: u32,              // 默认 10,000
    pub min_text_block_messages: u32, // 默认 5
    pub max_tokens: u32,              // 默认 40,000
}
```

### 14.5 AutoCompact 决策结果

```rust
/// autoCompactIfNeeded 返回值
pub enum AutoCompactOutcome {
    NotNeeded,
    Compacted {
        result: CompactionResult,
        consecutive_failures: u32,
    },
    Failed {
        consecutive_failures: u32,
    },
}
```

### 14.6 Content Replacement

```rust
/// 对应 ContentReplacementState
pub struct ContentReplacementState {
    pub seen_ids: HashSet<String>,
    pub replacements: HashMap<String, String>,
}

/// 对应 ContentReplacementRecord
pub enum ContentReplacementRecord {
    ToolResult {
        tool_use_id: String,
        replacement: String,
    },
}
```

---

## 15. Rust 实现设计

### 15.1 压缩管线作为 trait

```rust
/// 统一压缩接口 — 所有压缩层实现此 trait
#[async_trait]
pub trait CompactionLayer: Send + Sync {
    /// 层名称 (用于日志)
    fn name(&self) -> &str;

    /// 是否启用
    fn is_enabled(&self) -> bool;

    /// 尝试压缩，返回修改后的消息
    async fn apply(
        &mut self,
        messages: Vec<Message>,
        ctx: &CompactionContext,
    ) -> CompactionLayerResult;
}

pub enum CompactionLayerResult {
    /// 无变化
    Unchanged(Vec<Message>),
    /// 消息已修改 (tool result cleared, snipped, etc.)
    Modified(Vec<Message>),
    /// 全量压缩完成
    Compacted(CompactionResult),
}
```

### 15.2 压缩管线编排器

```rust
/// 在 query loop 中调用，编排所有压缩层
pub struct CompactionPipeline {
    tool_result_budget: ToolResultBudgetLayer,
    snip: Option<SnipCompactLayer>,
    microcompact: MicrocompactLayer,
    context_collapse: Option<ContextCollapseLayer>,
    auto_compact: AutoCompactLayer,
}

impl CompactionPipeline {
    /// Phase A: Pre-API 上下文管理
    pub async fn run_pre_api(
        &mut self,
        messages: Vec<Message>,
        ctx: &CompactionContext,
    ) -> PreApiResult {
        let mut msgs = messages;

        // Layer 0: Tool result budget
        msgs = self.tool_result_budget.apply(msgs, ctx).await.into_messages();

        // Layer 1: Snip
        let mut snip_tokens_freed = 0;
        if let Some(snip) = &mut self.snip {
            let result = snip.apply(msgs, ctx).await;
            snip_tokens_freed = result.tokens_freed;
            msgs = result.messages;
        }

        // Layer 2: Microcompact
        let mc_result = self.microcompact.apply(msgs, ctx).await;
        msgs = mc_result.messages;
        let pending_cache_edits = mc_result.pending_cache_edits;

        // Layer 3: Context collapse
        if let Some(collapse) = &mut self.context_collapse {
            msgs = collapse.apply(msgs, ctx).await.into_messages();
        }

        // Layer 4: AutoCompact
        let compact_result = self.auto_compact.apply_with_snip(
            msgs.clone(), ctx, snip_tokens_freed
        ).await;

        PreApiResult {
            messages: match &compact_result {
                AutoCompactOutcome::Compacted { result, .. } =>
                    result.build_post_compact_messages(),
                _ => msgs,
            },
            compact_result,
            pending_cache_edits,
            snip_tokens_freed,
        }
    }
}
```

### 15.3 恢复状态机

```rust
/// Post-streaming recovery (Phase C)
pub async fn attempt_recovery(
    state: &mut QueryLoopState,
    last_message: &AssistantMessage,
    config: &RecoveryConfig,
) -> RecoveryDecision {

    // 1. Prompt-too-long recovery
    if is_prompt_too_long(last_message) {
        // 1a. Context collapse drain
        if let Some(collapse) = &mut config.context_collapse {
            if state.transition.as_ref().map_or(true, |t| t != &ContinueReason::CollapseDrainRetry{..}) {
                let drained = collapse.recover_from_overflow(&state.messages);
                if drained.committed > 0 {
                    return RecoveryDecision::Continue(ContinueReason::CollapseDrainRetry {
                        committed: drained.committed,
                    });
                }
            }
        }

        // 1b. Reactive compact
        if !state.has_attempted_reactive_compact {
            if let Some(result) = try_reactive_compact(&state.messages, config).await {
                return RecoveryDecision::Continue(ContinueReason::ReactiveCompactRetry);
            }
        }

        // 1c. Unrecoverable
        return RecoveryDecision::Terminal(TerminalReason::PromptTooLong);
    }

    // 2. Max output tokens recovery
    if is_max_output_tokens(last_message) {
        // 2a. Escalate (8k → 64k, one-shot)
        if state.max_output_tokens_override.is_none() && config.cap_enabled {
            return RecoveryDecision::Continue(ContinueReason::MaxOutputTokensEscalate);
        }

        // 2b. Multi-turn recovery (max 3)
        if state.max_output_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT {
            return RecoveryDecision::Continue(ContinueReason::MaxOutputTokensRecovery {
                attempt: state.max_output_tokens_recovery_count + 1,
            });
        }

        // 2c. Exhausted
        return RecoveryDecision::YieldError;
    }

    // 3. Normal completion
    RecoveryDecision::Completed
}

pub enum RecoveryDecision {
    Continue(ContinueReason),
    Terminal(TerminalReason),
    YieldError,    // yield 被扣留的错误后 return
    Completed,     // 正常完成
}
```

### 15.4 消息分组

```rust
/// 对应 groupMessagesByApiRound
pub fn group_messages_by_api_round(messages: &[Message]) -> Vec<Vec<&Message>> {
    let mut groups: Vec<Vec<&Message>> = Vec::new();
    let mut current: Vec<&Message> = Vec::new();
    let mut last_assistant_id: Option<&str> = None;

    for msg in messages {
        if let Message::Assistant(asst) = msg {
            if Some(asst.message.id.as_str()) != last_assistant_id
                && !current.is_empty()
            {
                groups.push(std::mem::take(&mut current));
            }
            last_assistant_id = Some(&asst.message.id);
        }
        current.push(msg);
    }

    if !current.is_empty() {
        groups.push(current);
    }

    groups
}
```

### 15.5 PTL 重试

```rust
const MAX_PTL_RETRIES: u32 = 3;
const PTL_RETRY_MARKER: &str = "[earlier conversation truncated for compaction retry]";

/// 对应 truncateHeadForPTLRetry
pub fn truncate_head_for_ptl_retry(
    messages: &[Message],
    ptl_response: &AssistantMessage,
) -> Option<Vec<Message>> {
    // 1. Strip prior retry marker
    let input = if matches!(messages.first(),
        Some(Message::User(u)) if u.is_meta && u.content_str() == PTL_RETRY_MARKER
    ) {
        &messages[1..]
    } else {
        messages
    };

    let groups = group_messages_by_api_round(input);
    if groups.len() < 2 {
        return None;
    }

    // 2. Calculate drop count
    let drop_count = match get_prompt_too_long_token_gap(ptl_response) {
        Some(gap) => {
            let mut acc = 0u32;
            let mut count = 0;
            for g in &groups {
                acc += estimate_group_tokens(g);
                count += 1;
                if acc >= gap { break; }
            }
            count
        }
        None => (groups.len() as f64 * 0.2).max(1.0).floor() as usize,
    };

    let drop_count = drop_count.min(groups.len() - 1);
    if drop_count < 1 {
        return None;
    }

    // 3. Flatten remaining groups
    let mut result: Vec<Message> = groups[drop_count..]
        .iter()
        .flat_map(|g| g.iter().cloned())
        .collect();

    // 4. Ensure user-first (API requirement)
    if matches!(result.first(), Some(Message::Assistant(_))) {
        result.insert(0, create_user_message_meta(PTL_RETRY_MARKER));
    }

    Some(result)
}
```

---

## 16. 实现优先级

### Phase 4 (本文档对应阶段) 实现顺序

| 步骤 | 模块 | 网络依赖 | 说明 |
|---|---|---|---|
| 1 | `compact::grouping` | 无 | 纯数据结构，消息分组 |
| 2 | `compact::config` | 无 | 阈值计算，常量定义 |
| 3 | `tool::result_storage` | 无 | ContentReplacementState, 磁盘持久化 |
| 4 | `compact::micro` | 无 | 时间触发微压缩 (缓存编辑路径跳过) |
| 5 | `compact::cleanup` | 无 | 状态重置清理 |
| 6 | `compact::auto_compact` | 无 | 电路断路器 + shouldAutoCompact 决策树 |
| 7 | `compact::compaction` | **需要 API** | 全量/部分压缩 (streamCompactSummary) |
| 8 | `compact::session_memory` | **需要 API** | Session Memory 压缩 |
| 9 | Query loop 集成 | 无 | 将压缩管线接入主循环 |

### 可跳过 / 延后的部分

| 组件 | 原因 |
|---|---|
| Cached Microcompact (cache_edits API) | 需要 cache_edits API 支持，ant-only |
| Context Collapse | 计划中的功能，接口已定义但未实现 |
| Reactive Compact | 需要 API (413 恢复用 compact)，ant-only feature |
| Snip Compact | HISTORY_SNIP feature gate，外部构建不存在 |
| Session Memory extraction | 需要 API (session memory 由 AI 生成) |
| Prompt cache break detection | 优化/可观测性 |
| GrowthBook 动态配置 | 远程配置，本地可用默认值 |
| 所有遥测事件 (tengu_*) | 需要 analytics 服务 |

### 本地优先可完整实现的部分

1. **阈值体系** — 所有常量和计算公式，纯数学
2. **AutoCompactTracking 状态机** — 电路断路器，纯状态
3. **groupMessagesByApiRound** — 消息分组，纯数据
4. **truncateHeadForPTLRetry** — PTL 重试裁剪，纯数据
5. **时间触发微压缩** — 检查时间戳 + 清除内容，纯本地
6. **Tool Result Budget** — 大小检查 + 磁盘持久化，纯本地
7. **postCompactCleanup** — 状态重置，纯本地
8. **CompactionPipeline 编排器** — 协调各层，使用 trait 抽象
9. **Recovery 状态机** — 恢复决策树，输入为消息分析结果

---

## 附录 A: 压缩层互斥关系矩阵

```
                  AutoCompact  ReactiveCompact  ContextCollapse  Microcompact  Snip
AutoCompact          —            共存             互斥            共存         共存
ReactiveCompact    共存             —              共存(后备)       共存         共存
ContextCollapse    互斥           共存(后备)          —             共存         共存
Microcompact       共存            共存              共存             —          共存
Snip               共存            共存              共存            共存          —

互斥含义:
  - ContextCollapse 启用时 → shouldAutoCompact() 返回 false
  - AutoCompact 不会触发，但 isAutoCompactEnabled() 仍为 true
  - ReactiveCompact 仍可作为 413 恢复后备
```

## 附录 B: 流式循环中的消息扣留规则

```
消息扣留 (withheld = true → 不 yield 到消费者):

  1. prompt_too_long:
     ├─ CONTEXT_COLLAPSE: contextCollapse.isWithheldPromptTooLong()
     └─ REACTIVE_COMPACT: reactiveCompact.isWithheldPromptTooLong()

  2. max_output_tokens:
     └─ isWithheldMaxOutputTokens(message)
        = message.type === 'assistant' && message.apiError === 'max_output_tokens'

  3. media_size_error:
     └─ mediaRecoveryEnabled && reactiveCompact.isWithheldMediaSizeError(message)
        (需要 hoisted gate: mediaRecoveryEnabled 在轮次开始固定)

扣留的消息仍 push 到 assistantMessages:
  - 恢复路径在 post-streaming 检查它们
  - 恢复成功: 消息被丢弃
  - 恢复失败: yield 被扣留的消息

关键: mediaRecoveryEnabled 在轮次开始 hoist 一次:
  - 防止扣留/恢复不一致 (stream 中翻转会导致 withhold-without-recover)
```

## 附录 C: compactConversation 错误常量

```rust
pub const ERROR_NOT_ENOUGH_MESSAGES: &str = "Not enough messages to compact.";
pub const ERROR_PROMPT_TOO_LONG: &str =
    "Conversation too long. Press esc twice to go up a few messages and try again.";
pub const ERROR_USER_ABORT: &str = "API Error: Request was aborted.";
pub const ERROR_INCOMPLETE_RESPONSE: &str =
    "Compaction interrupted · This may be due to network issues — please try again.";
pub const TIME_BASED_MC_CLEARED: &str = "[Old tool result content cleared]";
```
