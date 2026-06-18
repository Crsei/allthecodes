# Code-Split Plan: `crates/allthecodes-permissions/src/dangerous.rs`

> **File analyzed:** `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-permissions/src/dangerous.rs`
> **Current size:** ~1994 lines, single monolithic file
> **Context:** Auto-mode + Bash + PowerShell danger detection for the permission system

## 1. Current State

### 1.1 Module tree (before)

```
crates/allthecodes-permissions/src/
├── lib.rs                              # pub mod dangerous;
├── dangerous.rs                        # ~1994 lines — ALL the code below
├── bash_matcher.rs
├── decision.rs
├── path_validation.rs
├── permission_update.rs
├── rules.rs
├── shadowed_rules.rs
└── read_only_shell/                    # (existing directory-module pattern)
```

### 1.2 Current internal structure (three conceptual domains crammed into one file)

| Domain | Lines | Public API | Private helpers | Tests |
|---|---|---|---|---|
| **Auto-mode permission stripping** | 1--329, 1430--1701 (~510 total) | `AutoModePermissionStrip`, `AutoModeRuntimeTransition`, `strip_dangerous_permissions_for_auto_mode`, `restore_dangerous_permissions_after_auto_mode`, `set_permission_mode_with_auto_mode_safety`, `strip_dangerous_permissions_for_active_auto_mode`, `restore_auto_mode_stripped_permissions` | `merge_transition`, `dangerous_auto_mode_allow_reason`, `split_permission_rule`, `dangerous_shell_allow_reason`, `auto_allow_content_matches_pattern`, `auto_allow_content_matches_exact_pattern`, `windows_exe_auto_allow_pattern`, `normalize_auto_allow_specifier` | 7 test functions |
| **Generic shell danger detection** | 331--432, 767--812, 1397--1422, 1704--1788 (~365 total) | `is_dangerous_command` | `git_clean_forced_without_dry_run`; struct `DangerPattern` (shared); static `DANGER_PATTERNS` | 11 test functions |
| **PowerShell danger detection** | 434--766, 814--1396, 1791--1993 (~1120 total) | `is_dangerous_powershell_command` | 17 private helper functions; statics `POWERSHELL_DANGER_PATTERNS`, `POWERSHELL_CLM_ALLOWED_TYPES`, `POWERSHELL_NEW_OBJECT_RE` | 5 test functions |

### 1.3 Existing external callers

| Public item | External consumers |
|---|---|
| `is_dangerous_command` | `classifier.rs`, `bash.rs` |
| `is_dangerous_powershell_command` | `classifier.rs` |
| `set_permission_mode_with_auto_mode_safety` | `runtime_config.rs`, `handlers.rs`, `config_cmd.rs`, `permissions_cmd.rs`, `plan_mode.rs`, `plan_workflow.rs`, `plan.rs` |
| `strip_dangerous_permissions_for_active_auto_mode` | `permissions_cmd.rs`, `startup`-related callers |
| `AutoModeRuntimeTransition` | `permissions_cmd.rs` (return type + format helper) |
| `AutoModePermissionStrip` | Internal only (returned by `strip_dangerous_permissions_for_auto_mode`) |
| `restore_auto_mode_stripped_permissions` | Internal (called by `set_permission_mode_with_auto_mode_safety`) |
| `restore_dangerous_permissions_after_auto_mode` | Internal (called by `restore_auto_mode_stripped_permissions`) |
| `strip_dangerous_permissions_for_auto_mode` | Internal (called by `strip_dangerous_permissions_for_active_auto_mode`) |

## 2. Proposed Split

### 2.1 Target module tree (after)

```
crates/allthecodes-permissions/src/
├── lib.rs                              # UNCHANGED: pub mod dangerous;
├── dangerous/                          # directory module (REPLACES dangerous.rs)
│   ├── mod.rs                          # re-exports + shared DangerPattern struct
│   ├── auto_mode.rs                    # auto-mode permission stripping
│   ├── shell.rs                        # generic shell danger detection
│   └── powershell.rs                   # PowerShell-specific danger detection
├── ... (unchanged)
```

### 2.2 File-by-file specification

---

#### 2.2.1 `crates/allthecodes-permissions/src/dangerous/mod.rs`

**Approximate line count:** 30--35 lines

**Purpose:** Module root. Declares submodules, re-exports all public API, holds the shared `DangerPattern` struct.

**Items defined:**
- `struct DangerPattern` (private, shared by `shell.rs` and `powershell.rs`)
  - Fields: `regex: Regex`, `reason: &'static str`

**Submodule declarations:**
```rust
pub(crate) mod auto_mode;
pub(crate) mod shell;
pub(crate) mod powershell;
```

**Re-exports (all public):**
- `pub use auto_mode::{AutoModePermissionStrip, AutoModeRuntimeTransition, set_permission_mode_with_auto_mode_safety, strip_dangerous_permissions_for_active_auto_mode, restore_auto_mode_stripped_permissions, strip_dangerous_permissions_for_auto_mode, restore_dangerous_permissions_after_auto_mode};`
- `pub use shell::is_dangerous_command;`
- `pub use powershell::is_dangerous_powershell_command;`

**Dependencies:**
- `use regex::Regex;` (for `DangerPattern`)
- `use std::sync::LazyLock;` (no -- static definitions are in submodules)

**Tests:** None (no testable logic).

**Design note:** `DangerPattern` is placed here rather than duplicated because both `shell.rs` and `powershell.rs` need it for their `LazyLock<Vec<DangerPattern>>` statics. It remains `pub(crate)` -- not re-exported externally (it has no reason to be).

---

#### 2.2.2 `crates/allthecodes-permissions/src/dangerous/auto_mode.rs`

**Approximate line count:** ~470 lines (330 production + 140 tests)

**Items from original (exact names):**

| Item | Kind | Visibility | Notes |
|---|---|---|---|
| `AutoModePermissionStrip` | struct | `pub` | Re-exported via mod.rs |
| `AutoModeRuntimeTransition` | struct | `pub` | Re-exported via mod.rs; used by `permissions_cmd.rs` |
| `strip_dangerous_permissions_for_auto_mode` | fn | `pub` | Re-exported |
| `restore_dangerous_permissions_after_auto_mode` | fn | `pub` | Re-exported |
| `set_permission_mode_with_auto_mode_safety` | fn | `pub` | Re-exported |
| `strip_dangerous_permissions_for_active_auto_mode` | fn | `pub` | Re-exported |
| `restore_auto_mode_stripped_permissions` | fn | `pub` | Re-exported |
| `merge_transition` | fn | private | Internal helper; no change needed |
| `dangerous_auto_mode_allow_reason` | fn | private | Internal helper |
| `split_permission_rule` | fn | private | Internal helper |
| `dangerous_shell_allow_reason` | fn | private | Internal helper |
| `auto_allow_content_matches_pattern` | fn | private | Internal helper |
| `auto_allow_content_matches_exact_pattern` | fn | private | Internal helper |
| `windows_exe_auto_allow_pattern` | fn | private | Internal helper |
| `normalize_auto_allow_specifier` | fn | private | Internal helper |
| `CROSS_PLATFORM_CODE_EXEC_AUTO_ALLOW_PATTERNS` | const | private | Moved here |
| `DANGEROUS_BASH_AUTO_ALLOW_PATTERNS` | const | private | Moved here |
| `DANGEROUS_POWERSHELL_AUTO_ALLOW_PATTERNS` | const | private | Moved here |

**Helper function `test_permission_context` (line 1576):** Move to `#[cfg(test)]` section as a test utility.

**Dependencies (use statements):**
```rust
use allthecodes_types::permissions::{
    PermissionMode, StrippedPermissionRule, ToolPermissionContext, ToolPermissionRulesBySource,
};
// (no regex, no HashSet, no LazyLock -- this module uses none of those)
```

**Tests (moved from lines 1430--1701, excluding PowerShell/shell tests):**
- `test_strip_dangerous_permissions_for_auto_mode`
- `test_strip_dangerous_permissions_keeps_narrow_shell_rules`
- `test_restore_dangerous_permissions_after_auto_mode`
- `test_auto_mode_runtime_transition_strips_and_restores`
- `test_auto_mode_runtime_transition_respects_availability_policy`
- `test_auto_mode_policy_disable_restores_active_auto_mode_rules`
- `test_active_auto_mode_strips_new_session_rules`

---

#### 2.2.3 `crates/allthecodes-permissions/src/dangerous/shell.rs`

**Approximate line count:** ~260 lines (175 production + 85 tests)

**Items from original (exact names):**

| Item | Kind | Visibility | Notes |
|---|---|---|---|
| `DANGER_PATTERNS` | static `LazyLock<Vec<DangerPattern>>` | private | Uses `super::DangerPattern` |
| `is_dangerous_command` | fn | `pub` | Public API entry point |
| `git_clean_forced_without_dry_run` | fn | private | Internal helper |
| `has_unterminated_quotes` | fn | external dep | From `allthecodes_utils::bash` |
| `contains_multiline_string` | fn | external dep | From `allthecodes_utils::bash` |

**Dependencies (use statements):**
```rust
use regex::Regex;
use std::sync::LazyLock;

use super::DangerPattern;
use allthecodes_utils::bash::{contains_multiline_string, has_unterminated_quotes};
```

**Tests (moved from lines 1704--1788):**
- `test_safe_commands`
- `test_rm_rf_root`
- `test_rm_rf_home`
- `test_git_force_push`
- `test_git_reset_hard`
- `test_dd`
- `test_mkfs`
- `test_chmod_777`
- `test_fork_bomb`
- `test_device_write`
- `test_curl_pipe_sh`
- `test_git_clean_and_stash_destructive_commands`
- `test_database_destructive_commands`

**Public doc example:** Keep the existing doc-tests on `is_dangerous_command`:
```rust
/// ```
/// use allthecodes_permissions::dangerous::is_dangerous_command;
/// assert!(is_dangerous_command("rm -rf /").is_some());
/// assert!(is_dangerous_command("ls -la").is_none());
/// ```
```

---

#### 2.2.4 `crates/allthecodes-permissions/src/dangerous/powershell.rs`

**Approximate line count:** ~810 lines (610 production + 200 tests)

**Items from original (exact names):**

| Item | Kind | Visibility | Notes |
|---|---|---|---|
| `POWERSHELL_DANGER_PATTERNS` | static `LazyLock<Vec<DangerPattern>>` | private | Uses `super::DangerPattern` |
| `POWERSHELL_CLM_ALLOWED_TYPES` | static `LazyLock<HashSet<&'static str>>` | private | Mirrors Bun TypeScript allowlist |
| `POWERSHELL_NEW_OBJECT_RE` | static `LazyLock<Regex>` | private | |
| `is_dangerous_powershell_command` | fn | `pub` | Public API entry point |
| `powershell_obvious_parse_error_reason` | fn | private | |
| `powershell_ast_heuristic_reason` | fn | private | |
| `powershell_segment_command_before` | fn | private | |
| `strip_powershell_module_prefix` | fn | private | |
| `is_powershell_safe_script_block_consumer` | fn | private | |
| `powershell_new_object_type_outside_clm` | fn | private | |
| `powershell_new_object_type_from_args` | fn | private | |
| `split_powershell_args` | fn | private | |
| `parse_powershell_parameter` | fn | private | |
| `is_powershell_param_prefix` | fn | private | |
| `powershell_param_abbrev_matches` | fn | private | |
| `new_object_value_param_consumes_next` | fn | private | |
| `clean_powershell_arg` | fn | private | |
| `powershell_type_literal_outside_clm` | fn | private | |
| `read_bracketed_type_literal` | fn | private | |
| `normalize_powershell_type_name` | fn | private | |
| `next_non_ws` | fn | private | |
| `is_powershell_command_boundary` | fn | private | |
| `is_powershell_variable_or_subexpression_start` | fn | private | |
| `is_powershell_type_start` | fn | private | |
| `is_powershell_identifier_start` | fn | private | |
| `is_powershell_identifier_continue` | fn | private | |

**Dependencies (use statements):**
```rust
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use super::DangerPattern;
use super::shell::is_dangerous_command;
```

**Tests (moved from lines 1791--1993):**
- `test_powershell_destructive_commands`
- `test_powershell_security_validator_patterns`
- `test_powershell_new_object_typename_clm_boundary`
- `test_powershell_obvious_parse_errors_fail_closed`
- `test_powershell_script_blocks_fail_closed_except_safe_consumers`

**Note on cross-module test calls:** The test `test_powershell_security_validator_patterns` includes `is_dangerous_command("powershell.exe ...")` at line 1936. The `use super::shell::is_dangerous_command;` import at the module level makes this available inside both production and `#[cfg(test)]` code via `super::*`.

---

## 3. Dependency Graph

```
mod.rs
  ├── DangerPattern (struct)
  │
  ├── auto_mode.rs
  │     Dependencies: allthecodes_types::permissions::*
  │     Uses from mod.rs: nothing
  │     ═══════════════════════════  STANDALONE  ══════════════
  │
  ├── shell.rs
  │     Dependencies: regex, std::sync::LazyLock, allthecodes_utils::bash::*
  │     Uses from mod.rs: DangerPattern
  │     ═══════════════════════════  STANDALONE  ══════════════
  │
  └── powershell.rs
        Dependencies: regex, std::collections::HashSet, std::sync::LazyLock
        Uses from mod.rs: DangerPattern
        Uses from shell.rs: is_dangerous_command
        ═══════════════  DEPENDS ON shell.rs  ════════════════
```

**No circular dependencies.** The graph is a directed acyclic graph (DAG):

```
mod.rs  →  shell.rs  →  powershell.rs
  │
  └───→  auto_mode.rs
```

## 4. Risks and Mitigations

### 4.1 Circular dependencies
**Risk level: none.** Each submodule only references `super::DangerPattern` (from `mod.rs`) or `super::shell::is_dangerous_command` (from `powershell.rs`). No cycle is possible.

### 4.2 Shared mutable state
**Risk level: none.** The `LazyLock` statics (`DANGER_PATTERNS`, `POWERSHELL_DANGER_PATTERNS`, etc.) remain per-file with no cross-module mutation. `DangerPattern` is an immutable struct. No shared `RefCell`, `Mutex`, or `Atomic` is involved.

### 4.3 Visibility regressions
**Risk level: low.**

- `DangerPattern` is currently *implicitly* private (no `pub`). After the split it moves to `mod.rs` where it must be `pub(crate)` so that sibling submodules `shell.rs` and `powershell.rs` can access it via `use super::DangerPattern;`. This does NOT expose it to the rest of the crate or external consumers -- `pub(crate)` is crate-private.
- All re-exports in `mod.rs` must exactly match the current public surface. Use `pub use auto_mode::*;` and `pub use shell::*;` and `pub use powershell::*;` as a safe default, but explicitly listing each item is preferred to avoid accidentally re-exporting something that should be private.

### 4.4 Build implications
**Risk level: medium (one-time).**

- Delete `dangerous.rs` and create `dangerous/` directory. If a build script or IDE caches the old path, a `cargo clean` may be needed.
- `lib.rs` has `pub mod dangerous;` -- this auto-resolves to either `dangerous.rs` or `dangerous/mod.rs`. No change needed in `lib.rs`.
- Existing `use` paths like `allthecodes_permissions::dangerous::is_dangerous_command` continue to work because `mod.rs` re-exports everything.

### 4.5 Test isolation
**Risk level: low.**

- Each submodule's `#[cfg(test)] mod tests` block uses `use super::*;`. After the split, `super::*` only imports items from that submodule and from `mod.rs` re-exports.
- For `powershell.rs` tests that call `is_dangerous_command` (line 1936), the `super::*` import from `mod.rs` re-exports `shell::is_dangerous_command` -- so it will be available. If we use explicit re-exports instead of glob, we must ensure this is included.
- **Recommendation:** Use explicit re-exports in `mod.rs` rather than glob (`pub use shell::is_dangerous_command;` rather than `pub use shell::*;`) to maintain tight control over the public API.

### 4.6 The `test_permission_context` helper
**Risk level: low.** This test helper is currently defined outside the `#[cfg(test)]` block but is only used by tests. Move it into `#[cfg(test)]` in `auto_mode.rs`.

---

## 5. Migration Order (Step-by-Step)

### Phase A: Scaffold directory module (no deletion yet)

**Step 1** -- Create `crates/allthecodes-permissions/src/dangerous/mod.rs`:
- Define `DangerPattern` struct.
- Declare submodules (`pub(crate) mod auto_mode; mod shell; mod powershell;`).
- Write re-export lines for all 7 public API items.
- Keep `pub mod dangerous;` in `lib.rs` unchanged.

**Step 2** -- Create `crates/allthecodes-permissions/src/dangerous/shell.rs`:
- Copy `DangerPattern`-related code (lines 331--432: struct + `DANGER_PATTERNS` static).
- Copy `is_dangerous_command` (lines 767--812).
- Copy `git_clean_forced_without_dry_run` (lines 1397--1422).
- Copy shell tests (lines 1704--1788).
- Add `use super::DangerPattern;`.
- Keep doc example.

**Step 3** -- Create `crates/allthecodes-permissions/src/dangerous/auto_mode.rs`:
- Copy structs + constants + all public functions + all private helpers (lines 1--329).
- Copy auto-mode tests (lines 1430--1541, 1576--1701).
- Move `test_permission_context` into `#[cfg(test)]`.
- Add `use allthecodes_types::permissions::*;`.

**Step 4** -- Create `crates/allthecodes-permissions/src/dangerous/powershell.rs`:
- Copy all PowerShell statics (lines 434--766).
- Copy `is_dangerous_powershell_command` (lines 814--852).
- Copy all PowerShell private helpers (lines 854--1395).
- Copy PowerShell tests (lines 1791--1993).
- Add `use super::DangerPattern;` and `use super::shell::is_dangerous_command;`.

### Phase B: Verify and delete old file

**Step 5** -- Build and test:
```bash
cargo check -p allthecodes-permissions
cargo test -p allthecodes-permissions
```
Fix any import/visibility issues (most likely: missing re-exports in `mod.rs`, or incorrect `use` path in `powershell.rs`).

**Step 6** -- Check downstream consumers build:
```bash
cargo check -p allthecodes-commands -p allthecodes-tools -p allthecodes-engine -p allthecodes-safety -p allthecodes-web -p allthecodes-startup
```

**Step 7** -- Delete `crates/allthecodes-permissions/src/dangerous.rs`:
```bash
git rm crates/allthecodes-permissions/src/dangerous.rs
```

**Step 8** -- Final build and full test suite:
```bash
cargo test --workspace
```

### Phase C (optional): Re-export cleanup

**Step 9** -- If explicit re-exports were used, consider switching to glob re-exports or vice versa based on team preference. The `read_only_shell/mod.rs` precedent exports a mix of specific items and globs, so either style is acceptable.

---

## 6. Summary Table

| File (path relative to `crates/allthecodes-permissions/src/`) | New? | Approx lines | Contains |
|---|---|---|---|
| `dangerous.rs` | DELETED | -- | Replaced by directory module |
| `dangerous/mod.rs` | NEW | 35 | Submodule decls, `DangerPattern`, re-exports |
| `dangerous/auto_mode.rs` | NEW | 470 | Auto-mode stripping structs, fns, helpers, 7 tests |
| `dangerous/shell.rs` | NEW | 260 | `is_dangerous_command`, `DANGER_PATTERNS`, 13 tests |
| `dangerous/powershell.rs` | NEW | 810 | `is_dangerous_powershell_command`, 3 statics, 17 helpers, 5 tests |

**Total lines after split:** ~1575 (vs ~1994 today) -- the reduction comes from eliminating module-scope comments that only apply once, and slightly tighter imports. The real value is not line count but separation of concerns: each file has a single, well-defined responsibility.
