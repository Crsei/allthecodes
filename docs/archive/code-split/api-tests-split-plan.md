# Code-Split Plan: `tests.rs` (2807 lines, 78+ tests)

## 1. Current State

### Old Module Tree (single file)

```
crates/allthecodes-api/src/api/client/
  mod.rs                   -- module root; declares `#[cfg(test)] mod tests;`
  body.rs                  -- cache-control body manipulation
  builder.rs               -- ApiClient factory/construction methods
  headers.rs               -- Anthropic HTTP header construction
  messages.rs              -- ApiClient operational methods (streaming, URL, headers)
  model.rs                 -- model alias resolution, token-count body
  provider.rs              -- provider-specific URL helpers
  stream.rs                -- SSE byte-stream parser
  types.rs                 -- type definitions (ApiProvider, MessagesRequest, etc.)
  tests.rs <-- HERE       -- 2807 lines, 78+ tests + ~70 lines shared helpers
```

### Contents of `tests.rs` — 9 Logical Domains

| Section | Lines | Tests | Shared Test Support |
|---|---|---|---|
| Shared infrastructure | 1--231 | 0 | ENV_LOCK, TEST_KEYCHAIN, helpers, fixture JSON consts, keyring mock |
| URL building | 236--434 | 11 | — |
| Header building | 440--772 | 12 | — |
| Model alias | 776--919 | 5 | — |
| Settings/env auth interaction | 922--1283 | 6 (merged with from_provider_info + from_env/auth below) | — |
| from_provider_info | 1289--1330 | 3 | — |
| from_env / from_auth | 1335--1885 | 16 | — |
| SSE line parsing | 1891--2065 | 9 | STREAM_ERROR_EVENT_SSE |
| Streaming / retry | 2067--2323 | 5 + 4 test-provider structs + 2 helpers | FlakyStreamProvider, StaticStreamProvider, PartialThenErrorStreamProvider |
| MessagesRequest serialization | 2349--2807 | 12 | — |

## 2. Target Module Tree

```
crates/allthecodes-api/src/api/client/
  mod.rs                   -- unchanged; `#[cfg(test)] mod tests;` auto-detects directory
  body.rs
  builder.rs
  headers.rs
  messages.rs
  model.rs
  provider.rs
  stream.rs
  types.rs
  tests/
    mod.rs                 -- shared infrastructure (was lines 1--231 + re-exports for submodules)
    url_building.rs        -- URL building tests
    headers.rs             -- header building tests
    model_alias.rs         -- model alias resolution tests
    auth_env.rs            -- auth/env integration + from_provider_info + from_env/from_auth
    sse_parsing.rs         -- SSE line parsing tests
    streaming_retry.rs     -- streaming/retry tests + test-provider structs
    request_serialization.rs  -- MessagesRequest serialization + body manipulation
```

## 3. File-by-File Specification

### File 1: `crates/allthecodes-api/src/api/client/tests/mod.rs`

**Purpose:** Shared test infrastructure, re-exports parent module items, and declares submodules.

**Exact items:**
- `use super::*;` — brings all `pub` items from `client/mod.rs` into scope
- `pub use super::*;` — re-exports them so child submodules can access via `use super::*;`
- Imports for submodules: `use crate::api::providers::AnthropicEndpointKind;`, `use crate::api::streaming::StreamAccumulator;`, `use allthecodes_types::message::StreamEvent;`, `use anyhow::Result;`, `use futures::Stream;`, `use std::collections::HashMap;`, `use std::pin::Pin;`, `use std::sync::atomic::{AtomicUsize, Ordering};`, `use std::sync::{Arc, Mutex, OnceLock};`, `use std::time::Duration;`
- `static ENV_LOCK: std::sync::Mutex<()>` (line 13)
- `static TEST_KEYCHAIN: OnceLock<Mutex<HashMap<(String, String), Vec<u8>>>>` (line 14)
- `const ANTHROPIC_MODEL_ENV_KEYS: &[&str]` (lines 15--23)
- `fn anthropic_config()` (lines 25--36)
- `fn anthropic_config_custom_url()` (lines 38--49)
- `fn save_env()` (lines 51--55)
- `fn restore_env()` (lines 57--64)
- `fn clear_env()` (lines 66--70)
- `struct CwdGuard` (lines 72--86)
- `fn fixture_json()` (lines 88--97)
- `const AUTH_HEADER_EXPECTED` (lines 99--102)
- `const BASE_URL_EXPECTED` (lines 104--107)
- `const MODEL_ALIAS_EXPECTED` (lines 109--112)
- `const PROMPT_CACHE_BODY_EXPECTED` (lines 114--134)
- `const STREAM_ERROR_EVENT_SSE` (lines 136--139)
- `fn save_and_clear_provider_keys()` (lines 141--150)
- `fn restore_provider_keys()` (lines 152--156)
- `struct PersistentTestCredential` + impl `keyring::credential::CredentialApi` (lines 158--197)
- `struct PersistentTestCredentialBuilder` + impl `CredentialBuilderApi` (lines 199--221)
- `fn use_persistent_test_keyring()` (lines 223--230)
- Submodule declarations:
  - `mod url_building;`
  - `mod headers;`
  - `mod model_alias;`
  - `mod auth_env;`
  - `mod sse_parsing;`
  - `mod streaming_retry;`
  - `mod request_serialization;`

**Approximate line count:** 210--240 lines.

**Dependencies:** `allthecodes_types`, `anyhow`, `futures`, `keyring`, `serde_json`.

---

### File 2: `crates/allthecodes-api/src/api/client/tests/url_building.rs`

**Top of file:**
```rust
use super::*;
```

**Domain section header:** `// --- URL building ---`

**Exact tests:**
| Test function | Origin line |
|---|---|
| `test_build_url_anthropic` | 237 |
| `test_build_url_anthropic_custom_base` | 244 |
| `test_build_url_anthropic_trailing_slash` | 251 |
| `test_build_url_bedrock_returns_aws_endpoint` | 268 |
| `test_build_url_bedrock_with_override` | 297 |
| `test_build_url_vertex_returns_streamrawpredict` | 321 |
| `test_build_url_vertex_uses_per_model_region_override` | 341 |
| `test_build_url_azure` | 367 |
| `test_build_url_openai_compat` | 383 |
| `test_build_url_openai_compat_trailing_slash` | 401 |
| `test_build_url_openai_codex` | 419 |

**Approximate line count:** 195 lines.

---

### File 3: `crates/allthecodes-api/src/api/client/tests/headers.rs`

**Top of file:**
```rust
use super::*;
```

**Domain section header:** `// --- Header building ---`

**Exact tests:**
| Test function | Origin line |
|---|---|
| `test_build_headers_has_required` | 441 |
| `test_build_headers_raw_header_map_has_required` | 463 |
| `compatible_anthropic_headers_omit_beta_extensions` | 494 |
| `anthropic_headers_include_effort_beta_for_output_config_effort` | 520 |
| `test_build_headers_azure_has_api_key` | 542 |
| `test_build_headers_openai_compat_bearer` | 558 |
| `test_build_headers_google_no_auth_header` | 578 |
| `test_build_headers_bedrock_no_api_key` | 597 |
| `regression_anthropic_auth_token_uses_authorization_bearer` | 619 |
| `regression_anthropic_base_url_env_currently_loses_compatible_routing` | 655 |
| `anthropic_auth_token_with_non_official_base_url_selects_compatible_messages_endpoint` | 697 |
| `provider_diagnostic_includes_endpoint_kind_and_host_without_secret` | 748 |

**Approximate line count:** 330 lines.

---

### File 4: `crates/allthecodes-api/src/api/client/tests/model_alias.rs`

**Top of file:**
```rust
use super::*;
```

**Domain section header:** `// --- Model alias ---`

**Exact tests:**
| Test function | Origin line |
|---|---|
| `regression_anthropic_model_alias_currently_not_resolved_for_wire_model` | 775 |
| `anthropic_alias_uses_new_default_model_env_before_official_fallback` | 822 |
| `anthropic_alias_uses_legacy_default_model_env_as_fallback` | 853 |
| `anthropic_compatible_alias_without_provider_default_is_config_error` | 886 |
| `anthropic_compatible_explicit_model_id_wins_over_alias_defaults` | 923 |

**Approximate line count:** 145 lines.

---

### File 5: `crates/allthecodes-api/src/api/client/tests/auth_env.rs`

**Top of file:**
```rust
use super::*;
```

**Note:** This is the largest file (~700 lines). Sub-sections:

**Sub-section: Settings/env auth interaction (origin lines 956--1283)**
| Test function | Origin line |
|---|---|
| `settings_runtime_env_is_visible_to_anthropic_provider_detection` | 956 |
| `startup_settings_env_overrides_inherited_anthropic_provider_env` | 1004 |
| `settings_runtime_env_supports_anthropic_legacy_model_alias_fallback` | 1053 |
| `settings_runtime_env_is_visible_to_codex_backend_auth` | 1103 |
| `active_codex_profile_env_builds_codex_client` | 1152 |
| `active_custom_profile_env_builds_anthropic_compatible_client` | 1210 |

**Sub-section: from_provider_info (origin lines 1289--1330)**
| Test function | Origin line |
|---|---|
| `test_from_provider_info_anthropic` | 1290 |
| `test_from_provider_info_deepseek` | 1306 |
| `test_from_provider_info_google` | 1320 |

**Sub-section: from_env / from_auth (origin lines 1335--1885)**
| Test function | Origin line |
|---|---|
| `test_from_env_with_anthropic_key` | 1336 |
| `test_from_env_no_keys` | 1378 |
| `test_from_auth_with_env` | 1406 |
| `test_from_codex_auth_with_env` | 1423 |
| `test_from_env_prefers_bedrock_when_flag_set` | 1452 |
| `test_from_env_prefers_vertex_when_flag_set` | 1486 |
| `test_from_bedrock_env_returns_none_without_auth` | 1526 |
| `test_from_env_result_errors_for_explicit_bedrock_without_auth` | 1553 |
| `test_from_env_result_errors_for_explicit_foundry` | 1584 |
| `test_from_env_result_errors_for_explicit_vertex_without_project` | 1608 |
| `test_from_backend_codex_prefers_codex_auth` | 1641 |
| `test_from_auth_uses_openai_keychain_when_api_provider_is_openai` | 1660 |
| `test_from_auth_anthropic_provider_does_not_read_openai_keychain` | 1710 |
| `test_from_auth_settings_provider_takes_priority_over_other_env_key` | 1755 |
| `test_active_anthropic_profile_ignores_inherited_codex_token` | 1804 |

**Approximate line count:** 690 lines.

**Key dependencies:** `allthecodes_auth`, `allthecodes_config::settings`, `tempfile`. The `use_persistent_test_keyring()` helper, `CwdGuard`, and `save_and_clear_provider_keys()` come from `super::*`.

---

### File 6: `crates/allthecodes-api/src/api/client/tests/sse_parsing.rs`

**Top of file:**
```rust
use super::*;
```
Additional import: `use crate::api::streaming::NormalizedApiError;` (needed for `downcast_ref` in the last test).

**Domain section header:** `// --- SSE line parsing ---`

**Exact tests:**
| Test function | Origin line |
|---|---|
| `test_sse_line_parsing_message_start` | 1892 |
| `test_sse_line_parsing_content_block_start` | 1910 |
| `test_sse_line_parsing_multiple_events` | 1930 |
| `test_sse_line_parsing_ping_ignored` | 1963 |
| `test_sse_line_parsing_accumulator_integration` | 1979 |
| `test_sse_line_parsing_no_trailing_newline` | 2022 |
| `test_sse_line_parsing_empty_text` | 2034 |
| `regression_anthropic_stream_error_event_currently_ignored` | 2040 |
| `anthropic_stream_error_event_preserves_available_metadata` | 2049 |

**Approximate line count:** 175 lines.

---

### File 7: `crates/allthecodes-api/src/api/client/tests/streaming_retry.rs`

**Top of file:**
```rust
use super::*;
```
Additional imports:
- `use std::sync::atomic::{AtomicUsize, Ordering};` (from mod.rs already, but needed for `AtomicUsize`)
- `use std::sync::Arc;` (from mod.rs already)
- `use crate::api::streaming::NormalizedApiError;`
- `use crate::api::retry::RetryConfig;` (or gets through `super::*`)

**Exact items:**

**Test-support structs:**
| Struct | Origin lines |
|---|---|
| `FlakyStreamProvider` | 2067--2071 |
| `StaticStreamProvider` | 2073--2075 |
| `PartialThenErrorStreamProvider` | 2077 |

**StreamProvider impl blocks:**
| impl | Origin lines |
|---|---|
| `impl StreamProvider for StaticStreamProvider` | 2080--2090 |
| `impl StreamProvider for PartialThenErrorStreamProvider` | 2092--2129 |
| `impl StreamProvider for FlakyStreamProvider` | 2131--2145 |

**Helper functions:**
| Function | Origin lines |
|---|---|
| `fn minimal_stream_request()` | 2147--2168 |
| `fn retry_test_config()` | 2170--2178 |

**Tests:**
| Test function | Origin lines |
|---|---|
| `messages_stream_retries_retryable_stream_start_errors` | 2181 |
| `messages_stream_does_not_retry_nonretryable_stream_start_errors` | 2215 |
| `messages_collects_stream_events_into_assistant_message` | 2246 |
| `messages_propagates_partial_stream_error_instead_of_fake_success` | 2306 |
| `normalizes_anthropic_error_body_with_request_metadata` | 2326 |

**Approximate line count:** 255 lines.

---

### File 8: `crates/allthecodes-api/src/api/client/tests/request_serialization.rs`

**Top of file:**
```rust
use super::*;
```
Additional imports:
- `use crate::api::client::{provider_supports_advisor, ApiProvider};` (may be needed if not visible through `super::*`)

**Domain section header:** `// --- MessagesRequest serialization ---`

**Exact tests:**
| Test function | Origin lines |
|---|---|
| `test_messages_request_serialization` | 2350 |
| `test_messages_request_optional_fields_serialize_when_present` | 2391 |
| `regression_prompt_cache_marker_serializes_in_anthropic_body` | 2430 |
| `test_prompt_cache_policy_defaults_do_not_add_ttl_or_global` | 2463 |
| `test_compatible_anthropic_body_strips_cache_and_thinking_extensions` | 2494 |
| `test_prompt_cache_policy_adds_ttl_and_global_only_when_capable` | 2530 |
| `test_strip_anthropic_cache_fields_recursively` | 2572 |
| `test_messages_request_with_thinking` | 2594 |
| `test_anthropic_count_tokens_body_omits_generation_only_fields` | 2625 |
| `test_exact_token_count_support_matrix` | 2669 |
| `test_messages_request_advisor_model_serializes_when_set` | 2759 |
| `test_provider_supports_advisor_matrix` | 2786 |

**Approximate line count:** 455 lines.

## 4. Complete Module Tree Comparison

### Before (single file):

```
tests.rs (2807 lines)
├── Shared infrastructure (231 lines)
├── URL building (203 lines)
├── Header building (338 lines)
├── Model alias (146 lines)
├── Settings/env auth + from_provider_info + from_env/from_auth (920 lines)
├── SSE parsing (179 lines)
├── Streaming/retry (257 lines)
└── Request serialization (461 lines)
```

### After (directory module):

```
tests/
├── mod.rs                  (235 lines)  -- shared infrastructure, submodule decl
├── url_building.rs         (195 lines)  -- 11 URL building tests
├── headers.rs              (330 lines)  -- 12 header building tests
├── model_alias.rs          (145 lines)  -- 5 model alias tests
├── auth_env.rs             (690 lines)  -- 24 auth/env integration tests
├── sse_parsing.rs          (175 lines)  -- 9 SSE parsing tests
├── streaming_retry.rs      (255 lines)  -- 5 streaming/retry tests + support structs
└── request_serialization.rs (455 lines) -- 12 serialization + body tests
```

Each file is under 700 lines. Largest file (`auth_env.rs`) at ~690 lines is still manageable; could be further split later into `auth_env.rs` and `from_env.rs` if desired.

## 5. Risks and Mitigations

### Risk 1: Shared mutable state (`ENV_LOCK`, `TEST_KEYCHAIN`)

All test submodules need access to `ENV_LOCK` (mutex for env isolation) and `TEST_KEYCHAIN` (in-memory keyring). These are declared as `static` in `tests/mod.rs`.

**Mitigation:** Make them `pub(crate)` (or just `pub`) in `tests/mod.rs`. Submodule files access them via `super::ENV_LOCK` and `super::TEST_KEYCHAIN`, or through `use super::*;` since they're public.

### Risk 2: Cargo test parallelism

`ENV_LOCK` is a `Mutex<()>` that all env-dependent tests acquire. This works across submodules as long as they all use the same `ENV_LOCK`. Since all tests reference `super::ENV_LOCK`, this is preserved. No risk.

### Risk 3: `parse_sse_text` visibility

`parse_sse_text` is declared `#[cfg(test)] pub fn` in `stream.rs` and imported into `client/mod.rs` via `#[cfg(test)] use stream::parse_sse_text;`. After the split, submodules need access.

**Mitigation:** `tests/mod.rs` has `pub use super::parse_sse_text;` or `use super::*;` covers it since `parse_sse_text` is brought into `client`'s namespace. With `pub use super::*;` in `tests/mod.rs`, it's re-exported to children.

### Risk 4: Circular dependencies

None. Test modules are leaf nodes — they depend on `super::*` (the `client` module) but nothing depends on tests. No circularity.

### Risk 5: `use super::*` chains

`tests/foo.rs` uses `use super::*;` which pulls items from `tests/mod.rs`. `tests/mod.rs` uses `use super::*;` which pulls items from `client/mod.rs`. However, `use` items are **not transitive** — a `use` in `tests/mod.rs` does NOT automatically make those names visible in `tests/foo.rs` via the child's `use super::*;`.

**Mitigation:** `tests/mod.rs` must use `pub use super::*;` (with `pub`) to re-export parent items into the module's public namespace, so that children can see them via `use super::*;`. All shared test items in `tests/mod.rs` must also be `pub`.

Alternative (simpler): Each submodule can directly import what it needs:
```rust
use crate::api::client::*;
use crate::api::providers::AnthropicEndpointKind;
```
This avoids the visibility chain issue entirely. But this duplicates imports across 7 files.

**Recommendation:** Use `pub use super::*;` in `tests/mod.rs` and `use super::*;` in each submodule. Also declare shared items as `pub`.

### Risk 6: `provider_supports_advisor` is `pub fn` in `model.rs`

In the current `tests.rs`, `use crate::api::client::{provider_supports_advisor, ApiProvider};` is used directly. With the submodule structure, `super::*` brings it in via `client/mod.rs`'s `pub use model::*;`. No issue.

### Risk 7: Macro / attribute hygiene

Attributes like `#[tokio::test]` and `#[test]` are unaffected by file splits. No risk.

## 6. Step-by-Step Migration Order

### Phase 1: Create directory and move

```
Step 1: mkdir -p crates/allthecodes-api/src/api/client/tests/
Step 2: git mv crates/allthecodes-api/src/api/client/tests.rs \
            crates/allthecodes-api/src/api/client/tests/mod.rs
```

### Phase 2: Restructure `tests/mod.rs`

```
Step 3: In tests/mod.rs:
   a. After `use super::*;`, add `pub use super::*;` (re-export parent items)
   b. Keep all shared infrastructure (lines 1--231)
   c. Remove all test functions (lines 232--2807)
   d. Add submodule declarations:
      mod url_building;
      mod headers;
      mod model_alias;
      mod auth_env;
      mod sse_parsing;
      mod streaming_retry;
      mod request_serialization;
   e. Mark shared items as `pub` where needed by submodules:
      - ENV_LOCK -> pub
      - TEST_KEYCHAIN -> pub
      - ANTHROPIC_MODEL_ENV_KEYS -> pub
      - anthropic_config() -> pub
      - anthropic_config_custom_url() -> pub
      - save_env() -> pub
      - restore_env() -> pub
      - clear_env() -> pub
      - CwdGuard -> pub (struct and impl)
      - fixture_json() -> pub
      - AUTH_HEADER_EXPECTED -> pub
      - BASE_URL_EXPECTED -> pub
      - MODEL_ALIAS_EXPECTED -> pub
      - PROMPT_CACHE_BODY_EXPECTED -> pub
      - STREAM_ERROR_EVENT_SSE -> pub
      - save_and_clear_provider_keys() -> pub
      - restore_provider_keys() -> pub
      - PersistentTestCredential -> pub (struct + impl)
      - PersistentTestCredentialBuilder -> pub (struct + impl)
      - use_persistent_test_keyring() -> pub
```

### Phase 3: Create submodule files

Create each file in order. Any order works since they are independent.

```
Step 4: Create tests/url_building.rs   -- copy URL-building tests from mod.rs
Step 5: Create tests/headers.rs        -- copy header tests from mod.rs
Step 6: Create tests/model_alias.rs    -- copy model alias tests from mod.rs
Step 7: Create tests/auth_env.rs       -- copy auth/env tests from mod.rs
Step 8: Create tests/sse_parsing.rs    -- copy SSE parsing tests from mod.rs
Step 9: Create tests/streaming_retry.rs -- copy streaming/retry tests + support from mod.rs
Step 10: Create tests/request_serialization.rs -- copy serialization tests from mod.rs
```

Each submodule file starts with:
```rust
use super::*;
```
No additional imports are needed unless noted in Section 3 above.

### Phase 4: Verify

```
Step 11: cargo check --tests -p allthecodes-api
Step 12: cargo test -p allthecodes-api -- api::client
```

### Phase 5: Clean up (only after green tests)

No additional cleanup needed. The `tests.rs` file is already `git mv`'d to `tests/mod.rs` and then trimmed down. No changes to `client/mod.rs`.

## 7. Future Improvements

1. **Further split `auth_env.rs`** if it grows beyond ~700 lines: split `from_env/from_auth` tests (lines 1335--1885) into a separate `from_env.rs`.
2. **Move `request_serialization.rs` body-manipulation tests** into a `body_tests.rs` if more body tests are added.
3. **Consider deduplicating** the `save_env` / `restore_env` patterns across auth tests — they use the same boilerplate with different env-var lists. A higher-level test-scope fixture could reduce repetition, but that is a cleanup beyond this split.
