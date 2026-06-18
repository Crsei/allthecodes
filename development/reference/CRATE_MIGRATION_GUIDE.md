# Workspace Crate Migration Guide

> Scope: guidance for moving code out of `crates/claude-code-rs/src/**` into
> workspace crates such as `cc-engine`, `cc-query`, `cc-tools`, `cc-ipc`,
> `cc-daemon`, `cc-commands`, `cc-ui`, and other `cc-*` crates.

This document is the operational checklist for crate migration. It complements
the broader split design in
[`docs/superpowers/specs/2026-04-20-workspace-split-design.md`](../superpowers/specs/2026-04-20-workspace-split-design.md)
and the module-specific plans under [`development/archive/plan/`](../archive/plan). The ideal
acceptance state for the completed migration is documented in
[`CRATE_MIGRATION_TARGET_STATE.md`](CRATE_MIGRATION_TARGET_STATE.md). The
ordered execution plan is
[`crate-migration-phase-plan-2026-05-14.md`](../archive/plan/crate-migration-phase-plan-2026-05-14.md).

## Target Shape

`claude-code-rs` should converge toward a thin binary crate:

- CLI argument parsing and startup mode selection.
- Runtime adapter registration and dependency injection.
- Binary-only glue for TUI/headless/daemon/web entry points.
- Short-lived compatibility shims during a migration phase.

Real implementation ownership should live in library crates. A moved module is
not considered migrated while its new crate still reaches back into
`claude-code-rs` private modules for normal operation.

## Dependency Direction

Use this as the default dependency direction unless a module-specific plan says
otherwise:

```text
cc-types / cc-ipc-protocol
  -> leaf crates: cc-models, cc-config, cc-auth, cc-skills, cc-observability
  -> domain crates: cc-api, cc-session, cc-compact, cc-permissions, cc-sandbox
  -> runtime crates: cc-engine, cc-query, cc-tools, cc-teams, cc-daemon, cc-ipc
  -> UI and binary glue: cc-ui, claude-code-rs
```

Rules:

- DTOs, traits, protocol envelopes, and stable contracts belong in low-level
  crates such as `cc-types` or `cc-ipc-protocol`.
- Runtime behavior belongs in the crate that owns the domain, not in a shared
  DTO crate.
- A library crate must not depend on `claude-code-rs`.
- Avoid `#[path = "..."]` references across crate boundaries. They are
  migration shims only, never the final architecture.
- Root shims may re-export or forward during a phase, but must not keep the
  real implementation once the target crate owns it.

## Pre-Migration Checklist

Before moving files:

1. Identify the target crate and write down the intended owner of every public
   type, trait, and persisted DTO touched by the move.
2. Inventory direct `crate::...` imports in the source module with `rg`.
3. Record baseline tests for the old module path.
4. Check whether the module reads or writes persistent files, JSON contracts,
   IPC messages, session transcripts, settings, credentials, daemon events, or
   audit data.
5. Check whether the module relies on startup-time global registration or
   process-wide callbacks.
6. Decide the shim lifetime and the exact `rg` guard that will prove the shim
   is gone later.

Do not combine a physical file move with behavior rewrites unless the rewrite is
required to create a clean crate boundary.

## Ownership Rules

### Shared Contracts

Put these in `cc-types` or `cc-ipc-protocol`:

- Serializable DTOs shared by multiple crates.
- Trait boundaries used for dependency injection.
- IPC/headless/daemon message envelopes.
- Permission, tool, callback, hook, and agent event shapes that must be seen by
  multiple runtime crates.

Do not put runtime singletons, filesystem writes, subprocess management, HTTP
servers, or tool execution in shared contract crates.

### Engine And Query

`cc-engine` should own `QueryEngine`, lifecycle state, SDK-facing output types,
system prompt assembly, agent runtime, and engine-facing adapters.

`cc-query` should own the async query-loop driver and loop helper logic. It
should depend on explicit traits and DTOs rather than root modules.

Watch for these pressure points:

- `AppState`, `ToolUseContext`, `QueryParams`, `SdkMessage`, and
  `CommandDispatcher` ownership.
- Hook, permission, task, MCP, and tool execution adapters.
- Langfuse/observability setup and shutdown order.
- Background agent channels and agent tree runtime registration.

### Tools

`cc-tools` should stay small unless a module-specific plan explicitly expands
it. Prefer using it for stable tool metadata, schemas, registry policies, and
pure helper logic.

Execution that needs engine, teams, daemon, plugins, UI, or root-only runtime
should go behind adapter traits or stay in the domain crate that owns that
runtime.

### IPC

`cc-ipc-protocol` owns wire DTOs. `cc-ipc-client` owns client transport,
frontend sink, callback bridge helpers, and query-turn helpers. `cc-ipc` owns
runtime orchestration only after its dependencies are explicit.

Never change JSONL field names, enum tags, ordering assumptions, or error
classification while doing a mechanical crate move. Add roundtrip tests before
changing protocol shapes.

### Daemon

`cc-daemon` owns daemon process state, worker command/event files, local daemon
client, supervisor lifecycle, HTTP/SSE control plane, gateway routes, webhook
handling, and daemon notification/tick logic.

The root crate supplies runtime adapters for engine, commands, webhook routing,
team memory, and any still-root-only behavior. `cc-daemon` must not call
`crate::engine`, `crate::tools`, `crate::commands`, `crate::plugins`,
`crate::teams`, or `crate::plan_workflow` directly.

### UI

UI migration should keep Rust TUI code under the Rust UI boundary. Move reusable
rendering or state-machine code into `cc-ui` only when it no longer needs root
runtime state directly.

TUI integration points that drive terminal raw mode, channels, engine events, or
daemon/headless startup remain binary glue until there is a stable adapter.

## Adapter Boundaries

Use trait adapters when a target crate needs behavior still owned elsewhere.

Good adapter candidates:

- Command dispatch from engine input processing.
- Hook execution and criticality policy.
- Tool execution and progress callbacks.
- Agent tree registration and background-agent updates.
- Daemon engine submit/abort/status operations.
- Webhook delivery into team/tools runtime.
- UI-facing snapshots of tasks, teams, MCP, LSP, and permissions.

Adapter traits should:

- Live in the lowest crate that needs the contract.
- Return domain DTOs, not root-private structs.
- Make error cases explicit instead of silently defaulting.
- Be installed during startup before any runtime path can use them.

## Persistent Data And Path Isolation

Path isolation is a hard requirement. Migrated code must continue using
cc-rust paths and service names:

| Purpose | Required cc-rust location |
| --- | --- |
| Global data | `~/.cc-rust/` |
| Project config | `.cc-rust/settings.json` |
| Project skills | `.cc-rust/skills/` |
| Credentials | `~/.cc-rust/credentials.json` |
| Keychain service | `cc-rust` |
| Daemon state | `~/.cc-rust/daemon/` |

Do not introduce writes to `~/.Codex/`, `.Codex/`, `~/.codex/`, or upstream
Claude/Codex service names while moving code. If a fallback intentionally reads
upstream compatibility data, keep that read-only behavior explicit and tested.

## Feature And Dependency Hygiene

When adding dependencies to a target crate:

- Prefer workspace dependencies with crate-local feature selection.
- Keep optional features optional after the move.
- Avoid pulling heavy UI, HTTP, telemetry, syntax highlighting, image, or AST
  dependencies into leaf crates.
- Run `cargo tree -p <crate> -e normal --depth 1` after meaningful dependency
  changes.
- Do not add dependency version bumps as part of a migration unless required by
  the move.

## Shim Policy

Allowed temporary shims:

```rust
pub use cc_engine::lifecycle::*;
```

```rust
use cc_config as config;
```

Risky temporary shims:

```rust
#[path = "../../../../cc-engine/src/lifecycle/mod.rs"]
pub mod lifecycle;
```

Forbidden final states:

- Root crate shim contains real logic after the target crate has a copy.
- New crate imports root-private modules.
- A moved module is tested only through the old root path.
- `allow(dead_code)` or `allow(unused_imports)` is added to hide an incomplete
  migration boundary.

Every shim phase should include a deletion guard, for example:

```bash
rg "crate::engine::lifecycle|#\\[path = .*cc-engine" crates/claude-code-rs/src
```

## Verification Gates

Use the local Rust toolchain from the workspace parent when running Cargo:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

Minimum checks for each migration slice:

```bash
cargo fmt --all --check
cargo check -p <target-crate> --message-format short
cargo test -p <target-crate> <focused-filter>
cargo check -p claude-code-rs --message-format short
cargo tree -p <target-crate> -e normal --depth 1
```

Add root crate tests when root glue or compatibility shims change:

```bash
cargo test -p claude-code-rs <old-module-filter>
```

Run workspace-level checks before a lane closeout:

```bash
cargo check --workspace --all-targets --message-format short
cargo build --workspace --release
```

If a check is blocked by unrelated existing dirty work, record the blocking file
and error in the migration note instead of treating it as target-crate success.

## Closeout Checklist

A migration slice is complete only when:

- The target crate owns the real implementation.
- Root crate paths are direct imports from the target crate or deleted.
- Temporary `#[path]` shims are removed or have a documented next deletion
  phase.
- The target crate has focused tests for moved behavior.
- Old root tests still pass or have been intentionally moved and renamed.
- Protocol, persistence, and path-isolation behavior is unchanged.
- `cargo tree` shows no new root dependency or unexpected heavy dependency.
- `rg` guards show no stale imports, stale module declarations, or duplicate
  implementation copies.
- Documentation and verification commands reference the new crate path.
