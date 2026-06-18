# Refactoring Plan: `allthecodes-session/src/storage.rs`

> **Date:** 2026-06-16
> **File:** `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/crates/allthecodes-session/src/storage.rs`
> **Current size:** 2,756 lines (95204 bytes)
> **Threshold:** 1,500 lines

---

## 1. Summary: Current State

| Metric | Value |
|--------|-------|
| Total lines | 2,756 |
| Public structs | 5 (`SessionInfo`, `SessionListCursor`, `SessionListPage`, `SessionFile`, `SerializableMessage`) |
| Public enums | 0 (1 internal: `SessionLookup`) |
| Public consts | 1 (`MAX_CUSTOM_TITLE_LEN`) |
| Public functions | 16 |
| `pub(crate)` functions | 4 |
| Private functions | 22 |
| Inline `mod sqlite_store` | ~714 lines (lines 917–1631) |
| Test module | ~856 lines (lines 1904–2756) |
| Non-test, non-whitespace logic | ~1,750 lines |

The file is 1.8x over the 1,500-line threshold. The biggest contributors are the inline `sqlite_store` module (714 lines) and the test module (856 lines). The remaining logic (~1,186 lines of non-test code outside `sqlite_store`) is still unwieldy but far less urgent.

---

## 2. Breakdown: Major Sections

| # | Section | Lines | Count | Character |
|---|---------|-------|-------|-----------|
| 1 | Module doc + imports | 1–17 | 17 | boilerplate |
| 2 | **Types (structs)** | 18–107 | 90 | 5 structs |
| 3 | **Path helpers** | 109–205 | 97 | 4 pub fns + 4 private fns |
| 4 | **Title derivation** | 207–272 | 66 | 2 private fns |
| 5 | **Session info builder / filtering** | 274–313 | 40 | 2 private fns |
| 6 | **Persistence operations (JSON)** | 315–915 | 601 | 10 pub fns + 12 private fns |
| 7 | **Inline `mod sqlite_store`** | 917–1631 | 715 | 5 pub(super) fns + 8 private/async fns + 2 helpers + 4 migrations |
| 8 | **Serialization helpers** | 1633–1898 | 266 | 2 pub-scope fns + 4 private helpers |
| 9 | **Tests** | 1900–2756 | 857 | 1 mod with 20+ test fns |

---

## 3. Problems

### 3.1 God functions

**`sqlite_store::save_session_file()` (lines 983–1087, 105 lines)**
- Opens a transaction, re-computes workspace metadata, runs an upsert, deletes old messages, re-inserts all messages.
- 70% structurally identical to `save_session_file_to_pool()` (lines 1336–1418, 83 lines), differing only in the `ON CONFLICT` clause (`DO UPDATE` vs `DO NOTHING`).

**`sqlite_store::list_session_page_from_pool()` (lines 1420–1570, 151 lines)**
- A 4-arm `match` block with massive duplicated SQL strings (the same 11-column SELECT repeated in every arm, with different WHERE clauses).
- Each arm differs by 2–4 lines of WHERE conditions, but carries ~25 lines of copied SQL boilerplate.

**`serializable_to_messages()` (lines 1736–1829, 94 lines)**
- Large match on message type with deep nested data extraction. Could be split by message variant.

### 3.2 Code duplication

1. **`save_session_file()` vs `save_session_file_to_pool()`** — Same INSERT pattern, same workspace key computation (`workspace_root`, `workspace_key`, `workspace_name`, `display_title`), same message loop. The only difference is `ON CONFLICT ... DO UPDATE` vs `ON CONFLICT ... DO NOTHING`.

2. **SQL queries in `list_session_page_from_pool()`** — The same 11-column SELECT (`session_id`, `created_at`, `last_modified`, `message_count`, `cwd`, `title`, `custom_title`, `chat_mode_override`, `workspace_key`, `workspace_root`, `workspace_name`) repeated 4 times. Only the WHERE clause varies.

3. **`workspace_key()` computation** — Called in `build_session_info()`, `save_session_file()`, `save_session_file_to_pool()`, `list_workspace_session_page()`, `session_info_from_row()`. The computation (git common dir → normalize) is repeated.

### 3.3 Mixed concerns in one file

The file conflates:
- **Data model definitions** (structs + serialization)
- **Path/workspace utilities** (path normalization, git root detection)
- **JSON file persistence** (read/write/list/archive/truncate JSON files)
- **SQLite persistence** (the entire `sqlite_store` inline module)
- **Data serialization** (Message ↔ SerializableMessage conversion)
- **Tests** (856 lines)

### 3.4 Public API surface vs internal helpers

Items that are `pub` but should arguably be `pub(crate)`:
- `SessionListCursor`, `SessionListPage` — only used by web handlers and crate-internal pagination; no external consumer inspects them directly.
- `SerializableMessage` — used externally by `allthecodes-commands/src/insights.rs` and `allthecodes-web/src/handlers/usage.rs` (raw session file access). This is a legitimate public type.
- `session_info_from_row()` is `pub(super)` within `sqlite_store` — correct.

`pub(crate)` items are well-chosen (the `*_in_file` variants). No change needed here, but the boundary should be explicitly documented.

### 3.5 Unnecessary recomputation

`session_info_from_row()` (lines 1572–1605) always recomputes `workspace_root`, `workspace_key`, and `workspace_name` from `cwd`, even when the database already stores valid values. The computed values only serve as fallback for empty columns. This is defensible but wasteful — all recent saves populate these columns.

---

## 4. Proposed Split

The goal is to extract `storage.rs` into a `storage/` directory module with the following files:

```
src/storage/
  mod.rs              — Re-exports; shared imports; top-level API dispatch
  types.rs            — All data structs (SessionInfo, SessionListCursor,
                        SessionListPage, SessionFile, SerializableMessage)
  paths.rs            — Path helpers (get_session_dir, get_session_file, etc.)
                        + workspace utilities (workspace_key, workspace_root,
                        workspace_name, git_common_dir, normalize_*)
  title.rs            — Title derivation (derive_title, extract_user_text)
  json_store.rs        — JSON file persistence (save/load/list/archive/truncate)
                        + in-memory pagination helpers
  sqlite_store.rs      — SQLite persistence (extracted from inline mod)
  serialize.rs         — Message ↔ SerializableMessage conversion
  helpers.rs           — Shared utility functions used by json_store and sqlite_store
                        (build_session_info, filter_sessions_for_workspace,
                         sort_session_infos, display_title, session_info_from_row)
```

### 4.1 File-by-file mapping

| New file | Extracted from | Approx lines | Description |
|----------|---------------|-------------|-------------|
| `mod.rs` | N/A (new) | ~40-60 | Re-exports all public API; keeps `use` statements DRY |
| `types.rs` | Lines 18–107 | ~90 | 5 structs + doc comments unchanged |
| `paths.rs` | Lines 109–205 + lines 174–205 | ~100 | 4 pub fns + 4 private fns (git/normalize helpers) |
| `title.rs` | Lines 207–272 | ~70 | `derive_title()`, `extract_user_text()` |
| `json_store.rs` | Lines 315–915 (minus helpers used by both) | ~500 | All JSON persistence: save, load, list, archive, truncate, pagination |
| `sqlite_store.rs` | Lines 917–1631 | ~720 | Full `sqlite_store` module (MIGRATIONS, SessionIndex, SessionLookup, all DB ops) |
| `serialize.rs` | Lines 1633–1898 | ~270 | `messages_to_serializable()`, `serializable_to_messages()`, sub-helpers |
| `helpers.rs` | Shared between json_store and sqlite_store | ~50 | `build_session_info()`, `filter_sessions_for_workspace()`, `session_info_from_row()`, `display_title()` |

### 4.2 Detailed module structure

#### `storage/mod.rs`

```rust
//! Session storage — persisting conversation state to SQLite and JSON.
//!
//! New writes prefer the shared SQLite state database while retaining JSON
//! files under `~/.allthecodes/sessions/` for migration compatibility and
//! file-based tooling.

mod helpers;
mod json_store;
mod paths;
mod serialize;
mod sqlite_store;
mod title;
mod types;

pub use types::{SerializableMessage, SessionFile, SessionInfo, SessionListCursor, SessionListPage};
pub use paths::{get_archived_session_dir, get_archived_session_file, get_session_dir,
                get_session_file, workspace_key, workspace_name, workspace_root};
pub use json_store::{
    archive_session, list_sessions, list_sessions_page, list_workspace_sessions,
    list_workspace_sessions_page, load_session, load_session_info, save_session,
    set_session_chat_mode_override, set_session_title, truncate_session,
};
pub use json_store::MAX_CUSTOM_TITLE_LEN;
```

#### `storage/helpers.rs`

Extract:
- `build_session_info()` (line 274)
- `filter_sessions_for_workspace()` (line 301)
- `sort_session_infos()` (line 807)
- `clamp_session_page_limit()` (line 816)
- `cursor_for_session()` (line 820)
- `session_is_after_cursor()` (line 828)
- `page_sessions_in_memory()` (line 836)
- `is_default_session_path()` (line 887)

These are shared between `json_store.rs` and `sqlite_store.rs`.

#### `storage/sqlite_store.rs` (extract from inline module)

The existing `sqlite_store` module (lines 917–1631) moves verbatim but becomes a non-inline module. Key changes:
- `pub(super)` → `pub(crate)` since it's now a sibling module, not an inline child.
- Remove `use super::*` — import specific items from `crate::storage::*`.
- The `MIGRATIONS` constant could be extracted to a separate `migrations.rs` under `storage/`, but since it's small (4 migrations, ~50 lines), it's fine to keep it in `sqlite_store.rs` for now.

The god functions within this module can be tackled later (Step 2), but the extraction itself is Step 1.

#### `storage/json_store.rs`

Contains all the JSON-specific persistence logic. The `*-in-file` `pub(crate)` variants stay here. The public-facing functions that dispatch to JSON and optionally SQLite (`save_session`, `load_session`, `list_sessions`, etc.) remain here, calling into `sqlite_store` when the feature is enabled.

### 4.3 File tree after refactoring

```
src/
  lib.rs                              — unchanged: `pub mod storage;` → `pub mod storage;`
  storage.rs                          — deleted
  storage/
    mod.rs                            — new: re-exports
    types.rs                          — new
    paths.rs                          — new
    title.rs                          — new
    helpers.rs                        — new
    json_store.rs                     — new
    sqlite_store.rs                   — new
    serialize.rs                      — new
  audit_export.rs                     — unchanged
  export.rs                           — unchanged
  fork.rs                             — unchanged
  memdir/                             — unchanged
  migrations.rs                       — unchanged
  request_snapshot.rs                 — unchanged
  resume.rs                           — unchanged
  session_export/                     — unchanged
  transcript.rs                       — unchanged
```

---

## 5. Migration Strategy

### Step 1: Structural extraction (non-breaking, 1-2 hours)

**Goal:** Split into files without changing any logic.

1. Create `src/storage/` directory.
2. Create `src/storage/types.rs` — move structs (lines 18–107), add `use` imports.
3. Create `src/storage/paths.rs` — move path/workspace functions (lines 109–205), add `use` imports.
4. Create `src/storage/title.rs` — move `derive_title()` + `extract_user_text()` (lines 207–272).
5. Create `src/storage/helpers.rs` — move shared helpers (build_session_info, filter_sessions_for_workspace, sort_session_infos, clamp_session_page_limit, cursor_for_session, session_is_after_cursor, page_sessions_in_memory, is_default_session_path).
6. Create `src/storage/serialize.rs` — move serialization functions (lines 1633–1898).
7. Create `src/storage/json_store.rs` — move persistence operations (lines 315–915), including the `*_in_file` variants that call SQLite.
   - Remove the inline `sqlite_store` module (it was at lines 917–1631, before serialization).
   - Add `use crate::storage::sqlite_store;` at the top.
8. Create `src/storage/sqlite_store.rs` — move the `sqlite_store` module contents (lines 917–1631).
   - Change from `mod sqlite_store { ... }` to a standalone file.
   - Convert `pub(super)` visibility to `pub(crate)`.
   - Replace `use super::*` with specific imports.
9. Create `src/storage/mod.rs` — re-export everything that was previously `pub` from the flat module.
10. Delete `src/storage.rs`.
11. Update `src/lib.rs`: `pub mod storage;` stays the same (it was already `pub mod storage;`). The module type changes from a file to a directory, which Rust handles transparently.

**Verification:** `cargo test -p allthecodes-session` passes. No import changes needed in dependent crates because all `pub` items are re-exported at the same path.

### Step 2: God-function refactoring (safe, 2-4 hours)

**Goal:** Decompose the 4 largest functions identified in section 3.1.

#### 2a. `save_session_file()` and `save_session_file_to_pool()` → deduplicate

Extract a shared core that accepts a closure for the ON CONFLICT policy:

```rust
// In sqlite_store.rs
async fn upsert_session_row(
    tx: &mut Transaction<'_, SqliteConnection>,
    file: &SessionFile,
    on_conflict: OnConflictPolicy,
) -> Result<()> { ... }

enum OnConflictPolicy { DoUpdate, DoNothing }
```

The two existing functions become thin wrappers.

#### 2b. `list_session_page_from_pool()` → deduplicate SQL

Build the query dynamically using a `QueryBuilder` pattern or a helper that assembles the WHERE clause:

```rust
async fn list_session_page_from_pool(
    pool: &SqlitePool,
    limit: usize,
    cursor: Option<SessionListCursor>,
    workspace_key_filter: Option<String>,
) -> Result<SessionListPage> {
    let mut builder = QueryBuilder::new(
        "SELECT session_id, created_at, last_modified, ... FROM sessions WHERE archived = 0"
    );
    if let Some(ref wk) = workspace_key_filter {
        builder.push(" AND workspace_key = ");
        builder.push_bind(wk);
    }
    if let Some(ref c) = cursor {
        builder.push(" AND (last_modified < ? OR ...)");
        // bind cursor fields
    }
    builder.push(" ORDER BY ... LIMIT ");
    builder.push_bind(limit + 1);
    // execute, deduplicate
}
```

This eliminates 3 of the 4 query arms.

#### 2c. `serializable_to_messages()` → split by variant

Extract per-variant deserializers:

```rust
fn deserialize_user_message(sm: &SerializableMessage) -> Message { ... }
fn deserialize_assistant_message(sm: &SerializableMessage) -> Message { ... }
fn deserialize_system_message(sm: &SerializableMessage) -> Message { ... }
```

The main function becomes a clean match dispatching to these.

### Step 3: Visibility tightening (safe, 30 min)

After splitting, review every `pub` item and determine if it's consumed outside the crate:

| Item | Currently | Should be | Rationale |
|------|-----------|-----------|-----------|
| `SessionListCursor` | `pub` | Keep `pub` — returned by pagination API |
| `SessionListPage` | `pub` | Keep `pub` — returned by pagination API |
| `SessionInfo` | `pub` | Keep `pub` — widely used externally |
| `SessionFile` | `pub` | Keep `pub` — used by `allthecodes-web` and `allthecodes-commands` |
| `SerializableMessage` | `pub` | Keep `pub` — used by `allthecodes-commands` |
| `MAX_CUSTOM_TITLE_LEN` | `pub` | Keep `pub` — documented public constant |
| `save_session_to_file` | `pub(crate)` | Keep `pub(crate)` — only used within crate |
| `set_session_title_in_file` | `pub(crate)` | Keep `pub(crate)` |
| `set_session_chat_mode_override_in_file` | `pub(crate)` | Keep `pub(crate)` |
| `load_session_info_from_file` | `pub(crate)` | Keep `pub(crate)` |
| `sqlite_store` internals | `pub(super)` → `pub(crate)` | Planned in Step 1 |

No visibility changes needed for external consumers.

### Step 4: Optional cleanups (risk: low, 1-2 hours)

- **`session_info_from_row()`** — skip redundant workspace recomputation when DB columns are non-empty (minor optimization).
- **Move test module** — extract `src/storage/tests.rs` or keep tests per-module. Given the current test structure (all tests in one inline module), the simplest approach is to move them to `src/storage/tests.rs` that imports all sub-modules. Alternatively, keep the flat `tests` module in `storage.rs` (reduced to re-exports). The latter is simpler for Step 1.
- **SQL query deduplication** — as described in Step 2b.

---

## 6. Estimated Impact: Post-Refactor Line Counts

| File | Before | After | Reduction |
|------|--------|-------|-----------|
| `src/storage.rs` | 2,756 | — | -2,756 |
| `src/storage/mod.rs` | — | ~50 | +50 |
| `src/storage/types.rs` | — | ~90 | +90 |
| `src/storage/paths.rs` | — | ~100 | +100 |
| `src/storage/title.rs` | — | ~70 | +70 |
| `src/storage/helpers.rs` | — | ~50 | +50 |
| `src/storage/json_store.rs` | — | ~500 | +500 |
| `src/storage/sqlite_store.rs` | — | ~720 | +720 |
| `src/storage/serialize.rs` | — | ~270 | +270 |
| **Total (logic)** | **~1,900** | **~1,850** | **~-50** (no logic change) |
| **Tests** (in `tests.rs` or per-file) | **856** | **~856** | **0** |

After Step 1, the largest file is `sqlite_store.rs` at ~720 lines — well under the 1,500 threshold. After Step 2, the largest god functions are decomposed.

---

## 7. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| **Import breakage** — a `use` path changes and external crates fail to compile | Medium | High | Use `cargo check --workspace` after extraction. All re-exports preserve the exact `allthecodes_session::storage::*` paths. |
| **Circular dependencies** — sub-modules import each other | Low | Medium | The dependency graph is acyclic: `types → paths → title → helpers → json_store/sqlite_store/serialize → mod`. No cycles possible. |
| **Test module import errors** — tests reference `super::*` which no longer works | Medium | Low | Tests in `storage/` would use `crate::storage::*`. If tests stay in a top-level `tests` module under `storage`, they import from `super::*` still works. Best to put tests in `src/storage/tests.rs` and use `crate::storage::*`. |
| **`pub(super)` visibility in sqlite_store breaks** — was `pub(super)` within the flat module, now `pub(crate)` | Medium | Low | The only callers of `pub(super)` functions were within `storage.rs` itself. After extraction, `pub(crate)` works identically. |
| **Feature-gated code (`sqlite-storage`)** — conditional compilation breaks with sub-modules | Low | Medium | The feature gate `#[cfg(feature = "sqlite-storage")]` must be moved to `mod sqlite_store` declaration in `mod.rs`. The functions in `json_store.rs` that call into `sqlite_store` need conditional compilation too. This is already handled with `#[cfg(feature = "sqlite-storage")]` inline — no change needed. |
| **Serialization helpers used by both json_store and sqlite_store** | Low | Low | Both `json_store.rs` and `sqlite_store.rs` import `serialize.rs`. This is clean. |
| **`SessionFile` is used in `allthecodes-web` for direct JSON access** | Low | Low | `SessionFile` stays `pub` and is re-exported at the same path. |

### Rollback plan

If the extraction breaks something:
1. Keep `storage.rs` in place and gate the sub-module approach behind a feature flag.
2. More simply: `git checkout -- crates/allthecodes-session/src/` restores the original file.

---

## 8. Summary of Action Items (ordered)

| Step | Action | Files touched | Est. time |
|------|--------|---------------|-----------|
| 1a | Create `src/storage/` + `mod.rs` | New | 10 min |
| 1b | Extract `types.rs` | New + `mod.rs` | 15 min |
| 1c | Extract `paths.rs` | New + `mod.rs` | 15 min |
| 1d | Extract `title.rs` | New + `mod.rs` | 10 min |
| 1e | Extract `helpers.rs` | New + `mod.rs` | 15 min |
| 1f | Extract `serialize.rs` | New + `mod.rs` | 20 min |
| 1g | Extract `json_store.rs` | New + `mod.rs` | 30 min |
| 1h | Extract `sqlite_store.rs` | New + `mod.rs` | 30 min |
| 1i | Delete `storage.rs`, update `lib.rs` | `storage.rs`, `lib.rs` | 5 min |
| 1j | `cargo test -p allthecodes-session` + `cargo check --workspace` | — | 5 min |
| 2a | Deduplicate `save_session_file` / `save_session_file_to_pool` | `sqlite_store.rs` | 45 min |
| 2b | Deduplicate `list_session_page_from_pool` SQL | `sqlite_store.rs` | 60 min |
| 2c | Split `serializable_to_messages` by variant | `serialize.rs` | 30 min |
| 3 | Visibility review | All files | 15 min |
| 4 | Extract tests to `src/storage/tests.rs` | New + `mod.rs` | 20 min |

**Total estimated effort:** ~5-6 hours for a single engineer.
