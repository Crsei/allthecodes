# TOKEN_BUDGET — Token 预算跟踪

> 功能标志：`TaskBudget.total`（引擎配置）
> 实现状态：完整可用
> 源码文件数：2

## 一、功能概述

Token 预算跟踪（Token Budget）为对话设置输出 token 使用上限。当模型输出接近或达到预算限制时，系统注入提示消息引导模型收尾，或在收益递减时终止对话。这防止了无限消耗 API 费用，特别适用于批量处理或预算敏感场景。

### 核心概念

- **预算（Budget）**：任务的总 token 消耗上限
- **补丁消息（Nudge Message）**：达到阈值时注入的继续提示
- **收益递减检测（Diminishing Returns）**：连续多轮输出很少 token 时提前终止
- **完成事件（Completion Event）**：预算用尽时生成的事件记录

## 二、实现架构

### 2.1 Token 预算决策

文件：`crates/allthecodes-query/src/token_budget.rs`

```rust
pub fn check_token_budget(
    tracker: &mut BudgetTracker,
    agent_id: Option<&str>,
    budget: Option<u64>,
    global_turn_tokens: u64,
) -> TokenBudgetDecision
```

### 2.2 决策逻辑

```
check_token_budget(tracker, agent_id, budget, global_turn_tokens)
    │
    ├── agent_id.is_some() || budget.is_none() || budget == 0
    │   └── → TokenBudgetDecision::Stop (子 Agent 或无预算)
    │
    ├── turn_tokens < budget * COMPLETION_THRESHOLD (0.9)
    │   │   && !is_diminishing
    │   │
    │   └── → TokenBudgetDecision::Continue
    │       └── 注入补丁消息: "Token budget at X% (N/M) — continue working."
    │
    ├── is_diminishing || continuation_count > 0
    │   │
    │   └── → TokenBudgetDecision::Stop
    │       └── 生成 BudgetCompletionEvent
    │
    └── else
        → TokenBudgetDecision::Stop (无事件)
```

### 2.3 阈值常量

```rust
const COMPLETION_THRESHOLD: f64 = 0.9;     // 90% 预算触发终止
const DIMINISHING_THRESHOLD: u64 = 500;    // 连续 3 轮 <500 token 视为收益递减
```

### 2.4 收益递减检测

```rust
let is_diminishing = tracker.continuation_count >= 3
    && delta_since_last < DIMINISHING_THRESHOLD
    && tracker.last_delta_tokens < DIMINISHING_THRESHOLD;
```

条件：
1. 已连续续期 3 次以上
2. 本轮增量小于 500 token
3. 上一轮增量也小于 500 token

### 2.5 决策类型

```rust
pub enum TokenBudgetDecision {
    Continue {
        nudge_message: String,         // 注入给模型的提示
        continuation_count: usize,     // 续期次数
        pct: usize,                    // 预算使用百分比
        turn_tokens: u64,              // 已使用 token
        budget: u64,                   // 总预算
    },
    Stop {
        completion_event: Option<BudgetCompletionEvent>,
    },
}
```

### 2.6 完成事件

```rust
pub struct BudgetCompletionEvent {
    pub continuation_count: usize,
    pub pct: usize,
    pub turn_tokens: u64,
    pub budget: u64,
    pub diminishing_returns: bool,
    pub duration_ms: u64,
}
```

### 2.7 查询循环集成

文件：`crates/allthecodes-query/src/loop_impl.rs`

在查询循环中，每轮迭代后检查 token 预算：

```rust
match check_token_budget(
    &mut budget_tracker,
    agent_id.as_deref(),
    task_budget.map(|b| b.total),
    global_turn_tokens,
) {
    TokenBudgetDecision::Continue { nudge_message, .. } => {
        // 注入补丁消息，继续下一轮
    }
    TokenBudgetDecision::Stop { completion_event } => {
        // 终止对话，可选生成完成事件
    }
}
```

### 2.8 引擎端状态

```rust
pub struct BudgetTracker {
    pub continuation_count: usize,
    pub last_delta_tokens: u64,
    pub last_global_turn_tokens: u64,
    pub started_at: i64,
}
```

预算是通过 `QueryEngineConfig.task_budget` 传入的：

```rust
pub struct TaskBudget {
    pub total: u64,
}
```

### 2.9 查询参数传递

```rust
pub struct QueryParams {
    // ...
    pub task_budget: Option<TaskBudget>,
    // ...
}
```

## 三、测试覆盖

查询循环测试中验证了 Token 预算的补丁机制：

```rust
#[tokio::test]
async fn test_token_budget_continuation_injects_nudge_message() {
    // 预算 total=100, 每轮输出 50 token
    // 第一轮后: 50% → 注入补丁消息
    // 第二轮后: 100% → 终止

    let nudge = params[1].messages.iter().rev().find_map(|message| { ... });
    assert!(matches!(nudge, Some((true, text)) if text.contains("Token budget at 50%")));
}
```

## 四、关键设计决策

1. **子 Agent 豁免**：只有顶层（agent_id.is_none()）受预算限制，子 Agent 不计入
2. **90% 终止阈值**：到达 90% 预算时触发完成事件，而非立即截断
3. **收益递减保护**：连续低输出轮次提前终止，防止无限续期
4. **补丁而非截断**：注入引导消息让模型自然收尾，而非硬截断输出
5. **完整事件记录**：`BudgetCompletionEvent` 记录完整上下文供外部系统审计

## 五、使用方式

```rust
// 通过引擎配置设置预算
let config = QueryEngineConfig {
    task_budget: Some(TaskBudget { total: 10_000 }),
    // ...
};
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-query/src/token_budget.rs` | Token 预算决策逻辑 |
| `crates/allthecodes-query/src/loop_impl.rs` | 查询循环集成 |
| `crates/allthecodes-query/src/loop_tests.rs` | 预算补丁测试 |
