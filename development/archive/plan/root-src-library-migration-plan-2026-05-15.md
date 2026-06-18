# Root Source Library Migration Plan

> 日期：2026-05-15
> 范围：将 `crates/claude-code-rs/src/**` 中除 Rust TUI 外的剩余实现完整迁入
> workspace library crates。
>
> 目标状态：`crates/claude-code-rs` 只保留 thin binary、CLI / startup wiring、
> build script，以及当前 intentional root-owned 的 `src/ui/**` Rust TUI 代码。
>
> 参考：
>
> - [`development/reference/CRATE_DEPENDENCY_TARGETS.md`](../../reference/CRATE_DEPENDENCY_TARGETS.md)
> - [`development/archive/plan/crate-migration-phase-plan-2026-05-14.md`](crate-migration-phase-plan-2026-05-14.md)
> - [`development/archive/plan/crate-migration-phase-0-inventory-2026-05-14.md`](crate-migration-phase-0-inventory-2026-05-14.md)
> - [`development/archive/plan/workspace-decycle-plan-2026-05-14.md`](workspace-decycle-plan-2026-05-14.md)

本文是剩余 root source 清空计划。它不取代 2026-05-14 的 crate migration phase
plan，而是在其基础上收紧最终边界：除 `src/ui/**` 外，root crate 不再拥有可复用
runtime/domain 实现。

## Final Target

最终 `crates/claude-code-rs/src` 允许保留：

- `main.rs`：thin binary entry，只负责调用 bootstrap runner、安装 root-only frontend。
- `cli.rs`：CLI argument shape。后续如批准 CLI crate extraction，可迁到
  `cc-bootstrap` 或 `cc-cli`；本计划不强制。
- `startup/` 中极少量 binary-only mode selection 可以短期保留；长期目标是迁入
  `cc-bootstrap`，root 只传入 `Cli` 和 frontend。
- `shutdown.rs` 中 terminal reset / process signal glue 可以短期保留；长期目标是让
  reusable shutdown state 进入 `cc-bootstrap`、`cc-session`、`cc-observability`。
- `ui/**`：当前 intentional root-owned Rust TUI source。任何 `cc-*` crate 不得
  依赖 root UI source 或通过 `#[path]` 读取它。

最终 `crates/claude-code-rs/src` 不应再保留：

- `browser/`、`commands/`、`computer_use/`、`daemon/`、`engine/`、`ide/`、`ipc/`
- `lsp_service/`、`mcp/`、`plugins/`、`safety/`、`services/`、`teams/`、`tools/`
- `voice/`、`web/`
- `dashboard.rs`、`plan_workflow.rs`、`worktree_hooks.rs`
- 任何只为保留旧 `crate::...` 路径存在的 facade、re-export、compatibility alias

## Current Baseline

当前非 UI root source 仍然很大，不能按目录机械移动。2026-05-15 快照：

| Area | Rust files | Approx. lines | Target owner |
| --- | ---: | ---: | --- |
| `tools/` | 52 | 24406 | `cc-tools` plus domain crates |
| `commands/` | 63 | 19013 | `cc-commands` |
| `ipc/` | 12 | 5720 | `cc-ipc` / `cc-ipc-client` / `cc-ipc-protocol` |
| `teams/` | 14 | 4865 | `cc-teams` |
| `daemon/` | 17 | 4815 | `cc-daemon` / `gateway` |
| `engine/` | 8 | 4303 | `cc-engine` / `cc-query` |
| `lsp_service/` | 6 | 3542 | `cc-lsp-service` |
| `plugins/` | 5 | 2943 | `cc-plugins` |
| `services/` | 7 | 1841 | `cc-services` |
| `voice/` | 6 | 1360 | future `cc-voice` or `cc-services::voice` |
| `web/` | 4 | 1025 | `cc-daemon` or future `cc-web` |
| `computer_use/` | 4 | 808 | `cc-computer-use` |
| `safety/` | 2 | 798 | `cc-permissions` or `cc-safety` |
| `ide/` | 1 | 586 | future `cc-ide` |
| `browser/` | 2 | 155 | `cc-browser` |
| loose files | 6 | 2058 | owner-specific; see below |

`ui/` remains intentionally root-owned in this plan.

## Non-Negotiable Rules

- Move contract / DTO / trait first, runtime second, root facade deletion last.
- New library crates must not depend on `claude-code-rs`.
- No cross-crate `#[path]` bridge.
- No root compatibility crate, root re-export facade, or path-preserving wrapper as a migration endpoint.
- No library crate may import root UI internals. UI rendering must consume DTOs exposed by owner crates.
- Feature-heavy dependencies must land in the narrow owner crate. Do not add broad dependencies to root to make migration easier.
- Persistence and path isolation remain under cc-rust paths: `~/.cc-rust/`, `.cc-rust/`, keychain service `"cc-rust"`.
- Each slice must remove stale `allow(dead_code|unused_imports|unused)` in touched owner code instead of adding new long-lived allowances.

## Target Owner Matrix

| Root source | Target | Migration notes |
| --- | --- | --- |
| `browser/` | `cc-browser` | Keep browser detection, permission text, result rendering, Chrome/native-host reusable behavior here. Root only starts binary modes. |
| `computer_use/` | `cc-computer-use` | Move detection/setup/tool capability. Permission prompt callbacks should call `cc-computer-use` APIs, not root modules. |
| `mcp/` | `cc-mcp` | Move tool wrapper and MCP runtime behavior. Shared browser/MCP classification must use DTOs, not direct `cc-browser <-> cc-mcp` runtime coupling. |
| `lsp_service/` | `cc-lsp-service` | Move client, transport, conversions, server registry, diagnostics, completion DTOs. |
| `ide/` | new `cc-ide` | Move IDE detection, selection persistence, reconnect logic, event emission contract. |
| `plugins/` | `cc-plugins` | Move manifest, loader, refresh, plugin tools, plugin skill discovery. |
| `safety/` | `cc-permissions` or new `cc-safety` | If pure classifier policy, move to `cc-permissions`; if model-backed, expose adapter into `cc-engine`. |
| `services/` | `cc-services` | Move onboarding, scheduler, session analytics. Avoid making this a catch-all for unrelated runtime domains. |
| `voice/` | new `cc-voice` | Move audio backend, feasibility, language normalization, STT client. If intentionally unsupported, record final surface in gaps. |
| `web/` | `cc-daemon` or new `cc-web` | Move reusable handlers/state/static file serving. Root may keep port binding and browser-open glue. |
| `daemon/` | `cc-daemon` and `gateway` | Move routes/server/sse/supervisor/process state/team memory proxy. No root duplicate protocol DTOs. |
| `ipc/` | `cc-ipc`, `cc-ipc-client`, `cc-ipc-protocol` | Wire DTOs in protocol, client transport in client, headless/runtime/subsystem handlers in ipc. |
| `teams/` | `cc-teams` | Move coordinator, mailbox, in-process backend, runner, context, helpers, command runtime. |
| `tools/` | `cc-tools` plus domain crates | Registry/contracts in `cc-tools`; tasks in `cc-tasks`; team tools in `cc-teams`; LSP tools in `cc-lsp-service`; shell/exec in new exec-policy/shell crates. |
| `commands/` | `cc-commands` | Move handlers and command registry. UI-specific rendering must become DTO output consumed by root UI. |
| `engine/` | `cc-engine`, `cc-query` | Delete root facade; call sites import owner crates directly. |
| `plan_workflow.rs` | `cc-commands` + `cc-engine` adapter | Plan records/status in `cc-types`; command surface in `cc-commands`; engine mutation via explicit adapter. |
| `worktree_hooks.rs` | `cc-engine`, `cc-sandbox`, or `cc-git-utils` | Hook DTOs in `cc-types`; path policy in sandbox/config-aware helper. |
| `dashboard.rs` | `cc-services` / `cc-observability` / `cc-daemon` | Event DTOs in observability/types; process launch is binary glue if still needed. |
| `startup/` | `cc-bootstrap` | Logging, fast paths, runtime config, mode construction move out. Root retains final mode dispatch only until bootstrap runner exists. |
| `shutdown.rs` | `cc-bootstrap` + `cc-session` + `cc-observability` | Reusable graceful shutdown moves to libraries; terminal/process signal glue can stay root. |
| `cli.rs` | root or future `cc-cli` | Keep root initially to avoid over-expanding migration. |
| `main.rs` | root | Thin entry only; no reusable behavior, no compatibility aliases. |

## Candidate New Crates

Add these only when the migration slice has a real owner boundary and tests.

| Crate | Purpose | First migration slice |
| --- | --- | --- |
| `cc-execpolicy` | Prefix-based command approval, policy explanation, optional Starlark policy | `tools/exec`, sandbox command policy |
| `cc-shell-command` | Shell parsing, display, risk summary, escalation adapters | `tools/exec`, shell hooks |
| `cc-file-search` | ignore-aware file enumeration and fuzzy matching | IPC file search, TUI command/file completion |
| `cc-git-utils` | Repo detection, branch/status helpers, worktree path helpers | git commands, worktree hooks |
| `cc-state` | Turn/session runtime state, dependency prompts, temporary env injection | IPC/session/skill dependency state |
| `cc-ide` | IDE detection, selection, reconnect, MCP bridge config | `ide/`, IDE slash command, IPC subsystem |
| `cc-voice` | Voice capability and unsupported backend surfaces | `voice/`, voice command |
| `cc-hooks` | Hook config, execution, notification/file/task events | `tools/hooks`, file/task tool hook firing |

## Phase Plan

### Phase 0: Guard And Inventory Refresh

Purpose: make the current migration state measurable before moving code.

Actions:

- Update owner matrix if new root files appear.
- Record exact root non-UI directories still present.
- Record dependency graph for root and major owners.
- Add guard commands to CI or at least to migration closeout docs.

Exit criteria:

- Owner matrix covers every root file except `src/ui/**`.
- Guard commands below have a known baseline.
- No new cross-crate `#[path]` bridge exists.

Verification:

```bash
find crates/claude-code-rs/src -mindepth 1 -maxdepth 1 -printf '%f\n' | sort
rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway -g '*.rs'
rg '#\[path = ".*cc-' crates/claude-code-rs/src -g '*.rs'
cargo tree -p claude-code-rs -e normal --depth 1
```

### Phase 1: Workspace Dependency Governance

Purpose: make future moves smaller by avoiding root-owned feature expansion.

Actions:

- Add `workspace.package` and migrate member crates to workspace version/edition/license.
- Start low-risk `workspace.lints`; defer `unwrap_used` / `expect_used`.
- Downscope `tokio`, `reqwest`, `keyring`, `image`, `syntect`, `tree-sitter` features.
- Add dependency audit plan or `deny.toml` with documented exceptions.

Exit criteria:

- New dependencies can be reviewed against a single workspace policy.
- Pure contract crates do not acquire HTTP, terminal, image, shell, or runtime-heavy features.

Verification:

```bash
cargo check --workspace --all-targets --message-format short
cargo tree -p cc-types -e normal --depth 1
cargo tree -p cc-ipc-protocol -e normal --depth 1
```

### Phase 2: Contract And DTO Extraction

Purpose: unblock runtime moves by giving shared data stable owners.

Actions:

- Move command result/status DTOs to `cc-commands` or `cc-types`.
- Move IPC wire-only shapes to `cc-ipc-protocol`.
- Move tool identity/schema/result contracts to `cc-tools` or `cc-types`.
- Move team, task, LSP, plugin, browser, computer-use DTOs to their owner crates.
- Replace root-only structs in moved code with owner crate imports.

Exit criteria:

- Runtime crates can compile without importing root module types.
- DTO duplicates in root and target crates are removed or explicitly marked temporary.

Verification:

```bash
cargo test -p cc-types
cargo test -p cc-ipc-protocol
cargo check -p cc-commands --message-format short
cargo check -p cc-tools --message-format short
```

### Phase 3: Adapter Runtime Foundation

Purpose: replace root callback tangles with explicit adapter traits.

Actions:

- Replace `commands/runtime_bridge.rs` with explicit command runtime adapter registration owned by `cc-commands`.
- Replace IPC `runtime_adapters.rs` root reach-through with adapter traits owned by `cc-ipc`.
- Add engine/team/tool/plugin/daemon adapters where runtime domains must call each other.
- Forbid silent global fallback for required adapters; return explicit errors.

Exit criteria:

- `cc-commands`, `cc-ipc`, `cc-tools`, `cc-teams`, `cc-daemon` do not need root imports.
- Root startup only installs adapters; it does not own business logic.

Verification:

```bash
rg 'crate::(commands|ipc|tools|teams|daemon|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'
cargo check -p cc-commands --message-format short
cargo check -p cc-ipc --message-format short
cargo check -p claude-code-rs --message-format short
```

### Phase 4: Horizontal Domain Runtime Migration

Purpose: move smaller domain runtimes before the large tools/commands cuts.

Actions:

- Move `browser/` to `cc-browser`.
- Move `computer_use/` to `cc-computer-use`.
- Move `lsp_service/` to `cc-lsp-service`.
- Move `ide/` to `cc-ide`.
- Move `mcp/` to `cc-mcp`.
- Move `plugins/` to `cc-plugins`.
- Move `safety/` to `cc-permissions` or `cc-safety`.
- Move `voice/` to `cc-voice`.

Exit criteria:

- Corresponding root directories are deleted.
- Slash commands and IPC subsystem handlers call library crate APIs.
- Browser/MCP and IDE/MCP coupling uses DTOs or adapters, not direct runtime internals.

Verification:

```bash
cargo check -p cc-browser --message-format short
cargo check -p cc-computer-use --message-format short
cargo check -p cc-lsp-service --message-format short
cargo check -p cc-mcp --message-format short
cargo check -p cc-plugins --message-format short
cargo check -p claude-code-rs --message-format short
```

### Phase 5: Tools Migration

Purpose: delete `crates/claude-code-rs/src/tools/**`.

Actions:

- Move registry and stable metadata to `cc-tools`.
- Move task tools and store integration to `cc-tasks`.
- Move team messaging/spawn/PR activity tools to `cc-teams`.
- Move LSP tool wrappers to `cc-lsp-service`.
- Move filesystem tools to `cc-tools` or a narrower file tool module.
- Move exec/shell/sleep/sandbox-facing tools to `cc-shell-command`, `cc-execpolicy`,
  `cc-sandbox`, or `cc-tools` depending on ownership.
- Move hook machinery to `cc-hooks` or the narrow domain owner.
- Keep tool execution context owned by `cc-engine` / `cc-tools` contract, not root.

Exit criteria:

- `crates/claude-code-rs/src/tools` is deleted.
- `cc-tools` does not become a dependency sink for teams/daemon/plugins implementation internals.
- Tool registry construction happens in a library crate and is wired by root only at process startup.

Verification:

```bash
cargo check -p cc-tools --message-format short
cargo test -p cc-tools
cargo check -p cc-tasks --message-format short
cargo check -p cc-teams --message-format short
cargo check -p cc-engine --message-format short
cargo check -p claude-code-rs --message-format short
```

### Phase 6: Teams Migration

Purpose: delete `crates/claude-code-rs/src/teams/**`.

Actions:

- Move coordinator, context, helper, identity, mailbox, protocol, runner, backend to `cc-teams`.
- Replace direct `crate::tools` calls with `cc-tools` registry contract or adapter.
- Replace direct `crate::ipc` calls with `cc-ipc` / `cc-ipc-protocol` DTOs.
- Ensure in-process backend uses `cc-engine` public agent runtime contract.

Exit criteria:

- `crates/claude-code-rs/src/teams` is deleted.
- `cc-teams` does not depend on root, UI internals, or daemon process state.

Verification:

```bash
cargo check -p cc-teams --message-format short
cargo test -p cc-teams
cargo check -p cc-tools --message-format short
cargo check -p cc-engine --message-format short
```

### Phase 7: Commands Migration

Purpose: delete `crates/claude-code-rs/src/commands/**`.

Actions:

- Move all handlers and registry construction to `cc-commands`.
- Convert UI-specific command output into DTOs that root TUI renders.
- Move plan workflow command surface to `cc-commands`.
- Use adapters for engine, daemon, plugin, team, browser, voice, and IPC operations.
- Remove `commands/runtime_bridge.rs` after all providers live in explicit runtime adapters.

Exit criteria:

- `crates/claude-code-rs/src/commands` is deleted.
- Root imports `cc_commands::get_all_commands` or equivalent.
- No command implementation imports root UI, root daemon, root tools, or root teams.

Verification:

```bash
cargo check -p cc-commands --message-format short
cargo test -p cc-commands
cargo check -p claude-code-rs --message-format short
```

### Phase 8: IPC Migration

Purpose: delete `crates/claude-code-rs/src/ipc/**`.

Actions:

- Move headless event loop, ingress, query runner, SDK mapper, callbacks to `cc-ipc`.
- Keep wire and JSONL DTOs in `cc-ipc-protocol`.
- Keep client transport in `cc-ipc-client`.
- Move subsystem handlers behind adapters to LSP, MCP, plugins, skills, IDE, tasks, teams.
- Replace UI status-line root references with DTOs from `cc-engine` or `cc-types`.

Exit criteria:

- `crates/claude-code-rs/src/ipc` is deleted.
- Root only starts headless mode by calling `cc_ipc` entry API.
- IPC tests cover representative JSONL and subsystem command roundtrips.

Verification:

```bash
cargo test -p cc-ipc-protocol
cargo check -p cc-ipc-client --message-format short
cargo check -p cc-ipc --message-format short
cargo check -p claude-code-rs --message-format short
```

### Phase 9: Daemon, Gateway, And Web Migration

Purpose: delete `crates/claude-code-rs/src/daemon/**` and `src/web/**`.

Actions:

- Move daemon protocol duplicate users to `cc_daemon::protocol`.
- Move routes, server, SSE, supervisor, process state, webhook, memory proxy to `cc-daemon`.
- Move gateway bridge/client/routes to `gateway` or `cc-daemon::gateway`.
- Move reusable web handlers/static state to `cc-daemon` or `cc-web`.
- Use `cc-commands` and `cc-engine` adapters instead of root command/engine calls.

Exit criteria:

- `crates/claude-code-rs/src/daemon` and `src/web` are deleted.
- Daemon can be built/tested without root modules.
- Root only chooses daemon/web startup mode and calls library entrypoints.

Verification:

```bash
cargo check -p cc-daemon --message-format short
cargo test -p cc-daemon
cargo check -p gateway --message-format short
cargo check -p claude-code-rs --message-format short
```

### Phase 10: Engine And Query Facade Deletion

Purpose: delete root `engine/` facade and remove root aliases.

Actions:

- Ensure all lifecycle/query/agent/status-line code is owned by `cc-engine` or `cc-query`.
- Update call sites to import `cc_engine` / `cc_query` directly.
- Delete any root `engine/` facade or re-export module.
- Move `worktree_hooks.rs` to its final owner.

Exit criteria:

- `crates/claude-code-rs/src/engine` is deleted.
- No source imports `crate::engine::...` outside root UI references that must be converted.

Verification:

```bash
rg 'crate::engine::' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'
cargo test -p cc-engine
cargo test -p cc-query
cargo check -p claude-code-rs --message-format short
```

### Phase 11: Services, Startup, Shutdown, Loose Files

Purpose: remove non-UI loose implementation files and reduce root to binary glue.

Actions:

- Move `services/` to `cc-services`.
- Move `dashboard.rs` event/service logic to `cc-services`, `cc-observability`, or `cc-daemon`.
- Move `plan_workflow.rs` to `cc-commands` plus engine adapter.
- Move reusable startup helpers to `cc-bootstrap`.
- Move reusable shutdown/session flush/audit logic to owner crates.
- Remove root compatibility aliases in `main.rs`.

Exit criteria:

- Root loose files are either `main.rs`, `cli.rs`, `shutdown.rs` with binary-only glue,
  or deleted.
- Root `main.rs` is orchestration only.

Verification:

```bash
rg '^use cc_.* as ' crates/claude-code-rs/src/main.rs
rg 'mod (browser|commands|computer_use|daemon|engine|ide|ipc|lsp_service|mcp|plugins|safety|services|teams|tools|voice|web);' crates/claude-code-rs/src/main.rs
cargo check -p claude-code-rs --message-format short
```

### Phase 12: Final Root Thin-Binary Closeout

Purpose: prove the final source boundary is clean.

Actions:

- Delete all remaining non-UI root modules that are not explicitly binary-only.
- Re-run path isolation checks.
- Remove migration-only `allow(...)`.
- Update `CRATE_DEPENDENCY_TARGETS.md`, `WORK_STATUS.md`, and gap/archive docs.
- Move completed migration docs to archive only after all guards pass.

Exit criteria:

- `find crates/claude-code-rs/src -mindepth 1 -maxdepth 1` shows only accepted root entries.
- No `cc-*` crate references `claude-code-rs` source or root-private module names.
- Full workspace build/test passes.

Final verification:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --message-format short
cargo test --workspace
cargo build --workspace --release
rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway -g '*.rs'
rg '#\[path = ".*cc-' crates/claude-code-rs/src -g '*.rs'
rg 'crate::(browser|commands|computer_use|daemon|engine|ide|ipc|lsp_service|mcp|plugins|safety|services|teams|tools|voice|web)::' crates/cc-* crates/gateway -g '*.rs'
rg '~/.Codex|\.Codex/|~/.codex|service.*Codex|service.*Claude' crates -g '*.rs'
rg 'allow\((dead_code|unused_imports|unused)\)' crates/cc-* crates/claude-code-rs/src -g '*.rs'
```

## Critical Dependency Cuts

These cuts should be completed before large file moves:

- `cc-engine` must not depend on IPC client runtime; IPC should depend on engine via adapter.
- `cc-tools` must not depend on teams, daemon, plugins, UI, or root implementation internals.
- `cc-commands` must not depend on root UI; command outputs must be DTOs.
- `cc-teams` and `cc-tools` must not call each other through root paths.
- `cc-daemon` must not call root command registry or root plan workflow.
- `cc-mcp` and `cc-browser` must not own each other's runtime behavior.
- `cc-ipc` must call subsystem owners through library APIs or explicit adapters.

## Suggested Work Slices

Keep PRs small. A good slice is one owner boundary plus tests:

- Move one pure DTO file and update all imports.
- Move one small domain runtime directory after DTOs are owned.
- Replace one root callback cluster with a trait adapter.
- Delete one root facade after all call sites import the owner crate.
- Add one guard test or source scan for a completed owner.

Avoid slices that move `tools/` and `commands/` together; they are too coupled and make
regressions hard to isolate.

## Documentation Requirements

Every completed phase must update:

- This plan's status section if added later.
- [`development/reference/CRATE_DEPENDENCY_TARGETS.md`](../../reference/CRATE_DEPENDENCY_TARGETS.md)
  if owner boundaries or candidate crates change.
- [`docs/WORK_STATUS.md`](../WORK_STATUS.md) for active migration status.
- [`development/archive/IMPLEMENTATION_GAPS.md`](../IMPLEMENTATION_GAPS.md) for intentional leftovers.
- Archive docs only after final verification, not after partial code movement.
