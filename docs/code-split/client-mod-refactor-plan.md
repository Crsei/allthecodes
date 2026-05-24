# client/mod.rs 拆分计划

> 原文件: `crates/cc-api/src/api/client/mod.rs`
> 原始行数: ~1798
> 目标: 拆分为 7 个子模块 + mod.rs，每个 ≤ 500 行

## 当前结构分析

文件包含:
- **6 public constants** (OPENAI_CODEX_PROVIDER_NAME, OPENAI_PROVIDER_NAME, 3 env var names, model env vars)
- **4 private constants** (official model names, default alias)
- **1 static** (Once for legacy warning)
- **5 types** (PromptCacheTtl, PromptCacheScope, CacheControl, PromptCacheCapability, PromptCachePolicy) with associated impls
- **2 enums** (ApiProvider with 6 variants, AnthropicAuth with 2 variants)
- **2 structs** (MessagesRequest with 16 fields, ExactTokenCount with 2 fields)
- **1 config struct** (ApiClientConfig)
- **1 main struct** (ApiClient with 3 fields)
- **~25 free functions** (header building, body manipulation, model resolution, validation, env utilities)
- **4 `impl ApiProvider` methods** (endpoint_kind, base_url_host, langfuse_provider_name, capabilities)
- **~22 `impl ApiClient` methods** (construction, streaming, counting, headers, URL building)
- **Already split out**: `stream.rs` (126 lines, SSE parsing), `tests.rs` (2655 lines, comprehensive tests)

**External consumers** (from outside the `client` module):
- `stream_provider.rs`: `apply_prompt_cache_policy_to_body`, `build_anthropic_headers_for_body`, `build_anthropic_headers_for_body_with_beta_policy`, `is_official_anthropic_base_url`, `parse_sse_byte_stream`, `strip_anthropic_compatible_only_fields`, `AnthropicAuth`, `MessagesRequest`, `PromptCacheCapability`
- `openai_compat.rs`: `build_openai_compat_url`, `is_openai_codex_provider`, `strip_anthropic_cache_fields`, `MessagesRequest`, `OPENAI_CODEX_PROVIDER_NAME`
- `bedrock.rs`: `strip_anthropic_cache_fields`, `MessagesRequest`
- `vertex.rs`: `parse_sse_byte_stream`, `strip_anthropic_cache_fields`, `MessagesRequest`
- `google_provider.rs`: `strip_anthropic_cache_fields`, `MessagesRequest`
- External crates (`cc-commands`, `cc-engine`, `claude-code-rs`, `start-up`): `ApiClient`, `ApiProvider`, `MessagesRequest`, `is_env_truthy`, `provider_supports_advisor`, various env var constants

## 拆分方案

### 子模块 1: `types.rs` (~250 行)

- **职责**: All type definitions, enums, structs, and constants. No logic, just data shapes and constants. Foundation module that everything else depends on.
- **迁移内容**:
  - All 6 public constants + 4 private constants
  - `ANTHROPIC_LEGACY_MODEL_ENV_WARNING` static
  - `PromptCacheTtl`, `PromptCacheScope`, `CacheControl`, `PromptCacheCapability`, `PromptCachePolicy` enums/structs + impls
  - `ApiProvider` enum + impl (endpoint_kind, base_url_host, langfuse_provider_name, capabilities)
  - `AnthropicAuth` enum
  - `MessagesRequest` struct
  - `ExactTokenCount` struct
  - `ApiClientConfig` struct
  - `ApiClient` struct
- **依赖**: `cc_types`, `serde`, `anyhow`
- **被依赖**: Every other sub-module

### 子模块 2: `headers.rs` (~128 行)

- **职责**: Anthropic HTTP header construction.
- **迁移内容**:
  - `build_anthropic_headers` (Lxxx)
  - `build_anthropic_headers_for_body`
  - `build_anthropic_headers_for_body_with_beta_policy`
  - `build_anthropic_headers_with_cache_betas` (private, needs `pub(super)`)
  - `body_contains_key` (private, needs `pub(super)`)
  - `body_contains_output_effort` (private, needs `pub(super)`)
  - `body_contains_cache_attr` (private, needs `pub(super)`)
  - `extend_header_string_map` (private, needs `pub(super)`)
- **依赖**: `super::types` (PromptCacheCapability, PromptCachePolicy, MessagesRequest)
- **被依赖**: `builder.rs`, `messages.rs`

### 子模块 3: `body.rs` (~98 行)

- **职责**: JSON body manipulation for cache control and provider compatibility.
- **迁移内容**:
  - `prompt_cache_marker_value`
  - `is_env_value`
  - `strip_anthropic_cache_fields`
  - `strip_anthropic_compatible_only_fields`
  - `strip_anthropic_thinking_blocks`
  - `apply_prompt_cache_policy_to_body`
  - `replace_cache_control_markers`
  - `is_official_anthropic_base_url`
- **依赖**: `super::types` (PromptCacheScope, PromptCacheTtl, CacheControl, MessagesRequest)
- **被依赖**: `builder.rs`, `messages.rs`

### 子模块 4: `model.rs` (~165 行)

- **职责**: Model alias resolution and request preparation.
- **迁移内容**:
  - `build_anthropic_count_tokens_body`
  - `provider_supports_advisor`
  - `anthropic_base_url_from_env`
  - `AnthropicModelAlias` (private struct + impl)
  - `non_empty_env`
  - `selected_api_provider_from_settings`
  - `warn_legacy_anthropic_model_env_once`
  - `anthropic_model_env_for_alias`
  - `resolve_anthropic_model_alias`
  - `resolve_anthropic_default_model`
  - `resolve_model_for_request`
  - `resolve_request_model_for_provider`
- **依赖**: `super::types` (ApiProvider, MessagesRequest)
- **被依赖**: `builder.rs`, `messages.rs`

### 子模块 5: `provider.rs` (~111 行)

- **职责**: Provider-related URL helpers and `impl ApiProvider` methods.
- **迁移内容**:
  - `build_openai_compat_url`
  - `is_openai_codex_provider`
  - 4 `impl ApiProvider` methods (endpoint_kind, base_url_host, langfuse_provider_name, capabilities) -- note: these are method impls, may stay in `types.rs` since they're inherent methods on the enum
- **依赖**: `super::types` (ApiProvider)
- **被依赖**: `builder.rs`, `messages.rs`

### 子模块 6: `builder.rs` (~490 行)

- **职责**: All `ApiClient` construction and factory methods.
- **迁移内容**:
  - `make_stream_provider`
  - `require_non_empty`
  - `validate_base_url`
  - `validate_provider_config`
  - All `impl ApiClient` construction methods: `try_new`, `new`, `from_provider_info`, `from_env_result`, `from_env`, `from_bedrock_env_result`, `from_vertex_env_result`, `from_codex_auth`, `from_codex_auth_result`, `from_openai_api_keychain_result`, `from_backend_result`, `from_backend`, `from_auth_result`, `from_auth`
- **依赖**: `super::types`, `super::provider`, `super::body`, `super::model`, `super::headers`
- **被依赖**: `messages.rs` (for `ApiClient` instances)

### 子模块 7: `messages.rs` (~390 行)

- **职责**: Core `ApiClient` operational methods (streaming, counting, headers, URL building).
- **迁移内容**:
  - `supports_exact_token_count`
  - `count_input_tokens_exact`
  - `count_token_usage_exact`
  - `count_anthropic_input_tokens`
  - `build_url`
  - `build_headers`
  - `build_headers_map`
  - `messages_stream`
  - `messages_stream_with_backoff`
  - `messages`
  - `config`
  - `langfuse_provider_name` (method)
  - `provider_diagnostic`
- **依赖**: `super::types`, `super::provider`, `super::body`, `super::model`, `super::headers`, `super::builder`
- **被依赖**: External crates via `mod.rs` re-exports

### 更新后的 `mod.rs` (~55 行)

- **职责**: Module declarations, re-exports, utility functions (`is_env_truthy`, `is_codex_backend`), and `stream.rs` re-export.
- **内容**:
  ```rust
  mod types;
  mod headers;
  mod body;
  mod model;
  mod provider;
  mod builder;
  mod messages;
  pub mod stream;
  #[cfg(test)]
  mod tests;

  pub use types::*;
  pub use headers::*;
  pub use body::*;
  pub use model::*;
  pub use provider::*;
  pub use builder::*;
  pub use messages::*;
  pub(crate) use stream::parse_sse_byte_stream;

  pub fn is_env_truthy(var: &str) -> bool { ... }
  pub fn is_codex_backend() -> bool { ... }
  ```

## 拆分后目录结构

```
client/
├── mod.rs        (~55 行) — 模块声明 + 重导出 + 工具函数
├── types.rs      (~250 行) — 类型定义、枚举、常量
├── headers.rs    (~128 行) — HTTP 头构建
├── body.rs       (~98 行) — JSON body 操作
├── model.rs      (~165 行) — 模型解析与请求准备
├── provider.rs   (~111 行) — Provider URL 与能力查询
├── builder.rs    (~490 行) — ApiClient 工厂方法
├── messages.rs   (~390 行) — ApiClient 运行时方法
├── stream.rs     (126 行, 已存在)
└── tests.rs      (2655 行, 已存在)
```

## 依赖图（无循环）

```
types.rs          <-- standalone
headers.rs        <-- types.rs
body.rs           <-- types.rs
model.rs          <-- types.rs
provider.rs       <-- types.rs
builder.rs        <-- types.rs, provider.rs, body.rs, model.rs, headers.rs
messages.rs       <-- types.rs, provider.rs, body.rs, model.rs, headers.rs, builder.rs
```

## 迁移步骤

### Step 1: Create `types.rs`
- Move all type definitions and constants.
- Update `mod.rs` to `mod types; pub use types::*;`
- Verify `cargo check -p cc-api` compiles.

### Step 2: Create `headers.rs`
- Move header building functions.
- Update imports, add `pub(super)` to private functions needed by siblings.
- Verify `cargo check -p cc-api`.

### Step 3: Create `body.rs`
- Move body manipulation functions.
- Verify `cargo check -p cc-api`.

### Step 4: Create `model.rs`
- Move model resolution functions.
- Verify `cargo check -p cc-api`.

### Step 5: Create `provider.rs`
- Move `impl ApiProvider` block and URL helpers.
- Verify `cargo check -p cc-api`.

### Step 6: Create `builder.rs`
- Move factory/construction methods.
- Verify `cargo check -p cc-api`.

### Step 7: Create `messages.rs`
- Move operational methods.
- Verify `cargo check -p cc-api`.

### Step 8: Final verification
- Verify `cargo build --workspace --release` passes.
- Verify `cargo test -p cc-api` passes.
- Confirm all external consumers still compile.

## 风险与注意事项

1. **`builder.rs` at ~490 lines** exceeds the 400-line soft target. Splitting factory methods across two files would reduce cohesion since `from_env_result` calls `from_bedrock_env_result` and `from_vertex_env_result`. Acceptable trade-off.

2. **Private function visibility**: Functions like `build_anthropic_headers_with_cache_betas`, `body_contains_key`, `resolve_anthropic_model_alias`, `make_stream_provider`, `validate_provider_config` are currently private. When moved to sub-modules, they need `pub(super)` visibility.

3. **`Once` static**: `ANTHROPIC_LEGACY_MODEL_ENV_WARNING` moves to `model.rs` since it's only used by `warn_legacy_anthropic_model_env_once`.

4. **Test module imports**: `tests.rs` uses `super::*`, so it continues to work as long as `mod.rs` re-exports everything.

5. **`stream.rs` re-export**: The existing `pub(crate) use stream::parse_sse_byte_stream` must remain.

6. **`impl ApiProvider` methods**: These are inherent methods on the enum. They could stay in `types.rs` with the enum definition, or go to `provider.rs`. Putting them in `provider.rs` keeps `types.rs` as pure data.
