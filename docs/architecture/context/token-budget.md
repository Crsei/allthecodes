---
title: "Token 预算追踪 — 会话内 Token 消耗的监控与控制"
description: "allthecodes（Claude Code Rust 移植版）的 Token 预算追踪系统。涵盖全局上下文窗口的自动压缩阈值、每个查询轮的 Token 预算检查（含递减回报检测）、以及跨预设的常量定义。"
keywords: ["Token 预算", "Token Budget", "上下文窗口", "递减回报", "自动压缩", "allthecodes"]
---

<!--
本文对应源文件：
  crates/allthecodes-engine/src/query/token_budget.rs  — 每次查询的 Token 预算检查
  crates/allthecodes-compact/src/auto_compact.rs       — 自动压缩阈值判定
  crates/allthecodes-config/src/constants.rs           — Token 相关常量定义
  crates/allthecodes-types/src/state.rs                — BudgetTracker 等状态类型
-->

## Token 预算追踪的两层体系

allthecodes 的 Token 预算管理分为两个层次：

1. **全局上下文窗口管理** — 通过上下文压缩管道确保消息总量不超过模型上下文窗口
2. **每次查询的 Token 预算检查** — 针对特定查询任务（如 bug 修复、功能开发）的预算限制

## 第一层：全局上下文窗口与自动压缩阈值

源文件：`crates/allthecodes-config/src/constants.rs` 和 `crates/allthecodes-compact/src/auto_compact.rs`

### 上下文窗口常量

```rust
// constants.rs — tokens 模块
pub const MODEL_CONTEXT_WINDOW_DEFAULT: u64 = 200_000;  // 200K 默认
pub const CONTEXT_WINDOW_1M: u64 = 1_000_000;           // 1M 扩展窗口

pub const MAX_OUTPUT_TOKENS_DEFAULT: u64 = 32_000;
pub const MAX_OUTPUT_TOKENS_UPPER_LIMIT: u64 = 64_000;
pub const CAPPED_DEFAULT_MAX_TOKENS: u64 = 8_000;       // P99 输出约 5K
pub const ESCALATED_MAX_TOKENS: u64 = 64_000;            // 重试时的上限
```

### 自动压缩阈值

```rust
// auto_compact.rs
const AUTO_COMPACT_THRESHOLD_RATIO: f64 = 0.8;

pub fn auto_compact_threshold_tokens(model: &str) -> u64 {
    let context_window = get_context_window_size(model);
    (context_window as f64 * AUTO_COMPACT_THRESHOLD_RATIO) as u64
    // 200K → 160K 触发自动压缩
    // 1M   → 800K 触发自动压缩
}
```

### 精确计数回退带

```rust
const EXACT_FALLBACK_BAND_RATIO: f64 = 0.05;

pub fn should_check_exact_for_auto_compact(estimated_tokens: u64, model: &str) -> bool {
    let threshold = auto_compact_threshold_tokens(model);
    let band = (context_window * 0.05).max(1);
    estimated_tokens.abs_diff(threshold) <= band
    // 当估算值落在阈值 ±5% 范围内时，使用 provider 精确计数验证
}
```

### 缓冲区常量

```rust
pub const AUTOCOMPACT_BUFFER_TOKENS: u64 = 13_000;    // 自动压缩触发前的缓冲区
pub const WARNING_THRESHOLD_BUFFER_TOKENS: u64 = 20_000; // 显示警告前的缓冲区
pub const MANUAL_COMPACT_BUFFER_TOKENS: u64 = 3_000;     // 手动压缩缓冲区
```

### 跟踪状态

```rust
// allthecodes_types::state::AutoCompactTracking
pub struct AutoCompactTracking {
    pub compacted: bool,                  // 当前轮次是否已压缩
    pub turn_counter: u64,                // 自上次压缩以来的轮次
    pub turn_id: String,                  // 最后压缩的轮次 ID
    pub consecutive_failures: usize,       // 连续压缩失败次数（熔断器阈值=3）
}
```

## 第二层：查询 Token 预算检查

源文件：`crates/allthecodes-engine/src/query/token_budget.rs`

当用户为查询指定了 Token 预算（`TaskBudget`）时，系统在每次查询轮次后检查是否超出预算。

### BudgetTracker 状态

```rust
// allthecodes_types::state
pub struct BudgetTracker {
    pub started_at: i64,                   // 预算开始时间戳
    pub continuation_count: usize,          // 连续续费次数
    pub last_global_turn_tokens: u64,       // 上一轮的全局 token 消耗
    pub last_delta_tokens: u64,             // 上一轮的增量 token 消耗
}

pub struct BudgetCompletionEvent {
    pub continuation_count: usize,
    pub pct: usize,                         // (turn_tokens / budget) * 100
    pub turn_tokens: u64,
    pub budget: u64,
    pub diminishing_returns: bool,          // 是否检测到递减回报
    pub duration_ms: u64,
}
```

### 检查逻辑

```rust
// token_budget.rs
const COMPLETION_THRESHOLD: f64 = 0.9;    // 90% 阈值
const DIMINISHING_THRESHOLD: u64 = 500;    // 递减回报的 token 增量阈值

pub fn check_token_budget(
    tracker: &mut BudgetTracker,
    agent_id: Option<&str>,    // 子代理 ID
    budget: Option<u64>,       // 预算总额
    global_turn_tokens: u64,   // 当前轮全局 token 消耗
) -> TokenBudgetDecision {
```

### 决策逻辑

#### 1. 跳过检查的条件

```rust
// 子代理（agent_id 非空）→ 总是 Stop（不消耗父会话的预算）
// 无预算（budget 为 None 或 0）→ 总是 Stop
if agent_id.is_some() || budget.map_or(true, |b| b == 0) {
    return TokenBudgetDecision::Stop {
        completion_event: None,
    };
}
```

#### 2. 继续（Continue）的条件

```rust
// 当满足以下所有条件时，允许继续：
// 1. turn_tokens < budget * 90%（未到完成阈值）
// 2. 未检测到递减回报

let is_diminishing = tracker.continuation_count >= 3
    && delta_since_last < DIMINISHING_THRESHOLD    // 增量 < 500 token
    && tracker.last_delta_tokens < DIMINISHING_THRESHOLD; // 上次增量也 < 500

if !is_diminishing && (turn_tokens as f64) < (budget as f64 * COMPLETION_THRESHOLD) {
    tracker.continuation_count += 1;
    return TokenBudgetDecision::Continue {
        nudge_message: format!(
            "Token budget at {}% ({}/{}) — continue working.",
            pct, turn_tokens, budget
        ),
        continuation_count: tracker.continuation_count,
        pct,
        turn_tokens,
        budget,
    };
}
```

#### 3. 停止（Stop）的条件

```rust
// 触发停止的场景：
// A. 递减回报已检测到（is_diminishing == true）
// B. 已有继续尝试（continuation_count > 0，但 token 已超 90%）
// C. 未满足继续条件

if is_diminishing || tracker.continuation_count > 0 {
    // 记录完成事件，包含递减回报状态
    return TokenBudgetDecision::Stop {
        completion_event: Some(BudgetCompletionEvent { ... }),
    };
}

// 默认也停止（无完成事件）
```

### 递减回报检测

递减回报（Diminishing Returns）是预算系统中的一个关键优化，防止模型在同一个问题上反复消耗 token 却没有实质性进展：

```rust
// 判定标准（需同时满足）：
// 1. continuation_count >= 3（已续费至少 3 次）
// 2. 当前增量 token < 500（最近一次生成内容很少）
// 3. 上次增量 token < 500（连续两次增量太少）

// 这意味着模型在同一轮次中已经尝试了多次，
// 但每次新生成的 token 越来越少——表明可能陷入循环或没有新思路。
```

### TokenBudgetDecision 枚举

```rust
pub enum TokenBudgetDecision {
    Continue {
        nudge_message: String,        // 显示给用户的提示
        continuation_count: usize,    // 当前续费计数
        pct: usize,                   // 预算使用百分比
        turn_tokens: u64,             // 当前轮 token
        budget: u64,                  // 总预算
    },
    Stop {
        completion_event: Option<BudgetCompletionEvent>,
    },
}
```

### QuerySource 对预算的影响

不同的查询来源会影响是否检查预算：

```rust
pub enum QuerySource {
    Sdk,              // SDK 调用 → 检查预算
    ReplMainThread,   // 主交互循环 → 检查预算
    Compact,          // 压缩过程 → 跳过预算检查（递归防护）
    SessionMemory,    // 会话记忆 → 跳过预算检查
    Agent(String),    // 子代理 → 总是 Stop（不消耗父会话预算）
    ProactiveTick,    // 主动 Tick → 检查预算
    WebhookEvent,     // Webhook → 检查预算
    ChannelNotification, // 频道通知 → 检查预算
}
```

## 上下文分析（Context Analysis）

源文件：`crates/allthecodes-compact/src/context_analysis.rs`

`analyze_context_usage()` 提供一个全方位的 Token 使用报告，用于 `/compact` 命令的诊断输出。

```rust
pub struct ContextAnalysis {
    pub model: String,
    pub context_window: u64,        // 模型上下文窗口大小
    pub total_used: u64,            // 总使用量
    pub total_percent: f32,         // 使用百分比
    pub compacted: bool,            // 是否已执行压缩
    pub messages_in: usize,         // 压缩前的消息数
    pub messages_out: usize,        // 压缩后的消息数
    pub categories: Vec<ContextCategory>,  // 各类别 Token 明细
    pub unavailable_categories: Vec<String>, // 不可用的类别列表
    pub estimation_notes: Vec<String>,     // 估算说明
}
```

### 分类统计

`analyze_context_usage_with_window()` 将 token 使用量分为以下类别：

- `messages` — 对话历史消息
- `cached input` — 缓存文件内容（字符数 / 4）
- `hook results` — Hook 执行结果
- `system prompt` — 系统提示词（需要 `system_prompt` 输入）
- `skills` — 技能清单（需要 `skills_manifest` 输入）
- `tools schema` — 工具 Schema（需要 `tools_schema` 输入）
- `free` — 剩余可用空间

类别按 token 数降序排列，`free` 始终在最后。

### 压缩前置变换

分析在统计前会先应用 Snip 和 MicroCompact 变换，以反映实际发送到 API 的消息：

```rust
let snipped = snip::snip_compact_if_needed(input.messages.to_vec(), DEFAULT_SNIP_MAX_TURNS);
let micro = microcompact::microcompact_messages(snipped.messages);
let effective = micro.messages;
```

## 关键常量总览

```rust
// Token 估算常量
pub const CHARS_PER_TOKEN: f64 = 4.0;        // ~4 字符 = 1 token
pub const BYTES_PER_TOKEN: u64 = 4;          // ~4 字节 = 1 token

// 工具结果限制
pub const MAX_TOOL_RESULT_TOKENS: u64 = 100_000;
pub const MAX_TOOL_RESULT_BYTES: u64 = 400_000;

// 压缩相关
pub const COMPACT_MAX_OUTPUT_TOKENS: u64 = 20_000;  // 摘要生成最大输出

// 自动压缩触发
pub const AUTO_COMPACT_THRESHOLD_RATIO: f64 = 0.8;  // 上下文窗口的 80%
pub const EXACT_FALLBACK_BAND_RATIO: f64 = 0.05;    // 精确计数回退带 ±5%

// Snip 最大轮次
pub const DEFAULT_SNIP_MAX_TURNS: usize = 200;      // 正常轮次限制
pub const REACTIVE_SNIP_MAX_TURNS: usize = 5;       // 紧急降级限制

// 工具结果预算
pub const DEFAULT_MAX_SIZE_CHARS: usize = 100_000;  // 100K 字符落盘阈值

// MicroCompact
pub const KEEP_RECENT_TOOL_RESULTS: usize = 10;     // 保留近 10 个工具结果
pub const SIZE_THRESHOLD_CHARS: usize = 1000;       // 1K 字符压缩阈值

// Context Collapse
pub const MAX_TURNS_BEFORE_COLLAPSE: usize = 40;    // 折叠触发轮次
pub const KEEP_RECENT_TURNS: usize = 20;            // 保留最近 20 轮
pub const CONTEXT_WINDOW_TRIGGER_RATIO: f64 = 0.6;  // 60% 窗口触发

// Token Budget
pub const COMPLETION_THRESHOLD: f64 = 0.9;          // 预算完成阈值 90%
pub const DIMINISHING_THRESHOLD: u64 = 500;         // 递减回报增量阈值
```
