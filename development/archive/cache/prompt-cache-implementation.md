# 模型 Prompt Cache 实现说明

> 最后更新: 2026-05-27

本文说明 allthecodes 当前如何实现模型 prompt cache。这里的 prompt cache 特指 Anthropic Messages API 的服务端 prompt caching，不包括文件读写状态缓存、WebFetch 响应缓存、TUI 渲染缓存、插件下载缓存等本地缓存。

## 结论

allthecodes 不在本地保存模型 prompt cache 的内容，也不维护本地 cache key/value 数据库。当前实现是：

1. 在模型请求 JSON 中给可缓存内容加 `cache_control` 标记。
2. 由支持 prompt caching 的上游模型服务端创建、读取和淘汰缓存。
3. 从模型返回的 `usage` 中读取 `cache_read_input_tokens` 与 `cache_creation_input_tokens`。
4. 将这些 usage 字段保存在 assistant message、会话统计、状态栏 payload、session export 和 usage 命令输出中。

因此，本项目本地能记录的是“请求上哪些位置尝试启用 prompt cache”以及“模型返回了多少 cache read / cache creation token”，不是缓存内容本身。

## 主要代码入口

| 责任 | 文件 |
|------|------|
| 构造 `MessagesRequest` 并插入 prompt cache marker | `crates/allthecodes-engine/src/lifecycle/helpers.rs` |
| 系统 prompt 静态/动态边界 | `crates/allthecodes-engine/src/prompt_sections.rs` |
| 系统 prompt 生成并插入动态边界 | `crates/allthecodes-engine/src/system_prompt.rs` |
| Provider 发送请求前应用/移除 cache 字段 | `crates/allthecodes-api/src/api/stream_provider.rs` |
| cache policy、TTL、global scope 类型 | `crates/allthecodes-api/src/api/client/types.rs` |
| JSON cache 字段替换/剥离 | `crates/allthecodes-api/src/api/client/body.rs` |
| Anthropic beta header 自动选择 | `crates/allthecodes-api/src/api/client/headers.rs` |
| SSE usage 解析与 assistant message 聚合 | `crates/allthecodes-api/src/api/streaming.rs` |
| Usage DTO | `crates/allthecodes-types/src/message.rs` |
| Session usage 累计 | `crates/allthecodes-types/src/sdk.rs`、`crates/allthecodes-engine/src/lifecycle/types.rs` |
| 会话与 request snapshot 落盘 | `crates/allthecodes-session/src/storage.rs`、`crates/allthecodes-session/src/request_snapshot.rs` |

## 请求构造阶段

### 1. Query loop 传入 `ModelCallParams`

主查询循环在每轮调用模型前生成 `ModelCallParams`，其中包含：

- `messages`
- `system_prompt`
- `tools`
- `model`
- `max_output_tokens`
- `skip_cache_write`
- thinking / effort / advisor 等其他模型配置

`skip_cache_write` 是 prompt cache marker 选择逻辑的一部分。它不会直接关闭所有缓存字段，而是影响 user message marker 放在哪一条消息上。

自动压缩、token count 等内部模型调用会将 `skip_cache_write` 设为 `Some(true)`，避免把临时计数/摘要请求的新内容写进 prompt cache。

### 2. `build_messages_request` 转成 API 请求

`build_messages_request` 负责把内部 `Message` 转成 Anthropic 风格请求体：

- user message 转成 `{ "role": "user", "content": ... }`
- assistant message 转成 `{ "role": "assistant", "content": ... }`
- 部分 attachment 会转成 user message
- system/progress message 不发送给模型

随后它对 system prompt 和 user message 加 cache marker。

## 系统 Prompt Cache Marker

### 静态/动态边界

系统 prompt 中使用常量：

```text
__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__
```

该常量定义在 `prompt_sections.rs`，含义是：

- 边界之前：静态、跨会话或跨组织更稳定的系统 prompt 内容，适合 prompt cache。
- 边界之后：动态、用户/会话/环境相关内容，不应放进全局或长期缓存前缀。

`system_prompt.rs` 在生成完整系统 prompt 时会插入这个边界。

### marker 插入规则

`build_system_prompt_blocks(parts)` 会扫描 `system_prompt`：

1. 边界前的内容合并成一个 system text block。
2. 如果静态前缀非空，给这个 block 加：

```json
{
  "type": "text",
  "text": "...",
  "cache_control": { "type": "ephemeral" }
}
```

3. 边界后的内容合并成另一个 system text block，但不加 `cache_control`。

这样可以让稳定系统 prompt 命中缓存，同时避免把动态环境信息错误纳入缓存前缀。

## User Message Cache Marker

Anthropic prompt caching 通常需要在请求中设置 cache breakpoint。allthecodes 除 system 静态前缀外，还会尝试给 user message 加一个 marker。

### marker budget

当前逻辑保守地限制 marker 数量：

```rust
let message_marker_budget = 4usize.saturating_sub(system_marker_count).min(1);
```

实际效果是：

- system 静态前缀最多用一个 marker。
- user messages 最多再加一个 marker。
- 如果 system 已经耗尽预算，则省略 user message marker。

### eligible user message

只有 role 为 `user` 且 content 中有非空 text 的消息才可加 marker：

- string content 非空即可。
- array content 中至少有一个 `{ "type": "text", "text": "..." }` 非空即可。

工具结果、图片、空文本等不会作为 marker 目标。

### 默认行为

当 `skip_cache_write != Some(true)` 时：

- 选择最后一条 eligible user message。
- 如果它是 string content，会转换成 text block array 并加 `cache_control`。
- 如果它已经是 block array，会给最后一个非空 text block 加 `cache_control`。

### `skip_cache_write = true`

当 `skip_cache_write == Some(true)` 时：

- 选择倒数第二条 eligible user message。
- 如果 eligible user message 少于 2 条，则不加 user message marker。

这个设计用于“只读缓存、尽量不写入当前临时请求”的场景。例如自动压缩和 token 计数类请求可以复用已有上下文缓存，但避免把本轮临时请求末尾写成新的 cache breakpoint。

## Provider 阶段

### 官方 Anthropic

`AnthropicStreamProvider::stream` 在发送请求前会把 `MessagesRequest` 序列化成 JSON，并判断 base URL 是否是官方 Anthropic：

- 官方 host: `api.anthropic.com`
- 官方 Anthropic 才应用 prompt cache policy。

对于官方 Anthropic，请求体中的 marker 会经过：

```rust
apply_prompt_cache_policy_to_body(...)
```

该函数会递归查找所有 `cache_control` 字段，并将其替换为最终 policy。

### Anthropic-compatible

如果 base URL 不是官方 Anthropic，则视为 Anthropic-compatible endpoint。当前实现会调用：

```rust
strip_anthropic_compatible_only_fields(...)
```

这会移除：

- `cache_control`
- `cache_reference`
- `cache_edits`
- `thinking`
- `output_config`
- `context_management`
- thinking / redacted_thinking content blocks

原因是兼容服务通常不支持 Anthropic 的专有 prompt cache 和 thinking 字段，保留这些字段可能导致请求失败。

### OpenAI-compatible / Google

OpenAI-compatible 和 Google provider 不使用 Anthropic prompt cache 字段。转换请求体时会剥离或忽略 `cache_control` 等 Anthropic 专有字段。它们的 usage 一般只填 input/output/reasoning token，cache read/create 默认为 0。

## Cache Policy

Prompt cache policy 定义在 `PromptCachePolicy`：

```rust
pub struct PromptCachePolicy {
    pub enabled: bool,
    pub ttl_1h: bool,
    pub global_scope: bool,
}
```

当前环境变量：

| 变量 | 作用 |
|------|------|
| `ALLTHECODES_PROMPT_CACHE_TTL=1h` | 如果 provider capability 支持，给 cache marker 加 `"ttl": "1h"` |
| `ALLTHECODES_PROMPT_CACHE_GLOBAL=true` | 如果是官方 Anthropic 且 capability 支持，给 cache marker 加 `"scope": "global"` |
| `ALLTHECODES_PROMPT_CACHE_BREAK_DETECTION=true` | 输出 prompt cache marker 省略原因的 debug 诊断 |
| `CC_RUST_PROMPT_CACHE_BREAK_DETECTION=true` | 旧环境变量兼容 |

默认 marker 是：

```json
{ "type": "ephemeral" }
```

开启 1h TTL 和 global scope 后可能变成：

```json
{
  "type": "ephemeral",
  "ttl": "1h",
  "scope": "global"
}
```

`global_scope` 额外要求 `direct_official_anthropic == true`，避免对非官方 endpoint 发送 global cache scope。

## Beta Header

Anthropic prompt cache 需要对应 beta header。`build_anthropic_headers_for_body` 会检查最终请求体：

- 只要 body 里有 `cache_control`，加入 `prompt-caching-2024-07-16`
- 如果 `cache_control` 中含 `ttl`，加入 `extended-cache-ttl-2025-04-11`
- 如果 `cache_control` 中含 `scope`，加入 `prompt-caching-scope-2026-01-05`
- token counting 请求额外加入 `token-counting-2024-11-01`

兼容 endpoint 会使用 `build_anthropic_headers_for_body_with_beta_policy(..., include_anthropic_beta_header = false)`，避免发送 Anthropic beta header。

## Count Tokens 请求

`count_input_tokens_exact` 对 Anthropic count_tokens 请求也会应用类似逻辑：

- 官方 Anthropic：保留并应用 prompt cache marker policy。
- 非官方 compatible：剥离 Anthropic-only 字段。

这能让 token diagnostics 更接近真实请求形状，同时避免兼容 endpoint 因 cache 字段失败。

## 返回 Usage 的记录链路

### 1. SSE 解析

Anthropic SSE 中：

- `message_start` 包含初始 usage。
- `message_delta` 可能包含增量/最终 usage。

`parse_sse_event` 将它们解析成统一的 `StreamEvent`。

### 2. StreamAccumulator 聚合

`StreamAccumulator` 持有一个 `Usage`：

```rust
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}
```

处理规则：

- `MessageStart` 时用服务端返回的 usage 初始化。
- `MessageDelta` 时更新 output/reasoning 等 token。
- 构造最终 `AssistantMessage` 时写入 `usage: Some(self.usage)`。

注意：当前 `MessageDelta` 聚合逻辑主要更新 input/output/reasoning token；Anthropic 的 cache read/create 通常来自 `message_start` 的 usage，因此会保留在初始 usage 中。

### 3. Query loop 累计

查询循环拿到 `assistant_message.usage` 后，会把当前 API call 的 usage 加到 `cumulative_usage`：

- `input_tokens`
- `output_tokens`
- `cache_read_input_tokens`
- `cache_creation_input_tokens`

随后 assistant message 会被 yield 给 lifecycle 层。

### 4. Session usage 累计

`submit_message` 收到 assistant message 后：

1. 将 assistant message 加入 engine state 的 `messages`。
2. 如果有 usage，则调用 `UsageTrackingExt::add_usage`。
3. `UsageTracking` 累计：
   - `total_input_tokens`
   - `total_output_tokens`
   - `total_cache_read_tokens`
   - `total_cache_creation_tokens`
   - `total_cost_usd`
   - `api_call_count`

## 成本计算

成本计算会把 cache token 单独按不同倍率计价：

- 普通 input token: `input_per_1m`
- output token: `output_per_1m`
- cache read token: `input_per_1m * 0.1`
- cache creation token: `input_per_1m * 1.25`

这在 `allthecodes-models/src/pricing.rs` 和 `/cost`、`/extra-usage` 等命令里都会体现。

## 落盘与导出

### Session 文件

会话保存在：

```text
{ALLTHECODES_HOME 或 ~/.allthecodes}/sessions/<session_id>.json
```

assistant message 的 `usage` 会一起保存，因此历史会话中能看到：

```json
{
  "usage": {
    "input_tokens": 123,
    "output_tokens": 45,
    "reasoning_output_tokens": 0,
    "cache_read_input_tokens": 1000,
    "cache_creation_input_tokens": 2000
  }
}
```

### Request snapshot

每次模型请求会记录 request snapshot：

```text
{ALLTHECODES_HOME 或 ~/.allthecodes}/sessions/<session_id>.requests.ndjson
```

这里保存的是 engine 交给 provider client 前的规范化请求体，不包含 credentials 和 headers。它可以用来检查系统 prompt / user message 上是否有 `cache_control` marker。

注意：snapshot 记录发生在 provider 实际发送前，因此它通常保存 `build_messages_request` 生成的 marker 形态。TTL/global scope 的最终替换是在 provider 发送阶段完成的，不一定反映在 snapshot 中。

### Session export

session export 会把 transcript 和 context snapshot 输出到：

```text
{ALLTHECODES_HOME 或 ~/.allthecodes}/exports/<session_id>.session.json
```

其中 context snapshot 会汇总 `cache_read_tokens`。raw transcript 会保留 assistant message 的完整 `usage`。

## UI / 命令展示

### 状态栏

TUI 收到最终 `SdkMessage::Result` 后，会把：

- `total_cache_read_tokens`
- `total_cache_creation_tokens`

传给 `App::update_session_usage`。状态栏 payload 中的 `context.cacheReadTokens` 和 `context.cacheCreationTokens` 来自这里。

### `/cost`

`/cost` 从当前 messages 中遍历 assistant message，汇总：

- input
- output
- cache read
- cache creation
- total cost

当 cache token 非 0 时会显示 `Cache read` 与 `Cache creation`。

### `/extra-usage`

`/extra-usage` 提供更细的统计：

- 每次 API call 的 input/output/cache read/cost
- cache hit rate
- cache read / cache write 估算成本

## Prompt Section 本地缓存与模型 Prompt Cache 的区别

`prompt_sections.rs` 中还有一个 `SECTION_CACHE`：

```rust
static SECTION_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>>
```

这是本地进程内 memoization，用于避免每轮重复计算稳定系统 prompt section。它和模型 prompt cache 不同：

- 它只存在于当前进程内。
- 保存的是系统 prompt section 的字符串结果。
- `/clear` 和 `/compact` 会清理。
- 它不会代表模型服务端是否命中 prompt cache。

模型 prompt cache 是否命中，只能看模型返回的 `cache_read_input_tokens`。

## 诊断方法

### 1. 检查 request snapshot

查看请求是否包含 `cache_control`：

```bash
rg '"cache_control"' ~/.allthecodes/sessions/*.requests.ndjson
```

如果请求使用官方 Anthropic，且 snapshot 中有 marker，provider 发送阶段会继续应用 prompt cache policy。

### 2. 检查 usage

查看 session 文件里的 cache token：

```bash
rg '"cache_read_input_tokens"|"cache_creation_input_tokens"' ~/.allthecodes/sessions
```

或者在应用内运行：

```text
/cost
/extra-usage
```

### 3. 打开 marker 省略诊断

```bash
ALLTHECODES_PROMPT_CACHE_BREAK_DETECTION=1 allthecodes
```

当没有 eligible user message、marker budget 被 system 耗尽、`skip_cache_write` 但 eligible user message 不足两条时，会输出 debug 诊断。

### 4. 检查 provider

只有官方 Anthropic path 会保留并应用 Anthropic prompt cache 字段。Anthropic-compatible、OpenAI-compatible、Google 等 provider 不应期望出现 cache read/create token。

## 常见问题

### 为什么本地找不到 prompt cache 文件？

因为模型 prompt cache 由服务端管理。本地不会保存缓存内容，只保存请求 marker 和 usage 统计。

### 为什么 snapshot 里有 `cache_control`，实际 usage 仍然是 0？

常见原因：

- provider 不是官方 Anthropic，发送前 cache 字段被剥离。
- 第一次请求创建缓存，只会产生 `cache_creation_input_tokens`，不会产生 read hit。
- 上下文前缀变化导致服务端无法命中。
- TTL 到期或服务端缓存策略未命中。
- 模型/provider 没有返回 cache usage 字段。

### 为什么 `skip_cache_write=true` 还可能看到 cache marker？

`skip_cache_write=true` 当前语义不是“删除所有 cache marker”，而是把 user message marker 从最后一条 eligible user message 移到倒数第二条。这样可以复用已有缓存，同时避免把当前临时尾部写入新的 cache breakpoint。

### 为什么 system 动态部分不加 cache marker？

动态部分可能包含 cwd、环境、权限、工具状态、会话上下文等变化信息。把它放进可缓存前缀会降低命中率，也可能让跨会话/global cache 的语义变差。

## 已知边界

- 当前实现没有本地 prompt cache key 管理，也没有本地 cache 内容持久化。
- Request snapshot 记录的是 provider 发送前的 engine 请求形态，不包含最终 headers，也不一定包含 TTL/global policy 替换后的最终 body。
- OpenAI-compatible 和 Google provider 的 cache token 字段通常为 0。
- `StreamAccumulator` 的 cache read/create token 依赖 provider 在 usage 中返回；如果 provider 不返回，allthecodes 不会推断。
- prompt cache marker 数量当前保守限制为 system 静态前缀一个 + user message 一个。
