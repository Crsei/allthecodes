# openai_compat.rs 拆分计划

> 原文件: `crates/cc-api/src/api/openai_compat.rs`
> 原始行数: ~1622
> 目标: 拆分为 4 个子模块 + mod.rs，每个 <= 400 行

## 当前结构分析

文件导出一个 `pub(crate)` 函数 (`openai_compat_stream`) 和大量私有辅助函数。仅有的外部调用者在 `stream_provider.rs` (L136), 通过 `crate::api::openai_compat::openai_compat_stream` 调用.

整个文件按职责自然划分成四个逻辑区域:

| 区域 | 行号 | 近似行数 | 内容 |
|---|---|---|---|
| Doc + imports | L1-25 | 25 | 模块注释、use 语句 |
| 格式转换辅助函数 | L27-276 | 250 | `reasoning_output_tokens_from_usage`, `extract_system_text`, `flatten_content`, `content_items_for_responses`, `build_responses_input`, `build_responses_tools`, `sanitize_responses_function_parameters` |
| 请求构建器 | L278-563 | 286 | `normalize_codex_reasoning_effort`, `build_codex_responses_request`, `build_openai_request` |
| HTTP 流入口 | L565-630 | 66 | `pub(crate) async fn openai_compat_stream` — URL构建、鉴权、HTTP请求、派发到SSE解析器 |
| SSE 字节流解析器 | L632-1115 | 484 | `parse_openai_sse_byte_stream` — 包含两条完全独立的路径(codex和标准chat) |
| Tests | L1117-1622 | 506 | `#[cfg(test)] mod tests` |

**函数间依赖关系** (调用图):

```
openai_compat_stream
  -> build_openai_request
        -> extract_system_text, flatten_content, build_codex_responses_request
              -> build_responses_input -> content_items_for_responses -> flatten_content
              -> build_responses_tools -> sanitize_responses_function_parameters
              -> normalize_codex_reasoning_effort
  -> parse_openai_sse_byte_stream
        -> reasoning_output_tokens_from_usage
        (内部两条独立分支: is_codex_provider 和 !is_codex_provider)
```

**关键发现**: `parse_openai_sse_byte_stream` 内部两条分支 (`is_codex_provider` vs `!is_codex_provider`) 完全独立，没有共享状态或控制流。`build_openai_request` 也以 `is_openai_codex_provider` 快速派发到 `build_codex_responses_request`。

## 拆分方案

### 子模块 1: `format.rs` (~180 行 + ~60 行测试)

- **职责**: 纯格式转换辅助函数，将 Anthropic 消息格式转换为 OpenAI 格式。无外部依赖。
- **迁移内容**:
  - `fn reasoning_output_tokens_from_usage` (L30-37)
  - `fn extract_system_text` (L42-57)
  - `fn flatten_content` (L63-83)
  - `fn content_items_for_responses` (L85-93)
  - `fn sanitize_responses_function_parameters` (L264-276)
- **测试 (colocated)**:
  - `test_extract_system_text_blocks` (L1127)
  - `test_extract_system_text_empty` (L1138)
  - `test_flatten_content_string` (L1144)
  - `test_flatten_content_text_blocks` (L1150)
  - `test_flatten_content_none` (L1159)
  - `test_flatten_content_tool_result` (L1164)
- **依赖**: `serde_json::{json, Value}`
- **被依赖**: `builder.rs`, `chat_stream.rs`, `codex.rs`

### 子模块 2: `builder.rs` (~240 行 + ~220 行测试)

- **职责**: 构建请求体 JSON。包含 chat completions 格式和 codex Responses API 格式的完整构建逻辑。
- **迁移内容**:
  - `fn build_responses_input` (L95-232)
  - `fn build_responses_tools` (L234-262)
  - `fn normalize_codex_reasoning_effort` (L311-321)
  - `fn build_codex_responses_request` (L278-309)
  - `fn build_openai_request` (L328-563)
- **测试 (colocated)**:
  - `test_build_openai_request_basic` (L1173)
  - `test_build_openai_request_strips_anthropic_cache_fields` (L1199)
  - `test_build_openai_request_codex_compatible_shape` (L1250)
  - `test_build_codex_responses_skips_invalid_function_schema_roots` (L1290)
  - `test_build_openai_request_no_system` (L1337)
  - `test_build_openai_request_content_blocks` (L1367)
  - `test_build_deepseek_request_preserves_empty_reasoning_content_for_tool_call` (L1400)
  - `test_build_openai_request_does_not_send_deepseek_reasoning_to_other_providers` (L1444)
- **依赖**: `super::format` (extract_system_text, flatten_content, content_items_for_responses, sanitize_responses_function_parameters), `crate::api::client::{MessagesRequest, strip_anthropic_cache_fields, is_openai_codex_provider}`
- **被依赖**: `chat_stream.rs`

### 子模块 3: `chat_stream.rs` (~70 行入口 + ~280 行解析器 + ~120 行测试)

- **职责**:
  1. `openai_compat_stream` — HTTP 入口, 执行 URL 构建、鉴权、HTTP 请求发送, 然后派发到正确的解析器
  2. `parse_chat_sse_byte_stream` — 标准 OpenAI chat completions SSE 解析 (`choices[n].delta` 格式)
- **迁移内容**:
  - `pub(crate) async fn openai_compat_stream` (L571-630)
  - `fn parse_chat_sse_byte_stream` — 从原有 `parse_openai_sse_byte_stream` 提取 chat 分支 (L916-1096 的主体逻辑, 加上 L656-714 的通用框架)
- **测试 (colocated)**:
  - `test_parse_deepseek_empty_reasoning_content_tool_call` (L1546)
- **依赖**: `super::format`, `super::builder` (build_openai_request), `super::codex` (parse_codex_sse_byte_stream), `crate::api::client::{build_openai_compat_url, is_openai_codex_provider, MessagesRequest}`, `cc_types::message::*`
- **被依赖**: 无 (这是唯一有 `pub(crate)` 导出的模块)

### 子模块 4: `codex.rs` (~200 行 + ~90 行测试)

- **职责**: OpenAI Responses API (codex) SSE 事件流解析。对应 `is_codex_provider` 路径的所有 SSE 事件类型处理。
- **迁移内容**:
  - `fn parse_codex_sse_byte_stream` — 从原有 `parse_openai_sse_byte_stream` 提取 codex 分支 (L721-913 的主体逻辑, 加上 L656-714 的通用框架)
- **测试 (colocated)**:
  - `test_parse_codex_responses_text_stream` (L1486)
- **依赖**: `super::format` (reasoning_output_tokens_from_usage), `cc_types::message::*`
- **被依赖**: `chat_stream.rs`

### 模组根: `mod.rs` (~25 行)

- **职责**: 模块文档、子模块声明、重导出
- **内容**:
  ```rust
  pub(crate) use chat_stream::openai_compat_stream;

  mod format;
  mod builder;
  mod chat_stream;
  mod codex;
  ```

## 拆分后目录结构

```
openai_compat/
├── mod.rs          (~25 行) — 子模块声明 + 重导出 `openai_compat_stream`
├── format.rs       (~240 行) — 格式转换辅助函数 + 内置测试
├── builder.rs      (~460 行) — 请求体构建函数 + 内置测试
├── chat_stream.rs  (~470 行) — HTTP 入口 + chat SSE 解析 + 内置测试
└── codex.rs        (~290 行) — Codex SSE 解析 + 内置测试
```

**关键行数分布**:
- `builder.rs` 最大(~460行), 因为 `build_openai_request` 本身 236 行, 且包含 8 个复杂测试用例
- `chat_stream.rs` 第二大(~470行), 因为 `parse_openai_sse_byte_stream` 被拆为 chat + codex 两部分但框架代码(行缓冲、行解析)需要复制一份到每个解析函数中

## `parse_openai_sse_byte_stream` 拆分策略

当前 `parse_openai_sse_byte_stream` (L647-1115, 469行) 内部结构:

```
通用框架(L656-714) — 流初始化、行缓冲、行切分、`data:` 前缀剥离、`[DONE]` 处理、header 发射
|
+-- if is_codex_provider (L721-913):
|     match v["type"]: response.output_text.delta | response.reasoning_text.delta |
|       response.output_item.done | response.failed | response.completed
|
+-- else (L916-1096):
      process choices[n].delta: reasoning_content | content | tool_calls | finish_reason
      process usage at chunk level

通用收尾(L1100-1114) — 流意外结束时关闭打开的 block 并发射 MessageStop
```

拆分方案:
- `parse_chat_sse_byte_stream` 和 `parse_codex_sse_byte_stream` 各自独立, 各包含自己的框架逻辑 (流初始化 + 行缓冲 + 行切分 + 处理循环 + 收尾). 框架代码会有少量重复, 但每个函数完全自包含, 没有共享可变状态.
- 或者, 提取公共的行缓冲 + 行切分逻辑到一个工具函数 `fn parse_lines_from_buffer` 以消除重复.

## 模块内部依赖图

```
            mod.rs
              |
    +---------+---------+
    |         |         |
  format   builder   chat_stream ---> codex
    |         |           |
    +----+----+           |
         |                |
   [cc_types]       [client::*]
                        |
                   [reqwest, anyhow, futures]
```

所有箭头为单向, 无循环依赖。

## 迁移步骤

### Step 1: 创建目录结构
- 创建 `crates/cc-api/src/api/openai_compat/` 目录
- 创建 5 个新文件: `mod.rs`, `format.rs`, `builder.rs`, `chat_stream.rs`, `codex.rs`

### Step 2: 提取 `format.rs`
- 从原文件复制 `use` imports (仅 `serde_json` 相关的)
- 复制 `reasoning_output_tokens_from_usage`, `extract_system_text`, `flatten_content`, `content_items_for_responses`, `sanitize_responses_function_parameters`
- 复制对应的 5 个单元测试
- 验证: `cargo check` 通过

### Step 3: 提取 `codex.rs` (先于 chat_stream, 因为 chat_stream 依赖它)
- 复制 `use` imports (cc_types, serde_json, futures, async_stream)
- 编写 `parse_codex_sse_byte_stream` 函数 (提取自原 parse_openai_sse_byte_stream 的 codex 分支)
- 复制 `test_parse_codex_responses_text_stream` 测试
- 验证: `cargo check` 通过

### Step 4: 提取 `chat_stream.rs`
- 复制 `use` imports (anyhow, futures, reqwest, serde_json, cc_types, client::*)
- 复制 `openai_compat_stream` 函数 (L571-630), 修改其内部逻辑使其在 codex provider 时调用 `super::codex::parse_codex_sse_byte_stream`, 否则调用自身的 `parse_chat_sse_byte_stream`
- 编写 `parse_chat_sse_byte_stream` 函数 (提取自原 parse_openai_sse_byte_stream 的非 codex 分支)
- 复制 `test_parse_deepseek_empty_reasoning_content_tool_call` 测试
- 验证: `cargo check` 通过

### Step 5: 提取 `builder.rs`
- 复制 `use` imports (serde_json, client::*, format::*)
- 复制 `build_responses_input`, `build_responses_tools`, `normalize_codex_reasoning_effort`, `build_codex_responses_request`, `build_openai_request`
- 复制对应的 8 个单元测试
- 验证: `cargo check` 通过

### Step 6: 编写 `mod.rs`
- 模块文档 (来自原文件顶部 L1-12)
- `pub(crate) use chat_stream::openai_compat_stream;`
- `mod format; mod builder; mod chat_stream; mod codex;`
- 验证: `cargo check` 通过

### Step 7: 删除原文件, 更新父模块引用
- 删除 `crates/cc-api/src/api/openai_compat.rs`
- `mod.rs` 中的 `pub mod openai_compat;` 保持不变 (自动转为目录模块)
- 验证: `cargo build` 通过, `cargo test` 全部通过

## 风险与注意事项

### 1. SSE 解析器框架代码重复
`parse_chat_sse_byte_stream` 和 `parse_codex_sse_byte_stream` 会共享约 60 行的框架代码 (行缓冲、`data:` 前缀解析、`[DONE]` 处理、header 发射、收尾逻辑)。两个选项:
- (a) 接受重复, 保持两个函数完全独立自包含
- (b) 提取公共部分到 `format.rs` 或 `mod.rs`

**推荐 (a)**, 因为:
- 两个解析器的状态机 (block_index, thinking_block_open, text_block_open 等) 当前是交错在框架代码中的, 提取公共部分需要更多重构
- 重复代码在理解时会作为独立的整体, 改动一个不影响另一个
- 未来某个解析器可能演变出不同的行为

### 2. `openai_compat_stream` 的位置选择
放在 `chat_stream.rs` 中是因为:
- `chat_stream.rs` 已经是标准的默认路径, codex 是特例
- 如果放在 `mod.rs` 会导致 `mod.rs` 承载业务逻辑, 不符合 Rust 惯例
- 如果放在独立文件会让 `mod.rs` 的 re-export 显得多余

### 3. 测试可见性
`build_openai_request` 是私有函数 (`fn`, 无 `pub`)。测试模块需要能访问它。在 colocated 测试中这没问题 (`use super::*`)。拆分后, 每个子模块的测试位于该模块内部, 可以正常访问模块内的私有函数。

### 4. `client` 模块依赖
`format.rs` 不依赖 `client` 模块。`builder.rs` 依赖 `client::MessagesRequest` 和 `client::strip_anthropic_cache_fields`。`chat_stream.rs` 依赖 `client::build_openai_compat_url` 和 `client::is_openai_codex_provider`。这些都是已经存在的导入, 只需在新模块中重新导入。

### 5. `OPENAI_CODEX_PROVIDER_NAME` 常量
在原文件的测试中, `test_build_openai_request_codex_compatible_shape` 和 `test_build_codex_responses_skips_invalid_function_schema_roots` 使用了 `crate::api::client::OPENAI_CODEX_PROVIDER_NAME`。移动到 `builder.rs` 后, 需确保导入路径正确。
