# Workspace Split Design — cc-rust

**Status**: draft — tracking issue TBD
**Owner**: TBD
**Date**: 2026-04-20

## Motivation

Today the Rust source is one crate (`claude-code-rs`) containing **356 files / ~92k LOC / 518 transitive deps**. Any edit to any file triggers a full-crate re-`codegen` because Rust compiles at crate granularity. Incremental `cargo build` after touching `src/main.rs` takes ~7.5s (after the rust-lld linker switch) — the re-codegen cost dominates.

Splitting into a Cargo workspace gives:

1. **Per-crate incremental rebuild** — editing `tools/file_read.rs` only rebuilds `cc-tools` + the root bin.
2. **Parallel codegen** — independent crates compile in parallel (today Cargo already does this for deps; we'd extend it to our own code).
3. **Cleaner architecture** — forces the dependency graph into a DAG; hidden cycles surface and get fixed.
4. **Optional features per crate** — heavy deps (opentelemetry, syntect, image) get scoped to the crate that actually uses them.

## Current Dependency Graph (analyzed 2026-04-20)

Grep of `use crate::X;` across all modules yields:

### Leaves (zero internal deps)

| Module | Files | LOC |
|---|---|---|
| `types` | 7 | 1,104 |
| `auth` | 8 | 1,445 |
| `observability` | 4 | 990 |
| `skills` | 3 | 1,041 |
| `keybindings` | 7 | 2,362 |
| `bootstrap` | 7 | 922 |

### Level 1 (depend only on leaves)

| Module | Internal deps |
|---|---|
| `config` | types |
| `utils` | config, types |
| `mcp` | types |
| `services` | types |
| `computer_use` | types |

### Level 2

| Module | Internal deps |
|---|---|
| `compact` | types, utils |
| `browser` | mcp, types |
| `sandbox` | config, types, utils |
| `permissions` | types, utils |
| `session` | bootstrap, compact, types, utils |

### Hub (tightly coupled — contains **cycles**)

| Module | Internal deps |
|---|---|
| `tools` | config, engine, ipc, permissions, sandbox, skills, teams, types, utils |
| `engine` | bootstrap, commands, config, observability, query, services, session, tools, types |
| `commands` | api, auth, bootstrap, browser, compact, config, engine, keybindings, mcp, plugins, sandbox, session, skills, teams, types, ui, utils, voice |
| `query` | services, tools, types |
| `ipc` | commands, engine, services, types, ui |
| `teams` | engine, types |
| `daemon` | config, engine, types |
| `plugins` | permissions, tools, types, utils |
| `lsp_service` | tools |

**Cycles identified:**

- `tools::agent::{dispatch, tool_impl, worktree}` → `engine::lifecycle::QueryEngine` (3 files)
- `engine::lifecycle::submit_message` → `tools::hooks` (1 file)
- `engine::input_processing` → `commands` (1 file)
- `commands` → `engine` (2 files)

All cycles are narrow (1–3 call sites). Breakable without large refactor.

## Target Crate Layout

```
cc-rust/                        (workspace root)
├── Cargo.toml                  (workspace manifest)
├── crates/
│   ├── cc-types/               # types, shared interfaces (Tool, Hook, Command traits)
│   ├── cc-observability/       # tracing/OTel bridge
│   ├── cc-keybindings/         # vim-like input state machine
│   ├── cc-auth/                # API key + keychain + OAuth PKCE
│   ├── cc-bootstrap/           # startup path / feature detection
│   ├── cc-skills/              # built-in skill definitions
│   ├── cc-config/              # settings + feature gates
│   ├── cc-utils/               # misc helpers (depends on config/types)
│   ├── cc-mcp/                 # MCP protocol + client
│   ├── cc-services/            # tool_use_summary, session_memory, etc.
│   ├── cc-computer-use/        # desktop control
│   ├── cc-compact/             # conversation compaction
│   ├── cc-sandbox/             # process isolation
│   ├── cc-permissions/         # per-tool permission model
│   ├── cc-browser/             # Chrome devtools / extension bridge
│   ├── cc-session/             # persistence (depends on bootstrap/compact)
│   ├── cc-engine/              # QueryEngine + Agent tool + query loop
│   ├── cc-tools/               # all tools except Agent (which lives with engine)
│   ├── cc-lsp-service/         # LSP client (depends on tools)
│   ├── cc-plugins/             # plugin loader
│   ├── cc-teams/               # team coordination
│   ├── cc-daemon/              # KAIROS HTTP/SSE server
│   ├── cc-commands/            # slash commands
│   ├── cc-ipc/                 # headless / daemon IPC
│   └── claude-code-rs/         # thin bin crate: main.rs + cli.rs + startup
└── ...
```

**~24 crates**. Not all need to be separate — we can merge groups once deps are measured. This layout is the *maximum* split.

## Cycle-Breaking Strategies

### Strategy A: Move Agent tool into cc-engine

`tools::agent::*` is a single-purpose wrapper that spawns sub-`QueryEngine` instances. It is *the* reason `tools` depends on `engine`. Move its 3 files into `crates/cc-engine/src/agent/`. This eliminates the `tools → engine` edge cleanly.

**Risk**: low. Agent tool is self-contained; only entry point is `Tool` trait registration in `tools.rs`.

### Strategy B: Extract Hook trait into cc-types

`engine::lifecycle::submit_message` imports `tools::hooks` to run user hooks. Extract the `Hook` trait definition into `cc-types`; keep the concrete hook impls in `cc-tools`. The engine depends only on the trait.

**Risk**: low. Classic trait-object boundary.

### Strategy C: Invert commands → engine edge

`engine::input_processing` imports `commands` to dispatch slash commands. Options:
- C1: Extract a `CommandDispatcher` trait in `cc-types`; `cc-commands` impls it; engine takes `&dyn CommandDispatcher`.
- C2: Move input_processing into `cc-commands` (the command dispatcher is arguably command-layer responsibility anyway).

**Risk**: medium. Needs a small refactor but no behavior change.

## Phased Plan

Each phase is an independently-mergeable PR. Goal: **build and test pass at every phase**.

### Phase 0 — Workspace skeleton *(no code moves)*

- Convert root `Cargo.toml` to `[workspace]`.
- Move existing `src/`, `tests/`, `Cargo.toml` contents into `crates/claude-code-rs/`.
- Keep `.cargo/config.toml`, `build.rs`, `web-ui/` at workspace root.
- Verify `cargo build --release` and `cargo test` still green.
- Update CI paths if needed.

**Estimated effort**: S (1–2 hours)
**Risk**: low — zero semantic change.

### Phase 1 — Extract true leaves

Create 3 leaf crates, all with zero intra-workspace deps:

- `cc-types` (types)
- `cc-observability` (observability)
- `cc-keybindings` (keybindings)

Each extraction:
1. Copy files to `crates/cc-X/src/`
2. Add to workspace members
3. Replace `use crate::X` with `use cc_X` in root crate
4. Remove `mod X;` from root `lib.rs` / `main.rs`
5. Build + test

**Estimated effort**: M (1 day total, ~2h per crate)
**Risk**: low.

### Phase 2 — Extract auth + bootstrap + skills

All three are still leaves (zero internal deps).

- `cc-auth`
- `cc-bootstrap`
- `cc-skills`

**Estimated effort**: M (1 day)
**Risk**: low.

### Phase 3 — Extract level-1 crates

- `cc-config` (depends on cc-types)
- `cc-utils` (depends on cc-config, cc-types)
- `cc-mcp` (depends on cc-types)
- `cc-services` (depends on cc-types)
- `cc-computer-use` (depends on cc-types)

**Estimated effort**: L (1–2 days)
**Risk**: low — the deps are now workspace refs, not `crate::`.

### Phase 4 — Extract level-2 crates

- `cc-compact`
- `cc-sandbox`
- `cc-permissions`
- `cc-browser`
- `cc-session`

**Estimated effort**: L (1–2 days)
**Risk**: medium — `session` is 3,828 LOC and has broad surface area.

### Phase 5 — Break hub cycles

Three parallel PRs, each on its own:

- **5a: Move Agent tool into cc-engine** (Strategy A)
- **5b: Extract Hook trait to cc-types** (Strategy B)
- **5c: Invert input_processing→commands dep** (Strategy C)

After this phase, `tools`, `engine`, `commands`, `ipc` form a DAG.

**Estimated effort**: M (1 day per PR; can parallelize)
**Risk**: medium — behavior-preserving refactor, needs test coverage check.

### Phase 6 — Extract core crates

Now that cycles are gone:

- `cc-engine` (with Agent)
- `cc-query`
- `cc-tools` (without Agent, without Hook trait)
- `cc-lsp-service`

**Estimated effort**: L (2 days)
**Risk**: medium — large surface (17k LOC for tools).

**Current status (issue #75 — in progress)**: scaffolding + cycle-breaking landed:

- All four Phase 6 crates (`cc-engine`, `cc-query`, `cc-tools`, `cc-lsp-service`)
  exist as workspace members with stub `lib.rs` files and dependency
  declarations. The physical source move is staged in follow-up PRs.
- Phase-5 residual `query -> tools` cycle fully eliminated: the query loop
  now drives hooks through `cc_types::hooks::HookRunner` via a new
  `QueryDeps::hook_runner()` accessor. `HookRunner` gained a `run_stop_hooks`
  method, implemented by both `NoopHookRunner` (cc-types) and
  `ShellHookRunner` (root crate).
- `background_agents` (CompletedBackgroundAgent, PendingBackgroundResults)
  moved to `cc-types::background_agents` so the engine `QueryDeps` trait can
  cite them without touching `crate::tools::*`.
- Agent IPC type trio (`agent_types`, `agent_events`, `agent_channel`) moved
  to `cc-types::{agent_types, agent_events, agent_channel}`. Root
  `src/ipc/agent_*.rs` are now thin re-exports. This unblocks the
  `engine -> ipc` edge (the engine `sdk_to_agent_event` helper now depends
  only on cc-types).
- `ToolUseContext::bg_agent_tx` typed against `cc_types::agent_channel::AgentSender`
  instead of `crate::ipc::agent_channel::AgentSender`, removing the
  `types -> ipc` edge in the Tool trait surface.
- LSP shared types (`HoverInfo`, `SourceLocation`, `SymbolInfo`) moved from
  `tools/lsp.rs` into a new `lsp_service/types.rs` module. Tool side now
  re-exports. This breaks the `lsp_service -> tools::lsp` cycle; when
  `tools/lsp.rs` moves into `cc-lsp-service` in the next pass, the crate is
  self-contained.

**Remaining before the source move**:

1. Hoist `types/tool.rs` (Tool trait, `ToolUseContext`) to cc-types. Blocker:
   `ToolUseContext::get_app_state` returns `AppState`, which lives in
   `types/app_state.rs` and transitively reaches into `teams::types::TeamContext`
   and `ui::status_line::StatusLineRunner`. Either (a) abstract `AppState`
   behind an opaque `Arc<dyn AppStateHandle>` trait, or (b) move `TeamContext`
   and `StatusLineRunner` to cc-types first.
2. Move `types/config.rs` (QueryEngineConfig, QueryParams) and the
   `types/app_state.rs` residue to cc-types once (1) unblocks.
3. Move the three cycle-causing tool files to their natural homes:
   - `tools/lsp.rs` -> cc-lsp-service
   - `tools/send_message.rs` + `tools/team_spawn.rs` -> cc-teams
   - `tools/system_status.rs` -> cc-ipc
4. Physically move `src/engine/`, `src/query/`, `src/tools/`, `src/lsp_service/`
   into their respective crates and rewire `use crate::X` imports.

### Phase 7 — Extract high-level crates

- `cc-plugins`
- `cc-teams`
- `cc-daemon`
- `cc-commands` (largest at 12k LOC — biggest incremental-build win)
- `cc-ipc`

**Estimated effort**: L (2 days)
**Risk**: medium.

**Current status (issue #76 — scaffold only)**: all five Phase 7 crates exist
as workspace members with stub `lib.rs` files and description comments. The
source move blocks on completion of Phase 6, because every Phase 7 crate
depends transitively on `cc-engine`, `cc-tools`, or the `Tool` trait in
`types/tool.rs`. See Phase 6 remaining items above.

### Phase 8 — Thin root bin crate

After all extractions, `crates/claude-code-rs/src/` contains only:
- `main.rs`
- `cli.rs`
- `startup/` (unless also extracted)
- `shutdown.rs`
- feature-flag glue

**Estimated effort**: S (half day)
**Risk**: low.

## Non-Goals

- This design does *not* re-architect any behavior.
- No dependency version bumps tied to the split.
- No public API surface changes (there is no public API; this is a bin crate).
- Not doing monorepo tooling (turborepo etc.) — plain Cargo workspace is sufficient.

## Success Criteria

After all phases:

- [ ] `cargo build` after touching one file in `cc-tools` rebuilds only `cc-tools` + `claude-code-rs` (two crate codegens, not one giant one).
- [ ] Clean-build time is within 10% of pre-split (workspace overhead is minimal).
- [ ] Incremental-build time after a tool edit drops from ~7.5s to <3s (measured target).
- [ ] `cargo test` passes identically at every phase boundary.
- [ ] No new `unsafe`, no new runtime deps introduced by the split.

## Measurement Protocol

Before Phase 0, record baseline:
```bash
touch src/tools/file_read.rs && cargo build --offline -q 2>&1 | tail -1
touch src/main.rs && cargo build --offline -q 2>&1 | tail -1
cargo clean && cargo build --offline --release 2>&1 | tail -1
```

After each phase, re-run and append to `docs/workspace-split-measurements.md`.

## Rollback

Each phase is a single PR. Rollback = revert the PR. No phase writes a one-way door.

## Open Questions

1. Do we want `cc-` as crate-name prefix (`cc-types`, `cc-api`, etc.) or unprefixed (`types`, `api`)? The prefix avoids name collisions with external crates (there's already a `types` crate on crates.io). **Proposed: use `cc-` prefix.**
2. Should `cc-commands` be further split by command category (session/review/etc.)? Today one `commands` module has 53 files across concerns. **Proposed: defer until Phase 7 measurement shows it's still a bottleneck.**
3. Do we keep the existing `tests/` dir at workspace root (shared integration tests) or move per-crate unit tests into each crate? **Proposed: keep integration tests at root; add per-crate unit tests where they make sense.**
