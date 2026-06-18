# Crate Migration Target State

> Scope: ideal end state after the workspace crate migration is complete.
> Source: [`CRATE_MIGRATION_GUIDE.md`](CRATE_MIGRATION_GUIDE.md).
> Execution plan:
> [`crate-migration-phase-plan-2026-05-14.md`](../archive/plan/crate-migration-phase-plan-2026-05-14.md).
>
> This is an acceptance document, not a step-by-step migration plan. If current
> code disagrees with this document, treat that disagreement as remaining
> migration work unless a separate active document marks it as intentional.

## Completion Definition

`crate_migration` is complete only when `claude-code-rs` is a thin binary crate
and every reusable implementation is owned by a workspace library crate.

Completion requires all of the following:

- `claude-code-rs` owns CLI parsing, startup mode selection, runtime adapter
  wiring, binary entry points, and final process lifecycle glue only.
- Runtime domains are implemented in `cc-*` library crates, not in root-private
  modules.
- No library crate depends on `claude-code-rs` or imports files from
  `crates/claude-code-rs/src/**`.
- No normal runtime path uses `#[path = "..."]` across crate boundaries.
- Migration work does not introduce move-specific reverse dependencies or
  compatibility layers. If a move needs shared types, traits, DTOs, or adapter
  contracts, place those contracts in an appropriate shared crate instead.
- Persisted files, IPC JSON, session transcripts, daemon state, settings,
  credentials, audit output, and path-isolation behavior remain compatible with
  the pre-migration cc-rust behavior.
- Workspace checks pass without migration-introduced warnings.

## Final Dependency Direction

The final workspace graph is acyclic and follows this direction:

```text
cc-types / cc-ipc-protocol
  -> leaf crates: cc-models, cc-config, cc-auth, cc-skills, cc-observability
  -> domain crates: cc-api, cc-session, cc-compact, cc-permissions, cc-sandbox
  -> runtime crates: cc-engine, cc-query, cc-tools, cc-teams, cc-daemon, cc-ipc
  -> UI and binary glue: cc-ui, claude-code-rs
```

Read this as layer order: crates later in the list may depend on earlier
layers; earlier layers must not depend on later layers. The order does not mean
every crate must depend on every lower-level crate.

Final graph rules:

- `cc-types` and `cc-ipc-protocol` contain contracts only: DTOs, traits,
  protocol envelopes, and stable serialized shapes.
- Leaf crates do not pull in heavy runtime surfaces such as UI, daemon, HTTP
  servers, tool execution, or engine state.
- Domain crates own persistent storage, API/auth/session/compact/permission
  behavior, and sandbox-specific runtime logic for their domains.
- Runtime crates depend on explicit contracts and adapters. They do not reach
  sideways into unrelated runtime crates for global state.
- `cc-ui` depends on stable UI-facing contracts and runtime adapters; terminal
  bootstrapping remains binary glue unless it has a stable standalone boundary.
- `claude-code-rs` may depend on library crates to assemble the application,
  but no library crate may depend on it.

## Migration-Time Direction Rules

These rules apply during the move, not only after the final state is reached:

- Do not add a dependency from a target crate back to the root crate or to the
  previous owner of the code being moved just to keep existing call sites
  compiling.
- Do not add a bespoke compatibility crate, adapter module, re-export layer, or
  wrapper API whose only purpose is to preserve the old module path for one
  migration slice.
- Do not encode ownership through consumer-specific callbacks. A moved crate
  should depend on stable contracts, not on APIs shaped around one current
  caller.
- When two crates need the same type, trait, protocol shape, or classification
  data, keep or move that contract into a shared crate such as `cc-types`,
  `cc-ipc-protocol`, or another explicitly shared contract crate.
- If code cannot move without a reverse dependency or compatibility layer, leave
  the implementation in its current owner until the shared contract is extracted
  first.

## Root Binary Shape

At completion, `crates/claude-code-rs/src/**` should be small enough to audit as
process glue.

Expected root ownership:

- `main.rs` and startup modules parse CLI/config, initialize logging, choose
  TUI/headless/daemon/web/fast-path modes, and install runtime adapters.
- Binary-only adapters connect still-separate runtime components at startup.
- Root tests cover command-line behavior, end-to-end process behavior, and
  compatibility shims only when a shim is still intentionally present.
- Root modules do not contain engine lifecycle logic, query-loop logic, tool
  implementations, daemon workers, reusable command handlers, persisted DTOs, or
  reusable UI state machines.

Expected absent root ownership:

- No `engine/`, `query/`, `tools/`, `commands/`, `daemon/`, `ipc/`,
  `services/`, `teams/`, `plugins/`, `mcp/`, `lsp_service/`, `browser/`,
  `computer_use/`, `voice/`, or reusable `ui/` implementation remains in root
  unless the file is explicitly binary-only glue.
- No root module is the canonical owner of a public type that a workspace crate
  consumes.
- No root module is used as an integration registry by library crates.

## Crate Ownership

### Contracts

- `cc-types`: cross-crate domain DTOs, dependency-injection traits, callback
  shapes, tool/permission/hook/agent event contracts, and shared non-wire
  structs.
- `cc-ipc-protocol`: JSONL/headless/daemon IPC envelopes, enum tags, wire DTOs,
  protocol error categories, and roundtrip tests for serialized shapes.

Neither crate owns filesystem writes, subprocess execution, HTTP servers,
terminal state, daemon workers, or model-provider calls.

### Core Domains

- `cc-config`: global/project settings, config validation, path-isolated config
  discovery, and config schemas.
- `cc-auth`: API key, token, OAuth, keychain, and credential resolution using
  cc-rust paths and the `cc-rust` service name.
- `cc-api`: provider clients, streaming response decoding, retry/error mapping,
  model-provider request/response conversion, and provider-specific transport
  details.
- `cc-session`: session persistence, transcript read/write, resume lookup,
  session metadata, and compatibility readers.
- `cc-compact`: compaction pipeline, token-budget decisions, compact boundary
  DTO conversion, and compact transcript transforms.
- `cc-permissions`: permission policy, approval decisions, audit-friendly
  decision data, and sandbox permission integration contracts.
- `cc-sandbox`: sandbox policy and platform-specific execution constraints.
- `cc-skills`: built-in/user skill discovery, project skill paths, packaging
  metadata, and skill prompt materialization.
- `cc-observability`: tracing, metrics, diagnostic events, telemetry setup, and
  shutdown helpers that are not tied to the binary crate.

### Runtime

- `cc-engine`: `QueryEngine`, lifecycle state, system prompt assembly, SDK-facing
  output types, engine adapters, agent runtime, model invocation orchestration,
  and engine-owned status-line payload assembly.
- `cc-query`: async query-loop driver, stream/event loop helpers, cancellation
  and turn boundaries, expressed through explicit DTOs and adapter traits.
- `cc-tools`: stable tool metadata, schemas, registry policy, pure helper logic,
  and tool contracts. Execution needing engine, teams, daemon, plugins, or UI
  runs through adapters or lives in the owning runtime crate.
- `cc-tasks`: task store, task DTOs, task lifecycle behavior, teammate
  assignment state, and task persistence/locking.
- `cc-teams`: teammate runtime, coordinator behavior, mailbox/protocol,
  in-process worker control, and team memory integration contracts.
- `cc-commands`: slash command metadata, parsing, command result DTOs, handler
  contracts, and command implementations that do not require binary-private
  state. Binary-only command integrations are injected as adapters.
- `cc-daemon`: daemon process state, worker command/event files, local daemon
  client, supervisor lifecycle, HTTP/SSE control plane, gateway routes, webhook
  handling, notifications, and tick logic.
- `cc-ipc`: IPC runtime orchestration after dependencies are explicit.
- `cc-ipc-client`: client transport, frontend sink, callback bridge helpers,
  query-turn helpers, and client-side runtime installation helpers.
- `cc-mcp`: MCP client/server discovery, MCP transport/config contracts, and MCP
  tool integration that does not require browser runtime internals.
- `cc-browser`: browser detection, browser MCP bridge runtime, native host
  integration, screenshot/rendering support, and browser-specific diagnostics.
- `cc-lsp-service`: LSP process lifecycle, transport, server info, symbol/hover
  data, and LSP-facing command/tool adapters.
- `cc-plugins`: plugin manifest loading, refresh/install state, plugin tool
  metadata, and plugin runtime contracts.
- `cc-services`: reusable service loops such as session memory, prompt
  suggestion, tool-use summary, onboarding, and LSP lifecycle coordination when
  not better owned by a more specific crate.
- `cc-computer-use`: computer-use detection, setup, screenshot, input, and
  platform-specific automation.
- `cc-utils`: cross-cutting pure helpers that have no runtime ownership and no
  path-isolated persistence.

### UI

- `cc-ui`: reusable Rust TUI state machines, renderers, message rendering,
  command surfaces, selection surfaces, input helpers, snapshots, and UI-facing
  DTO adapters.
- `claude-code-rs` root UI glue: terminal raw-mode entry/exit, process signal
  integration, channel wiring, engine/daemon/headless startup handoff, and other
  binary-only terminal lifecycle details.

## Adapter Boundaries

All cross-runtime calls that would otherwise create cycles are expressed as
traits or explicit function tables.

Final adapter expectations:

- Engine calls command dispatch through a command adapter, not root command
  modules.
- Engine/tool execution calls hooks, permission decisions, task updates, and
  progress callbacks through contracts in low-level crates.
- Agent tree registration, background agent updates, and teammate state are
  injected through runtime adapters.
- Daemon submit/abort/status operations call engine through an engine adapter.
- Webhook and gateway routes deliver work into teams/tools/engine through
  adapters, not root-private modules.
- UI receives snapshots and command results through DTOs, not direct global
  runtime reads.
- Adapter installation happens during startup before any runtime path can call
  the adapter.

Adapter traits live in the lowest crate that owns the contract and return
domain DTOs instead of root-private structs.

## Persistent Data And Wire Compatibility

Completed migration must preserve cc-rust path isolation:

| Purpose | Required cc-rust location |
| --- | --- |
| Global data | `~/.cc-rust/` |
| Project config | `.cc-rust/settings.json` |
| Project skills | `.cc-rust/skills/` |
| Credentials | `~/.cc-rust/credentials.json` |
| Keychain service | `cc-rust` |
| Daemon state | `~/.cc-rust/daemon/` |

Final compatibility requirements:

- No migrated code writes to `~/.Codex/`, `.Codex/`, `~/.codex/`, or upstream
  Claude/Codex service names.
- Any upstream compatibility read is read-only, explicit, and covered by tests.
- IPC JSON field names, enum tags, ordering assumptions, and error
  classifications are covered by roundtrip tests.
- Session transcript, settings, credentials, daemon event, and audit formats
  have compatibility tests or documented schema snapshots.

## Shim-Free State

Final state has no cross-crate `#[path]` migration bridge and no root shim that
contains implementation logic.

Allowed final patterns:

```rust
pub use cc_engine::lifecycle::QueryEngine;
```

```rust
pub use cc_commands::{CommandDispatcher, CommandResult};
```

Forbidden final patterns:

```rust
#[path = "../../../../cc-engine/src/lifecycle/mod.rs"]
pub mod lifecycle;
```

```rust
// In any cc-* library crate:
#[path = "../../claude-code-rs/src/ui/app.rs"]
mod app;
```

```rust
// In any cc-* library crate:
use claude_code_rs::engine;
```

Root re-exports are not a migration strategy. If an existing re-export remains
temporarily, it must be a trivial leftover scheduled for deletion and must not
be extended to support a new move. New code should import the owning `cc-*`
crate or shared contract crate directly.

## Final Verification Gates

Use the local Rust toolchain from the workspace parent:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

Required clean checks:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --message-format short
cargo test --workspace
cargo build --workspace --release
```

Dependency shape checks:

```bash
cargo tree -p claude-code-rs -e normal --depth 1
cargo tree -p cc-engine -e normal --depth 1
cargo tree -p cc-query -e normal --depth 1
cargo tree -p cc-tools -e normal --depth 1
cargo tree -p cc-daemon -e normal --depth 1
cargo tree -p cc-ui -e normal --depth 1
```

Shim and root-dependency guards:

```bash
rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway
rg '#\[path = ".*cc-' crates/claude-code-rs/src
rg 'crate::(engine|query|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway
rg 'claude[-_]code[-_]rs' crates/cc-* crates/gateway -g '*.rs' -g '*.toml'
```

The expected result for the guard commands is no production dependency on root
implementation. Test names, documentation strings, binary names, and package
metadata may still mention `claude-code-rs` when they are not dependency edges.

Path-isolation guards:

```bash
rg '~/.Codex|\\.Codex/|~/.codex|service.*Codex|service.*Claude' crates -g '*.rs'
rg '~/.cc-rust|\\.cc-rust|cc-rust' crates docs -g '*.rs' -g '*.md'
```

The first command should return only documented, read-only compatibility
fallbacks. The second command should show the canonical cc-rust paths and
service name.

## Documentation Closeout

When the migration reaches this state:

- Move active migration plans that are no longer needed into `development/archive/`.
- Update `docs/WORK_STATUS.md` to remove crate migration from active work.
- Update `development/archive/IMPLEMENTATION_GAPS.md` so no completed migration gap remains
  listed as open.
- Move completed simplified/full-build notes into the appropriate archive
  document.
- Keep this file as the stable acceptance reference for future crate-boundary
  reviews.
