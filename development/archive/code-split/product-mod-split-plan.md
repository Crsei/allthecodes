# Product Module Split Plan

## 1. Current State

**File**: `crates/allthecodes-tools/src/product/mod.rs` — 1706 lines, 8 tool implementations, 15 supporting types/structs, 22 free functions, and one monolithic test module.

The module is a flat monolith. Every tool, helper, struct, and free function lives in the same file. There is no subdirectory, no submodule, no test separation.

### Current Module Tree

```
product/
  mod.rs
```

### Current Contents (homogeneous, single file)

| Category | Count | Items |
|---|---|---|
| Tool structs | 8 | `CtxInspectTool`, `MonitorTool`, `TerminalCaptureTool`, `SendUserFileTool`, `ReviewArtifactTool`, `SnipTool`, `ListPeersTool`, `RemoteTriggerTool` |
| Free functions | 22 | `tools()`, `shell_tool_use_ids`, `find_tool_result_text`, `tool_result_text`, `content_block_text`, `message_kind`, `message_text`, `tail_lines`, `truncate_chars`, `parse_process_capture_input`, `run_captured_process`, `spawn_output_reader`, `append_limited`, `shell_command`, `pty_capture_command`, `shell_quote`, `parse_send_file_input`, `validate_send_file_path`, `list_peers`, `read_local_peer`, `resolve_remote_target`, `normalize_daemon_url`, `configured_peers`, `configured_peer_token`, `read_local_control_token`, `append_remote_trigger_audit`, `data_root`, `daemon_dir`, `read_json` |
| Types/structs | 10 | `CapturedToolOutput`, `ProcessCaptureSpec`, `ProcessCaptureResult`, `SendFileInput`, `PeerInfo`, `DaemonStateFile`, `ControlTokenFile`, `ConfiguredPeer`, `RemoteTarget`, `RemoteTriggerAudit` |
| Test cases | 9 | Mix of unit tests for helpers + integration tests for peer/resolution logic |
| Uses | 16 | From std, anyhow, tokio, serde, url, uuid, and crate-internal modules |

### Dependency Import Lines (current)

```rust
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

use crate::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult,
};
use allthecodes_types::message::{
    AssistantMessage, Attachment, AttachmentMessage, ContentBlock, InfoLevel, Message,
    MessageContent, SystemMessage, SystemSubtype, ToolResultContent,
};
```

---

## 2. Target Module Tree (After Split)

```
product/
  mod.rs          (~70 lines)    Module declarations, re-exports, `tools()` factory
  common.rs       (~130 lines)   Shared message/text scanning utilities
  ctx_inspect.rs  (~100 lines)   CtxInspectTool implementation
  capture.rs      (~310 lines)   MonitorTool + TerminalCaptureTool + process capture helpers
  send_file.rs    (~110 lines)   SendUserFileTool + SendFileInput + helpers
  review.rs       (~105 lines)   ReviewArtifactTool
  snip.rs         (~115 lines)   SnipTool
  peers.rs        (~340 lines)   ListPeersTool + RemoteTriggerTool + peer/remote types + helpers
```

Total estimated line count after split: ~1280 lines (down from 1706 due to deduplication of `use` statements and module scaffolding).

---

## 3. Per-File Specifications

### 3.1 `product/mod.rs` — Module root, re-exports, `tools()` factory

**Functions/items**: `tools()` factory function only.

```rust
// ~70 lines
mod common;
mod capture;
mod ctx_inspect;
mod peers;
mod review;
mod send_file;
mod snip;

use std::sync::Arc;
use crate::tool::Tools;

pub fn tools() -> Tools {
    vec![
        Arc::new(ctx_inspect::CtxInspectTool),
        Arc::new(capture::MonitorTool),
        Arc::new(capture::TerminalCaptureTool),
        Arc::new(send_file::SendUserFileTool),
        Arc::new(review::ReviewArtifactTool),
        Arc::new(snip::SnipTool),
        Arc::new(peers::ListPeersTool),
        Arc::new(peers::RemoteTriggerTool),
    ]
}
```

**Visibility**: `tools()` is `pub`. No types need re-exporting from `mod.rs` — consumers are only other `crate::` modules that call `product::tools()`.

**Dependencies**: `std::sync::Arc`, `crate::tool::Tools`.

---

### 3.2 `product/common.rs` — Shared text/message scanning utilities

**Functions** (all `pub(super)`):

| Function | Signature | Lines | Current lines |
|---|---|---|---|
| `captured_tool_output` | `struct CapturedToolOutput` | 8 | 854–858 |
| `shell_tool_use_ids` | `fn(&[Message]) -> HashMap<String, String>` | 16 | 860–875 |
| `find_tool_result_text` | `fn(&[Message], Option<&str>, &HashMap<String, String>) -> Option<CapturedToolOutput>` | 34 | 877–910 |
| `tool_result_text` | `fn(&ToolResultContent) -> String` | 11 | 912–922 |
| `content_block_text` | `fn(&ContentBlock) -> String` | 14 | 924–937 |
| `message_kind` | `fn(&Message) -> &'static str` | 9 | 939–947 |
| `message_text` | `fn(&Message) -> String` | 22 | 949–970 |
| `tail_lines` | `fn(&str, usize) -> String` | 5 | 972–976 |
| `truncate_chars` | `fn(&str, usize) -> String` | 9 | 978–986 |

**Approx line count**: 130 lines (types + functions + tests).

**Exact `use` statements needed**:
```rust
use std::collections::HashMap;
use std::fmt::Write;
use serde_json::Value;
use crate::tool::ToolResult;
use allthecodes_types::message::{
    ContentBlock, Message, MessageContent, ToolResultContent,
};
```

**Tests** (inline, `#[cfg(test)]`):
- `tail_lines_returns_requested_suffix`

---

### 3.3 `product/ctx_inspect.rs` — CtxInspectTool

**Struct**: `pub(super) struct CtxInspectTool;`

**Trait impl**: `impl Tool for CtxInspectTool` with `name`, `description`, `input_json_schema`, `is_concurrency_safe`, `is_read_only`, `call`, `prompt`.

**Approx line count**: 100 lines.

**Dependencies**:
```rust
use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::{json, Value};
use crate::tool::{PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, Tools, ValidationResult};
use allthecodes_types::message::{AssistantMessage, Message};
use super::common::{message_kind, message_text, truncate_chars};
```

**Tests**: None specific (the message scanning helpers it calls are tested in `common.rs`). Could add a unit test for token estimation behavior.

---

### 3.4 `product/capture.rs` — MonitorTool + TerminalCaptureTool + process capture helpers

**Structs** (`pub(super)`):
- `MonitorTool`
- `TerminalCaptureTool`
- `ProcessCaptureSpec`
- `ProcessCaptureResult`

**Functions** (private to this file):
- `parse_process_capture_input`
- `run_captured_process`
- `spawn_output_reader`
- `append_limited`
- `shell_command`
- `pty_capture_command`
- `shell_quote`

**Trait impls**:
- `impl Tool for MonitorTool`
- `impl Tool for TerminalCaptureTool`

**Approx line count**: 310 lines.

**Dependencies**:
```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use crate::tool::{PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::{AssistantMessage, ContentBlock, ToolResultContent};
use super::common::{find_tool_result_text, shell_tool_use_ids, tail_lines, CapturedToolOutput};
```

**Tests** (inline):
- `parse_process_capture_input_validates_bounds`
- `pty_command_quotes_shell_input`

---

### 3.5 `product/send_file.rs` — SendUserFileTool

**Structs** (`pub(super)`):
- `SendUserFileTool`
- `SendFileInput`

**Functions** (private):
- `parse_send_file_input`
- `validate_send_file_path`

**Trait impl**: `impl Tool for SendUserFileTool`

**Approx line count**: 110 lines.

**Dependencies**:
```rust
use std::fs;
use std::path::PathBuf;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use anyhow::{bail, Context, Result};
use uuid::Uuid;
use crate::tool::{PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::{
    AssistantMessage, Attachment, AttachmentMessage, Message, ToolResultContent,
};
```

**Tests** (inline):
- `send_file_input_accepts_file_path_alias_and_size_limit`
- `validate_send_file_rejects_directories_and_large_files`

---

### 3.6 `product/review.rs` — ReviewArtifactTool

**Struct**: `pub(super) struct ReviewArtifactTool;`

**Trait impl**: `impl Tool for ReviewArtifactTool`

**Approx line count**: 105 lines (currently 97 lines for the impl + scaffolding).

**Dependencies**:
```rust
use async_trait::async_trait;
use serde_json::{json, Value};
use anyhow::Result;
use crate::tool::{Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;
```

**Tests**: None specific. Add a basic validation test for completeness.

---

### 3.7 `product/snip.rs` — SnipTool

**Struct**: `pub(super) struct SnipTool;`

**Trait impl**: `impl Tool for SnipTool`

**Approx line count**: 115 lines.

**Dependencies**:
```rust
use std::collections::HashSet;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use anyhow::Result;
use uuid::Uuid;
use crate::tool::{PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::{
    AssistantMessage, InfoLevel, Message, SystemMessage, SystemSubtype,
};
```

**Tests**: None specific. Add a basic validation test.

---

### 3.8 `product/peers.rs` — ListPeersTool + RemoteTriggerTool + peer infrastructure

**Structs** (`pub(super)`):
- `ListPeersTool`
- `RemoteTriggerTool`
- `PeerInfo`
- `DaemonStateFile`
- `ControlTokenFile`
- `ConfiguredPeer`
- `RemoteTarget`
- `RemoteTriggerAudit`

**Functions** (private):
- `list_peers`
- `read_local_peer`
- `resolve_remote_target`
- `normalize_daemon_url`
- `configured_peers`
- `configured_peer_token`
- `read_local_control_token`
- `append_remote_trigger_audit`
- `data_root`
- `daemon_dir`
- `read_json`

**Trait impls**:
- `impl Tool for ListPeersTool`
- `impl Tool for RemoteTriggerTool`

**Approx line count**: 340 lines.

**Dependencies**:
```rust
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::select;
use url::Url;
use uuid::Uuid;
use crate::tool::{PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult};
use allthecodes_types::message::AssistantMessage;
```

**Tests** (inline):
- `normalize_daemon_url_rejects_credentials`
- `list_peers_reads_local_daemon_state` (marked `#[serial]`)
- `configured_peers_support_array_and_token_env` (marked `#[serial]`)
- `remote_trigger_audit_appends_ndjson_without_token` (marked `#[serial]`)

The `EnvGuard` test helper struct stays here as it is only used by these tests.

---

## 4. Dependency Graph Within `product/`

```
mod.rs
  |
  +--> common.rs     (no submodule dependencies — pure utility)
  |
  +--> ctx_inspect.rs  ──depends on──>  common.rs  (message_kind, message_text, truncate_chars)
  |
  +--> capture.rs      ──depends on──>  common.rs  (CapturedToolOutput, find_tool_result_text,
  |                                          shell_tool_use_ids, tail_lines)
  |
  +--> send_file.rs  (no submodule dependencies)
  |
  +--> review.rs     (no submodule dependencies)
  |
  +--> snip.rs       (no submodule dependencies)
  |
  +--> peers.rs      (no submodule dependencies)
```

**Key observations**:
- `common.rs` is the only dependency target — it imports nothing from sibling submodules.
- `ctx_inspect.rs` and `capture.rs` both import from `common.rs`.
- No circular dependency risk.
- Each tool file can be compiled independently once `common.rs` is compiled.
- The `tools()` factory in `mod.rs` imports from all submodules — this is the correct convergence point.

---

## 5. Visibility Changes Required

These functions/types must change from private (`fn`/`struct`) to `pub(super)` so sibling modules can use them:

| Item | Current visibility | New visibility | Reason |
|---|---|---|---|
| `CapturedToolOutput` | `struct` (private) | `pub(super) struct` | Used by `capture.rs` via `common.rs` |
| `shell_tool_use_ids` | `fn` (private) | `pub(super) fn` | Used by `capture.rs` |
| `find_tool_result_text` | `fn` (private) | `pub(super) fn` | Used by `capture.rs` |
| `tool_result_text` | `fn` (private) | `pub(super) fn` | Used by `capture.rs` (via `find_tool_result_text` calling it) |
| `content_block_text` | `fn` (private) | `pub(super) fn` | Used by `capture.rs` |
| `message_kind` | `fn` (private) | `pub(super) fn` | Used by `ctx_inspect.rs` |
| `message_text` | `fn` (private) | `pub(super) fn` | Used by `ctx_inspect.rs` |
| `tail_lines` | `fn` (private) | `pub(super) fn` | Used by `capture.rs` |
| `truncate_chars` | `fn` (private) | `pub(super) fn` | Used by `ctx_inspect.rs` |

All tool structs change from `struct Name;` to `pub(super) struct Name;` since they need to be visible from `mod.rs`.

---

## 6. Risks and Mitigations

### 6.1 Shared mutable state
**None.** All tools are stateless unit structs. The async `call()` methods take `&self`. No `Arc<RwLock<>>` or `static` mutable state exists in this module.

### 6.2 Circular dependencies
**None.** `common.rs` depends only on external crates. All other files depend on `common.rs` or nothing. No cross-dependencies between tool files.

### 6.3 `TerminalCaptureTool` straddles two concerns
**Risk**: The tool uses process-capture helpers (PTY path) AND message-scanning helpers (history path). It lives in `capture.rs` but imports from `common.rs`.

**Mitigation**: Clean, one-directional dependency. `capture.rs` uses `common.rs`, not the reverse. No issue.

### 6.4 Test serialization across files
**Risk**: `peers.rs` tests use `#[serial]` because they set environment variables (`ALLTHECODES_HOME`). If other test files also set env vars, the `#[serial]` attribute won't help across files — `serial_test` only serializes within a single binary, but all tests in `allthecodes-tools` are in a single lib crate, so `#[serial]` works across files within the same crate.

**Mitigation**: Keep `#[serial]` on env-mutating tests in `peers.rs`. No other submodule in `product/` uses environment variables in tests.

### 6.5 Feature-gate fragmentation
**Risk**: The entire `product` module is gated on `#[cfg(feature = "full")]` in `lib.rs`. After the split, each submodule declaration in `mod.rs` also needs `#[cfg(feature = "full")]`.

**Mitigation**: Apply `#[cfg(feature = "full")]` to each `mod` declaration in `product/mod.rs`.

### 6.6 Build implications
**Impact**: More files = more compilation units, but the line count per file drops by an order of magnitude. Compiler parallelism may improve slightly because the optimizer can work on smaller translation units. No runtime binary size change.

### 6.7 Import duplication
**Impact**: Each file will carry its own `use` statements, adding ~3-8 lines of boilerplate per file. This is the standard Rust pattern (see `fs/` and `network/` sibling modules) and is accepted.

---

## 7. Step-by-Step Migration Order

### Phase 1: Create submodule files (no deletion yet)

All code is MOVED, not copied. Each step removes lines from `mod.rs` and places them in the new file.

**Step 1: Create `product/common.rs`**
- Move `CapturedToolOutput`, `shell_tool_use_ids`, `find_tool_result_text`, `tool_result_text`, `content_block_text`, `message_kind`, `message_text`, `tail_lines`, `truncate_chars`.
- Add `pub(super)` to each function/struct.
- Add `#[cfg(test)] mod tests { ... }` with `tail_lines_returns_requested_suffix`.
- Remove the above from `mod.rs`.
- Add `mod common;` to `mod.rs`.

**Step 2: Create `product/capture.rs`**
- Move `MonitorTool`, `TerminalCaptureTool`, `ProcessCaptureSpec`, `ProcessCaptureResult`, `parse_process_capture_input`, `run_captured_process`, `spawn_output_reader`, `append_limited`, `shell_command`, `pty_capture_command`, `shell_quote`.
- Add `pub(super)` to structs.
- Update imports: `use super::common::{CapturedToolOutput, find_tool_result_text, shell_tool_use_ids, tail_lines};`.
- Move matching test functions.
- Remove the above from `mod.rs`.
- Add `mod capture;` to `mod.rs`.

**Step 3: Create `product/ctx_inspect.rs`**
- Move `CtxInspectTool` impl.
- Add `pub(super) struct CtxInspectTool;`.
- Update imports: `use super::common::{message_kind, message_text, truncate_chars};`.
- Remove from `mod.rs`.
- Add `mod ctx_inspect;` to `mod.rs`.

**Step 4: Create `product/send_file.rs`**
- Move `SendUserFileTool`, `SendFileInput`, `parse_send_file_input`, `validate_send_file_path`.
- Add `pub(super)` to structs.
- Move matching test functions.
- Remove from `mod.rs`.
- Add `mod send_file;` to `mod.rs`.

**Step 5: Create `product/review.rs`**
- Move `ReviewArtifactTool` impl.
- Add `pub(super) struct ReviewArtifactTool;`.
- Remove from `mod.rs`.
- Add `mod review;` to `mod.rs`.

**Step 6: Create `product/snip.rs`**
- Move `SnipTool` impl.
- Add `pub(super) struct SnipTool;`.
- Remove from `mod.rs`.
- Add `mod snip;` to `mod.rs`.

**Step 7: Create `product/peers.rs`**
- Move `ListPeersTool`, `RemoteTriggerTool`, `PeerInfo`, `DaemonStateFile`, `ControlTokenFile`, `ConfiguredPeer`, `RemoteTarget`, `RemoteTriggerAudit`, `list_peers`, `read_local_peer`, `resolve_remote_target`, `normalize_daemon_url`, `configured_peers`, `configured_peer_token`, `read_local_control_token`, `append_remote_trigger_audit`, `data_root`, `daemon_dir`, `read_json`.
- Add `pub(super)` to structs.
- Move all peer-related tests and `EnvGuard`.
- Remove from `mod.rs`.
- Add `mod peers;` to `mod.rs`.

### Phase 2: Rewrite `product/mod.rs`

**Step 8**: Replace the full content of `product/mod.rs` with:
- Module declarations (each `#[cfg(feature = "full")]` — though since parent already gates this, the cfg is technically redundant; still include for clarity).
- `use std::sync::Arc; use crate::tool::Tools;`
- `pub fn tools()` returning the vec.

**Step 9**: Remove the obsolete `#[cfg(test)] mod tests { ... }` block. All tests are now distributed to their respective submodule files.

### Phase 3: Cross-file validation

**Step 10**: Run `cargo check -p allthecodes-tools` to verify compilation.
- Expected errors: missing imports, visibility mismatches.
- Fix all errors by adjusting `use` paths and visibility qualifiers.

**Step 11**: Run `cargo test -p allthecodes-tools -- product` to execute all product module tests.
- Verify all 9 test cases pass.
- Pay attention to `#[serial]` tests — they must not deadlock.

**Step 12**: Run `cargo clippy -p allthecodes-tools` to catch any lint issues.

### Phase 4: Cleanup

**Step 13**: Remove any residual commented-out code or orphaned imports from `mod.rs`.

**Step 14**: Verify `cargo build` across the entire workspace to ensure `allthecodes-engine` (which depends on `allthecodes-tools`) still compiles.

---

## 8. Old vs. New Comparison

### Old

```
product/mod.rs  (1706 lines)
  |
  +-- 8 tool struct definitions + impls
  +-- 10 supporting types
  +-- 22 free functions
  +-- 1 test module (9 tests)
  +-- all inline, no submodules
```

### New

```
product/
  mod.rs          (~70 lines)    Module root + `tools()` factory
  common.rs       (~130 lines)   Shared message/text scanning utilities (9 functions, 1 type)
  ctx_inspect.rs  (~100 lines)   CtxInspectTool
  capture.rs      (~310 lines)   MonitorTool + TerminalCaptureTool + process helpers (2 tools, 2 types)
  send_file.rs    (~110 lines)   SendUserFileTool (1 tool, 1 type)
  review.rs       (~105 lines)   ReviewArtifactTool
  snip.rs         (~115 lines)   SnipTool
  peers.rs        (~340 lines)   ListPeersTool + RemoteTriggerTool + peer infra (2 tools, 6 types)
```

### Before/After Metric Comparison

| Metric | Before | After | Delta |
|---|---|---|---|
| Lines in `mod.rs` | 1706 | ~70 | -1636 |
| Max file size | 1706 | ~340 | -1366 |
| Number of files | 1 | 8 | +7 |
| Test isolation | monolithic | per-file | better |
| Import density | 16 use stmts | 2-10 per file | slightly more total, but each file manages its own |
| Compile parallelism | serial per file | parallel across 8 files | better |

---

## 9. Post-Split Cleanup Checklist

- [ ] `mod.rs` no longer contains any tool `impl` blocks or helper functions
- [ ] All `use` statements in `mod.rs` are removed except those needed for the `tools()` factory
- [ ] Every submodule has its own `use` block
- [ ] `pub(super)` applied to all cross-file types and functions
- [ ] Tests are distributed: `common.rs` tests for text utilities, `capture.rs` for process parsing, `send_file.rs` for file validation, `peers.rs` for peer discovery
- [ ] `EnvGuard` lives only in `peers.rs`
- [ ] `#[serial]` tests are all in `peers.rs`
- [ ] `#[cfg(feature = "full")]` on every `mod` declaration in `product/mod.rs`
- [ ] `cargo check` passes
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes
- [ ] `cargo build --workspace` passes
