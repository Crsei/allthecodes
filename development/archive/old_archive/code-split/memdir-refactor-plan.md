# memdir.rs 拆分计划

> 原文件: `crates/cc-session/src/memdir.rs`
> 原始行数: ~1612
> 目标: 拆分为 5 个子模块，每个 ≤ 400 行

## 当前结构分析

The file implements a CLAUDE.md-based memory directory system with six distinct logical sections:

1. **Type definitions** (L9-L176): `MemoryEntry`, `RelevantMemory`, `MemoryType`, `MemoryScope` structs/enums and their impls. Two constants at module level, plus `MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT` and `MODEL_ASSISTED_RECALL_MAX_RESULTS`.

2. **Path helpers and index formatting** (L178-L340): `memory_dir`, `ensure_memory_dir`, `key_to_filename`, `memory_entrypoint_path`, `one_line_hook`, `truncate_chars`, `escape_markdown_link_text`, `memory_index_line`, `truncate_memory_index_content`, `append_memory_index_warning`, `build_memory_index_from_entries`, `build_memory_index`.

3. **Index persistence and memory context section formatting** (L342-L404): `read_memory_index`, `refresh_memory_index`, `format_memory_context_section`, `memory_identity`, `query_requests_memory_ignore`.

4. **Recall system** (L416-L828): `recall_scopes`, `tokenize_for_recall`, `entry_recall_text`, `score_memory_for_query`, `is_generic_recent_tool_memory`, `recall_relevant_memories`, `format_relevant_memory_context`, `build_model_assisted_recall_prompt`, `parse_model_assisted_recall_selection`, `select_relevant_memories_by_identity`, `parse_identity_candidates_from_response`, `identity_candidates_from_json`, `truncate_for_model_recall`, `build_relevant_memory_context_with`.

5. **CRUD operations** (L832-L957): `write_memory`, `read_memory`, `delete_memory`, `list_memories`, `search_memories`.

6. **Context injection** (L959-L1045): `build_memory_context`, `build_memory_context_with`.

7. **Tests** (L1049-L1612): Comprehensive test module covering CRUD, index generation, recall scoring, model-assisted recall, scope resolution, and legacy data compatibility.

External callers (identified via `grep`):
- `crates/cc-commands/src/memory.rs`: imports `MemoryEntry`, `MemoryScope`, and calls CRUD functions plus `search_memories`
- `crates/cc-commands/src/memory/selector.rs`: calls `memdir::list_memories`
- `crates/cc-engine/src/system_prompt.rs`: calls `build_memory_context_with`, `write_memory`
- `crates/cc-engine/src/lifecycle/submit_message.rs`: calls recall functions (`recall_relevant_memories`, `build_model_assisted_recall_prompt`, `parse_model_assisted_recall_selection`, `select_relevant_memories_by_identity`, `format_relevant_memory_context`, `query_requests_memory_ignore`, `build_relevant_memory_context_with`) and reads `MODEL_ASSISTED_RECALL_*` constants
- `crates/claude-code-rs/tests/e2e_memory_scopes.rs`: integration tests

All callers import through `cc_session::memdir`, so the re-export structure in `mod.rs` is critical for preserving the public API.

## 拆分方案

### 子模块 1: `types.rs` (~185 行)

- **职责**: All shared data structures, enums, constants, and their method implementations. This is the foundation module with zero internal dependencies.
- **迁移内容**:
  - `pub const MEMORY_ENTRYPOINT_NAME: &str` (L16)
  - `pub const MEMORY_ENTRYPOINT_MAX_LINES: usize` (L17)
  - `pub const MEMORY_ENTRYPOINT_MAX_BYTES: usize` (L18)
  - `pub const MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT: usize` (L66)
  - `pub const MODEL_ASSISTED_RECALL_MAX_RESULTS: usize` (L67)
  - `pub struct MemoryEntry` (L27-L55)
  - `pub struct RelevantMemory` (L57-L64)
  - `impl MemoryEntry` -- `effective_memory_type` and `display_label` (L69-L84)
  - `pub enum MemoryType` and its impl (L87-L137)
  - `pub enum MemoryScope` and its impl (L140-L176)
- **依赖**: `std::path::Path`, `serde`, `serde_json` (none)
- **被依赖**: Every other sub-module (`index.rs`, `recall.rs`, `context.rs`, `crud.rs`, and tests) depends on types. External callers (`cc-commands`, `cc-engine`) import `MemoryEntry`, `MemoryScope`, `MemoryType`, `RelevantMemory`, and the two `MODEL_ASSISTED_RECALL_*` constants.

### 子模块 2: `index.rs` (~250 行)

- **职责**: Filesystem path resolution, filename sanitization, MEMORY.md index file building/reading/refreshing, and memory-context section formatting.
- **迁移内容**:
  - `const MEMORY_INDEX_HOOK_MAX_CHARS: usize` (L19) -- moved here since its only consumer `one_line_hook` is here
  - `pub fn memory_dir` (L183-L190)
  - `fn ensure_memory_dir` (L193-L200)
  - `fn key_to_filename` (L203-L215)
  - `fn memory_entrypoint_path` (L217-L219)
  - `fn one_line_hook` (L221-L228)
  - `fn truncate_chars` (L230-L240)
  - `fn escape_markdown_link_text` (L242-L248)
  - `fn memory_index_line` (L249-L259)
  - `fn truncate_memory_index_content` (L261-L290)
  - `fn append_memory_index_warning` (L292-L326)
  - `fn build_memory_index_from_entries` (L328-L335)
  - `pub fn build_memory_index` (L337-L340)
  - `pub fn read_memory_index` (L342-L351)
  - `pub fn refresh_memory_index` (L353-L370)
  - `fn format_memory_context_section` (L372-L400)
  - `fn memory_identity` (L402-L404)
  - `pub fn query_requests_memory_ignore` (L406-L414)
  - `pub fn list_memories` (L907-L939) -- moved here to avoid circular dependency with crud.rs
- **依赖**: `super::types` (`MemoryEntry`, `MemoryScope`, `MEMORY_ENTRYPOINT_NAME`, `MEMORY_ENTRYPOINT_MAX_LINES`, `MEMORY_ENTRYPOINT_MAX_BYTES`), `cc_config::paths`, `anyhow`, `std::fs`, `std::path`
- **被依赖**: `crud.rs` (calls `ensure_memory_dir`, `key_to_filename`, `refresh_memory_index`), `recall.rs` (calls `format_memory_context_section`, `memory_identity`, `list_memories`), `context.rs` (calls `read_memory_index`, `build_memory_index_from_entries`, `list_memories`). External callers via `mod.rs` re-exports: `memory_dir`, `build_memory_index`, `read_memory_index`, `refresh_memory_index`, `query_requests_memory_ignore`.

### 子模块 3: `recall.rs` (~290 行)

- **职责**: Deterministic memory recall (tokenization, scoring, filtering), relevant-memory formatting, model-assisted recall prompt construction and response parsing, and the `build_relevant_memory_context_with` orchestrator.
- **迁移内容**:
  - `fn recall_scopes` (L416-L425)
  - `fn tokenize_for_recall` (L427-L449)
  - `fn entry_recall_text` (L451-L466)
  - `fn score_memory_for_query` (L468-L525)
  - `fn is_generic_recent_tool_memory` (L527-L566)
  - `pub fn recall_relevant_memories` (L568-L617)
  - `pub fn format_relevant_memory_context` (L619-L658)
  - `pub fn build_model_assisted_recall_prompt` (L660-L698)
  - `pub fn parse_model_assisted_recall_selection` (L700-L738)
  - `pub fn select_relevant_memories_by_identity` (L740-L763)
  - `fn parse_identity_candidates_from_response` (L765-L779)
  - `fn identity_candidates_from_json` (L781-L794)
  - `fn truncate_for_model_recall` (L796-L804)
  - `pub fn build_relevant_memory_context_with` (L807-L828)
- **依赖**: `super::types` (`MemoryType`, `MemoryScope`, `MemoryEntry`, `RelevantMemory`, `MODEL_ASSISTED_RECALL_*`), `super::index` (`memory_identity`, `format_memory_context_section`, `list_memories`, `query_requests_memory_ignore`), `cc_config::features`, `serde_json`, `std::collections::HashSet`, `anyhow`
- **被依赖**: `mod.rs` re-exports all public functions. External callers: `cc-engine/src/lifecycle/submit_message.rs` uses nearly all public functions from this module.

### 子模块 4: `crud.rs` (~130 行)

- **职责**: Core CRUD operations (write, read, delete, search) for memory entries, plus context injection assembly.
- **迁移内容**:
  - `pub fn write_memory` (L835-L876)
  - `pub fn read_memory` (L879-L888)
  - `pub fn delete_memory` (L891-L904)
  - `pub fn search_memories` (L942-L957)
  - `pub fn build_memory_context` (L968-L969)
  - `pub fn build_memory_context_with` (L980-L1045)
- **依赖**: `super::types` (`MemoryEntry`, `MemoryType`, `MemoryScope`), `super::index` (`ensure_memory_dir`, `key_to_filename`, `memory_dir`, `list_memories`, `refresh_memory_index`, `format_memory_context_section`), `chrono::Utc`, `serde_json`, `cc_config::features`, `anyhow`
- **被依赖**: `mod.rs` re-exports all public functions. External callers: `cc-commands/src/memory.rs` (`write_memory`, `read_memory`, `delete_memory`, `search_memories`), `cc-engine/src/system_prompt.rs` (`build_memory_context_with`, `write_memory`).

### 子模块 5: `mod.rs` (~750 行, 含测试 ~563 行)

- **职责**: Module declarations, public re-exports preserving the original `cc_session::memdir::*` API, and the test module.
- **迁移内容**:
  - Module-level doc comment (L1-L7)
  - `mod types; mod index; mod recall; mod crud;`
  - `pub use types::*;`
  - `pub use index::{memory_dir, build_memory_index, read_memory_index, refresh_memory_index, list_memories, query_requests_memory_ignore};`
  - `pub use recall::{recall_relevant_memories, format_relevant_memory_context, build_model_assisted_recall_prompt, parse_model_assisted_recall_selection, select_relevant_memories_by_identity, build_relevant_memory_context_with};`
  - `pub use crud::{write_memory, read_memory, delete_memory, search_memories, build_memory_context, build_memory_context_with};`
  - `#[cfg(test)] mod tests { ... }` (L1049-L1612, ~563 lines)

  The test module uses `super::*` to access everything via re-exports, so it should work unchanged.

- **依赖**: All sub-modules
- **被依赖**: External crates import via `cc_session::memdir`

## 拆分后目录结构

```
memdir/
├── mod.rs          (~750 行) -- re-exports + tests (tests alone are ~563 lines)
├── types.rs        (~185 行) -- MemoryEntry, RelevantMemory, MemoryType, MemoryScope, constants
├── index.rs        (~250 行) -- path helpers, index build/read/refresh, list_memories, format helpers
├── recall.rs       (~290 行) -- scoring, recall, model-assisted recall, formatting
└── crud.rs         (~130 行) -- write/read/delete/search, context injection assembly
```

Note: The test module alone is 563 lines. If desired, tests can be further split into `tests/` subdirectory files, but this is a lower-priority follow-up since test code does not affect compilation or runtime behavior. The `mod.rs` file will be dominated by tests, which is acceptable -- it keeps all test infrastructure (`make_temp_dir`, `cleanup`) in one place.

## 迁移步骤

### Step 1: Create `types.rs` and update `mod.rs`

1. Create `crates/cc-session/src/memdir/types.rs`.
2. Move into it:
   - All imports needed: `serde::{Deserialize, Serialize}`, `std::path::Path`
   - `MEMORY_ENTRYPOINT_NAME`, `MEMORY_ENTRYPOINT_MAX_LINES`, `MEMORY_ENTRYPOINT_MAX_BYTES` (L16-L18)
   - `MODEL_ASSISTED_RECALL_CANDIDATE_LIMIT`, `MODEL_ASSISTED_RECALL_MAX_RESULTS` (L66-L67)
   - `MemoryEntry` struct (L26-L55) and its impl (L69-L84)
   - `RelevantMemory` struct (L57-L64)
   - `MemoryType` enum and impl (L87-L137)
   - `MemoryScope` enum and impl (L140-L176)
3. Convert `memdir.rs` into `memdir/mod.rs` with `mod types; pub use types::*;` and keep everything else in `mod.rs` for now.
4. Verify `cargo check -p cc-session` compiles.

### Step 2: Extract `index.rs`

1. Create `crates/cc-session/src/memdir/index.rs`.
2. Move into it:
   - `MEMORY_INDEX_HOOK_MAX_CHARS` (L19) -- stays private here since only used by `one_line_hook`
   - All functions from "Path helpers" through "refresh_memory_index": `memory_dir`, `ensure_memory_dir`, `key_to_filename`, `memory_entrypoint_path`, `one_line_hook`, `truncate_chars`, `escape_markdown_link_text`, `memory_index_line`, `truncate_memory_index_content`, `append_memory_index_warning`, `build_memory_index_from_entries`, `build_memory_index`, `read_memory_index`, `refresh_memory_index`
   - Also move `list_memories` (from CRUD section, L907-L939) since `build_memory_index` and `refresh_memory_index` call it
   - `format_memory_context_section` (L372-L400)
   - `memory_identity` (L402-L404)
   - `query_requests_memory_ignore` (L406-L414)
3. Add `use super::types::*;` and necessary external imports.
4. Update `mod.rs` to add `mod index;` and appropriate `pub use index::*;` re-exports.
5. Verify `cargo check -p cc-session`.

### Step 3: Extract `recall.rs`

1. Create `crates/cc-session/src/memdir/recall.rs`.
2. Move into it:
   - `recall_scopes` (L416-L425)
   - `tokenize_for_recall` (L427-L449)
   - `entry_recall_text` (L451-L466)
   - `score_memory_for_query` (L468-L525)
   - `is_generic_recent_tool_memory` (L527-L566)
   - `recall_relevant_memories` (L568-L617)
   - `format_relevant_memory_context` (L619-L658)
   - `build_model_assisted_recall_prompt` (L660-L698)
   - `parse_model_assisted_recall_selection` (L700-L738)
   - `select_relevant_memories_by_identity` (L740-L763)
   - `parse_identity_candidates_from_response` (L765-L779)
   - `identity_candidates_from_json` (L781-L794)
   - `truncate_for_model_recall` (L796-L804)
   - `build_relevant_memory_context_with` (L807-L828)
3. Add `use super::types::*; use super::index::{list_memories, memory_identity, format_memory_context_section, query_requests_memory_ignore};` and external imports.
4. Update `mod.rs` to add `mod recall;` and `pub use recall::*;`.
5. Verify `cargo check -p cc-session`.

### Step 4: Extract `crud.rs`

1. Create `crates/cc-session/src/memdir/crud.rs`.
2. Move into it:
   - `pub fn write_memory` (L835-L876)
   - `pub fn read_memory` (L879-L888)
   - `pub fn delete_memory` (L891-L904)
   - `pub fn search_memories` (L942-L957)
   - `pub fn build_memory_context` (L968-L969)
   - `pub fn build_memory_context_with` (L980-L1045)
3. Add `use super::types::*; use super::index::{ensure_memory_dir, key_to_filename, memory_dir, list_memories, refresh_memory_index, format_memory_context_section};` and external imports.
4. Update `mod.rs` to add `mod crud;` and `pub use crud::*;`.
5. Verify `cargo check -p cc-session`.

### Step 5: Clean up `mod.rs`

1. `mod.rs` should now contain only:
   - Module-level doc comment (L1-L7)
   - `mod types; mod index; mod recall; mod crud;`
   - `pub use types::*;`
   - `pub use index::{...};` (explicit re-exports of public items)
   - `pub use recall::{...};`
   - `pub use crud::{...};`
   - `#[cfg(test)] mod tests { ... }` (the full test block, which uses `super::*`)
2. Verify the public API surface is identical by running `cargo check -p cc-session -p cc-commands -p cc-engine`.
3. Verify all tests pass: `cargo test -p cc-session`.

## 风险与注意事项

1. **`list_memories` placement**: This function is conceptually a CRUD operation but is called by `build_memory_index`, `refresh_memory_index` (index module), `recall_relevant_memories` (recall module), and `build_memory_context_with` (crud module). Placing it in `index.rs` avoids circular dependencies between `index.rs` and `crud.rs`.

2. **`format_memory_context_section` visibility**: Currently `fn` (private). After extraction, it must become `pub(super)` so that `crud.rs` can call it.

3. **`memory_identity` and `query_requests_memory_ignore` visibility**: Both are currently `fn` (private). They need `pub(super)` since `recall.rs` uses them. `query_requests_memory_ignore` is also called externally (from `cc-engine`), so it must be `pub` and re-exported from `mod.rs`.

4. **`display_label` method**: This is a private method on `MemoryEntry`. Since it lives in `types.rs`, it must become `pub(super)` because `index.rs` and `recall.rs` call it.

5. **Test module uses `super::*`**: Since all public items will be re-exported from `mod.rs`, the test module should work unchanged. However, 3 private functions need `pub(super)` for test access: `key_to_filename`, `memory_entrypoint_path`, `build_memory_index_from_entries`.

6. **Circular dependency prevention**: The dependency graph must be acyclic: `types.rs` has no internal dependencies; `index.rs` depends on `types.rs`; `recall.rs` depends on `types.rs` and `index.rs`; `crud.rs` depends on `types.rs` and `index.rs`. No module depends on `recall.rs` or `crud.rs` from within the `memdir` module.
