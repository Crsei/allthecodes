---
title: "流式响应机制 - allthecodes SSE 事件处理与实时输出"
description: "解析 allthecodes (Rust port of Claude Code) 的流式响应实现：SSE 字节流解析、StreamAccumulator 事件累积、多 Provider 适配（Anthropic/OpenAI-compat/Gemini）、错误处理与重试。"
keywords: ["流式响应", "SSE", "streaming", "StreamAccumulator", "Provider", "重试"]
sourceRef: "ea4e6ab2 (2026-05-28)"
---

{/* 本章目标：从源码角度揭示流式 API 调用的全链路——HTTP 字节流 → SSE 事件 → StreamAccumulator → AssistantMessage */}

## 为什么需要流式

想象 AI 需要 30 秒才能生成完整回答——如果等 30 秒后才一次性显示，用户体验是灾难性的。

allthecodes 的流式响应让用户**实时看到 AI 的思考过程**：
- 文字逐字出现，用户能提前判断方向是否正确
- 工具调用的参数在生成过程中就能预览
- 长时间任务不会让用户觉得"卡死了"
- [流式工具执行](../the-loop)在流式阶段就可启动并发安全的工具

## 整体架构

流式链路跨越三个层次：

```
HTTP 字节流（Provider 发送）
  ↓ parse_sse_byte_stream()    — SSE 协议解析（client/stream.rs）
  ↓ StreamEvent                 — 统一事件类型
  ↓ StreamAccumulator           — 事件累积 + AssistantMessage 构建（streaming.rs）
  ↓ query() loop                — 消费事件 + 驱动 Agentic Loop
  ↓ UI 层                       — 实时渲染
```

## SSE 字节流解析

`crates/allthecodes-api/src/api/client/stream.rs` 中的 `parse_sse_byte_stream()` 将 HTTP 响应字节流解析为 `StreamEvent`：

```
event: message_start
data: {"type":"message_start","message":{"usage":{...}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{...}}

event: message_stop
data: {"type":"message_stop"}
```

### 解析状态机

`parse_sse_byte_stream` 维护行缓冲区，按行处理：

```
缓冲 bytes → String buffer
  ↓ 逐行切割（找 \n）
  ↓ 空行 → flush 当前 event+data → parse_sse_event()
  ↓ "event: xxx" → 设置 current_event_type
  ↓ "data: xxx"  → 追加到 current_data
  ↓ 其他（":", "id:", "retry:"）→ 忽略
  ↓ 流结束 → flush 残留事件
```

关键设计：`data:` 字段支持多行拼接（`\n` 分隔），解决 JSON payload 跨行问题。

## StreamEvent 事件类型

`crates/allthecodes-api/src/api/streaming.rs` 中 `parse_sse_event()` 将解析后的 SSE 事件分发为 `StreamEvent` 枚举：

| SSE 事件 | StreamEvent 变体 | 携带数据 |
|----------|-----------------|---------|
| `message_start` | `MessageStart` | `Usage`（input_tokens、cache 统计） |
| `content_block_start` | `ContentBlockStart` | `index` + `ContentBlock` |
| `content_block_delta` | `ContentBlockDelta` | `index` + delta JSON |
| `content_block_stop` | `ContentBlockStop` | `index` |
| `message_delta` | `MessageDelta` | `Delta`（stop_reason）+ `Option<Usage>` |
| `message_stop` | `MessageStop` | —（流结束标记） |
| `ping` | 忽略返回 None | — |
| `error` | 作为 Error 向上传播 | `NormalizedApiError` |

### 内容块类型与 Delta 处理

| 内容块类型 | Delta 子类型 | 累加逻辑 |
|-----------|-------------|----------|
| `text` | `text_delta`（或 legacy 无 type） | `text.push_str(delta.text)` |
| `thinking` | `thinking_delta` + `signature_delta` | `thinking.push_str()` + `signature.push_str()` |
| `tool_use` | `input_json_delta` | `partial_json` 拼接 → `ContentBlockStop` 时解析 |
| `server_tool_use` | `input_json_delta` | 同 tool_use |
| `connector_text` | `connector_text_delta` + `signature_delta` | 同 thinking |

支持遗留格式（无 `type` 字段的 delta），保持与旧版 Anthropic API 的兼容。

## StreamAccumulator：事件累积器

`StreamAccumulator`（`crates/allthecodes-api/src/api/streaming.rs`）是流事件到 `AssistantMessage` 的转换核心：

```rust
struct StreamAccumulator {
    content_blocks: Vec<ContentBlock>,    // 正在构建的内容块数组
    usage: Usage,                         // Token 用量（message_start 初始化）
    stop_reason: Option<String>,          // 结束原因（message_delta 设置）
    tool_input_partials: Vec<String>,     // 工具输入的 partial JSON 缓冲区
    stopped_blocks: Vec<bool>,            // 哪些 block 已收到 ContentBlockStop
}
```

### 事件处理状态机

`process_event(&mut self, event: &StreamEvent)` 根据事件类型变更内部状态：

| 事件 | 状态变更 |
|------|---------|
| `MessageStart` | 初始化 `self.usage` |
| `ContentBlockStart` | 创建/覆盖 `content_blocks[index]`，清空 `tool_input_partials[index]` |
| `ContentBlockDelta` | 按 delta type 追加到对应 content_block |
| `ContentBlockStop` | `finalize_tool_input(index)` + `stopped_blocks[index] = true` |
| `MessageDelta` | 设置 `stop_reason`，合并最终 usage（支持 OpenAI-compat 的双向 token 报告） |

### 工具输入的 Partial JSON 处理

`tool_use` 块的 `input_json_delta` 是增量 JSON 片段，需要拼接后解析：

```rust
// 示例流式输入：
// delta 1: {"type":"input_json_delta","partial_json":"{\"command\":\"echo"}
// delta 2: {"type":"input_json_delta","partial_json":" hi\",\"timeout\":1000}"}

// ContentBlockStop 时：
fn finalize_tool_input(&mut self, index: usize) {
    let partial_json = self.tool_input_partials[index];
    if let Ok(input) = serde_json::from_str::<Value>(&partial_json) {
        content_blocks[index].input = input;
    }
}
```

`completed_tool_use(index)` 方法在流式过程中暴露已完成的 tool_use 块（需要 `stopped_blocks[index] == true`），供 Agentic Loop 的 `StreamingToolExecutor` 在流结束前启动工具执行。

### 构建最终 AssistantMessage

```rust
fn build_with_uuid(self, model: &str, uuid: Uuid) -> AssistantMessage {
    // 最终化所有 tool_input
    for index in 0..self.content_blocks.len() {
        self.finalize_tool_input(index);
    }
    // 计算费用
    let cost_usd = calculate_cost(model, &self.usage);
    AssistantMessage {
        uuid,
        timestamp: chrono::Utc::now().timestamp(),
        role: "assistant",
        content: self.content_blocks,
        usage: Some(self.usage),
        stop_reason: self.stop_reason,
        cost_usd,
    }
}
```

费用计算通过 `crate::api::pricing::calculate_cost()` 完成，基于 `allthecodes_models::get_pricing()` 返回的模型定价数据（input/output/cache 价格）。

## 多 Provider 适配（StreamProvider Trait）

`crates/allthecodes-api/src/api/stream_provider.rs` 定义统一的 `StreamProvider` trait：

```rust
#[async_trait]
trait StreamProvider: Send + Sync {
    async fn stream(
        &self,
        http: &reqwest::Client,
        request: &MessagesRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>;
}
```

三个实现：

| Provider | 实现类 | 协议 | 特殊处理 |
|----------|--------|------|----------|
| **Anthropic / Azure** | `AnthropicStreamProvider` | 原生 SSE（`/v1/messages`） | Beta header 策略、prompt cache 策略、request-id 提取 |
| **OpenAI 兼容** | `OpenAiCompatStreamProvider` | `chat/completions` SSE | OpenAI → Anthropic 事件格式转换 |
| **Google Gemini** | `GoogleStreamProvider` | `streamGenerateContent` | Google → Anthropic 事件格式转换 |

### Provider 路由

`ApiClient`（`crates/allthecodes-api/src/api/client/mod.rs`）存储 `Box<dyn StreamProvider>`，`messages_stream()` 方法通过多态分发请求，消除 `match` 式路由：

```rust
// crates/allthecodes-api/src/api/client/messages.rs
impl ApiClient {
    pub async fn messages_stream(&self, request: MessagesRequest)
        -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>
    {
        self.messages_stream_with_backoff(request, retry_config, sleep).await
    }

    async fn messages_stream_with_backoff(...) {
        loop {
            match self.stream_provider.stream(&self.http, &request).await {
                Ok(stream) => return Ok(stream),
                Err(error) => {
                    let category = categorize_stream_start_error(&error.to_string());
                    if !category.is_retryable() || retry_attempt >= max_retries {
                        return Err(error);
                    }
                    // 指数退避 + jitter
                    sleep(delay).await;
                    retry_attempt += 1;
                }
            }
        }
    }
}
```

### OpenAI 兼容 Provider 适配

`crates/allthecodes-api/src/api/openai_compat/` 处理 OpenAI 格式的流响应：
- 从 `/chat/completions` SSE 流中提取 `choices[0].delta`
- 转换为 Anthropic 内部的 `ContentBlockDelta`、`ContentBlockStop` 等事件
- 支持 codex provider（`openai-codex`）的 `/codex/responses` 端点

### Google Gemini 适配

`crates/allthecodes-api/src/api/google_provider.rs` 处理 Gemini 的 `streamGenerateContent`：
- Gemini 使用 `candidates[0].content.parts` 而非逐块 SSE
- 在 `api/vertex.rs` 和 `api/google_provider.rs` 中完成格式适配

## 错误处理与重试

### 流启动失败

`crates/allthecodes-api/src/api/retry.rs` 提供精细的错误分类：

| 错误类别 | 是否可重试 | 示例 |
|---------|-----------|------|
| `RateLimit` | 是（指数退避） | HTTP 429 |
| `Overloaded` | 是（指数退避 + 模型降级） | HTTP 529 |
| `ServerError` | 是 | HTTP 500/502/503、网络超时 |
| `InvalidRequest` | 否 | HTTP 400（bad request） |
| `AuthError` | 否 | HTTP 401/403 |
| `PromptTooLong` | 否（Agentic Loop 内部处理） | 上下文超限 |
| `MaxOutputTokens` | 否（Agentic Loop 内部处理） | 输出截断 |

`RetryConfig` 默认配置：
- `max_retries: 3`
- `initial_delay_ms: 1000`（1 秒）
- `max_delay_ms: 30000`（30 秒）
- `backoff_multiplier: 2.0`（指数退避）
- `retryable_status_codes: [429, 500, 502, 503, 529]`

退避延迟加入 20% 的随机 jitter，避免惊群效应。

### 流中断恢复

Agentic Loop（`loop_impl.rs`）消费流时检测两种超时：

| 超时类型 | 默认值 | 检测机制 | 可配置环境变量 |
|---------|-------|---------|--------------|
| **空闲超时** | 120 秒（生产）/ 50ms（测试） | `tokio::time::timeout` 包裹 `stream.next()` | `ALLTHECODES_STREAM_IDLE_TIMEOUT_MS` |
| **停滞超时** | 60 秒（生产）/ 25ms（测试） | 计算上次进度事件到当前时间差 | `ALLTHECODES_STREAM_STALL_TIMEOUT_MS` |

流中断后，`classify_model_call_failure` 评估恢复策略：
- 如果有 fallback 模型且错误是 overloaded → tombstone 当前 assistant → 切换到 fallback 重试
- 否则 → yield 错误消息 → 终止 Agentic Loop

### 非流式降级

`ApiClient::messages()` 提供纯同步（非流式）fallback：内部调用 `messages_stream()` 收集所有事件，通过 `StreamAccumulator` 一次性构建 `AssistantMessage`。这是流式失败的最后防线。

## 费用计算

`crates/allthecodes-api/src/api/pricing.rs` 的 `calculate_cost()` 在每次流结束后计算费用：

```rust
fn calculate_cost(model: &str, usage: &Usage) -> f64 {
    allthecodes_models::get_pricing(model).cost_from_counts(
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
    )
}
```

费用写入 `AssistantMessage.cost_usd` 字段，跨迭代累计在 `cumulative_usage` 中，最终随 Transcript 持久化。

## 事件流完整示例

```
时间线：
1. message_start
   → accumulator.usage = {input_tokens: 42, output_tokens: 0}
2. content_block_start(index=0, Text)
   → accumulator.content_blocks[0] = Text { text: "" }
3. content_block_delta(index=0, text_delta="我来帮")
   → accumulator.content_blocks[0].text += "我来帮"
4. content_block_delta(index=0, text_delta="你修复")
   → accumulator.content_blocks[0].text += "你修复"
5. content_block_stop(index=0)
   → accumulator.stopped_blocks[0] = true
6. content_block_start(index=1, ToolUse { name: "Bash" })
   → accumulator.content_blocks[1] = ToolUse
   → accumulator.tool_input_partials[1] = ""
7. content_block_delta(index=1, input_json_delta="{\"comman")
   → accumulator.tool_input_partials[1] += "{\"comman"
8. content_block_delta(index=1, input_json_delta="d\":\"ls\"}")
   → accumulator.tool_input_partials[1] += "d\":\"ls\"}"
9. content_block_stop(index=1)
   → accumulator.finalize_tool_input(1)
   → accumulator.completed_tool_use(1) = Some(...)
   → 触发 StreamingToolExecutor 启动 Bash 工具
10. message_delta(stop_reason="tool_use", usage={output_tokens: 15})
    → accumulator.stop_reason = "tool_use"
    → accumulator.usage.output_tokens = 15
11. message_stop
    → 流结束
    → accumulator.build("claude-sonnet-4-20250514")
    → AssistantMessage { content: [Text("我来帮你修复"), ToolUse(Bash, {command:"ls"})], ... }
```

Agentic Loop 消费 `QueryYield::Stream(StreamEvent)` 事件流，UI 层在收到每个事件后更新显示——实现打字机效果的实时输出。`StopHookResult::PreventStop` 等阻塞响应会触发额外轮次，产生新的流事件。
