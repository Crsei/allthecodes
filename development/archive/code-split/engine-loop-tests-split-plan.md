# Engine Query Loop Tests -- Code-Split Plan

## 1. Problem Statement

`crates/allthecodes-engine/src/query/loop_tests.rs` is a monolithic 2694-line test file
containing 33 tests, 7 mock types/struct implementations (4 implementing `QueryDeps`,
2 implementing `Tool`, 1 implementing `HookRunner`), and 11 shared helper functions.

It is currently included via a `#[path]` attribute in `loop_impl.rs`:

```rust
// loop_impl.rs, line 886
#[cfg(test)]
#[path = "loop_tests.rs"]
mod loop_tests;
```

This inlining means all tests share a single module scope (`use super::*`), making it
hard to navigate, maintain, or parallelize.

## 2. Target Module Tree

### Old structure

```
crates/allthecodes-engine/src/query/
  mod.rs                      -- 7 pub mod declarations
  deps.rs                     -- QueryDeps trait + data types
  loop_helpers.rs             -- Helper functions/structs for the query loop
  loop_impl.rs                -- Core query() function + #[path]-included tests
  loop_tests.rs               -- 2694 lines (EVERYTHING: mocks + 33 tests)
  stop_hooks.rs               -- Stop hook runner
  token_budget.rs             -- Token budget decision logic
  turn_context.rs             -- QueryRunContext + prepare_model_request
```

### New structure

```
crates/allthecodes-engine/src/query/
  mod.rs                      -- UNCHANGED
  deps.rs                     -- UNCHANGED
  loop_helpers.rs             -- UNCHANGED
  loop_impl.rs                -- CHANGED: remove #[path], add #[cfg(test)] mod tests;
  stop_hooks.rs               -- UNCHANGED
  token_budget.rs             -- UNCHANGED
  turn_context.rs             -- UNCHANGED
  tests/                      -- NEW directory
    mod.rs                    -- Top-level test harness: re-exports all sub-modules
    mocks.rs                  -- ALL mock types, trait impls, and shared helpers
    setup_tests.rs            -- Tests 1-6: tool filtering, refresh, steer
    flow_tests.rs             -- Tests 7, 21-27, 33: basic flow, tool results, snip
    recovery_tests.rs         -- Tests 8-18: prompt_too_long, fallback, stream, max_tokens
    control_tests.rs          -- Tests 19-20, 28-30: hooks, budget, limits, abort
    media_tests.rs            -- Tests 31-32: image tool results, computer-use smoke
```

**Total: 7 new/4 modified = 11 files in play (6 existing unchanged).**

## 3. File-by-File Inventory

### 3.1 `crates/allthecodes-engine/src/query/tests/mod.rs` (NEW, ~30 lines)

```rust
// Re-exports all sub-modules under #[cfg(test)].
// The `mod tests;` declaration lives in loop_impl.rs.
pub mod mocks;
pub mod setup_tests;
pub mod flow_tests;
pub mod recovery_tests;
pub mod control_tests;
pub mod media_tests;
```

Each sub-module is declared `pub` so that sibling test modules can reference each
other's items if needed (though in practice `mocks` is the only cross-file dependency).

### 3.2 `crates/allthecodes-engine/src/query/tests/mocks.rs` (NEW, ~550 lines)

| Item | Kind | Lines in original | Notes |
|---|---|---|---|
| `MockStreamStep` | enum | 28-33 | 3 variants |
| `MockDeps` | struct | 36-59 | 16 fields of atomic/mutex state |
| `MockDeps::new()` | fn | 62-69 | Creates from `Vec<ModelResponse>` |
| `MockDeps::from_steps()` | fn | 71-98 | Creates from `Vec<MockStreamStep>` |
| `MockDeps::with_tools()` | fn | 100-103 | Builder |
| `MockDeps::with_refreshed_tools()` | fn | 105-108 | Builder |
| `MockDeps::with_tool_delay()` | fn | 110-113 | Builder |
| `MockDeps::with_app_state()` | fn | 115-118 | Builder |
| `MockDeps::recorded_params()` | fn | 120-122 | Accessor |
| `MockDeps::recorded_autocompact_params()` | fn | 124-126 | Accessor |
| `MockDeps::set_reactive_compact_result()` | fn | 128-130 | Mutator |
| `MockDeps::set_collapse_drain_result()` | fn | 132-134 | Mutator |
| `MockDeps::set_hook_runner()` | fn | 136-138 | Mutator |
| `MockDeps::set_steer_drains()` | fn | 140-142 | Mutator |
| `MockDeps::stop_after_tool_execution()` | fn | 144-147 | Mutator |
| `MockDeps::pop_stream_step()` | fn | 149-156 | Internal helper |
| `impl QueryDeps for MockDeps` | impl | 158-328 | ~170 lines (call_model, streaming, compact, execute_tool, etc.) |
| `LoopTestTool` | struct | 358-361 | Simple tool for testing |
| `impl Tool for LoopTestTool` | impl | 368-399 | Minimal stub |
| `ObservableInputTool` | struct | 363-366 | Tool with backfill_observable_input |
| `impl Tool for ObservableInputTool` | impl | 401-451 | Includes backfill logic |
| `StopContinuationHookRunner` | struct | 764-767 | Hook runner that stops continuation |
| `impl HookRunner for StopContinuationHookRunner` | impl | 778-842 | Delegates to Stop event hooks |
| `ImageMockDeps` | struct | 2099-2102 | Custom mock for image tests |
| `impl QueryDeps for ImageMockDeps` | impl | 2113-2224 | ~111 lines, returns image blocks in tool result |
| `CuMockDeps` | struct | 2362-2366 | Custom mock for computer-use tests |
| `impl QueryDeps for CuMockDeps` | impl | 2376-2494 | ~118 lines, dispatches by tool name |
| `make_user_message_for_test()` | fn | 330-340 | Creates a UserMessage |
| `make_query_params()` | fn | 342-357 | Creates QueryParams for tests |
| `make_auto_compact_tracking()` | fn | 453-460 | Creates AutoCompactTracking |
| `make_text_response()` | fn | 462-464 | Shortcut for text model response |
| `make_text_response_with_stop_and_output_tokens()` | fn | 466-500 | Configurable model response |
| `request_start_count()` | fn | 502-507 | Helper to count RequestStart yields |
| `has_api_error_containing()` | fn | 509-521 | Helper to find API errors |
| `tool_use_summary_gate_case()` | async fn | 1545-1577 | Parametrized helper for summary tests |
| `run_observable_input_backfill_case()` | async fn | 1610-1649 | Parametrized helper for backfill tests |
| `yielded_tool_input()` | fn | 1651-1666 | Extract tool input from yielded items |
| `request_tool_input()` | fn | 1668-1684 | Extract tool input from recorded params |

**Use statements needed** (explicit, not relying on `super::super::*`):

```rust
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use allthecodes_types::hooks::{HookEventConfig, HookOutput, HookRunner, HooksMap, PostToolHookResult, PreToolHookResult};
use anyhow::Result;
use futures::{Stream, StreamExt};
use serde_json::Value;
use super::super::deps::{CompactionResult, ModelCallParams, ModelResponse, QueryDeps, ToolExecRequest, ToolExecResult};
use crate::types::app_state::AppState;
use crate::types::config::{QueryGates, QuerySource, TaskBudget};
use crate::types::message::{AssistantMessage, ContentBlock, ImageSource, Message, MessageContent, StreamEvent, ToolResultContent, Usage, UserMessage};
use crate::types::state::AutoCompactTracking;
use crate::types::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, Tools};
```

All items defined in this file must be `pub` (or `pub(crate)`) so that sibling
test files can import them.

### 3.3 `crates/allthecodes-engine/src/query/tests/setup_tests.rs` (NEW, ~250 lines)

Contains 6 tests focused on the pre-request setup phase (tool filtering, refresh,
autocompact, steer drain).

| Test | Lines | What it verifies |
|---|---|---|
| `query_refreshes_tools_before_first_model_call` | 524-545 | Tools are refreshed before the first API call |
| `query_shapes_autocompact_with_final_request_context` | 548-574 | Autocompact receives correct system prompt/tools/output tokens |
| `deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery` | 578-611 | Deferred tool loading strips discovered tools from schema |
| `text_only_model_filters_view_image_from_request_tools` | 614-670 | Text-only model removes ViewImage from tools |
| `agent_session_filters_prompt_and_recursive_tools_from_request_tools` | 673-731 | Agent source gated-hides AskUserQuestion, Agent, Task, etc. |
| `query_drains_steer_before_next_model_request` | 734-762 | Steer messages are injected before second model call |

**Use statements:**

```rust
use std::sync::Arc;
use super::super::*;                                // query(), apply_snip_projection(), etc.
use super::mocks::{
    MockDeps, LoopTestTool, make_user_message_for_test, make_query_params,
    make_text_response, request_start_count,
};
use crate::types::config::{QueryGates, QuerySource};
use crate::types::message::{Message, MessageContent, QueryYield};
use crate::types::state::AutoCompactTracking;
```

### 3.4 `crates/allthecodes-engine/src/query/tests/flow_tests.rs` (NEW, ~350 lines)

Contains 11 tests covering basic query termination, tool call/response cycles,
tool use summary gates, observable input backfill, and the snip unit test.

| Test | Lines | What it verifies |
|---|---|---|
| `test_simple_text_response_terminates` | 845-884 | Basic one-turn text response |
| `test_tool_use_then_text_response` | 1466-1543 | Tool call followed by text (2 turns) |
| `test_abort_before_api_call` | 2058-2096 | Abort flag before streaming |
| `tool_use_summary_gate_defaults_off` | 1580-1589 | Summary not emitted when gated off |
| `tool_use_summary_gate_yields_summary_when_enabled` | 1592-1608 | Summary emitted when gated on |
| `observable_input_backfill_clones_yield_without_changing_next_request_gate_off` | 1687-1708 | Backfill visible in yield but not next request |
| `observable_input_backfill_matches_with_streaming_tool_gate_on` | 1711-1732 | Same behavior with streaming gate on |
| `streaming_tool_execution_gate_starts_safe_tools_before_message_stop` | 1735-1856 | Safe tools start before message_stop |
| `streaming_tool_execution_aborts_started_tools_on_stream_fallback` | 1859-1941 | Fallback aborts in-flight streaming tools |
| `snip_projection_removes_requested_messages_only` | 2662-2693 | Sync test for apply_snip_projection |

**Use statements:**

```rust
use std::sync::Arc;
use std::time::Duration;
use super::super::*;                                // query(), apply_snip_projection(), etc.
use super::mocks::{
    MockDeps, MockStreamStep, LoopTestTool, ObservableInputTool,
    make_user_message_for_test, make_query_params, make_text_response,
    make_text_response_with_stop_and_output_tokens, request_start_count,
    yielded_tool_input, request_tool_input,
    tool_use_summary_gate_case, run_observable_input_backfill_case,
};
use crate::types::config::{QueryGates, QuerySource};
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, QueryYield,
    StreamEvent, Usage, UserMessage,
};
use allthecodes_api::api::streaming::StreamAccumulator;
```

### 3.5 `crates/allthecodes-engine/src/query/tests/recovery_tests.rs` (NEW, ~500 lines)

Contains 11 tests covering all error recovery paths.

| Test | Lines | What it verifies |
|---|---|---|
| `test_prompt_too_long_reactive_compact_retries_model_call` | 887-933 | prompt_too_long -> collapse_drain -> reactive_compact -> retry |
| `test_prompt_too_long_collapse_drain_retries_before_reactive_compact` | 936-978 | Collapse drain restores context, reactive compact skipped |
| `test_prompt_too_long_terminals_after_collapse_and_reactive_fail` | 981-1000 | Both compact strategies fail -> API error |
| `test_fallback_model_retries_stream_start_capacity_error` | 1003-1045 | 529 at start -> retry on fallback model |
| `test_fallback_strips_signature_blocks_from_retry_messages` | 1048-1139 | Thinking/RedactedThinking blocks stripped for fallback |
| `test_fallback_tombstones_partial_assistant_after_stream_error` | 1142-1211 | Mid-stream 529 -> tombstone -> fallback |
| `test_chunk_read_error_after_text_accepts_partial_assistant` | 1214-1256 | Chunk read error with text -> accept partial |
| `test_fallback_exhaustion_releases_terminal_stream_start_error` | 1259-1287 | Both primary + fallback fail -> release error |
| `test_stream_idle_watchdog_errors_when_first_event_never_arrives` | 1290-1312 | Idle timeout before first event |
| `test_stream_stall_detection_errors_after_handshake_without_progress` | 1315-1353 | Stall timeout after handshake |
| `test_max_tokens_recovery_escalates_next_request_limit` | 1356-1390 | max_tokens stop -> escalate max_output_tokens -> retry |

**Use statements:**

```rust
use std::sync::Arc;
use std::time::Duration;
use super::super::*;                                // query(), apply_snip_projection(), etc.
use super::mocks::{
    MockDeps, MockStreamStep,
    make_user_message_for_test, make_query_params, make_text_response,
    make_text_response_with_stop_and_output_tokens, make_auto_compact_tracking,
    request_start_count, has_api_error_containing,
};
use super::super::super::deps::{CompactionResult, ModelCallParams};
use crate::types::message::{
    AssistantMessage, ContentBlock, Message, MessageContent, QueryYield,
    StreamEvent, Usage, UserMessage,
};
use crate::types::state::AutoCompactTracking;
use crate::types::config::QueryGates;
use super::super::super::loop_helpers::ESCALATED_MAX_TOKENS;
```

Note: `ESCALATED_MAX_TOKENS` is currently referenced as
`super::super::loop_helpers::ESCALATED_MAX_TOKENS` in the original file (line 1379).
After the split it becomes
`super::super::super::loop_helpers::ESCALATED_MAX_TOKENS`.

### 3.6 `crates/allthecodes-engine/src/query/tests/control_tests.rs` (NEW, ~250 lines)

Contains 5 tests covering flow control: stop hooks, token budget, max turns, hook-stopped
tool execution.

| Test | Lines | What it verifies |
|---|---|---|
| `test_stop_hook_continuation_injects_meta_user_message_once` | 1393-1429 | StopPrevention hook adds one meta user message |
| `test_token_budget_continuation_injects_nudge_message` | 1432-1463 | Token budget nudge triggers continuation |
| `test_max_turns_limit` | 1944-2002 | MaxTurnsReached attachment emitted |
| `test_hook_stopped_tool_execution_yields_attachment_and_stops` | 2005-2055 | Hook-stopped continuation emits attachment |

**Use statements:**

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use super::super::*;                                // query(), etc.
use super::mocks::{
    MockDeps, make_user_message_for_test, make_query_params,
    make_text_response, make_text_response_with_stop_and_output_tokens,
    request_start_count, has_api_error_containing,
};
use crate::types::config::{QueryGates, QuerySource, TaskBudget};
use crate::types::message::{
    AssistantMessage, Attachment, AttachmentMessage, ContentBlock,
    Message, MessageContent, QueryYield, Usage, UserMessage,
};
```

### 3.7 `crates/allthecodes-engine/src/query/tests/media_tests.rs` (NEW, ~400 lines)

Contains 2 integration-style tests with specialized mock deps.

| Test | Lines | What it verifies |
|---|---|---|
| `test_image_tool_result_flows_as_blocks` | 2227-2353 | Image blocks from tool result propagate as structured content |
| `test_computer_use_screenshot_click_round_trip` | 2501-2659 | Full CU cycle: screenshot -> image -> click -> text |

**Use statements:**

```rust
use std::sync::Arc;
use super::super::*;                                // query(), etc.
use super::mocks::{ImageMockDeps, CuMockDeps};
use crate::types::config::QuerySource;
use crate::types::message::{
    AssistantMessage, ContentBlock, ImageSource, Message, MessageContent,
    QueryYield, ToolResultContent, Usage, UserMessage,
};
```

### 3.8 `loop_impl.rs` -- Modification

**Remove** (line 885-887 of current file):

```rust
#[cfg(test)]
#[path = "loop_tests.rs"]
mod loop_tests;
```

**Add after the last line** (or in its place):

```rust
#[cfg(test)]
mod tests;
```

This enables the new `tests/mod.rs` module tree.

### 3.9 Deleted file

**`crates/allthecodes-engine/src/query/loop_tests.rs`** -- Entirely removed. All content is
redistributed across the 7 new files listed above.

## 4. Detailed Migration Order

The migration must be performed in the order listed below to keep `cargo test`
green at every step (or at least not break the build):

### Step 1: Create `tests/` directory and `tests/mod.rs`

Create the directory and the root module file. This file initially declares no
sub-modules -- it is a valid empty module.

```bash
mkdir crates/allthecodes-engine/src/query/tests/
```

```rust
// tests/mod.rs -- start empty
```

### Step 2: Update `loop_impl.rs` to point to the new module

Replace the old `#[path]`-based include with the standard module declaration.

**Change in loop_impl.rs:**

Old (lines 885-887):
```rust
#[cfg(test)]
#[path = "loop_tests.rs"]
mod loop_tests;
```

New:
```rust
#[cfg(test)]
mod tests;
```

At this point `cargo test` will still compile (the old `loop_tests.rs` is no longer
included, but the new module tree is a valid empty module).

### Step 3: Create `tests/mocks.rs` (the shared infrastructure)

This is the highest-risk step because every other test file depends on it.

1. Copy all mock structs/enums/impls from `loop_tests.rs` into `tests/mocks.rs`.
2. Change all `use super::*` to explicit `use super::super::*` for items from `loop_impl.rs`.
3. Change all `use super::super::deps::*` to `use super::super::super::deps::*` (since mocks.rs
   is at depth 3 from `query/`).
4. Convert all `use super::super::loop_helpers::*` items to
   `use super::super::super::loop_helpers::*`.
5. Re-export all public items via `pub` visibility.
6. Wire `tests/mod.rs` with `pub mod mocks;`.

**Verify:** `cargo test --lib` compiles (tests won't run yet because no test functions exist).

### Step 4: Create `tests/setup_tests.rs` (simplest group -- no edge cases)

1. Copy the 6 setup tests and their associated helper function
   (`drain_steer_messages` helper is auto-available via `super::super::*`).
2. Add `pub mod setup_tests;` to `tests/mod.rs`.
3. Build and run: `cargo test setup_tests`

### Step 5: Create `tests/recovery_tests.rs` (largest group)

1. Copy the 11 recovery tests.
2. Watch for the `ESCALATED_MAX_TOKENS` import path change
   (from `super::super::loop_helpers::` to `super::super::super::loop_helpers::`).
3. Add `pub mod recovery_tests;` to `tests/mod.rs`.
4. Build and run: `cargo test recovery_tests`

### Step 6: Create `tests/flow_tests.rs`

1. Copy the 11 flow/integration tests.
2. Add `pub mod flow_tests;` to `tests/mod.rs`.
3. Build and run: `cargo test flow_tests`

### Step 7: Create `tests/control_tests.rs`

1. Copy the 4 control-flow tests.
2. Add `pub mod control_tests;` to `tests/mod.rs`.
3. Build and run: `cargo test control_tests`

### Step 8: Create `tests/media_tests.rs`

1. Copy the 2 media/image tests.
2. Add `pub mod media_tests;` to `tests/mod.rs`.
3. Build and run: `cargo test media_tests`

### Step 9: Run full test suite

```bash
cargo test --lib
```

Verify all 33 tests pass.

### Step 10: Delete `loop_tests.rs`

```bash
rm crates/allthecodes-engine/src/query/loop_tests.rs
```

Verify that the file is no longer referenced and the build succeeds.

### Step 11: Final verification

1. `cargo test --lib` -- all 33 query-loop tests pass.
2. `cargo clippy --tests` -- no regressions.
3. `cargo build --release` -- release build unaffected (all new code is `#[cfg(test)]`).

## 5. Risks and Mitigations

### Risk 1: Visibility of private items in `loop_impl.rs`

**Issue:** The test files currently access private functions from `loop_impl.rs`:
- `apply_snip_projection()` (line 867) -- no `pub` visibility
- `drain_steer_messages()` (line 853) -- no `pub` visibility
- `should_accept_partial_response_after_chunk_read_error()` (line 829) -- no `pub`
  visibility (not directly accessed in tests, but called within the query loop)

**Mitigation:** In Rust, `super::super::*` in a module nested inside `loop_impl.rs`
gives access to private items in `loop_impl.rs`. Since the new module tree is
nested under `loop_impl.rs` via `mod tests;`, this access path is preserved.
**No visibility changes needed.**

### Risk 2: `super` path depth changes

**Issue:** Helper items in `loop_helpers.rs` and `deps.rs` are accessed via
`super::super::loop_helpers::*` in the old file. After the split, each level
of nesting adds one more `super::`:

| Old path | New path |
|---|---|
| `super::*` | `super::super::*` (loop_impl items) |
| `super::super::deps::*` | `super::super::super::deps::*` |
| `super::super::loop_helpers::*` | `super::super::super::loop_helpers::*` |
| `crate::types::*` | `crate::types::*` (unchanged) |

**Mitigation:** Each test file must use the correct depth. The plan specifies
exact `use` statements per file. A compile-check after each step catches errors.

### Risk 3: `cargo test` parallel test interference

**Issue:** Some tests (e.g., `deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery`)
use `#[serial_test::serial]` and global test state (`clear_discovered_tools_for_tests()`).
After splitting into separate files, `cargo test` runs test functions across files in
parallel, which could cause interference.

**Mitigation:** The `#[serial_test::serial]` attribute is test-function-scoped, not
file-scoped. It serializes the specific function against all other `#[serial]` tests.
This works identically across files as it does within the current single file.
**No remediation needed.**

### Risk 4: `cfg(test)` conditionals on `DEFAULT_STREAM_IDLE_TIMEOUT` etc.

**Issue:** `loop_helpers.rs` defines test-specific timeout values:
```rust
#[cfg(test)]
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_millis(50);
```

These are already in `loop_helpers.rs` and are not in `loop_tests.rs`. The test
files access them via `super::super::super::loop_helpers::stream_idle_timeout()`
(which internally uses these constants). **No change needed.**

### Risk 5: The `#[path]` attribute removal may cause relative-path issues

**Issue:** The current `#[path = "loop_tests.rs"]` string is a relative path from
the parent file (`loop_impl.rs`). After removal, Rust's standard module resolution
uses `tests/mod.rs` + `tests/{name}.rs`.

**Mitigation:** Standard Rust module resolution is well-understood and deterministic.
The `#[path]` approach was the non-standard one. **No risk, strictly clean-up.**

### Risk 6: Cross-file `use` of mock types in test files

**Issue:** Test files need to import mock types defined in `tests/mocks.rs`.
If those types are not `pub`, sibling modules cannot see them.

**Mitigation:** All structs, impls, and helper functions in `mocks.rs` are declared
`pub` (or `pub(crate)`). The `mod.rs` re-exports with `pub mod mocks;`.

### Risk 7: Ownership boundary of helper functions like `tool_use_summary_gate_case`

**Issue:** This async function is used by tests in `flow_tests.rs` but could
reasonably live in `mocks.rs`. If it lives in `mocks.rs` alongside the mock
infrastructure, it needs access to `MockDeps`, `make_query_params`, etc., which
it already has. This is cleaner than duplicating it.

**Solution:** Keep ALL helper functions in `mocks.rs` and import them where needed.

## 6. New vs Old Line Count Summary

| File | Old lines | New lines | Delta |
|---|---|---|---|
| `loop_tests.rs` | 2694 | 0 | -2694 |
| `tests/mod.rs` | 0 | ~30 | +30 |
| `tests/mocks.rs` | 0 | ~550 | +550 |
| `tests/setup_tests.rs` | 0 | ~250 | +250 |
| `tests/flow_tests.rs` | 0 | ~350 | +350 |
| `tests/recovery_tests.rs` | 0 | ~500 | +500 |
| `tests/control_tests.rs` | 0 | ~250 | +250 |
| `tests/media_tests.rs` | 0 | ~400 | +400 |
| `loop_impl.rs` (delta) | -- | ~-30 (remove #[path] block, add `mod tests;`) | -2 |
| **Total** | **~2694** | **~2330** | **~-364** (slight reduction from dedup) |

The slight line reduction comes from consolidating duplicate `use` statements
and removing the `#[path]` boilerplate.

## 7. Verification Checklist

After migration, confirm:

- [ ] 33 unique test functions are present across the 6 test files (count: 6+10+11+4+2+2 = wait, that's 35. Need to recount.)

Let me recount carefully:

| File | Test count |
|---|---|
| `setup_tests.rs` | 6 |
| `flow_tests.rs` | 10 |
| `recovery_tests.rs` | 11 |
| `control_tests.rs` | 4 |
| `media_tests.rs` | 2 |
| **Total** | **33** |

Check: 6 + 10 + 11 + 4 + 2 = 33. Correct.

- [ ] `#[serial_test::serial]` attribute is preserved on
  `deferred_enabled_request_keeps_only_core_tool_schemas_after_discovery`.
- [ ] `allthecodes_tools::deferred_tools::clear_discovered_tools_for_tests()` is
  still called in that test.
- [ ] No tests reference `super::*` or `use super::super::deps::*` directly --
  all use statements are explicit.

## 8. Test Category Mapping (Old -> New)

```
Original test                                          New file
-----------------------------------------------------  ----------------------
query_refreshes_tools_before_first_model_call          setup_tests.rs
query_shapes_autocompact_with_final_request_context     setup_tests.rs
deferred_enabled_request_keeps_only_core_...            setup_tests.rs
text_only_model_filters_view_image_from_...             setup_tests.rs
agent_session_filters_prompt_and_recursive_...          setup_tests.rs
query_drains_steer_before_next_model_request            setup_tests.rs
test_simple_text_response_terminates                    flow_tests.rs
test_tool_use_then_text_response                        flow_tests.rs
test_abort_before_api_call                              flow_tests.rs
tool_use_summary_gate_defaults_off                      flow_tests.rs
tool_use_summary_gate_yields_summary_when_enabled       flow_tests.rs
observable_input_backfill_clones_yield_...              flow_tests.rs
observable_input_backfill_matches_with_...              flow_tests.rs
streaming_tool_execution_gate_starts_safe_tools_...     flow_tests.rs
streaming_tool_execution_aborts_started_tools_on_...    flow_tests.rs
snip_projection_removes_requested_messages_only         flow_tests.rs
test_prompt_too_long_reactive_compact_retries_...       recovery_tests.rs
test_prompt_too_long_collapse_drain_retries_...         recovery_tests.rs
test_prompt_too_long_terminals_after_collapse_...       recovery_tests.rs
test_fallback_model_retries_stream_start_capacity_err  recovery_tests.rs
test_fallback_strips_signature_blocks_from_retry_...    recovery_tests.rs
test_fallback_tombstones_partial_assistant_after_...    recovery_tests.rs
test_chunk_read_error_after_text_accepts_partial_...    recovery_tests.rs
test_fallback_exhaustion_releases_terminal_...          recovery_tests.rs
test_stream_idle_watchdog_errors_when_first_event_...   recovery_tests.rs
test_stream_stall_detection_errors_after_handshake_...  recovery_tests.rs
test_max_tokens_recovery_escalates_next_request_limit   recovery_tests.rs
test_stop_hook_continuation_injects_meta_user_...       control_tests.rs
test_token_budget_continuation_injects_nudge_message    control_tests.rs
test_max_turns_limit                                    control_tests.rs
test_hook_stopped_tool_execution_yields_attachment_..   control_tests.rs
test_image_tool_result_flows_as_blocks                  media_tests.rs
test_computer_use_screenshot_click_round_trip           media_tests.rs
```

## 9. Rollback Plan

If the migration causes test failures:

1. **Partial rollback:** Revert individual test files by commenting out `pub mod <name>;`
   in `tests/mod.rs`. The remaining tests still compile and run. This allows
   incremental debugging.

2. **Full rollback:** Revert `loop_impl.rs` to restore the `#[path]` attribute and
   delete `tests/` directory. The original `loop_tests.rs` is still on disk until
   Step 10.

3. **The original file should NOT be deleted (Step 10 skipped) until the full suite
   passes twice** consecutively.
