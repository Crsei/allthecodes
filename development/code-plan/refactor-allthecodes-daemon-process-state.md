# Refactoring Plan: `process_state.rs` (1877 lines)

## 1. Summary

**Current state** of `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-daemon/src/process_state.rs`:

| Metric | Count |
|--------|-------|
| Total lines | 1877 |
| Public structs | 7 |
| Public enums | 3 |
| Public functions | 35 |
| `pub(crate)` functions | 8 |
| Private functions | 11 |
| Inline modules (`sqlite_store`) | 1 |
| Test helper structs | 1 (EnvGuard) |
| Test functions | 11 |

**External consumers** (4 files across 2 crates):
- `allthecodes/src/command_runtime_bridge.rs`
- `allthecodes/src/full_init.rs`
- `allthecodes/src/main.rs`
- `allthecodes-web/src/handlers/gateways.rs`

**Internal consumers** (11 files within the `allthecodes-daemon` crate).

---

## 2. Breakdown: Major Sections

| # | Section | Lines | Lines | Description |
|---|---------|-------|-------|-------------|
| 1 | **Types & Enums** | 1-133 | 133 | `DaemonRunStatus`, `DaemonWorkerStatus` (with `as_str()`), `DaemonWorkerSummary`, `DaemonProcessState`, `DaemonWorkerState`, `DaemonShutdownRequest`, `DaemonControlToken`, `DaemonSleepState`, `DaemonStatusSnapshot`, `StaleStateCleanupReport` |
| 2 | **Path Helpers** | 135-210 | 76 | `data_root()`, `daily_log_path()`, `team_memory_dir()`, `daemon_dir()`, `state_path()`, `shutdown_request_path()`, `control_token_path()`, `sleep_state_path()`, `workers_dir()`, `worker_state_path()`, `logs_dir()`, `worker_log_path()` |
| 3 | **State Writers — Supervisor** | 212-280 | 69 | `write_started()`, `write_stopped()`, `write_supervisor_heartbeat()` |
| 4 | **State Writers — Workers** | 282-346 | 65 | `write_worker_running()`, `write_worker_heartbeat()`, `write_worker_stopped()`, `write_worker_stale()` |
| 5 | **State Readers** | 348-427 | 80 | `read_worker_state()`, `read_worker_states()`, `worker_summaries()`, `read_state()` |
| 6 | **Status Snapshot & Cleanup** | 429-468, 606-709 | 144 | `status_snapshot()`, `cleanup_stale_state_before_start()`, `cleanup_worker_state_files()`, `cleanup_worker_state_records()` |
| 7 | **Shutdown Request** | 470-502 | 33 | `request_shutdown()`, `shutdown_requested()`, `clear_shutdown_request()` |
| 8 | **Control Token** | 504-544 | 41 | `write_control_token()`, `read_control_token()`, `verify_control_token()`, `clear_control_token()` |
| 9 | **Sleep State** | 546-604 | 59 | `write_sleep_state()`, `write_sleep_state_until()`, `read_sleep_state()`, `active_sleep_state()`, `clear_sleep_state()` |
| 10 | **CLI Management Commands** | 711-995 | 285 | `try_run_management_command()` + 13 subcommand handlers (`start_daemon`, `stop_daemon`, `restart_daemon`, `submit_worker_command`, `abort_worker_command`, `print_worker_command`, `print_worker_events`, `print_control_token`, `schedule_sleep_command`, `wake_daemon_command`, `print_stale_cleanup_report`, `require_running_daemon`, `print_status`) |
| 11 | **I/O Utilities** | 1036-1141, 1454-1475 | 128 | `remove_file_if_exists()`, `write_state()`, `write_worker_state()`, `write_state_value()`, `remove_state_value()`, `read_worker_state_file()`, `ensure_daemon_dir()`, `atomic_write_json()`, `health_url()`, `sanitize_worker_id()`, `parse_port()`, `print_result()`, `print_usage()` |
| 12 | **SQLite Store Module** | 1143-1452 | 310 | Inline `mod sqlite_store` — migrations, CRUD for state and workers, legacy JSON import |
| 13 | **Platform Helpers** | 1493-1554 | 62 | `configure_detached()` (win/unix), `process_is_alive()` (unix/win), `terminate_process_tree()` (unix/win) |
| 14 | **Tests** | 1556-1877 | 322 | `mod tests` — 11 test functions + `EnvGuard` helper |

---

## 3. Problems Identified

### 3.1 Mixed Concerns (Primary Issue)

The file conflates six distinct responsibilities:

1. **Data model types** — the structs and enums that define the on-disk schema
2. **Path resolution** — computing file paths under `~/.allthecodes/daemon/`
3. **State CRUD** — reading/writing/deleting state files (JSON + SQLite dual-write)
4. **CLI command parsing and execution** — `try_run_management_command` and all its subcommand handlers
5. **SQLite persistence** — a 310-line inline module with its own migrations and legacy import
6. **Platform abstractions** — process management (`process_is_alive`, `terminate_process_tree`) with cfg-gated implementations

### 3.2 Duplicated Read-JSON Pattern

The pattern `read file -> serde_json::from_str -> map error` is repeated verbatim in 5 functions with only the path and type changing:

- `read_state()` (line 422-426)
- `read_control_token()` (line 529-533)
- `read_sleep_state()` (line 584-588)
- `read_worker_state_file()` (line 1090-1095)
- `import_state_file()` (line 1347-1350)

This could be a single helper: `read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T>`.

### 3.3 Long Functions (>50 lines, candidates for extraction)

| Function | Lines | Issue |
|----------|-------|-------|
| `cleanup_worker_state_files()` | 57 | Single cleanup logic that could be decomposed into smaller steps |
| `start_daemon()` | 67 | Orchestrates stale cleanup, status check, feature gate, port parsing, process spawn, readiness wait — 5+ distinct phases |
| `try_run_management_command()` | 46 | God dispatcher — match on subcommand, each arm calling a private handler. Long but acceptable; could be a dispatch table |
| `cleanup_stale_state_before_start()` | 23 | Acceptable length but orchestrates 6 distinct cleanup steps that could be helper functions |
| `atomic_write_json()` | 35 | Reasonable but could be simplified |
| `print_status()` | 38 | Pure output formatting — low priority to split |

### 3.4 Naming Inconsistency

- `write_supervisor_heartbeat()` vs `write_started()` / `write_stopped()` — the heartbeat function also does a read-modify-write cycle, while `write_started` and `write_stopped` always write fresh state. The name does not reflect this behavioral difference.
- `read_worker_state_file()` (private) vs `read_worker_state()` (public) — similar names but the former reads from a specific path, the latter does SQLite-fallback.

### 3.5 Conditional Compilation Scattered

`#[cfg(feature = "sqlite-storage")]` appears at 9 different call sites (lines 349, 358, 369, 375, 410, 417, 464, 489, 496, 517, 524, 571, 578, 1051, 1065, 1078). This introduces noise throughout the read/write functions. The sqlite_store module itself is gated, but each caller still needs `#[cfg]` blocks for the fallback logic.

### 3.6 Daemon Dir Creation Duplicated

`ensure_daemon_dir()` already creates all three subdirectories (daemon, workers, logs), but `write_state_value()` also calls `ensure_daemon_dir()` before calling `atomic_write_json()`. The `write_state()` and `write_worker_state()` functions also end up calling it indirectly. This is not harmful but indicates unclear ownership of directory creation.

---

## 4. Proposed Split

### 4.1 Recommended File Structure

Convert `process_state.rs` into a `process_state/` directory module:

```
src/process_state/
  mod.rs              — Re-exports; public API facade (was lib.rs pub items)
  types.rs            — Data model structs and enums (~133 lines)
  paths.rs            — Path helper functions (~76 lines)
  state.rs            — CRUD operations for supervisor & worker state (~320 lines)
  sqlite_store.rs     — SQLite persistence layer (was inline mod, ~310 lines)
  sleep.rs            — Sleep state functions (~60 lines)
  control_token.rs    — Control token functions (~40 lines)
  shutdown.rs         — Shutdown request functions (~35 lines)
  platform.rs         — process_is_alive, terminate_process_tree, configure_detached (~60 lines)
  io.rs               — atomic_write_json, ensure_daemon_dir, read_json_file, sanitize_worker_id (~100 lines)
  cli.rs              — try_run_management_command + subcommand handlers (~285 lines)
  tests.rs            — All test code (~320 lines)
```

### 4.2 New File Responsibilities

#### `mod.rs` — Public API Facade

Re-export all public items from submodules so that existing callers continue to work with `allthecodes_daemon::process_state::*`.

```rust
mod types;
mod paths;
mod state;
mod sqlite_store;
mod sleep;
mod control_token;
mod shutdown;
mod platform;
mod io;
mod cli;

pub use types::*;
pub use paths::*;
pub use state::*;
pub use sleep::*;
pub use control_token::*;
pub use shutdown::*;
pub use platform::*;
pub use io::*;
pub use cli::*;

// pub(crate) re-exports needed by other crate modules
pub(crate) use io::atomic_write_json;
pub(crate) use paths::{data_root, daily_log_path, team_memory_dir};
pub(crate) use platform::{process_is_alive, terminate_process_tree};
```

#### `types.rs` — Data Models

Move (lines 1-133):
- `DaemonRunStatus` enum
- `DaemonWorkerStatus` enum + `impl as_str()`
- `DaemonWorkerSummary` struct
- `DaemonProcessState` struct
- `DaemonWorkerState` struct
- `DaemonShutdownRequest` struct
- `DaemonControlToken` struct
- `DaemonSleepState` struct
- `DaemonStatusSnapshot` enum
- `StaleStateCleanupReport` struct
- `SCHEMA_VERSION` constant
- `DEFAULT_DAEMON_PORT` constant (cfg(test))

No public API change. These types are consumed externally by `allthecodes` and `allthecodes-web`.

#### `paths.rs` — Path Resolution

Move (lines 135-210):
- `data_root()` → `pub(crate)`
- `daily_log_path()` → `pub(crate)`
- `team_memory_dir()` → `pub(crate)`
- `daemon_dir()` → `pub`
- `state_path()` → `pub`
- `shutdown_request_path()` → `pub`
- `control_token_path()` → `pub`
- `sleep_state_path()` → `pub`
- `workers_dir()` → `pub`
- `worker_state_path()` → `pub`
- `logs_dir()` → `pub`
- `worker_log_path()` → `pub`

Visibility stays the same.

#### `state.rs` — State CRUD Operations

Move (lines 212-427, 429-468, 606-709, 1045-1105):
- `write_started()`, `write_stopped()`, `write_supervisor_heartbeat()` (supervisor writers)
- `write_worker_running()`, `write_worker_heartbeat()`, `write_worker_stopped()`, `write_worker_stale()` (worker writers)
- `read_worker_state()`, `read_worker_states()`, `worker_summaries()`, `read_state()` (readers)
- `status_snapshot()`, `cleanup_stale_state_before_start()`, `cleanup_worker_state_files()`, `cleanup_worker_state_records()` (status & cleanup)
- Internal helpers: `write_state()`, `write_worker_state()`, `write_state_value()`, `remove_state_value()`, `read_worker_state_file()`, `ensure_daemon_dir()`

This moves from `pub` to `pub(crate)`:
- `cleanup_worker_state_files()` (currently private)
- `cleanup_worker_state_records()` (currently private)

#### `shutdown.rs` — Shutdown Request

Move (lines 470-502):
- `request_shutdown()` → `pub`
- `shutdown_requested()` → `pub`
- `clear_shutdown_request()` → `pub`

#### `control_token.rs` — Control Token

Move (lines 504-544):
- `write_control_token()` → `pub`
- `read_control_token()` → `pub`
- `verify_control_token()` → `pub`
- `clear_control_token()` → `pub`

#### `sleep.rs` — Sleep State

Move (lines 546-604):
- `write_sleep_state()` → `pub`
- `write_sleep_state_until()` → `pub`
- `read_sleep_state()` → `pub`
- `active_sleep_state()` → `pub`
- `clear_sleep_state()` → `pub`

#### `io.rs` — I/O Helpers

Move (lines 1036-1105, 1107-1141, 1454-1475):
- `atomic_write_json()` → `pub(crate)`
- `remove_file_if_exists()` → keep private
- `ensure_daemon_dir()` → keep private
- `health_url()` → keep private
- `sanitize_worker_id()` → keep private
- `parse_port()` → keep private (used by cli.rs)
- `print_result()` → keep private (used by cli.rs)
- `print_usage()` → keep private (used by cli.rs)
- New: `read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T>` — consolidate duplicated JSON read patterns

#### `platform.rs` — Platform-specific Helpers

Move (lines 1493-1554, plus health_url):
- `configure_detached()` (cfg-gated)
- `process_is_alive()` (cfg-gated) → `pub(crate)`
- `terminate_process_tree()` (cfg-gated) → `pub(crate)`

#### `cli.rs` — CLI Management Commands

Move (lines 711-995, 1477-1491):
- `try_run_management_command()` → `pub`
- `start_daemon()` → private
- `stop_daemon()` → private
- `restart_daemon()` → private
- `submit_worker_command()` → private
- `abort_worker_command()` → private
- `print_worker_command()` → private
- `print_worker_events()` → private
- `print_control_token()` → private
- `schedule_sleep_command()` → private
- `wake_daemon_command()` → private
- `print_stale_cleanup_report()` → private
- `require_running_daemon()` → private
- `print_status()` → private
- `print_result()` → private (or move to io.rs)
- `print_usage()` → private (or move to io.rs)

This is the largest extraction (285 lines). In a future phase, this module could be further split into `cli/dispatch.rs` and `cli/commands.rs`.

#### `sqlite_store.rs` — SQLite Persistence

Move the inline `mod sqlite_store` (lines 1143-1452) to its own file:
- All migration definitions, CRUD operations, legacy import helpers
- Gated by `#[cfg(feature = "sqlite-storage")]` at the module level
- Imported as `super::sqlite_store` from `state.rs`

#### `tests.rs` — All Tests

Move `mod tests` (lines 1556-1877):
- `EnvGuard` helper struct
- 11 test functions
- Gated by `#[cfg(test)]`
- No visibility change needed

---

## 5. Migration Strategy

### Phase 0: Preparation (no behavior change)

1. Create `src/process_state/` directory
2. Create `mod.rs` that `#[path = "../process_state.rs"] pub mod process_state;` as a forward-compat shim
3. Verify `cargo test --workspace` passes

### Phase 1: Extract types.rs

1. Copy lines 1-133 (types) into `src/process_state/types.rs`
2. Add `mod types;` and `pub use types::*;` to `mod.rs`
3. Remove the duplicated types from the old `process_state.rs`
4. Verify compilation and tests

### Phase 2: Extract paths.rs

Same approach: copy, add module + re-export, remove from old file, verify.

### Phase 3-9: Extract remaining files

Extract one file at a time in dependency order (leaf modules first, then those that depend on them):

| Order | File | Depends on |
|-------|------|------------|
| 1 | `types.rs` | nothing |
| 2 | `paths.rs` | nothing |
| 3 | `io.rs` | nothing |
| 4 | `platform.rs` | nothing |
| 5 | `sqlite_store.rs` | `types`, `paths`, `io` |
| 6 | `shutdown.rs` | `types`, `paths`, `io`, `sqlite_store` |
| 7 | `control_token.rs` | `types`, `paths`, `io`, `sqlite_store` |
| 8 | `sleep.rs` | `types`, `paths`, `io`, `sqlite_store` |
| 9 | `state.rs` | all above |
| 10 | `cli.rs` | state, shutdown, control_token, sleep |
| 11 | `tests.rs` | everything |

After each extraction: `cargo build` and `cargo test -p allthecodes-daemon`.

### Phase 10: Flip the switch

1. Delete the old `process_state.rs`
2. Rename `process_state/` `mod.rs` to remove the path shim
3. Update `lib.rs` (currently `pub mod process_state;` — this already points to the directory module, so no change needed if `mod.rs` exists)
4. Full `cargo build --workspace && cargo test --workspace`

### Phase 11: Introduce `read_json_file`

After extraction, consolidate the 5 duplicated read-JSON patterns into a single helper in `io.rs`. This is a pure internal refactor:

```rust
pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse JSON {}", path.display()))
}
```

### Phase 12 (optional): Extract `io.rs` helpers into cli.rs

`print_result()` and `print_usage()` are only used by `cli.rs`. Move them there.

---

## 6. Estimated Post-Refactor Line Counts

| File | Est. Lines | Gain |
|------|-----------|------|
| `src/process_state/mod.rs` | ~50 | Re-exports only |
| `src/process_state/types.rs` | ~130 | Unchanged from section |
| `src/process_state/paths.rs` | ~75 | Unchanged |
| `src/process_state/io.rs` | ~90 | ~10% reduction from dedup |
| `src/process_state/platform.rs` | ~60 | Unchanged |
| `src/process_state/sqlite_store.rs` | ~310 | Unchanged |
| `src/process_state/shutdown.rs` | ~35 | Unchanged |
| `src/process_state/control_token.rs` | ~40 | Unchanged |
| `src/process_state/sleep.rs` | ~60 | Unchanged |
| `src/process_state/state.rs` | ~320 | Minor reduction from helper extraction |
| `src/process_state/cli.rs` | ~280 | Minor reduction from print_result/print_usage move |
| `src/process_state/tests.rs` | ~320 | Unchanged |
| **Total** | **~1770** | ~100 lines saved from deduplication |

No single file will exceed 330 lines. The largest will be `sqlite_store.rs` (~310 lines), `state.rs` (~320 lines), and `tests.rs` (~320 lines).

---

## 7. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| **Re-export breakage**: external code uses `allthecodes_daemon::process_state::SomeType` and the path changes | Low | High | `mod.rs` must re-export everything. Run `cargo build --workspace` after each phase. |
| **Internal import breakage**: crates within `allthecodes-daemon` import `super::process_state::foo` and the module structure changes | Medium | High | All internal consumers import through `crate::process_state::*`. As long as `mod.rs` re-exports, these continue to work. |
| **Symbol name conflicts**: two extracted files define a function with the same name | Low | Medium | Currently no name conflicts exist. If introduced, rename or keep as private within the file. |
| **sqlite_store module visibility**: the inline mod is `#[cfg(feature = "sqlite-storage")]` and calls `super::*` | Low | Medium | When extracted to its own file, change `super::*` to explicit imports from the crate root (`crate::process_state::*`). |
| **Test files referencing internal functions**: tests may call private functions from the old file | Low | Low | Tests can access `pub(crate)` items since they're in the same crate. Ensure visibility is correct. |
| **Merge conflicts from other branches**: `process_state.rs` is actively developed | Medium | Medium | Perform this refactor in a single focused PR. Consider pinning the branch and rebasing after other changes land. |
| **`pub(crate)` vs `pub` misclassification**: a function currently `pub` is only needed `pub(crate)` | Low | Low | Audit external consumers before changing visibility. Break into separate PR if risky. |

### Audit of `pub` vs `pub(crate)` candidates

The following functions currently `pub` but are only used within the crate (could be tightened to `pub(crate)`):

- `clear_shutdown_request()` — used only in `state.rs` and `cli.rs` internally
- `clear_control_token()` — used only within the crate
- `clear_sleep_state()` — used only within the crate

However, changing visibility of these is a minor behavioral change if anyone outside depends on them. Recommend keeping them `pub` unless explicitly confirmed unused externally (grep shows no external usage).

### Rollback Plan

Each extraction phase is a pure move — no logic changes. If a phase breaks CI, revert the single file addition/removal for that phase. The old `process_state.rs` is kept intact until Phase 10, providing a clean rollback path at any point.
