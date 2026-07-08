# TIER3_STUBS — 第三层测试桩

> 功能标志：无（测试基础设施）
> 实现状态：MockDeps 完整可用
> 源码文件数：2

## 一、功能概述

第三层测试桩（Tier 3 Stubs）是 allthecodes 查询引擎的专用 mock 基础设施，用于模拟模型 API 调用、工具执行、自动压缩、上下文折叠等外部依赖。它使开发者可以在不连接真实 API 的情况下，对查询循环进行全面的行为验证。

### 测试分层

| 层级 | 描述 | 工具 |
|------|------|------|
| Tier 1 | 纯文本 Q&A，无工具使用 | 基本 MockDeps |
| Tier 2 | 需要工具执行管道的测试 | MockDeps + 模拟工具 |
| **Tier 3** | **完整的查询循环端到端测试** | **MockDeps + 流式事件模拟** |

## 二、实现架构

### 2.1 MockDeps

文件：`crates/allthecodes-engine/src/query/loop_tests.rs:34-115`

`MockDeps` 是所有查询循环测试的核心 mock 实现：

```rust
struct MockDeps {
    stream_steps: parking_lot::Mutex<Vec<MockStreamStep>>,     // 预定义的流步骤
    call_params: parking_lot::Mutex<Vec<ModelCallParams>>,      // 记录模型调用参数
    autocompact_params: parking_lot::Mutex<Vec<ModelCallParams>>, // 记录自动压缩参数
    collapse_drain_result: parking_lot::Mutex<Option<CompactionResult>>,
    collapse_drain_calls: AtomicUsize,
    reactive_compact_result: parking_lot::Mutex<Option<CompactionResult>>,
    reactive_compact_calls: AtomicUsize,
    aborted: AtomicBool,
    stream_finished: Arc<AtomicBool>,
    tool_executed_before_stream_finished: AtomicBool,
    tool_execution_count: AtomicUsize,
    tool_completed_count: AtomicUsize,
    hook_stopped_tool_execution: AtomicBool,
    active_tools: AtomicUsize,
    max_active_tools: AtomicUsize,
    tool_delay: Duration,
    tools: Tools,
    refreshed_tools: parking_lot::Mutex<Option<Tools>>,
    refresh_seen: AtomicBool,
    hook_runner: parking_lot::Mutex<Arc<dyn HookRunner>>,
}
```

### 2.2 流步骤枚举

```rust
enum MockStreamStep {
    Response(Box<ModelResponse>),         // 完整模型响应
    Error(String),                        // 模拟错误
    Events(Vec<Result<StreamEvent, String>>),  // 原始流事件
    DelayedEvents(Vec<(Duration, Result<StreamEvent, String>)>), // 带延迟的流事件
}
```

### 2.3 QueryDeps Trait 实现

`MockDeps` 实现了完整的 `QueryDeps` trait：

```rust
impl QueryDeps for MockDeps {
    async fn call_model(&self, params: ModelCallParams) -> Result<ModelResponse>
    async fn call_model_streaming(
        &self, params: ModelCallParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>
    async fn microcompact(&self, messages: Vec<Message>) -> Result<Vec<Message>>
    async fn autocompact(
        &self, params: ModelCallParams, tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>>
    async fn reactive_compact(&self, messages: Vec<Message>) -> Result<Option<CompactionResult>>
    async fn collapse_drain(
        &self, messages: Vec<Message>, tracking: Option<AutoCompactTracking>,
    ) -> Result<Option<CompactionResult>>
    async fn execute_tool(
        &self, request: ToolExecRequest, tools: &Tools,
        parent: &AssistantMessage, on_progress: ...
    ) -> Result<ToolExecResult>
    fn get_app_state(&self) -> AppState
    fn uuid(&self) -> String
    fn is_aborted(&self) -> bool
    fn get_tools(&self) -> Tools
    async fn refresh_tools(&self) -> Result<Tools>
    fn hook_runner(&self) -> Arc<dyn HookRunner>
}
```

### 2.4 辅助工具

#### LoopTestTool

用于模拟工具的简单实现：

```rust
struct LoopTestTool {
    name: &'static str,
    concurrency_safe: bool,  // 标记是否为并发安全工具
}
```

#### ObservableInputTool

用于测试 observable input backfill 机制的工具：

```rust
struct ObservableInputTool {
    name: &'static str,
    concurrency_safe: bool,
}
impl Tool for ObservableInputTool {
    fn backfill_observable_input(&self, input: &mut serde_json::Map<String, Value>) {
        // 自动填充 "type", "recipient", "content" 字段
    }
}
```

#### StopContinuationHookRunner

用于测试 Stop Hook 续期机制的 HookRunner：

```rust
struct StopContinuationHookRunner {
    message: String,
    calls: AtomicUsize,
}
impl HookRunner for StopContinuationHookRunner {
    async fn run_stop_hooks(&self, _hook_configs: &[HookEventConfig])
        -> Result<PostToolHookResult>
    {
        Ok(PostToolHookResult::StopContinuation { message: self.message.clone() })
    }
}
```

## 三、测试场景

### 3.1 基础文本响应

```rust
#[tokio::test]
async fn test_simple_text_response_terminates()
```

验证最简单的查询流程：用户输入 -> 模型回复文本 -> 终止。

### 3.2 工具使用流程

```rust
#[tokio::test]
async fn test_tool_use_then_text_response()
```

验证：模型输出工具调用 -> 执行工具 -> 工具结果反馈 -> 模型输出最终文本。

### 3.3 Token 预算补丁

```rust
#[tokio::test]
async fn test_token_budget_continuation_injects_nudge_message()
```

验证 Token 预算达到阈值时是否注入 "Token budget at 50%" 补丁消息。

### 3.4 流式工具执行

```rust
#[tokio::test]
async fn streaming_tool_execution_gate_starts_safe_tools_before_message_stop()
```

验证 `streaming_tool_execution` 门控开启时，并发安全工具在流式传输完成前就开始执行。

### 3.5 错误恢复

| 测试 | 场景 |
|------|------|
| `test_prompt_too_long_reactive_compact_retries_model_call` | prompt_too_long -> 反应式压缩 -> 重试 |
| `test_prompt_too_long_collapse_drain_retries_before_reactive_compact` | prompt_too_long -> collapse_drain -> 重试 |
| `test_prompt_too_long_terminals_after_collapse_and_reactive_fail` | prompt_too_long -> 全部压缩失败 -> 报错终止 |
| `test_fallback_model_retries_stream_start_capacity_error` | 529 错误 -> 回退模型 |
| `test_max_tokens_recovery_escalates_next_request_limit` | max_tokens 截断 -> 扩大输出限制 |

### 3.6 计算机视觉（Computer Use）端到端

```rust
#[tokio::test]
async fn test_computer_use_screenshot_click_round_trip()
```

完整的 Computer Use 三轮回合测试：截屏 -> 解析图像 -> 点击 -> 确认。

### 3.7 观察式输入回填

```rust
#[tokio::test]
async fn observable_input_backfill_clones_yield_without_changing_next_request()
```

验证 `backfill_observable_input` 仅改变 yield 输出，不影响后续请求中的原始输入。

### 3.8 中止与生命周期

```rust
#[tokio::test]
async fn test_abort_before_api_call()
#[tokio::test]
async fn test_max_turns_limit()
#[tokio::test]
async fn test_hook_stopped_tool_execution_yields_attachment_and_stops()
```

## 四、关键设计决策

1. **完整模拟**：`MockDeps` 实现所有 `QueryDeps` trait 方法，模拟完整的查询循环依赖
2. **步骤式响应**：`MockStreamStep` 允许精确控制每个迭代的模型响应
3. **可观测性**：`recorded_params()` 记录所有模型调用参数，便于断言验证
4. **并发控制**：`AtomicUsize` 计数器跟踪工具执行和完成状态
5. **延迟模拟**：`DelayedEvents` 支持超时场景测试（流空闲、流停滞）

## 五、使用方式

```rust
// 创建 MockDeps
let deps = Arc::new(MockDeps::new(vec![make_text_response("Hello!")]));

// 创建查询参数
let params = QueryParams {
    messages: vec![Message::User(...)],
    system_prompt: vec!["You are a helpful assistant.".to_string()],
    // ...
};

// 执行查询
let stream = query(params, deps);
let items: Vec<QueryYield> = stream.collect().await;

// 断言
assert!(items.iter().any(|item| matches!(item, QueryYield::Message(Message::Assistant(_)))));
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-engine/src/query/loop_tests.rs` | MockDeps 定义 + 所有测试用例 |
| `crates/allthecodes-engine/src/query/loop_impl.rs` | 被测试的查询循环实现 |
