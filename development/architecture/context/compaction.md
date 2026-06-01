---
title: "上下文压缩管道 — Compaction 分层策略与数据流"
description: "allthecodes（Claude Code Rust 移植版）上下文压缩管道的完整实现。涵盖工具结果预算、Snip 裁剪、MicroCompact 局部压缩、Context Collapse 折叠和 Auto Compact 自动摘要五层策略，以及 CompactBoundary 边界标记机制。"
keywords: ["上下文压缩", "Compaction", "token 管理", "对话压缩", "allthecodes", "compact"]
---

<!--
本文对应源文件：
  crates/allthecodes-compact/src/pipeline.rs     — 管道编排入口
  crates/allthecodes-compact/src/compaction.rs    — 全量摘要压缩
  crates/allthecodes-compact/src/microcompact.rs   — 局部工具结果压缩
  crates/allthecodes-compact/src/context_collapse.rs — 早期对话折叠
  crates/allthecodes-compact/src/snip.rs          — 历史轮次裁剪
  crates/allthecodes-compact/src/session_memory_compact.rs — 会话记忆压缩
  crates/allthecodes-compact/src/partial_compact.rs — 部分对话压缩
  crates/allthecodes-compact/src/tool_result_budget.rs — 溢出工具结果持久化
  crates/allthecodes-compact/src/messages.rs      — 边界标记与消息规范化
  crates/allthecodes-compact/src/auto_compact.rs  — 自动压缩阈值判定
  crates/allthecodes-compact/src/gates.rs         — 压缩功能门控
-->

## 概述

上下文压缩在 allthecodes 中不是单一操作，而是一个**五层递进**的管道（Pipeline），每次查询循环迭代时顺序执行。管道定义在 `pipeline.rs` 的 `run_context_pipeline()` 函数中。

### 管道执行顺序

```
run_context_pipeline() 每次查询循环调用一次：
  1. applyToolResultBudget  — 将过大的工具结果落盘，替换为预览
  2. snipCompact            — 超出最大轮次限制时裁剪历史
  3. microcompact           — 清除旧的/冗余的工具结果内容
  4. contextCollapse        — 将早期对话折叠为摘要（Phase 2+）
  5. autoCompact            — 接近 token 限制时触发全量摘要压缩
```

### PipelineResult 输出结构

```rust
pub struct PipelineResult {
    pub messages: Vec<Message>,                    // 处理后的消息列表
    pub tracking: Option<AutoCompactTracking>,      // 更新后的追踪状态
    pub compacted: bool,                            // 是否实际执行了压缩
    pub auto_compact_triggered: bool,               // 是否触发了自动压缩
    pub estimated_tokens: u64,                      // 管道输出后的预估 token
    pub auto_compact_estimated_tokens: u64,         // 用于判断自动压缩的 token 数
    pub snip_tokens_freed: u64,                     // Snip 释放的 token
    pub microcompact_tokens_freed: u64,             // MicroCompact 释放的 token
    pub context_collapse_tokens_freed: u64,         // Context Collapse 释放的 token
    pub total_tokens_freed: u64,                    // 本地压缩释放的总 token
}
```

## 第一层：Tool Result Budget（工具结果预算）

源文件：`tool_result_budget.rs`

当工具返回结果超过 `DEFAULT_MAX_SIZE_CHARS`（100,000 字符）时，**将完整内容保存到磁盘临时文件**，并在消息流中替换为预览 + 文件路径。

```rust
// 预览策略：保留头部 500 字符 + 尾部 200 字符
const PREVIEW_HEAD_CHARS: usize = 500;
const PREVIEW_TAIL_CHARS: usize = 200;
```

### 数据结构

```rust
pub struct ReplacementRecord {
    pub tool_use_id: String,       // 被替换的工具调用 ID
    pub original_size: usize,      // 原始字符数
    pub file_path: String,         // 保存到磁盘的路径
}

pub struct ContentReplacementState {
    pub replacements: HashMap<String, ReplacementRecord>,
}
```

保存路径为 `$TMPDIR/allthecodes/tool-results/{tool_use_id}.txt`。如果落盘失败（如磁盘满），则退回到原地截断（truncate_in_place），保留头部和尾部各 250 字符。

## 第二层：Snip Compact（历史轮次裁剪）

源文件：`snip.rs`

当对话轮次超过 `DEFAULT_SNIP_MAX_TURNS`（200）时，裁剪最早的轮次，仅保留第一条消息 + 最近的 N 轮对话 + 紧凑边界标记。

### 轮次识别

```rust
fn identify_turn_starts(messages: &[Message]) -> Vec<usize> {
    // 轮次开始 = 无 tool_use_result 的 User 消息
    // （有 tool_use_result 的是工具结果续接，不算新轮次）
}
```

### 裁剪策略

```rust
// 默认最大轮次
const DEFAULT_SNIP_MAX_TURNS: usize = 200;

// 如果轮次数 > max_turns:
//   1. 保留 messages[0]（初始上下文）
//   2. 插入 CompactBoundary 系统消息
//   3. 保留最近的 max_turns 轮
```

Snip 在 `messages.rs` 之外使用自己独立的字符估算（~4 chars/token），而不是 `allthecodes_utils::tokens`，保持解耦。

## 第三层：MicroCompact（局部工具结果压缩）

源文件：`microcompact.rs`

MicroCompact 不删除消息，而是**清除旧工具输出的内容**，将其替换为截断摘要。这是最轻量的压缩方式，不调用 AI 模型。

```rust
// 保留最近的 N 个工具结果完整不变
const KEEP_RECENT_TOOL_RESULTS: usize = 10;

// 超过此字符数的工具结果才被压缩
const SIZE_THRESHOLD_CHARS: usize = 1000;
```

### 实现逻辑

1. 找到最后一个 Assistant 消息的索引（保护其关联的工具结果）
2. 收集所有携带工具结果的 User 消息索引
3. 最近 KEEP_RECENT_TOOL_RESULTS 个 + 最后一个 Assistant 轮次内的工具结果**完全保留**
4. 其余工具结果如果字符数超过阈值，替换为摘要

```rust
fn make_tool_result_summary(content: &ToolResultContent, original_len: usize) -> String {
    // 保留头部 200 字符 + 尾部 100 字符
    // 中间替换为 "[... N characters omitted (microcompacted) ...]"
}
```

### Microcompact Boundary

每次微压缩操作后，在消息列表末尾追加一条 `MicrocompactBoundary` 系统消息：

```rust
SystemSubtype::MicrocompactBoundary {
    microcompact_metadata: Some(MicrocompactMetadata {
        trigger: "auto".to_string(),      // MicroCompact 只有自动触发
        pre_tokens: u64,                  // 压缩前 token 数
        tokens_saved: u64,                // 节省的 token 数
        compacted_tool_ids: Vec<String>,  // 被压缩的工具 ID 列表
        cleared_attachment_uuids: Vec::new(),
    }),
}
```

与 `CompactBoundary` 的区别：Microcompact 保留原始消息结构，仅替换内容体，不生成摘要，不调用 API。

## 第四层：Context Collapse（早期对话折叠）

源文件：`context_collapse.rs`

Context Collapse 是 Phase 2+ 引入的实验性压缩策略。它将早期的对话轮次折叠为简短的文字摘要，而不需要调用 AI 模型。

```rust
// 触发条件：轮次数 > 40 且 token 使用量 > 上下文窗口的 60%
const MAX_TURNS_BEFORE_COLLAPSE: usize = 40;
const KEEP_RECENT_TURNS: usize = 20;
const CONTEXT_WINDOW_TRIGGER_RATIO: f64 = 0.6;
```

### 折叠逻辑

1. `identify_turn_starts()` — 识别对话中的轮次边界（与 Snip 相同逻辑）
2. 如果轮次数 <= 40 且 token 使用量 <= 60% 上下文窗口 → 不折叠
3. 如果轮次数 <= 20 → 不折叠（少于保留窗口）
4. 保留最近 20 轮，之前的消息折叠为摘要

```rust
fn summarize_collapsed_messages(messages: &[Message]) -> String {
    // 统计：用户消息数、助手消息数、工具使用数、工具结果数
    // 提取前 6 条消息的预览（每条 160 字符）
    // 输出格式：
    // "Collapsed N older messages (X user, Y assistant, Z tool_use, W tool_result)."
    // + 预览列表
}
```

折叠后插入 `CompactBoundary` 系统消息，内容为 `<context_collapse>...</context_collapse>` 包裹的摘要。

### API 不变性保护

`should_preserve_first_message()` 确保系统消息或初始用户消息总是保留。`identify_turn_starts()` 排除工具结果消息以防止 tool_use/tool_result 对断裂。

## 第五层：Auto Compact（自动摘要压缩与 Reactive Compact）

源文件：`compaction.rs`, `auto_compact.rs`

当 token 使用量超过上下文窗口的 **80%** 时触发（`AUTO_COMPACT_THRESHOLD_RATIO = 0.8`）。这是最重量级的压缩，需要调用 AI 模型生成对话摘要。

```rust
// auto_compact.rs
pub fn should_auto_compact(estimated_tokens: u64, model: &str) -> bool {
    estimated_tokens > auto_compact_threshold_tokens(model)
    // = estimated_tokens > context_window * 0.8
}
```

### 精确计数回退带（Exact Count Fallback Band）

```rust
const EXACT_FALLBACK_BAND_RATIO: f64 = 0.05;

// 当估算 token 落在阈值 ±5% 窗口内时，触发精确计数
// 防止估算误差导致误判
```

### CompactionConfig 与 CompactionResult

```rust
pub struct CompactionConfig {
    pub model: String,
    pub session_id: String,
    pub query_source: String,  // 用于递归防护
}

pub struct CompactionResult {
    pub messages: Vec<Message>,
    pub tracking: AutoCompactTracking,
    pub pre_compact_tokens: u64,
    pub post_compact_tokens: u64,
    pub boundary_message: Message,
}
```

### 摘要后消息组装

```rust
pub fn build_post_compact_messages(summary: &str, ...) -> Vec<Message> {
    // 1. 摘要作为 <context_compaction> user 消息
    // 2. 恢复最近 5 个文件路径（去重，反向遍历）
}
```

### Reactive Compact（紧急降级）

当发生 `prompt_too_long` 错误时，`try_reactive_compact()` 会采取更激进的压缩策略：

```rust
// 激进裁剪：仅保留最近 5 轮
const REACTIVE_SNIP_MAX_TURNS: usize = 5;

// 步骤：
// 1. 预算所有超大工具结果
// 2. Snip 到仅保留 5 轮
// 3. MicroCompact 清理旧工具结果
```

Reactive Compact 由 `CompactionFeatureGates::reactive_compact` 特性门控控制，默认启用。

### Session Memory Compact（无 API 调用压缩）

源文件：`session_memory_compact.rs`

当启用了 `session_memory_compact` 特性门控且有持久化会话记忆可用时，使用已有的会话摘要替代旧历史——**不需要调用 AI 模型**。

```rust
// 保留窗口默认配置
pub struct SessionMemoryCompactConfig {
    pub min_tokens: u64 = 10_000,           // 至少 10K token
    pub min_text_block_messages: usize = 5, // 至少 5 条文本消息
    pub max_tokens: u64 = 40_000,           // 最多 40K token
}
```

### Partial Compact（部分对话压缩）

源文件：`partial_compact.rs`

支持两种方向的局部压缩：

```rust
pub enum PartialCompactDirection {
    UpTo,  // 压缩锚点之前的内容，保留锚点及之后
    From,  // 保留锚点及之前的内容，压缩之后的内容
}
```

两种方向都包含 `adjust_start/end_to_preserve_api_invariants()` 确保 tool_use/tool_result 对完整性。

## CompactBoundary：压缩的边界标记

定义于 `allthecodes_types::message::SystemSubtype::CompactBoundary`。

每次压缩后，系统在消息流中插入一条边界消息，记录压缩元数据：

```rust
CompactMetadata {
    pre_compact_token_count: u64,
    post_compact_token_count: u64,
    preserved_segment: Option<PreservedSegment>,
}

PreservedSegment {
    summary_message_uuid: Option<String>,
    preserved_message_uuids: Vec<String>,
}
```

### 边界查询过滤

`get_messages_after_compact_boundary()` 返回最后一条边界之后的消息：

```rust
pub fn get_messages_after_compact_boundary(messages: &[Message]) -> &[Message] {
    // 从后向前扫描最后一条 CompactBoundary
    // 返回其之后的所有消息（不含边界本身）
    // 如果无边界，返回全部消息
}
```

## 功能门控

源文件：`gates.rs`

所有压缩策略都有独立的环境变量门控：

| 门控 | 环境变量 | 默认 |
|------|----------|------|
| `auto_compact` | `ALLTHECODES_AUTO_COMPACT` / `CC_RUST_AUTO_COMPACT` | true |
| `reactive_compact` | `ALLTHECODES_REACTIVE_COMPACT` / `CC_RUST_REACTIVE_COMPACT` | true |
| `session_memory_compact` | `ALLTHECODES_SESSION_MEMORY_COMPACT` / `CC_RUST_SESSION_MEMORY_COMPACT` | true |
| `partial_compact` | `ALLTHECODES_PARTIAL_COMPACT` / `CC_RUST_PARTIAL_COMPACT` | true |

禁用门控：`ALLTHECODES_DISABLE_AUTO_COMPACT=1` 专门禁用自动压缩。

## 递归防护与熔断器

```rust
const MAX_CONSECUTIVE_FAILURES: usize = 3;

// 在 should_auto_compact() 中：
// 1. query_source == "compact" 或 "session_memory" → 跳过（递归防护）
// 2. consecutive_failures >= 3 → 跳过（熔断器）
// 3. token 数低于阈值 → 跳过
```

## 消息规范化

源文件：`messages.rs`

在发送消息到 API 之前，`normalize_messages_for_api()` 执行以下预处理：

1. 过滤 Progress 消息
2. 过滤 System 消息（独立注入到 system prompt）
3. 将 Attachment 消息转换为 User 消息（QueuedCommand、NestedMemory）
4. `ensure_alternating_pattern()` — 确保 user/assistant 交替排列，必要时插入 `[continued]` 合成消息
5. 确保首条消息为 User 消息（API 要求）
