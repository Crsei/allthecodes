# Code-Split Plan: `loop_tests.rs` (2633 lines)

## 1. Current State

**File:** `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-query/src/loop_tests.rs`

**Current declaration** (at bottom of `loop_impl.rs`, lines 781-783):
```rust
#[cfg(test)]
#[path = "loop_tests.rs"]
mod loop_tests;
```

**Current module tree:**
```
crates/allthecodes-query/src/
  lib.rs                         <-- pub mod declarations for 5 modules
  deps.rs                        <-- QueryDeps trait + data types (141 lines)
  loop_helpers.rs                <-- helpers for loop_impl (891 lines)
  loop_impl.rs                   <-- core query() stream! macro (784 lines, includes loop_tests)
  stop_hooks.rs                  <-- stop hook logic + inline tests (132 lines)
  token_budget.rs                <-- budget tracking logic (67 lines, no tests)
  turn_context.rs                <-- request preparation helpers (197 lines)
  loop_tests.rs                  <-- 31 tests, all mocks and helpers (2633 lines)  <-- THIS FILE
```

**Virtual module path:** `crate::loop_tests` (because `#[path]` inside `loop_impl.rs` means it is a child of `loop_impl`, so true path is `crate::loop_impl::loop_tests`)

In practice, imported as:
```rust
use super::*;  // refers to loop_impl's parent scope
```

---

## 2. Content Inventory

### 2A. Shared Infrastructure (used by multiple test groups)

| Item | Kind | Lines | Used By |
|---|---|---|---|
| `MockStreamStep` | enum | 26-31 | All test groups |
| `MockDeps` | struct | 34-58 | All test groups |
| `MockDeps::new()` | impl | 60-68 | All test groups |
| `MockDeps::from_steps()` | impl | 70-98 | All test groups |
| `MockDeps` builder methods (`.with_*`) | impl | 100-128 | Most test groups |
| `MockDeps::recorded_params()` | impl | 130-136 | Most test groups |
| `MockDeps` setter methods | impl | 138-162 | Several test groups |
| `impl QueryDeps for MockDeps` | trait impl | 164-334 | All test groups |
| `make_user_message_for_test()` | fn | 336-346 | All test groups |
| `make_query_params()` | fn | 348-362 | All test groups |
| `LoopTestTool` | struct + Tool impl | 364-405 | All test groups |
| `ObservableInputTool` | struct + Tool impl | 407-457 | Backfill tests |
| `make_auto_compact_tracking()` | fn | 459-466 | Compact tests |
| `make_text_response()` | fn | 468-498 | All test groups |
| `make_text_response_with_stop_and_output_tokens()` | fn | 472-498 | Max-tokens tests |
| `request_start_count()` | fn | 500-505 | Several test groups |
| `has_api_error_containing()` | fn | 507-519 | Fallback tests |
| `StopContinuationHookRunner` | struct + HookRunner impl | 794-872 | Stop-hook tests |
| `ImageMockDeps` | struct + QueryDeps impl | 2077-2203 | Image tests |
| `CuMockDeps` | struct + QueryDeps impl | 2336-2471 | Computer Use tests |
| `tool_use_summary_gate_case()` | async fn | 1532-1562 | Summary tests |
| `run_observable_input_backfill_case()` | async fn | 1595-1632 | Backfill tests |
| `yielded_tool_input()` | fn | 1634-1649 | Backfill tests |
| `request_tool_input()` | fn | 1651-1667 | Backfill tests |

### 2B. Tests By Feature Group

| Group | Tests | Lines | Mock Deps Used |
|---|---|---|---|
| A. Tool discovery/setup | `query_refreshes_tools_before_first_model_call`, `query_shapes_autocompact_with_final_request_context`, `deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery`, `text_only_model_filters_view_image_from_request_tools`, `agent_session_filters_prompt_and_recursive_tools_from_request_tools`, `deferred_enabled_annotates_compact_boundaries_with_discovered_tools` (6 tests) | 521-792 | MockDeps |
| B. Simple text response | `test_simple_text_response_terminates` (1 test) | 874-914 | MockDeps |
| C. Prompt-too-long | `test_prompt_too_long_reactive_compact_retries_model_call`, `test_prompt_too_long_collapse_drain_retries_before_reactive_compact`, `test_prompt_too_long_terminals_after_collapse_and_reactive_fail` (3 tests) | 916-1031 | MockDeps |
| D. Fallback model | `test_fallback_model_retries_stream_start_capacity_error`, `test_fallback_strips_signature_blocks_from_retry_messages`, `test_fallback_tombstones_partial_assistant_after_stream_error`, `test_fallback_exhaustion_releases_terminal_stream_start_error` (4 tests) | 1034-1276 | MockDeps |
| E. Stream watchdog | `test_stream_idle_watchdog_errors_when_first_event_never_arrives`, `test_stream_stall_detection_errors_after_handshake_without_progress` (2 tests) | 1278-1342 | MockDeps |
| F. Max tokens | `test_max_tokens_recovery_escalates_next_request_limit` (1 test) | 1344-1379 | MockDeps |
| G. Stop hook | `test_stop_hook_continuation_injects_meta_user_message_once` (1 test) | 1381-1418 | MockDeps + StopContinuationHookRunner |
| H. Token budget | `test_token_budget_continuation_injects_nudge_message` (1 test) | 1420-1452 | MockDeps |
| I. Tool use | `test_tool_use_then_text_response`, `tool_use_summary_gate_defaults_off`, `tool_use_summary_gate_yields_summary_when_enabled` (3 tests) | 1454-1593 | MockDeps |
| J. Observable backfill | `observable_input_backfill_clones_yield_without_changing_next_request_gate_off`, `observable_input_backfill_matches_with_streaming_tool_gate_on` (2 tests) | 1669-1715 | MockDeps + ObservableInputTool |
| K. Streaming tool exec | `streaming_tool_execution_gate_starts_safe_tools_before_message_stop`, `streaming_tool_execution_aborts_started_tools_on_stream_fallback` (2 tests) | 1717-1924 | MockDeps |
| L. Max turns | `test_max_turns_limit` (1 test) | 1926-1983 | MockDeps |
| M. Hook stopped | `test_hook_stopped_tool_execution_yields_attachment_and_stops` (1 test) | 1985-2034 | MockDeps |
| N. Abort | `test_abort_before_api_call` (1 test) | 2036-2075 | MockDeps |
| O. Image tool result | `test_image_tool_result_flows_as_blocks` (1 test) | 2205-2330 | ImageMockDeps |
| P. Computer Use smoke | `test_computer_use_screenshot_click_round_trip` (1 test) | 2477-2632 | CuMockDeps |

---

## 3. New Module Structure

### 3A. Directory Layout

```
crates/allthecodes-query/src/
  lib.rs                                   <-- unchanged
  deps.rs                                  <-- unchanged
  loop_helpers.rs                          <-- unchanged
  loop_impl.rs                             <-- change #[path] to point to loop_tests/mod.rs
  stop_hooks.rs                            <-- unchanged
  token_budget.rs                          <-- unchanged
  turn_context.rs                          <-- unchanged

  loop_tests/                              <-- NEW directory (replaces loop_tests.rs)
    mod.rs                                 <-- module declarations, re-exports shared items
    mocks.rs                               <-- ALL shared mocks, helpers, custom deps
    tool_setup_tests.rs                    <-- Group A: tool discovery/setup, deferred, filters
    recovery_tests.rs                      <-- Groups C+D+F: prompt_too_long, fallback, max_tokens
    continuation_tests.rs                  <-- Groups G+H+L+M: stop hooks, budget, max turns
    tool_execution_tests.rs                <-- Groups I+J+K: tool use, backfill, streaming
    misc_tests.rs                          <-- Groups B+E+N+O+P: text response, watchdog, abort, image
```

### 3B. Detailed File Contents

---

#### File: `loop_tests/mod.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/mod.rs`

**Purpose:** Module root; declares sub-modules, re-exports shared test infrastructure.

**Contents:**

```rust
// Shared mocks and helpers
pub(crate) mod mocks;

// Test modules
mod tool_setup_tests;
mod recovery_tests;
mod continuation_tests;
mod tool_execution_tests;
mod misc_tests;

// Re-exports so test sub-modules can use `use super::*` as before
pub(crate) use mocks::*;
```

**Approximate line count:** 20

**Key details:**
- Sub-modules use `mod` (private), but `mocks` uses `pub(crate) mod` so it's visible to sibling modules.
- Re-exports all mocks/helpers so each test file can `use super::*` and get everything they currently import from `use super::*` (which today refers to `loop_impl`'s parent, i.e., `crate`-level).

---

#### File: `loop_tests/mocks.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/mocks.rs`

**Purpose:** Single home for ALL shared mocks, helper functions, and custom `MockDeps` implementations used by tests.

**Contains:**

| Name | Kind | Notes |
|---|---|---|
| `MockStreamStep` | enum | Transferred as-is (lines 26-31) |
| `MockDeps` | struct | Transferred as-is (lines 34-58) |
| `MockDeps` impl block 1 | impl | `new()`, `from_steps()`, builder methods (lines 60-128) |
| `MockDeps` impl block 2 | impl | `recorded_params()`, `recorded_autocompact_params()`, setters (lines 130-162) |
| `impl QueryDeps for MockDeps` | trait impl | The full ~170-line trait impl (lines 164-334) |
| `LoopTestTool` | struct + Tool impl | Transferred (lines 364-405) |
| `ObservableInputTool` | struct + Tool impl | Transferred (lines 407-457) |
| `StopContinuationHookRunner` | struct + HookRunner impl | Transferred (lines 794-872) |
| `ImageMockDeps` | struct + QueryDeps impl | Transferred (lines 2077-2203) |
| `CuMockDeps` | struct + QueryDeps impl | Transferred (lines 2336-2471) |
| `make_user_message_for_test()` | fn | Transferred (lines 336-346) |
| `make_query_params()` | fn | Transferred (lines 348-362) |
| `make_auto_compact_tracking()` | fn | Transferred (lines 459-466) |
| `make_text_response()` | fn | Transferred (lines 468-498) |
| `make_text_response_with_stop_and_output_tokens()` | fn | Transferred (lines 472-498) |
| `request_start_count()` | fn | Transferred (lines 500-505) |
| `has_api_error_containing()` | fn | Transferred (lines 507-519) |
| `tool_use_summary_gate_case()` | async fn | Transferred (lines 1532-1562) |
| `run_observable_input_backfill_case()` | async fn | Transferred (lines 1595-1632) |
| `yielded_tool_input()` | fn | Transferred (lines 1634-1649) |
| `request_tool_input()` | fn | Transferred (lines 1651-1667) |

**Approximate line count:** 950

**Key dependencies (`use` statements needed):**
```rust
use super::super::*;  // or use super::* when re-exported
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use allthecodes_types::hooks::{HookEventConfig, HookOutput, HookRunner, HooksMap, PostToolHookResult, PreToolHookResult};
use anyhow::Result;
use futures::{StreamExt, Stream};
use serde_json::Value;
use crate::deps::{CompactionResult, ModelCallParams, ModelResponse, QueryDeps, ToolExecRequest, ToolExecResult};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::config::{QueryGates, QuerySource, TaskBudget};
use allthecodes_engine::types::message::{AssistantMessage, CompactMetadata, ContentBlock, ImageSource, MessageContent, StreamEvent, SystemMessage, SystemSubtype, ToolResultContent, Usage, UserMessage};
use allthecodes_engine::types::state::AutoCompactTracking;
use allthecodes_engine::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools};
```

---

#### File: `loop_tests/tool_setup_tests.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/tool_setup_tests.rs`

**Purpose:** Tests for tool discovery, refresh, deferred loading, model capability filtering, session gates.

**Contains (6 tests):**

| Test Name | Group | Original Lines |
|---|---|---|
| `query_refreshes_tools_before_first_model_call` | A | 521-543 |
| `query_shapes_autocompact_with_final_request_context` | A | 545-572 |
| `deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery` | A | 574-615 |
| `text_only_model_filters_view_image_from_request_tools` | A | 617-674 |
| `agent_session_filters_prompt_and_recursive_tools_from_request_tools` | A | 676-735 |
| `deferred_enabled_annotates_compact_boundaries_with_discovered_tools` | A | 737-792 |

**Approximate line count:** 310

**Key dependencies:**
```rust
use super::mocks::*;
use std::sync::Arc;
use allthecodes_engine::types::app_state::AppState;
// ... others as needed
```

---

#### File: `loop_tests/recovery_tests.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/recovery_tests.rs`

**Purpose:** All model-call recovery mechanisms: prompt-too-long, fallback, max-tokens escalation.

**Contains (8 tests):**

| Test Name | Group | Original Lines |
|---|---|---|
| `test_prompt_too_long_reactive_compact_retries_model_call` | C | 916-963 |
| `test_prompt_too_long_collapse_drain_retries_before_reactive_compact` | C | 965-1010 |
| `test_prompt_too_long_terminals_after_collapse_and_reactive_fail` | C | 1012-1032 |
| `test_fallback_model_retries_stream_start_capacity_error` | D | 1034-1077 |
| `test_fallback_strips_signature_blocks_from_retry_messages` | D | 1079-1173 |
| `test_fallback_tombstones_partial_assistant_after_stream_error` | D | 1175-1245 |
| `test_fallback_exhaustion_releases_terminal_stream_start_error` | D | 1247-1276 |
| `test_max_tokens_recovery_escalates_next_request_limit` | F | 1344-1379 |

**Approximate line count:** 520

**Key dependencies:**
```rust
use super::mocks::*;
use crate::loop_helpers::ESCALATED_MAX_TOKENS;
// ...others via mocks re-exports
```

---

#### File: `loop_tests/continuation_tests.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/continuation_tests.rs`

**Purpose:** Tests for continuation/stop decisions: stop hooks, token budget, hook-stopped tool execution, max turns.

**Contains (4 tests):**

| Test Name | Group | Original Lines |
|---|---|---|
| `test_stop_hook_continuation_injects_meta_user_message_once` | G | 1381-1418 |
| `test_token_budget_continuation_injects_nudge_message` | H | 1420-1452 |
| `test_hook_stopped_tool_execution_yields_attachment_and_stops` | M | 1985-2034 |
| `test_max_turns_limit` | L | 1926-1983 |

**Approximate line count:** 370

**Note:** The `StopContinuationHookRunner` lives in `mocks.rs`, so this file only contains the test functions.

---

#### File: `loop_tests/tool_execution_tests.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/tool_execution_tests.rs`

**Purpose:** Tests for tool-use execution path: basic tool use, summaries, observable backfill, streaming execution.

**Contains (7 tests):**

| Test Name | Group | Original Lines |
|---|---|---|
| `test_tool_use_then_text_response` | I | 1454-1530 |
| `tool_use_summary_gate_defaults_off` | I | 1564-1574 |
| `tool_use_summary_gate_yields_summary_when_enabled` | I | 1576-1593 |
| `observable_input_backfill_clones_yield_without_changing_next_request_gate_off` | J | 1669-1691 |
| `observable_input_backfill_matches_with_streaming_tool_gate_on` | J | 1693-1715 |
| `streaming_tool_execution_gate_starts_safe_tools_before_message_stop` | K | 1717-1839 |
| `streaming_tool_execution_aborts_started_tools_on_stream_fallback` | K | 1841-1924 |

**Approximate line count:** 500

---

#### File: `loop_tests/misc_tests.rs`

**Path:** `crates/allthecodes-query/src/loop_tests/misc_tests.rs`

**Purpose:** Catch-all for tests that don't fit other categories: basic flow, stream watchdog, abort, image/computer-use.

**Contains (5 tests):**

| Test Name | Group | Original Lines |
|---|---|---|
| `test_simple_text_response_terminates` | B | 874-914 |
| `test_stream_idle_watchdog_errors_when_first_event_never_arrives` | E | 1278-1301 |
| `test_stream_stall_detection_errors_after_handshake_without_progress` | E | 1303-1342 |
| `test_abort_before_api_call` | N | 2036-2075 |
| `test_image_tool_result_flows_as_blocks` | O | 2205-2330 |
| `test_computer_use_screenshot_click_round_trip` | P | 2477-2632 |

**Approximate line count:** 400

---

### 3C. Old vs New Module Tree

**Before (flat + inline):**
```
src/
  lib.rs
  deps.rs
  loop_helpers.rs
  loop_impl.rs
    └── #[path = "loop_tests.rs"] mod loop_tests   <-- 2633 lines
  stop_hooks.rs
    └── mod tests { ... }                           <-- inline
  token_budget.rs
  turn_context.rs
```

**After (directory-based):**
```
src/
  lib.rs
  deps.rs
  loop_helpers.rs
  loop_impl.rs
    └── #[path = "loop_tests/mod.rs"] mod loop_tests
  stop_hooks.rs
    └── mod tests { ... }
  token_budget.rs
  turn_context.rs
  loop_tests/
    mod.rs                      <-- 20 lines: declarations + re-exports
    mocks.rs                    <-- ~950 lines: all shared mocks + helpers
    tool_setup_tests.rs         <-- ~310 lines: 6 tool config tests
    recovery_tests.rs           <-- ~520 lines: 8 recovery tests
    continuation_tests.rs       <-- ~370 lines: 4 continuation tests
    tool_execution_tests.rs     <-- ~500 lines: 7 tool execution tests
    misc_tests.rs               <-- ~400 lines: 6 misc tests
```

---

## 4. Impact on `use` Imports

### 4A. Current state (inside `loop_tests.rs`)

The current file starts with:
```rust
use super::*;  // pulls in everything from loop_impl's parent scope (i.e., crate root)
use std::pin::Pin;
use std::sync::atomic::*;
// ... etc
```

The `use super::*` gives access to:
- `crate::deps::*` (already public)
- `crate::loop_helpers::*` (might need to be `pub(crate)`)
- `crate::stop_hooks::*`
- `crate::token_budget::*`
- `crate::turn_context::*`
- Re-exports from `crate::loop_impl::query`

### 4B. New state (inside sub-files of `loop_tests/`)

Each test file will use:
```rust
use super::mocks::*;
```

This gives access to all re-exported mocks/helpers via `mod.rs`'s `pub(crate) use mocks::*`.

For crate-level items needed by specific tests:
- `crate::loop_helpers::ESCALATED_MAX_TOKENS` -- needed in `recovery_tests.rs`
- `crate::loop_helpers::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT` -- potentially needed
- `allthecodes_tools::deferred_tools` -- needed in `tool_setup_tests.rs`
- `allthecodes_tools::media` -- needed in `tool_setup_tests.rs`
- `allthecodes_tools::registry` -- needed in `tool_setup_tests.rs`

These can either be imported directly, or pulled through `super::*` (which requires they be re-exported from `mod.rs`). Since `use super::*` from inside a submodule of `loop_tests` reaches `loop_impl`'s parent scope (the crate root), it still works. The `mod.rs` re-exports cover the shared mocks.

**Clarification on scope:** Currently `loop_tests.rs` is `#[path = "loop_tests.rs"] mod loop_tests` inside `loop_impl.rs`. The `use super::*` in the test file refers to `loop_impl`'s parent, which is the crate root.

After the split, `loop_tests/mod.rs` will be at the same path, with `#[path = "loop_tests/mod.rs"]`. `use super::*` in `mod.rs` will still refer to the crate root. Sub-modules (e.g., `tool_setup_tests.rs`) use `use super::mocks::*` where `super` refers to `loop_tests/mod.rs` level, which has re-exported the mocks. For crate-root items they can do `use super::super::*` or directly use the full path `crate::loop_helpers::...`.

**Better approach:** Have `mod.rs` re-export the modules and mocks so test sub-files can do:
```rust
use super::*;  // gets mocks + crate-level items via re-export
```

Actually, `super::*` in a file like `loop_tests/tool_setup_tests.rs` refers to `loop_tests/mod.rs`'s scope. If `mod.rs` has `pub(crate) use mocks::*` and `pub(crate) use crate::loop_helpers::ESCALATED_MAX_TOKENS;` then `use super::*` will see all of them.

But the cleanest pattern is: test sub-files explicitly import what they need from `crate::...` paths, and only use `super::*` for the shared mocks. Let me note this in the risk section.

---

## 5. Migration Order

### Step 1: Create the `loop_tests/` directory

```bash
mkdir crate/allthecodes-query/src/loop_tests
```

### Step 2: Create `loop_tests/mocks.rs` (copy+prune from loop_tests.rs)

- Copy into `mocks.rs`:
  - All mock types and helpers listed in section 3B.
  - All `use` imports except `use super::*` (which changes meaning).
  - **DO NOT** copy any test functions.
- Adjust `use super::*` to `use crate::{...}` or use appropriate import paths.
- Verify the file compiles: `cargo test -p allthecodes-query --test mocks` -- but actually they won't run because there are no `#[test]` functions.

### Step 3: Create `loop_tests/mod.rs`

```rust
pub(crate) mod mocks;
mod tool_setup_tests;
mod recovery_tests;
mod continuation_tests;
mod tool_execution_tests;
mod misc_tests;
```

### Step 4: Update `loop_impl.rs` to point to the new module

Change:
```rust
#[path = "loop_tests.rs"]
mod loop_tests;
```
To:
```rust
#[path = "loop_tests/mod.rs"]
mod loop_tests;
```

### Step 5: Remove `loop_tests.rs` entirely

Delete the old file. No code should be orphaned.

### Step 6: Create the 5 test sub-files

Create each file in the order below, copying the relevant test functions and their specific `use` imports.

**Recommended creation order** (no inter-dependency among test files):

1. `misc_tests.rs` -- simplest, fewest deps
2. `continuation_tests.rs` -- self-contained
3. `tool_setup_tests.rs` -- self-contained
4. `tool_execution_tests.rs` -- self-contained
5. `recovery_tests.rs` -- self-contained

### Step 7: Build and test

```bash
cargo test -p allthecodes-query 2>&1 | tee build.log
```

Fix any import errors. The likely issues:
- `use super::*` in mocks.rs needs to become use `crate::...` paths or use appropriate relative paths
- Visibility: some items from `loop_helpers` may need to become `pub(crate)` if they aren't already

### Step 8: Clean up

- Ensure `loop_tests.rs` is deleted (not left as dead file)
- Commit all changes

---

## 6. Risks and Mitigations

### Risk 1: `use super::*` semantics change

**Severity:** High
**Detail:** Currently `loop_tests.rs` uses `use super::*` which refers to the crate root (since the file is embedded in `loop_impl.rs` via `#[path]`). After the split, `use super::*` from inside `mocks.rs` or sub-files will refer to different scopes depending on file location.
**Mitigation:** Replace `use super::*` in `mocks.rs` with explicit `use crate::{deps, loop_helpers, ...}` imports. The test sub-files use `use super::mocks::*` which is unambiguous.

### Risk 2: Visibility of `loop_helpers` items

**Severity:** Medium
**Detail:** Some items in `loop_helpers` may be `pub(crate)`. Tests today access them via `use super::*` (crate-root scope). After the split, tests will need the same access. Since the test module is still `#[cfg(test)]` and declared inside the same crate, `pub(crate)` items remain accessible. No change needed unless some items are private.
**Mitigation:** Audit that `loop_helpers.rs` items used in tests are at least `pub(crate)`. Currently: `ESCALATED_MAX_TOKENS` (line 39), `MAX_OUTPUT_TOKENS_RECOVERY_LIMIT` (line 36), `classify_model_call_failure` (line 199), `strip_fallback_signature_blocks` (line 223), `StreamingToolExecutor` (line 55), etc. are already `pub(crate)`.

### Risk 3: `#[path]` attribute and module nesting depth

**Severity:** Low
**Detail:** The test module is currently a child of `loop_impl`. After the split, it will still be a child of `loop_impl` (same `#[path]`). The internal module tree depth increases by one, but this has no effect on compilation or runtime.
**Mitigation:** None needed.

### Risk 4: Merged test helper functions in wrong file

**Severity:** Low
**Detail:** `tool_use_summary_gate_case()` and `run_observable_input_backfill_case()` are shared helper functions used by specific tests. These go into `mocks.rs` (where all shared helpers live), not in the test files.
**Mitigation:** Document in `mocks.rs` with a comment: "// -- Test helper functions --"

### Risk 5: Divergent compilation issues

**Severity:** Low
**Detail:** With multiple files, incremental compilation may recompile more files when one changes. In practice, this is negligible compared to the maintainability gain.
**Mitigation:** None needed.

### Risk 6: Test sub-modules cannot see each other's items

**Severity:** Low
**Detail:** Today, all items in `loop_tests.rs` can reference each other because they share one module scope. After splitting, items in `tool_setup_tests.rs` cannot directly use items from `recovery_tests.rs` without importing. Since no tests share cross-group helper functions, this is not an issue.
**Mitigation:** None needed. All shared items are in `mocks.rs`.

### Risk 7: `StopContinuationHookRunner` used in two test files

**Severity:** Low
**Detail:** `StopContinuationHookRunner` is defined in `mocks.rs` and used in `continuation_tests.rs`. By placing it in `mocks.rs`, it's available everywhere.
**Mitigation:** Already handled by the plan.

---

## 7. Summary Statistics

| Metric | Before | After |
|---|---|---|
| Files in crate | 8 | 12 |
| Lines in `loop_tests.rs` | 2633 | 0 (deleted) |
| Lines in `loop_tests/` | - | ~3070 (includes deduplicated imports) |
| Largest single file | `loop_tests.rs` at 2633 lines | `mocks.rs` at ~950 lines |
| Tests per file range | 31 in one file | 4-8 per file |
| Shared mock file | None isolated | `mocks.rs` |

The primary benefit is that each test file now has a clear thematic focus, making it easier to:
1. Find and modify a specific test without scrolling through hundreds of unrelated lines
2. Understand what feature area each test covers
3. Run a subset of tests by filename pattern
4. Add new tests in the appropriate file without contributing to a monolithic file
5. Review PRs that touch only one area of query-loop behavior
