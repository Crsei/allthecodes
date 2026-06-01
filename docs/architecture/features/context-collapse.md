# CONTEXT_COLLAPSE — 上下文折叠

> 功能标志：`FEATURE_CONTEXT_COLLAPSE=1`
> 实现状态：完整可用
> 源码文件数：8

## 一、功能概述

上下文折叠（Context Collapse）是对话历史压缩流水线中的关键环节。当对话增长到接近上下文窗口限制时，自动将旧的对话轮次折叠为压缩摘要，保留关键信息的同时释放 token 空间。

上下文折叠是整体上下文管理流水线（Pipeline）的一部分，位于工具结果预算、历史裁剪和微压缩之后。

### 关联模块

| 模块 | 功能 |
|------|------|
| `tool_result_budget` | 将过大的工具结果保存到磁盘 |
| `snip` | 裁剪超出轮次限制的旧对话 |
| `microcompact` | 移除缓存/冗余的工具结果 |
| `context_collapse` | 将旧片段折叠为摘要 |
| `auto_compact` | 接近 token 限制时的完整摘要 |

## 二、实现架构

### 2.1 折叠触发条件

文件：`crates/allthecodes-compact/src/context_collapse.rs:26-120`

```rust
fn context_collapse_if_needed(messages: Vec<Message>, model: &str) -> ContextCollapseResult
```

触发条件：
1. **轮次超过阈值**：`MAX_TURNS_BEFORE_COLLAPSE = 40` 轮
2. **token 超过阈值**：占用上下文窗口的 `CONTEXT_WINDOW_TRIGGER_RATIO = 60%`
3. **保留最近轮次**：`KEEP_RECENT_TURNS = 20` 轮不折叠

### 2.2 折叠结果

```rust
pub struct ContextCollapseResult {
    pub messages: Vec<Message>,           // 折叠后的消息
    pub tokens_freed: u64,                // 释放的 token 数
    pub collapsed_messages: usize,        // 折叠的消息数
    pub boundary_message: Option<Message>, // 插入的边界消息
}
```

### 2.3 边界消息

折叠后的消息列表包含一个 `SystemSubtype::CompactBoundary` 消息：

```rust
Message::System(SystemMessage {
    subtype: SystemSubtype::CompactBoundary {
        compact_metadata: Some(CompactMetadata {
            pre_compact_token_count: initial_tokens,   // 折叠前 token 数
            post_compact_token_count: final_tokens,     // 折叠后 token 数
            preserved_segment: Some(preserved_segment), // 保留片段信息
        }),
    },
    content: "<context_collapse>\nCollapsed N older messages...\n</context_collapse>",
})
```

### 2.4 保留片段（Preserved Segment）

`create_preserved_segment()` 创建保留片段，记录折叠消息的 UUID 列表，以便在需要时进行精确引用恢复。

### 2.5 摘要生成

`summarize_collapsed_messages()` 生成摘要文本：
- 统计：用户消息数、助手消息数、工具使用数、工具结果数
- 预览：保留最多 6 条消息的文本预览（每种类型截取首部内容）
- 限制：摘要预览上限 `SUMMARY_PREVIEW_LIMIT_CHARS = 2000` 字符

### 2.6 消息保留策略

- **第一条消息**：总是保留（系统消息或用户初始上下文）
- **最近轮次**：保留最后 `KEEP_RECENT_TURNS` 轮
- **工具结果配对**：确保 tool_use 和 tool_result 不会被拆分

## 三、整体压缩流水线

文件：`crates/allthecodes-compact/src/pipeline.rs`

压缩流水线的执行顺序：

```
┌─────────────────────────────────────────────┐
│  1. applyToolResultBudget (async, 磁盘 I/O) │
│     将超大工具结果保存到磁盘，替换为预览      │
├─────────────────────────────────────────────┤
│  2. snipCompact                              │
│     裁剪超出 DEFAULT_SNIP_MAX_TURNS=200 的轮次 │
├─────────────────────────────────────────────┤
│  3. microcompact                             │
│     移除缓存的冗余工具结果                   │
├─────────────────────────────────────────────┤
│  4. contextCollapse                          │
│     将旧轮次折叠为摘要（当前文件）           │
├─────────────────────────────────────────────┤
│  5. autoCompact 检查                         │
│     超过 80% 上下文窗口时触发自动压缩        │
└─────────────────────────────────────────────┘
```

### 3.1 Reactive Compact

当 API 返回 `prompt_too_long` 错误时，触发紧急反应式压缩：

```rust
pub async fn try_reactive_compact(messages: Vec<Message>, model: &str)
    -> Option<ReactiveCompactResult>
```

策略：
1. 预算超大工具结果
2. 激进裁剪到 `REACTIVE_SNIP_MAX_TURNS = 5` 轮
3. 微压缩剩余消息
4. 目标是压缩到上下文窗口的 60% 以内

### 3.2 Collapse Drain

当 `prompt_too_long` 发生且反应式压缩不足时，`collapse_drain` 作为最终手段：
- 优先调用 `context_collapse::context_collapse_if_needed()` 释放空间
- 如果仍然不够，再尝试 `reactive_compact`
- 两者都失败时，终止并返回 API 错误消息

## 四、Snip 子功能

文件：`crates/allthecodes-compact/src/snip.rs`

Snip 是更简单的裁剪机制，不生成 LLM 摘要，直接移除旧消息：

```rust
pub fn snip_compact_if_needed(messages: Vec<Message>, max_turns: usize) -> SnipResult
```

- 保留第一条消息
- 保留最近 `max_turns` 轮
- 插入 `[History snipped: ...]` 边界消息
- 边界消息同样包含 `CompactMetadata` 和 `PreservedSegment`

## 五、Microcompact 微压缩

文件：`crates/allthecodes-compact/src/microcompact.rs`

```rust
pub fn microcompact_messages(messages: Vec<Message>) -> MessagesCompactResult
```

移除重复的、缓存的工具结果，释放 token 而不丢失语义信息。

## 六、关键设计决策

1. **非 LLM 折叠**：当前实现使用基于轮次计数的直接裁剪，不是 LLM 生成的摘要
2. **保留片段可追溯**：通过 `PreservedSegment` 记录被折叠消息的 UUID，支持精确恢复
3. **分层压缩**：从轻量（snip）到重量（auto_compact），逐步释放上下文空间
4. **反应式紧急压缩**：API 413 错误触发激进压缩，尽力恢复
5. **消息完整性**：确保 tool_use/tool_result 配对不被拆分

## 七、使用方式

```bash
# 上下文折叠自动在查询循环中运行（不需要手动启用）
# 压缩流水线在每个查询循环迭代中自动执行
```

## 八、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-compact/src/context_collapse.rs` | 上下文折叠核心实现 |
| `crates/allthecodes-compact/src/snip.rs` | 历史裁剪实现 |
| `crates/allthecodes-compact/src/microcompact.rs` | 微压缩实现 |
| `crates/allthecodes-compact/src/pipeline.rs` | 压缩流水线编排 |
| `crates/allthecodes-compact/src/tool_result_budget.rs` | 工具结果预算 |
| `crates/allthecodes-compact/src/auto_compact.rs` | 自动压缩逻辑 |
| `crates/allthecodes-compact/src/context_analysis.rs` | 上下文分析 |
| `crates/allthecodes-compact/src/messages.rs` | 消息工具函数 |
