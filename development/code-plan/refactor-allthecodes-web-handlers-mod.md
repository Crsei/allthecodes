# Refactoring Plan: `allthecodes-web/src/handlers/mod.rs`

Date: 2026-06-16
Target file: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-web/src/handlers/mod.rs`
Current size: 2471 lines, 85794 bytes on disk

---

## 1. Summary: Current State

| Metric | Count |
|---|---|
| Total lines | 2471 |
| Module declarations | 37 `pub mod` + 1 `pub(crate) mod` = 38 |
| Wildcard re-exports | 37 `pub use *` globs |
| Structs/enums defined inline | 0 (all in submodules) |
| Functions defined inline | 3 (`set_command_provider`, `get_all_commands`, `setting_bool`) |
| Statics | 1 (`COMMAND_PROVIDER: OnceLock<RwLock<Option<CommandProvider>>>`) |
| Type aliases | 1 (`CommandProvider`) |
| Test functions in `mod tests` | **33 test functions** |
| Test module total lines | **~2334 lines** (94.5% of the file) |

The file is essentially a **"barrel" module** (re-exports + module declarations) that has accreted a **monolithic test suite** in the `#[cfg(test)] mod tests { ... }` block spanning lines 137--2471.

---

## 2. Breakdown: Every Major Section

| Section | Lines | Lines count | Content |
|---|---|---|---|
| Module doc comment | 1--7 | 7 | `//! Axum route handlers...` |
| Imports (`use std/use allthecodes`) | 8--10 | 3 | 2 import lines |
| Module declarations (`pub mod`) | 12--51 | 40 | 38 submodule declarations |
| Re-exports (`pub use`) | 53--92 | 40 | 37 glob re-exports + `use api_errors` |
| Shared types (empty) | 94--97 | 4 | Two comment separators, no actual code |
| **Command-provider registry** | 98--131 | 34 | `type CommandProvider`, `COMMAND_PROVIDER` static, `set_command_provider()`, `get_all_commands()`, `setting_bool()` |
| **Tests module** | 133--2471 | 2339 | `#[cfg(test)] mod tests { ... }` |

### Tests module internal structure

| Subsection | Lines | Line count | Test functions |
|---|---|---|---|
| Imports + uses | 137--148 | 12 | -- |
| Session archive tests | 150--203 | 54 | 3 tests |
| Workspace patch test | 206--241 | 36 | 1 test |
| Session new target tests | 243--298 | 56 | 2 tests |
| Session resume permissions | 300--379 | 80 | 2 tests |
| Cold session engine permissions | 381--416 | 36 | 1 test |
| Workspace archive test | 418--459 | 42 | 1 test |
| Models set default test | 461--483 | 23 | 1 test |
| MCP servers CRUD test | 485--598 | 114 | 1 very large combined test |
| Plugins round-trip test | 600--667 | 68 | 1 large combined test |
| Channels daemon stopped test | 669--679 | 11 | 1 test |
| System action 501 endpoints test | 681--721 | 41 | 1 combined test (5 endpoints) |
| Activity recorder test | 723--748 | 26 | 1 combined test (3 endpoints) |
| Agents REST handlers test | 750--850 | 101 | 1 very large combined test |
| People CRUD test | 852--937 | 86 | 1 large combined test |
| Hooks CRUD test | 939--1009 | 71 | 1 large combined test |
| Prompts CRUD test | 1011--1080 | 70 | 1 large combined test |
| Usage tests | 1082--1193 | 112 | 5 tests |
| Memory API tests | 1195--1338 | 144 | 4 tests |
| Files API tests | 1340--2018 | 679 | **19 tests** (largest block) |
| Skills API tests | 2020--2470 | 451 | **10 tests** (second largest) |

---

## 3. Problems

### 3.1 Monolithic test suite (P0 -- primary problem)

The single `mod tests { ... }` block contains **33 test functions** covering **17 different handler modules**. This is the root cause of the file exceeding the 1500-line threshold. Every engineer adding a new handler is tempted to add tests in `mod.rs` rather than in the handler's own module.

**Consequences:**
- Poor locality of testing -- tests for `files.rs` are 1400+ lines away from the implementation
- Merge conflicts when two people add tests for different handlers
- Impossible to navigate -- you cannot jump from a handler function to its tests
- Test file effects are coupled -- a change to test infra (like test_support) touches this massive file

### 3.2 No module boundary for internals

The three inline items belong conceptually to different modules but live in the barrel:

| Item | Natural home |
|---|---|
| `COMMAND_PROVIDER` / `set_command_provider` / `get_all_commands` | Its own `commands.rs` module, or a `handler_shared.rs` |
| `pub(crate) fn setting_bool(...)` | `handler_shared.rs` or inline where used |

### 3.3 Large combined test functions (code smell)

Several tests test **multiple endpoints** in a single `#[tokio::test]` function, making them harder to debug when they fail:

| Test function | Endpoints covered | Lines |
|---|---|---|
| `mcp_servers_crud_uses_editable_settings_scopes` | list, create, update, delete | 114 |
| `plugins_local_install_list_marketplace_and_uninstall_round_trip` | install, list, marketplace, uninstall, list-again | 68 |
| `agents_rest_handlers_list_persist_and_restore` | list, create (x3), update, delete, restore | 101 |
| `system_action_endpoints_return_explicit_501_codes` | computer-use, appshots, chrome-relay (5 endpoints) | 41 |

These are acceptable as integration tests since they exercise stateful workflows, but they should still live in the module that owns the handler.

### 3.4 Test fragmentation across the "barrel"

The `#[cfg(test)] mod tests` block uses `use super::*;` to pull in all re-exports, which also pulls in the test_support items. When tests are extracted to individual handler files, they need explicit imports. This is straightforward but requires adjusting every test.

### 3.5 Comment sections as module boundaries that don't exist

Lines use `// --- Memory API tests ---` style comments to create pseudo-sections, which is a classic sign that the file should be split by those section boundaries.

---

## 4. Proposed Split

### Target file structure after refactoring

```
crates/allthecodes-web/src/handlers/
  mod.rs                     # ~120 lines: only pub mod + pub use + re-exports
  commands.rs                # NEW ~35 lines: COMMAND_PROVIDER, set/get commands
  handler_shared.rs          # NEW ~15 lines: setting_bool, other shared helpers
  activity_recorder.rs       # existing + #[cfg(test)] mod tests
  admin.rs                   # existing
  agents.rs                  # existing + #[cfg(test)] mod tests
  appshots.rs                # existing + #[cfg(test)] mod tests
  auth.rs                    # existing
  backend_services.rs        # existing
  capabilities.rs            # existing
  channels.rs                # existing + #[cfg(test)] mod tests
  chat.rs                    # existing
  chat_modes.rs              # existing
  chrome_relay.rs            # existing + #[cfg(test)] mod tests
  computer_use.rs            # existing + #[cfg(test)] mod tests
  credentials.rs             # existing
  files.rs                   # existing + #[cfg(test)] mod tests
  git.rs                     # existing
  group_chat.rs              # existing
  health.rs                  # existing
  hooks.rs                   # existing + #[cfg(test)] mod tests
  jobs.rs                    # existing
  kanban.rs                  # existing
  launchpad.rs               # existing
  logs.rs                    # existing
  mcp_servers.rs             # existing + #[cfg(test)] mod tests
  memory.rs                  # existing + #[cfg(test)] mod tests
  models.rs                  # existing + #[cfg(test)] mod tests
  people.rs                  # existing + #[cfg(test)] mod tests
  plugins.rs                 # existing + #[cfg(test)] mod tests
  profiles.rs                # existing
  prompts.rs                 # existing + #[cfg(test)] mod tests
  providers.rs               # existing
  proxy.rs                   # existing
  sessions.rs                # existing + #[cfg(test)] mod tests
  settings_phase1.rs         # existing
  skills.rs                  # existing + #[cfg(test)] mod tests
  usage.rs                   # existing + #[cfg(test)] mod tests
  workspaces.rs              # existing + #[cfg(test)] mod tests
  test_support.rs            # existing (already #[cfg(test)])
```

### 4.1 `commands.rs` -- extract provider registry

```rust
//! Slash-command provider registry for web command routes.

use std::sync::{OnceLock, RwLock};
use allthecodes_commands::Command;

type CommandProvider = fn() -> Vec<Command>;

static COMMAND_PROVIDER: OnceLock<RwLock<Option<CommandProvider>>> = OnceLock::new();

/// Install the root-owned slash-command registry.
pub fn set_command_provider(provider: CommandProvider) {
    let slot = COMMAND_PROVIDER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(provider);
    }
}

pub(crate) fn get_all_commands() -> Vec<Command> {
    COMMAND_PROVIDER
        .get()
        .and_then(|slot| slot.read().ok().and_then(|guard| *guard))
        .map(|provider| provider())
        .unwrap_or_default()
}
```

### 4.2 `handler_shared.rs` -- extract shared helpers

```rust
//! Small utility functions shared across handler modules.

pub(crate) fn setting_bool(state: &crate::state::WebState, path: &str) -> Option<bool> {
    let map = state.engine().app_state().settings.settings_map();
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = map.get(first)?;
    for part in parts {
        value = value.get(part)?;
    }
    value.as_bool()
}
```

### 4.3 Move tests into handler modules

Each handler module that has tests in `mod.rs` gets a `#[cfg(test)] mod tests { ... }` appended to its own file. The table of where each test goes:

| Current test | Target module | Lines to move |
|---|---|---|
| `session_archive_handler_returns_404_for_missing_session` | `sessions.rs` | 152--166 |
| `session_archive_handler_rejects_active_session_with_409` | `sessions.rs` | 168--183 |
| `session_archive_handler_archives_inactive_session` | `sessions.rs` | 185--203 |
| `workspaces_patch_persists_sidebar_metadata` | `workspaces.rs` | 206--241 |
| `session_new_handler_can_target_known_workspace_cwd` | `sessions.rs` | 243--273 |
| `session_new_handler_can_target_existing_local_cwd` | `sessions.rs` | 275--298 |
| `session_resume_reuses_cached_engine_session_grants` | `sessions.rs` | 300--323 |
| `session_new_inherits_runtime_permissions_but_clears_session_grants` | `sessions.rs` | 325--379 |
| `cold_session_engine_inherits_runtime_permissions_but_clears_session_grants` | `sessions.rs` | 381--416 |
| `workspace_archive_skips_active_and_archives_inactive_sessions` | `workspaces.rs` | 418--459 |
| `models_set_default_persists_model_setting` | `models.rs` | 461--483 |
| `mcp_servers_crud_uses_editable_settings_scopes` | `mcp_servers.rs` | 485--598 |
| `plugins_local_install_list_marketplace_and_uninstall_round_trip` | `plugins.rs` | 600--667 |
| `channels_report_stopped_daemon_without_starting_it` | `channels.rs` | 669--679 |
| `system_action_endpoints_return_explicit_501_codes` | `computer_use.rs` | 681--721 |
| `activity_recorder_empty_store_status_sessions_and_clear_round_trip` | `activity_recorder.rs` | 723--748 |
| `agents_rest_handlers_list_persist_and_restore` | `agents.rs` | 750--850 |
| `people_crud_round_trips_json_and_validates_ids` | `people.rs` | 852--937 |
| `hooks_crud_updates_user_settings_only_and_test_is_explicit_501` | `hooks.rs` | 939--1009 |
| `prompts_crud_round_trips_store_and_rejects_slash_names` | `prompts.rs` | 1011--1080 |
| `usage_empty_store_returns_200_with_zero_totals` | `usage.rs` | 1082--1112 |
| `usage_invalid_period_returns_400` | `usage.rs` | 1114--1135 |
| `usage_profile_id_is_echoed` | `usage.rs` | 1137--1155 |
| `usage_period_24h_is_accepted` | `usage.rs` | 1157--1174 |
| `usage_period_all_is_accepted` | `usage.rs` | 1176--1193 |
| `memory_list_empty_returns_200` | `memory.rs` | 1199--1215 |
| `memory_list_with_profile_id_echoes_it` | `memory.rs` | 1217--1236 |
| `memory_update_persists_changes` | `memory.rs` | 1238--1290 |
| `memory_update_unknown_id_returns_404` | `memory.rs` | 1292--1313 |
| `memory_update_content_too_long_returns_400` | `memory.rs` | 1315--1338 |
| `files_tree_lists_workspace_root` | `files.rs` | 1344--1368 |
| `files_tree_rejects_path_traversal` | `files.rs` | 1370--1389 |
| `files_stat_returns_file_metadata` | `files.rs` | 1391--1415 |
| `files_stat_with_profile_id_echoes_it` | `files.rs` | 1417--1437 |
| `files_read_returns_text_content` | `files.rs` | 1439--1463 |
| `files_read_detects_binary` | `files.rs` | 1465--1488 |
| `files_read_truncates_large_content` | `files.rs` | 1490--1513 |
| `files_write_creates_new_file` | `files.rs` | 1515--1541 |
| `files_write_rejects_overwrite_without_flag` | `files.rs` | 1543--1569 |
| `files_write_with_overwrite_flag_succeeds` | `files.rs` | 1571--1595 |
| `files_write_enforces_hash` | `files.rs` | 1597--1639 |
| `files_read_with_profile_id_echoes_it` | `files.rs` | 1641--1662 |
| `files_mkdir_creates_directory` | `files.rs` | 1664--1681 |
| `files_mkdir_creates_nested_directories` | `files.rs` | 1683--1700 |
| `files_mkdir_idempotent_on_existing` | `files.rs` | 1702--1719 |
| `files_delete_removes_file` | `files.rs` | 1721--1740 |
| `files_delete_rejects_non_empty_dir_without_recursive` | `files.rs` | 1742--1762 |
| `files_delete_recursive_removes_directory` | `files.rs` | 1764--1784 |
| `files_copy_duplicates_file` | `files.rs` | 1786--1810 |
| `files_copy_rejects_overwrite_without_flag` | `files.rs` | 1812--1832 |
| `files_rename_moves_file` | `files.rs` | 1834--1855 |
| `files_move_with_overwrite_replaces_destination` | `files.rs` | 1857--1881 |
| `files_upload_accepts_multiple_files` | `files.rs` | 1883--1916 |
| `files_download_streams_bytes` | `files.rs` | 1918--1951 |
| `files_endpoints_reject_path_traversal` | `files.rs` | 1953--1997 |
| `files_tree_with_profile_id_echoes_it` | `files.rs` | 1999--2018 |
| `skills_list_returns_all_registered_skills` | `skills.rs` | 2039--2071 |
| `skills_list_with_profile_id_echoes_it` | `skills.rs` | 2073--2092 |
| `skills_detail_returns_skill_info` | `skills.rs` | 2094--2117 |
| `skills_detail_missing_returns_404` | `skills.rs` | 2119--2132 |
| `skills_files_missing_skill_returns_404` | `skills.rs` | 2134--2154 |
| `skills_files_bundled_skill_has_no_files` | `skills.rs` | 2156--2178 |
| `skills_files_reads_file_from_user_skill` | `skills.rs` | 2180--2222 |
| `skills_files_rejects_path_traversal` | `skills.rs` | 2224--2263 |
| `skills_files_rejects_hidden_files` | `skills.rs` | 2265--2301 |
| `skills_files_rejects_directory` | `skills.rs` | 2303--2340 |
| `skills_files_echoes_profile_id` | `skills.rs` | 2342--2378 |
| `skills_patch_persists_enabled_and_pinned` | `skills.rs` | 2380--2413 |
| `skills_patch_missing_skill_returns_404` | `skills.rs` | 2415--2434 |
| `skills_patch_partial_update_only_changes_provided_fields` | `skills.rs` | 2436--2470 |

---

## 5. Migration Strategy

### Phase 1: Extract shared internals (safe, no test changes)

**Step 1a:** Create `commands.rs` with the command provider static + functions.

**Step 1b:** Replace the inline definitions in `mod.rs` with `pub mod commands;` and re-export.

**Step 1c:** Update all callers of `set_command_provider` (check `crate::handlers::set_command_provider`). Only the web crate's router builder calls this; verify with a grep.

**Step 1d:** Create `handler_shared.rs` with `pub(crate) fn setting_bool`.

**Step 1e:** Replace the inline `setting_bool` in `mod.rs` with `pub mod handler_shared;` and re-export.

**Testing:** `cargo build --workspace` should pass. The existing monolithic tests still work because the re-exports remain unchanged.

### Phase 2: Extract tests, one module at a time

This is the bulk of the work. Do **one module at a time** to minimize risk. For each module:

**Step 2.x:**
1. Append `#[cfg(test)] mod tests { ... }` to the handler file (e.g., `sessions.rs`)
2. Copy the test functions from `mod.rs`'s `tests` block into the new `mod tests` block in the target file
3. Adjust imports in the test block:
   - `use super::*;` — works for re-exports from the module itself
   - `use crate::handlers::test_support::*;` — needs this for test helpers
   - For cross-module references like `crate::handlers::files::file_hash(...)` — already use proper paths, no change needed
4. Remove the copied test functions from `mod.rs`
5. Run `cargo test -p allthecodes-web` to verify

**Priority order (recommended):**

| Order | Module | Tests | Risk | Reason |
|---|---|---|---|---|
| 1 | `channels.rs` | 1 test, 11 lines | Low | Minimal change, quick validation |
| 2 | `models.rs` | 1 test, 23 lines | Low | Simple test |
| 3 | `activity_recorder.rs` | 1 test, 26 lines | Low | Simple test |
| 4 | `computer_use.rs` | 1 test, 41 lines | Low | 501 stubs |
| 5 | `workspaces.rs` | 2 tests, 78 lines | Low | Straightforward |
| 6 | `sessions.rs` | 7 tests, ~226 lines | Medium | Most tests, but well-defined |
| 7 | `usage.rs` | 5 tests, 112 lines | Medium | Tests cross-crate types |
| 8 | `memory.rs` | 4 tests, 144 lines | Medium | References `MemoryStore`/`save_store` |
| 9 | `models.rs` (already done) | -- | -- | -- |
| 10 | `mcp_servers.rs` | 1 test, 114 lines | Medium | Complex CRUD workflow |
| 11 | `agents.rs` | 1 test, 101 lines | Medium | Complex CRUD |
| 12 | `people.rs` | 1 test, 86 lines | Medium | Complex CRUD |
| 13 | `hooks.rs` | 1 test, 71 lines | Medium | Complex CRUD |
| 14 | `prompts.rs` | 1 test, 70 lines | Medium | Complex CRUD |
| 15 | `plugins.rs` | 1 test, 68 lines | Medium | Complex CRUD |
| 16 | `skills.rs` | 10 tests, 451 lines | High | Many tests, file-system dependent |
| 17 | `files.rs` | 19 tests, 679 lines | Highest | Largest block, most complex |

### Phase 3: Cleanup mod.rs

After all tests are extracted:

**Step 3a:** Remove the empty `#[cfg(test)] mod tests { ... }` block from `mod.rs` (or keep it as a placeholder with a comment directing to module-level tests).

**Step 3b:** The final `mod.rs` will be approximately:
- 7 lines: module doc comment
- 3 lines: imports
- 40 lines: `pub mod` declarations
- 40 lines: `pub use` re-exports
- **Total: ~90 lines** (plus formatting)

### Phase 4 (optional): Verify test_support visibility

The `test_support.rs` file uses `pub(super)` visibility. When tests move to handler module files (e.g., `sessions.rs`), they access test_support as `super::test_support::make_web_state()` or `crate::handlers::test_support::make_web_state()`. The current `pub(super)` on test_support items is sufficient because:
- From a handler file like `sessions.rs`, `super` = `handlers` module
- `handlers` module contains `mod test_support`
- The items are `pub(super)` which means visible to the parent module (`handlers`) and all its children (which includes `sessions`)

**No visibility changes are required** for test_support items. They remain `pub(super)`.

---

## 6. Estimated Impact: Post-Refactor Line Counts

| File | Current lines | After refactor | Delta |
|---|---|---|---|
| `mod.rs` | 2471 | **~90** | -2381 |
| `commands.rs` | (new) | **~35** | +35 |
| `handler_shared.rs` | (new) | **~15** | +15 |
| `activity_recorder.rs` | 26 | **~60** | +34 |
| `agents.rs` | 115 | **~230** | +115 |
| `channels.rs` | 110 | **~130** | +20 |
| `computer_use.rs` | 80 | **~130** | +50 |
| `files.rs` | 870 | **~1580** | +710 |
| `hooks.rs` | 175 | **~260** | +85 |
| `mcp_servers.rs` | 260 | **~390** | +130 |
| `memory.rs` | 170 | **~340** | +170 |
| `models.rs` | 185 | **~220** | +35 |
| `people.rs` | 290 | **~390** | +100 |
| `plugins.rs` | 300 | **~380** | +80 |
| `prompts.rs` | 285 | **~370** | +85 |
| `sessions.rs` | 1125 | **~1380** | +255 |
| `skills.rs` | 510 | **~1000** | +490 |
| `usage.rs` | 780 | **~910** | +130 |
| `workspaces.rs` | 390 | **~480** | +90 |

**Key concerns:**
- `files.rs` will approach ~1580 lines, which pushes the threshold again. Consider whether the files handler module itself should be split further (e.g., `files/tree.rs`, `files/crud.rs`, `files/upload.rs`) if this becomes a problem.
- `sessions.rs` at ~1380 lines and `skills.rs` at ~1000 lines are still large but within acceptable bounds.
- `files.rs` at ~1580 lines will be the new largest file in the handlers directory. This is a **separate refactoring concern** and should be tracked as a follow-up.

---

## 7. Risk Assessment

### 7.1 Risks and Mitigations

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| **Import breakage** | High | Medium | Each extraction breaks the handler file's imports. Fix by cargo-checking after each extraction. Use `cargo test -p allthecodes-web` to validate. |
| **test_support visibility** | Low | High | Current `pub(super)` items are accessible from handler module tests via `super::test_support::*`. If a test fails with visibility errors, change test_support items to `pub(crate)` or adjust import paths. |
| **serial_test conflicts** | Low | Medium | The tests use `#[serial]` to avoid concurrency conflicts. This is preserved as-is in extracted tests. Ensure the serial_test dependency is in the handler modules' scope (it's already available through workspace deps). |
| **`file_hash` path breaks** | Low | Low | Test code in `files.rs` tests uses `crate::handlers::files::file_hash(...)` -- after moving to `files.rs`, change to `super::file_hash(...)` or keep the full path. |
| **`build_engine_for_session` path** | Low | Low | Test uses `crate::handlers::sessions::build_engine_for_session(...)` -- after moving to `sessions.rs`, keep the full path or use `super::build_engine_for_session(...)`. |
| **External callers of `mod.rs` items** | Low | Medium | The `pub use *` re-exports remain unchanged, so external callers (like `crate::web_state_routes`, `crate::handler_registry`) that reference `crate::handlers::function_name` continue to work. |
| **`crate::handlers::UsageQuery` style paths in tests** | Low | Low | After extraction, these paths still work since the items are re-exported from `mod.rs` via `pub use usage::*;`. No change needed. |

### 7.2 Rollback strategy

If an extraction breaks the build:
1. Revert the single changed file (`git checkout -- crates/allthecodes-web/src/handlers/<file>.rs`)
2. Restore the test functions in `mod.rs`
3. The module structure remains intact

Each extraction is independently revertible.

### 7.3 Verification checklist per extraction

- [ ] `cargo build --workspace` passes
- [ ] `cargo test -p allthecodes-web` passes
- [ ] The extracted test file compiles (`cargo test -p allthecodes-web -- <module_name>`)
- [ ] No duplicate test names (check with `cargo test -p allthecodes-web -- --list`)
- [ ] No dead code warnings (check `cargo clippy -p allthecodes-web`)
- [ ] No breaking changes to public API (the `pub use *` re-export is unchanged)

---

## 8. Estimated Effort

| Phase | Files touched | Estimated person-hours |
|---|---|---|
| Phase 1: Extract commands.rs + handler_shared.rs | 3 files (2 new + mod.rs) | 0.5 hr |
| Phase 2: Extract tests (17 modules, low/medium risk) | ~17 files | 3--4 hr |
| Phase 2: Extract tests (files.rs, high risk) | 1 file | 1 hr |
| Phase 2: Extract tests (skills.rs, high risk) | 1 file | 0.75 hr |
| Phase 3: Cleanup mod.rs | 1 file | 0.25 hr |
| **Total** | **~23 files** | **~5--6 hours** |

---

## 9. Future Considerations

1. **files.rs is the next candidate** -- at ~1580 lines post-refactor, it will exceed the threshold. Consider splitting it into `files/mod.rs` + `files/tree.rs` + `files/crud.rs` + `files/upload.rs` as a follow-up.

2. **sessions.rs at ~1380 lines** is large but acceptable. Consider splitting into `sessions/handlers.rs` and `sessions/processors.rs` if it continues growing.

3. **Adopt a convention**: new handler modules should always add tests inline (within their own file), not in `mod.rs`. This can be enforced via code review or a simple CI check (`test -f <file>.rs && grep -q 'mod tests' <file>.rs`).

4. **test_support.rs** could eventually be expanded with generic test fixtures (mock sessions, mock settings) that all handler tests share.
