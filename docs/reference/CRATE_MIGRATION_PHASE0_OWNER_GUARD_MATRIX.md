# Crate Migration Phase 0 Owner And Guard Matrix

> Date: 2026-05-14
> Scope: Phase 0 baseline for the ordered crate migration plan.
>
> Sources:
>
> - [`docs/plan/crate-migration-phase-plan-2026-05-14.md`](../plan/crate-migration-phase-plan-2026-05-14.md)
> - [`docs/plan/workspace-decycle-plan-2026-05-14.md`](../plan/workspace-decycle-plan-2026-05-14.md)
> - [`docs/reference/CRATE_MIGRATION_GUIDE.md`](CRATE_MIGRATION_GUIDE.md)
> - [`docs/reference/CRATE_MIGRATION_TARGET_STATE.md`](CRATE_MIGRATION_TARGET_STATE.md)

Phase 0 makes no runtime behavior changes. It freezes owner expectations,
protocol and persistence risks, and guard commands so later phases can migrate
by contract instead of by directory name.

## Current Workspace Inventory

Root `crates/claude-code-rs/src/` still contains the migration targets listed
in the phase plan:

```text
browser/ commands/ computer_use/ daemon/ engine/ ipc/ lsp_service/
mcp/ plugins/ services/ teams/ tools/ ui/ voice/ web/
cli.rs dashboard.rs main.rs plan_workflow.rs shutdown.rs worktree_hooks.rs
```

The workspace already contains target crates for all listed domains except a
dedicated voice crate. `voice/` remains a Phase 8 decision: either introduce a
planned `cc-voice` crate or document which pieces are intentionally binary-only.
Rust TUI ownership was later rolled back to `claude-code-rs/src/ui/**`;
`cc-ui` remains only as an empty boundary crate unless UI extraction is
explicitly re-approved.

## Risk Legend

- `P`: path-isolated persistence or filesystem state.
- `IPC`: JSONL/headless/daemon/gateway wire shape.
- `S`: session transcript or resumable session data.
- `Cfg`: settings/config schema.
- `Cred`: credentials, keychain, OAuth, or token state.
- `D`: daemon event, worker command/event, HTTP/SSE, or gateway state.
- `Audit`: audit/export/security review output.
- `Path`: path isolation risk for `~/.cc-rust/`, `.cc-rust/`, or upstream read-only fallback.

## Owner Matrix

| Root module | Target owner crate | Root binary-only glue to retain | Public type owner | Risk flags | Current guard and notes | Focused validation |
| --- | --- | --- | --- | --- | --- | --- |
| `commands/` | `cc-commands`; runtime integrations injected by root or owning crates | CLI/TUI command dispatch wiring, startup adapter install, binary-private command handlers | Command metadata/result DTOs in `cc-commands`; shared UI/wire payloads in `cc-types` or `cc-ipc-protocol` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | `commands/remote_cmd.rs` now imports the daemon-owned gateway client through normal root module ownership; guard `rg '#\[path = "../daemon/gateway_client.rs"\]|crate::commands::' crates/claude-code-rs/src crates/cc-* -g '*.rs'` | `cargo check -p cc-commands --message-format short`; `cargo test -p cc-commands`; `cargo check -p claude-code-rs --message-format short` |
| `tools/` | `cc-tools` for specs/schema/registry policy; `cc-tasks`, `cc-permissions`, `cc-sandbox`, engine/root adapters for runtime execution | Tool registry installation, binary runtime callbacks, subprocess/process environment wiring that cannot live in a reusable crate | Tool metadata/contracts in `cc-tools`; hook/permission/progress DTOs in `cc-types`; task DTOs in `cc-tasks` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::tools::' crates/cc-* crates/gateway -g '*.rs'`; current hits include `cc-engine` and `cc-ui` adapter paths | `cargo check -p cc-tools --message-format short`; `cargo test -p cc-tools`; `cargo check -p cc-tasks --message-format short` |
| `daemon/` | `cc-daemon`; gateway control plane remains `gateway` | Process startup, engine submit/abort/status adapter install, local binary worker entry, signal/lifecycle glue | Worker command/event DTOs in `cc-daemon` or `cc-ipc-protocol`; gateway DTOs in `gateway`; shared status DTOs in `cc-types` | `P`, `IPC`, `S`, `Cfg`, `Cred`, `D`, `Audit`, `Path` | Guard `rg 'crate::(engine|tools|commands|plugins|teams|plan_workflow)::' crates/cc-daemon crates/gateway -g '*.rs'`; Phase 6 removed the root command `#[path]` shim, while full `cc-daemon` ownership remains Phase 7/9 work | `cargo check -p cc-daemon --message-format short`; `cargo test -p cc-daemon`; `cargo check -p gateway --message-format short` |
| `ipc/` | `cc-ipc` runtime; `cc-ipc-client` client bridge; `cc-ipc-protocol` wire DTOs | Headless binary entry, stdio setup, startup adapter installation, process signal glue | JSONL/headless/daemon envelopes in `cc-ipc-protocol`; client callbacks in `cc-ipc-client` | `IPC`, `S`, `D`, `Audit`, `Path` | Guard `rg 'crate::(engine|tools|commands)::' crates/cc-ipc crates/cc-ipc-client -g '*.rs'` | `cargo check -p cc-ipc-protocol --message-format short`; `cargo test -p cc-ipc-protocol`; `cargo check -p cc-ipc-client --message-format short`; `cargo check -p cc-ipc --message-format short` |
| `teams/` | `cc-teams` | Startup adapter install and any binary-private worker process integration | Team/task protocol DTOs in `cc-teams` or `cc-types`; task storage DTOs in `cc-tasks` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::teams::' crates/cc-* crates/gateway -g '*.rs'`; current hits include `cc-engine` and `cc-ui` root-style access | `cargo check -p cc-teams --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| `engine/` | `cc-engine`; query loop owner is `cc-query` | Root startup sequencing, model/config assembly, adapter install before `QueryEngine` construction | `QueryEngine`, lifecycle, SDK output, status-line payload in `cc-engine`; turn/loop DTOs in `cc-query` or `cc-types` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | `engine/lifecycle/mod.rs` has `#[path = "../../../../cc-engine/src/lifecycle/mod.rs"]`; guard `rg 'crate::engine::lifecycle|#\[path = .*cc-engine' crates/claude-code-rs/src -g '*.rs'` | `cargo check -p cc-engine --message-format short`; `cargo test -p cc-engine`; `cargo check -p cc-query --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| `ui/` | Intentional root owner: `claude-code-rs/src/ui/**`; `cc-ui` is an empty boundary crate | Terminal raw mode, signal integration, channel wiring, engine/headless/daemon startup handoff, and Rust TUI state/render/input remain together in root | UI state/render DTOs remain root-private unless later moved to `cc-types` or `cc-ipc-protocol` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Cross-crate UI `#[path]` bridges were removed by rollback; guards `rg '#\[path = ".*claude-code-rs/src/ui' crates/cc-ui -g '*.rs'` and `rg '#\[path = ".*cc-ui' crates/claude-code-rs/src -g '*.rs'` should stay empty | `cargo check -p claude-code-rs --message-format short` |
| `browser/` | `cc-browser` | CLI flag/startup wiring, native host process entry if still binary-private | Browser detection/rendering/native-host DTOs in `cc-browser`; MCP shared config DTOs in `cc-types` when shared | `P`, `IPC`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::browser::' crates/cc-* crates/gateway -g '*.rs'`; Phase 1 Cut 4 must remove browser/MCP ownership bleed | `cargo check -p cc-browser --message-format short`; `cargo check -p cc-mcp --message-format short` |
| `computer_use/` | `cc-computer-use` | CLI enable flag and permission prompt callback install | Computer-use capability/tool DTOs in `cc-computer-use` or `cc-types`; permission text callback remains adapter | `P`, `IPC`, `Cfg`, `Audit`, `Path` | Guard `rg 'crate::computer_use::' crates/cc-* crates/gateway -g '*.rs'`; current hits include `cc-engine::system_prompt` | `cargo check -p cc-computer-use --message-format short`; `cargo check -p cc-permissions --message-format short` |
| `lsp_service/` | `cc-lsp-service` | Event sender install and binary process startup wiring | LSP server/info/recommendation DTOs in `cc-lsp-service` or `cc-ipc-protocol` when surfaced over IPC | `IPC`, `S`, `Cfg`, `D`, `Path` | Guard `rg 'crate::lsp_service::' crates/cc-* crates/gateway -g '*.rs'`; current hits include `cc-engine` and `cc-ui` paths | `cargo check -p cc-lsp-service --message-format short`; `cargo check -p cc-ui --message-format short` |
| `mcp/` | `cc-mcp` | Startup manager installation and binary adapters to browser/plugins/tools | MCP discovery/transport/config DTOs in `cc-mcp`; shared browser/plugin classification DTOs in `cc-types` if needed | `P`, `IPC`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::mcp::' crates/cc-* crates/gateway -g '*.rs'`; Phase 1 Cut 4 owns browser/MCP decycle | `cargo check -p cc-mcp --message-format short`; `cargo check -p cc-browser --message-format short` |
| `plugins/` | `cc-plugins` | CLI/startup refresh/install adapter wiring | Plugin manifest, refresh/install state, plugin tool metadata in `cc-plugins`; shared tool DTOs in `cc-tools`/`cc-types` | `P`, `IPC`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::plugins::' crates/cc-* crates/gateway -g '*.rs'`; `cc-mcp` comments note prior plugin discovery coupling | `cargo check -p cc-plugins --message-format short`; `cargo check -p cc-mcp --message-format short` |
| `services/` | `cc-services`, except domain-specific loops moved to owning crate | Startup scheduling and shutdown ordering glue | Service payloads in `cc-services`; cross-runtime snapshots in `cc-types` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::services::' crates/cc-* crates/gateway -g '*.rs'`; current `cc-engine` has `services` module ownership pressure | `cargo check -p cc-services --message-format short`; `cargo check -p cc-engine --message-format short` |
| `voice/` | Phase 8 decision: planned `cc-voice` or documented binary-only residual | Audio device access, terminal/TUI voice wiring, feature feasibility callback install | Voice capability/controller/STT DTOs in `cc-voice` if created; otherwise binary-only types must not leak to libraries | `P`, `IPC`, `Cfg`, `Cred`, `Audit`, `Path` | Guard `rg 'crate::voice::' crates/cc-* crates/gateway -g '*.rs'`; current hits are in `cc-ui` path-included TUI/app sources | Until `cc-voice` exists: `cargo check -p claude-code-rs --message-format short`; after scaffold: `cargo check -p cc-voice --message-format short` |
| `web/` | `cc-daemon` or a future web owner for reusable static routing/SSE/state; root only binds/startups | Port binding, browser auto-open, process lifecycle | Web route/state DTOs in owning runtime crate; public control-plane DTOs in `gateway` or `cc-ipc-protocol` | `P`, `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::web::|mod web' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'` | `cargo check -p claude-code-rs --message-format short`; `cargo check -p cc-daemon --message-format short`; `cargo check -p gateway --message-format short` |
| `plan_workflow.rs` | `cc-commands` for command workflow; shared persisted/event DTOs in `cc-types` or `cc-ipc-protocol` | Adapter between UI command surface, engine approval flow, and binary runtime | Plan workflow event/state DTOs in `cc-types` or `cc-ipc-protocol` if serialized | `P`, `IPC`, `S`, `D`, `Audit`, `Path` | Guard `rg 'crate::plan_workflow|plan_workflow::' crates/cc-* crates/gateway crates/claude-code-rs/src -g '*.rs'`; Phase 6 owns command migration | `cargo check -p cc-commands --message-format short`; `cargo test -p cc-commands`; `cargo check -p claude-code-rs --message-format short` |
| `worktree_hooks.rs` | `cc-engine` for engine-facing hook adapter or `cc-tools`/`cc-types` for hook contracts; pure path helpers may move to `cc-utils` | Binary adapter install for local git/worktree side effects | Hook contract DTOs in `cc-types`; execution owner in runtime crate | `P`, `S`, `Cfg`, `Audit`, `Path` | Guard `rg 'crate::worktree_hooks|worktree_hooks::' crates/cc-* crates/gateway crates/claude-code-rs/src -g '*.rs'` | `cargo check -p cc-engine --message-format short`; `cargo check -p cc-tools --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| `dashboard.rs` | `cc-daemon`/`gateway` if reusable dashboard data; otherwise root daemon/web entry glue | Binary local dashboard route or launch glue | Dashboard/status DTOs in `cc-daemon`, `gateway`, or `cc-types` depending on wire exposure | `IPC`, `S`, `Cfg`, `D`, `Audit`, `Path` | Guard `rg 'crate::dashboard|dashboard::' crates/cc-* crates/gateway crates/claude-code-rs/src -g '*.rs'` | `cargo check -p cc-daemon --message-format short`; `cargo check -p gateway --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| `shutdown.rs` | Root binary glue with reusable diagnostics in `cc-observability` if needed | Process signal handling, runtime teardown order, final flush | Shutdown diagnostics in `cc-observability`; no public runtime DTO expected unless IPC exposes it | `P`, `IPC`, `S`, `D`, `Audit`, `Path` | Guard `rg 'crate::shutdown|shutdown::' crates/cc-* crates/gateway crates/claude-code-rs/src -g '*.rs'` | `cargo check -p cc-observability --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| `cli.rs` | Root binary glue; reusable parsing helpers only if a future CLI crate is planned | Clap argument shape and startup mode selection | CLI-only type remains root-private unless shared with tests or gateway | `Cfg`, `Cred`, `D`, `Path` | Guard `rg 'crate::cli|cli::Cli' crates/cc-* crates/gateway -g '*.rs'` should stay empty | `cargo check -p claude-code-rs --message-format short` |
| `main.rs` | Root binary glue | Startup orchestration, adapter registration, fast paths, mode handoff | No reusable public type owner; extracted helper DTOs move to owning crates | `P`, `IPC`, `S`, `Cfg`, `Cred`, `D`, `Audit`, `Path` | Guard `rg 'fn main|crate::main|main::' crates/cc-* crates/gateway -g '*.rs'` should not show library dependency on root | `cargo check -p claude-code-rs --message-format short`; final `cargo check --workspace --all-targets --message-format short` |

## Current Guard Baseline

The Phase 0 shim baseline is reproducible with:

```bash
rg '#\[path = ' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'
```

Known cross-crate migration shims from the original Phase 0 guard:

- `crates/claude-code-rs/src/engine/lifecycle/mod.rs` imports
  `../../../../cc-engine/src/lifecycle/mod.rs`.
- `crates/claude-code-rs/src/ui/mod.rs` imports multiple `cc-ui/src/*`
  modules by `#[path]`.
- `crates/cc-ui/src/app.rs`, `messages.rs`, and `tui.rs` import many
  `crates/claude-code-rs/src/ui/**` files by `#[path]`.

2026-05-14 UI rollback update: the `cc-ui` / root UI shims above have been
removed. `cc-ui/src/` now only contains `lib.rs`, and UI source is intentional
root-owned under `crates/claude-code-rs/src/ui/**`.

Same-crate `#[path]` entries also exist for platform modules and tests in
`cc-computer-use`, `cc-query`, `cc-engine`, `cc-mcp`, and `cc-ui`; they are not
cross-crate migration bridges but should remain visible in the baseline output.

The root-private import guard is:

```bash
rg 'crate::(engine|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'
```

Phase 0 observed production-risk hits in `cc-engine` and `cc-ui`, especially:

- `cc-engine` still has root-style references to `teams`, `tools`, `mcp`,
  `computer_use`, `browser`, and `lsp_service` modules.
- `cc-ui` path-included sources still reference `engine`, `ipc`, `tools`,
  `teams`, `commands`, `voice`, and `lsp_service`.
- Some hits are comments documenting prior root ownership; later phases should
  keep comments separate from production dependency guards when closing a slice.

## Workspace Decycle Cut Index

This index is the Phase 1 handoff from
`workspace-decycle-plan-2026-05-14.md`.

| Cut | Current status | Phase 1 input | Exit guard |
| --- | --- | --- | --- |
| Cut 1: `cc-engine -> cc-ipc` | Completed first cut. `cc-engine` no longer depends on `cc-ipc`; root installs `RootAgentTreeRuntime` through `ipc::runtime_adapters::ensure_installed()` before `QueryEngine` creation. | Preserve adapter install ordering while moving more engine code. Do not reintroduce `cc-ipc` into `cc-engine`. | `cargo tree -p cc-engine -e normal --depth 1`; no `cc-ipc` dependency. |
| Cut 2: `cc-engine -> cc-ipc-client` | Open. `cc-engine::ipc_compat` still implements `cc_ipc_client` callback/query traits and `cc-engine/Cargo.toml` still depends on `cc-ipc-client`. | Move callback/query-turn host contract to `cc-types` or make `cc-ipc-client` consume closures installed by root glue. | `rg 'cc-ipc-client' crates/cc-engine/Cargo.toml crates/cc-engine/src -g '*.rs'`; `cargo check -p cc-engine --message-format short`. |
| Cut 3: `engine <-> tools` | Open. `cc-engine` still references `crate::tools::*`, and `cc-tools` should remain specs/schema/registry policy. | Add explicit tool registry/execution adapters; keep task/team/daemon/plugin runtime outside `cc-tools`. | `rg 'crate::tools::|cc_tools' crates/cc-engine crates/cc-tools crates/claude-code-rs/src -g '*.rs'`; `cargo check -p cc-engine --message-format short`; `cargo check -p cc-tools --message-format short`. |
| Cut 4: `browser <-> mcp` | Open. Plan requires deciding whether browser rendering/detection stays in `cc-browser` while `cc-mcp` owns MCP discovery/transport/config, or shared DTOs move to `cc-types::mcp`. | Remove `cc-mcp` dependency on browser runtime logic and keep browser-specific rendering/detection in `cc-browser`. | `cargo check -p cc-mcp --message-format short`; `cargo check -p cc-browser --message-format short`; `rg 'crate::browser::|cc_browser' crates/cc-mcp crates/cc-browser -g '*.rs'`. |
| Cut 5: `commands/ui/ipc` | Open. `cc-commands` has adapter foundations, while `cc-ui` still uses root-style direct access and `cc-ipc` must not depend on command/UI implementation. | Move shared status-line payloads, subsystem snapshots, command results, and plan workflow events to `cc-types` or `cc-ipc-protocol`; expose dispatcher contracts instead of direct globals. | `cargo check -p cc-commands --message-format short`; `cargo check -p cc-ui --message-format short`; `cargo check -p cc-ipc --message-format short`; `rg 'crate::(commands|ipc|ui)::' crates/cc-* crates/gateway -g '*.rs'`. |

## Phase Inputs And Blockers

- Phase 1 blocker: `cc-engine` still depends on `cc-ipc-client`. Remove or
  invert that edge before broad runtime moves.
- Phase 1 blocker: `cc-engine` still has root-style runtime references to
  `tools`, `teams`, `mcp`, `browser`, `computer_use`, and `lsp_service`.
- Phase 1 blocker: `cc-ui` is not yet a standalone UI source owner; it uses
  root UI files by `#[path]` and those files reference root runtime modules.
- Phase 1 blocker closed by Phase 6: `commands/remote_cmd.rs` no longer
  borrows root daemon `gateway_client.rs` by `#[path]`; full reusable daemon
  client ownership remains a Phase 7/9 crate-owner follow-up.
- Phase 2 input: freeze owners for `SdkMessage`, `QueryParams`,
  `ToolUseContext`, `AppState`, command result DTOs, subsystem snapshots,
  plan workflow events, worker command/event files, and status-line payloads.
- Phase 3 input: path-isolation tests must cover `~/.cc-rust/`,
  `.cc-rust/settings.json`, `.cc-rust/skills/`,
  `~/.cc-rust/credentials.json`, keychain service `cc-rust`, and daemon state
  under `~/.cc-rust/daemon/`.
- Phase 8 input: decide whether `voice/` gets a new `cc-voice` crate before
  migrating UI voice controller references.

## Phase 2 Contract Closeout

Phase 2 fixed one cross-crate contract without moving runtime ownership:

- `StatusLinePayload` and nested DTOs now live in `cc-types::status_line`.
  `cc-engine::status_line::payload` keeps runtime assembly helpers,
  git/workspace probing, context-window calculation, and compatibility
  re-exports for existing imports.
- `BackendMessage::StatusLineUpdate` still carries an opaque JSON `payload`
  field. Its serialized `type`, camelCase payload keys, omitted `None` fields,
  and `lines` / `error` behavior are covered by `cc-ipc-protocol` tests.

Current/final owner decisions for the requested Phase 2 contracts:

| Contract | Current owner after Phase 2 | Final owner | Cross-crate visible surface |
| --- | --- | --- | --- |
| `SdkMessage` | `cc-engine::sdk_types`, with root `engine::sdk_types` re-export/shim | `cc-engine` | SDK/TUI/web/daemon consumers import the engine-owned message enum; wire mapping remains in `cc-ipc` or root adapters until IPC runtime migration closes. |
| `QueryParams` | `cc-engine::types::config` | `cc-query` or `cc-types` split after tool/app-state adapters exist | Query loop may consume it through explicit engine/query adapters; do not move while it still carries runtime tool/app-state-adjacent config. |
| `ToolUseContext` | `cc-engine::types::tool` | runtime tool execution owner (`cc-tools` contract plus engine/root runtime adapter) | Only stable callback, permission, hook, command-dispatcher, and progress shapes should leak to `cc-types`; concrete app-state access and execution context remain runtime-owned for now. |
| `AppState` | `cc-engine::types::app_state` | likely `cc-engine` until UI/config/status runner fields are split | Other crates should receive snapshots or narrow adapters instead of depending on the full mutable state bag. |
| Command result DTOs | `cc-commands::CommandResult` | `cc-commands`; shared display/wire payloads move to `cc-types` / `cc-ipc-protocol` only when needed | Root command handlers can use `cc-commands`; IPC should not depend on command execution internals. |
| Subsystem snapshots | `cc-ipc-protocol::subsystem_types::SubsystemStatusSnapshot` | `cc-ipc-protocol` for wire snapshots | Runtime builders stay in `cc-ipc` / root adapters; status tool can consume the DTO without owning subsystem runtimes. |
| Plan workflow events | `cc-types::plan_workflow::PlanWorkflowRecord` plus `BackendMessage::PlanWorkflowEvent` | record in `cc-types`, wire event wrapper in `cc-ipc-protocol` | Engine/commands persist and mutate the record; IPC only serializes event name, summary, and record. |
| Worker command/event files | root `daemon/` today | `cc-daemon` or `cc-ipc-protocol` depending on whether they are daemon-internal files or public wire envelopes | Keep filesystem paths and process supervision out of contract crates. |
| Status-line payloads | `cc-types::status_line`; assembly in `cc-engine` | DTO in `cc-types`, runner/assembly in `cc-engine` | TUI, IPC, commands, and scripts share the DTO while subprocess execution stays in `cc-engine::status_line::runner`. |

Phase 3/next blockers:

- Root still exposes `engine/sdk_types.rs` as a shim. It should become a pure
  re-export or be deleted once root consumers consistently import
  `cc_engine::sdk_types`.
- `QueryParams`, `ToolUseContext`, and `AppState` still include runtime-heavy
  fields and should not be moved wholesale before tool execution, app-state
  snapshots, and callback adapters are narrower.
- The Phase 2 DTO scan still shows daemon command kinds,
  UI-local events, voice events, team messages, and the root SDK shim as later
  migration candidates.

## Phase 3 Slice Closeout

Phase 3 completed one path-isolation and ownership slice for leaf/domain
crates:

- `cc-config::paths` now owns the project skill directory helper
  (`{cwd}/.cc-rust/skills/`) alongside the existing data-root, credentials,
  global skills, and daemon path helpers.
- `cc-skills` uses `cc_config::paths::project_skills_dir()` when loading
  project skills, so project skill discovery no longer hand-builds that path.
- `cc-auth` exposes and uses `api_key::KEYCHAIN_SERVICE_NAME = "cc-rust"` for
  keychain API-key storage, with a guard test against upstream service names.
- `cc-auth::codex_cli` keeps the `~/.codex/auth.json` compatibility source as
  read-only during credential parsing, with a regression test that the fallback
  file content is unchanged.
- `cc-config` has a Phase 3 path guard covering `$CC_RUST_HOME` /
  `~/.cc-rust/`, user settings, `.cc-rust/settings.json`,
  `.cc-rust/skills/`, credentials, and daemon state paths.

Observed ownership/path-isolation state after this slice:

- Root `main.rs` still keeps compatibility aliases such as `use cc_config as
  config`, `use cc_auth as auth`, `use cc_session as session`, `use cc_compact
  as compact`, `use cc_permissions as permissions`, `use cc_sandbox as
  sandbox`, `use cc_skills as skills`, and `use cc_observability as
  observability`. These are re-export-style binary glue, not canonical leaf
  implementations.
- Root daemon process-state code still owns daemon runtime persistence files,
  but obtains the daemon directory through `cc-config` and keeps state under
  `~/.cc-rust/daemon/`. Do not move daemon runtime to Phase 3 crates.
- The required path guard still reports `~/.codex/auth.json` in `cc-auth`,
  `cc-api` provider auth description, and login/UI copy. Treat these as the
  intentional Codex CLI compatibility reader/import surface; it must remain
  read-only with any writes directed to cc-rust credentials.
- Root tool/UI surfaces still contain binary/runtime-specific config editing
  and display logic. Consolidating those is a later runtime/tool/UI phase task,
  not Phase 3 leaf crate ownership.

Phase 4 blockers:

- `tools/config_tool.rs` is still root tool execution code and hand-builds the
  project settings path. Phase 4 should either keep it as tool runtime glue
  using `cc-config` helpers or migrate only the pure config contract without
  moving tool execution into `cc-config`.
- Tool permission preflight still crosses root tool execution, `cc-permissions`,
  and `cc-sandbox`; Phase 4 needs explicit execution contracts without pulling
  engine/tool runtime into leaf crates.
- `cc-api` provider metadata still mentions the Codex CLI fallback path for UI
  display; future guards should distinguish compatibility strings from actual
  persistence writes.

## Phase 4 Slice Closeout

Phase 4 completed one tools/config boundary slice:

- Root `tools/config_tool.rs` remains tool execution/runtime glue, but its
  project settings path now resolves through
  `cc_config::settings::project_settings_path(&cwd)` instead of hand-building
  `cwd/.cc-rust/settings.json`.
- ConfigTool still owns its root runtime execution and JSON read/write adapter
  logic for now; no tool execution was moved into `cc-config`.
- A focused ConfigTool unit test covers the no-existing-project-dir case where
  the resolved settings path remains `{cwd}/.cc-rust/settings.json`.

Validation run for this slice:

- `cargo check -p cc-tools --message-format short`
- `cargo check -p cc-tasks --message-format short`
- `cargo check -p cc-permissions --message-format short`
- `cargo check -p cc-sandbox --message-format short`
- `cargo test -p cc-tools`
- `cargo test -p cc-tasks`
- `cargo test -p claude-code-rs tools::config_tool::tests::test_project_settings_path_uses_cc_rust_project_file --message-format short`
- `cargo check -p claude-code-rs --message-format short`
- `rg 'crate::tools::' crates/cc-* crates/gateway -g '*.rs'`

The cargo commands passed. The root crate check emitted only the known local
environment warning that `npm` is unavailable, so web-ui dependency install was
skipped.

Remaining Phase 4 / Phase 5 blockers:

- Root `tools/` still owns most concrete tool execution, including shell/file,
  hooks, web, skill, team, task, LSP, worktree, and plugin runtime adapters.
  Continue moving only pure metadata/schema/contracts to `cc-tools`; keep
  runtime execution with the owning runtime crate or binary adapter.
- `cc-engine` still has root-style `crate::tools::*` references for execution
  contracts and runtime tool catalog refresh. Phase 5 should replace these with
  explicit engine/tool adapters before deleting root engine shims.
- `cc-ui` path-included adapter code still references root `tools::tasks`.
  Treat this as UI/runtime wiring debt, not as a reason to move task execution
  or UI state into `cc-tools`.
- Permission preflight, hook criticality/progress callbacks, and sandbox
  preflight still need narrow low-level contracts across `cc-permissions`,
  `cc-sandbox`, `cc-tools`, and engine/root runtime glue. Preserve fail-closed
  Bash/PowerShell sandbox semantics while splitting those contracts.
- TaskTools remote/multi-type runtime parity remains separate from crate
  migration. `cc-tasks` owns task DTO/parsing/domain behavior today; root
  runtime still owns store/tool adapter integration.

## Phase 5 Slice Closeout

Phase 5 completed one engine lifecycle ownership slice:

- Root `engine/lifecycle/mod.rs` no longer path-includes
  `cc-engine/src/lifecycle/mod.rs`; it is now a normal compatibility re-export
  of `cc_engine::lifecycle::*`.
- Root `engine/mod.rs` now declares the local lifecycle compatibility module,
  while `cc-engine` remains the source owner for `QueryEngine`, lifecycle
  state, and lifecycle public types.
- `cc-engine` agent boundary tests no longer depend on the global runtime tool
  adapter. They use local stub tools when checking builtin/custom agent tool
  allow/deny filtering, which keeps adapter-default behavior explicit.
- `cc-engine` prompt-section cache coverage no longer asserts directly against
  global cache state that can be cleared by parallel tests in other modules.

Validation run for this slice:

- `cargo check -p cc-engine --message-format short`
- `cargo test -p cc-engine`
- `cargo check -p cc-query --message-format short`
- `cargo test -p cc-query`
- `cargo check -p claude-code-rs --message-format short`
- `rg 'crate::engine::lifecycle|#\[path = .*cc-engine' crates/claude-code-rs/src -g '*.rs'`

All cargo commands passed. The root crate check emitted only the known local
environment warning that `npm` is unavailable, so web-ui dependency install was
skipped. The guard command still reports root compatibility call sites using
`crate::engine::lifecycle::*`, but no longer reports any
`#[path = .*cc-engine]` migration bridge in root source.

Remaining Phase 5 / Phase 6 blockers:

- Root IPC, daemon, web, teams, shutdown, startup, UI, and root agent glue still
  import `crate::engine::lifecycle::*`. These should move either to direct
  `cc_engine::lifecycle::*` imports or to phase-specific adapters as their
  owning crates close.
- `cc-engine` still contains runtime-heavy tool, hook, permission, task,
  teammate, and command dispatch references behind partial adapters. Continue
  narrowing these contracts before treating engine/query ownership as complete.
- Agent tree registration/update/snapshot/active-count now has in-memory
  `cc-engine` coverage, but IPC-backed adapter scenarios still need explicit
  tests before the agent runtime closure exit condition is fully satisfied.
- Phase 6 command migration should avoid pulling engine/runtime state into
  `cc-commands`; command submission, plan workflow persistence, daemon/team
  integrations, and tool catalog refresh remain adapter-boundary work.

## Phase 6 Slice Closeout

Phase 6 completed one command/daemon boundary slice without changing `/remote`
behavior:

- Root `commands/remote_cmd.rs` no longer path-includes
  `../daemon/gateway_client.rs`. It now imports the daemon-owned gateway client
  through the normal root module path, and `daemon/mod.rs` exposes
  `gateway_client` as the owner module.
- `/remote` and `/channels` stay root binary command glue for this slice because
  the local gateway client still reads daemon process state and control tokens.
  Those runtime integrations should move only when `cc-daemon` owns the
  reusable local client/process-state contract.
- No daemon, engine, team, tool runtime, or plan-workflow behavior was moved
  into `cc-commands`.

Validation run for this slice:

- `cargo check -p cc-commands --message-format short`
- `cargo test -p cc-commands`
- `cargo check -p cc-daemon --message-format short`
- `cargo check -p claude-code-rs --message-format short`
- `cargo test -p claude-code-rs commands::remote_cmd`
- `cargo test -p claude-code-rs commands::channels`
- `rg '#\[path = "../daemon/gateway_client.rs"\]|crate::commands::' crates/claude-code-rs/src crates/cc-* -g '*.rs'`

All cargo commands passed. Root crate check/test emitted only the known local
environment warning that `npm` is unavailable, so web-ui dependency installation
was skipped. The Phase 6 guard no longer reports the gateway-client `#[path]`
shim; remaining hits are root/UI/daemon command-dispatch glue such as
`DefaultCommandDispatcher`, `/channels` calling `/remote adapters`, web command
handlers, gateway bridge command metadata, and the existing `cc-ui` voice
feasibility path-included call.

Phase 7 / Phase 9 blockers:

- `cc-daemon` is still a scaffold and does not yet own the reusable local
  gateway client or daemon process-state/control-token reader. Moving that
  contract requires daemon path-isolation and credential redaction coverage.
- Root daemon/web/gateway bridge code still calls `crate::commands::*` for
  command metadata and dispatch. Phase 7 should keep those as root adapters or
  narrow command-dispatch contracts instead of importing command implementation
  internals into daemon crates.
- `cc-ui` still path-includes root UI sources that call root command/voice
  modules. UI closure should use explicit UI-facing DTOs/adapters before
  deleting those residual `crate::commands::` hits.

## Phase 7 Slice Closeout

Phase 7 completed a focused `cc-ipc-client` test slice without changing runtime
architecture, headless JSONL field names, serde tags, ordering assumptions, or
error classification:

- `cc-ipc-client` remains the client transport/callback/query-turn helper owner.
  Added client-side tests proving that the Phase 1 contracts now owned by
  `cc-types` still let `cc-ipc-client` install permission, AskUser, and
  tool-progress callbacks, including `ExitPlanMode` rejection hook dispatch.
- Added a query-turn helper test proving `spawn_query_turn` resets abort state,
  submits through the `cc-types::query_host::QueryTurnHost` contract, and maps
  SDK messages to the frontend sink in stream order.
- `cc-ipc-protocol` remains the wire DTO owner, and `cc-ipc` remains the
  runtime/handler orchestration scaffold. Neither was expanded in this slice.

Boundary audit for this slice:

- Root `ipc/` still owns binary-private headless runtime glue, stdio loop,
  startup adapter installation, SDK mapping that depends on root engine state,
  and root runtime adapters.
- `cc-ipc-protocol` depends only on contract-level crates and serialization
  crates; it does not own filesystem, subprocess, terminal, daemon worker, or
  model-provider behavior.
- `cc-ipc-client` depends on `cc-types` host traits and `cc-ipc-protocol` DTOs;
  it does not import root engine/tool/command modules.
- `cc-ipc` uses `cc-types` and `cc-ipc-protocol` contracts plus host adapters;
  it does not import root engine/tool/command modules.

Validation run for this slice:

- `cargo check -p cc-ipc-protocol --message-format short`
- `cargo test -p cc-ipc-protocol`
- `cargo check -p cc-ipc-client --message-format short`
- `cargo test -p cc-ipc-client`
- `cargo check -p cc-ipc --message-format short`
- `cargo check -p claude-code-rs --message-format short`
- `rg 'crate::(engine|tools|commands)::' crates/cc-ipc crates/cc-ipc-client -g '*.rs'`

All cargo commands passed. The root crate check emitted only the known local
environment warning that `npm` is unavailable, so web-ui dependency
installation was skipped. The Phase 7 guard returned no matches.

Phase 8 blockers:

- Root subsystem bridge code still connects MCP/browser/plugin/LSP/IDE/skill
  runtimes through root `ipc/runtime.rs`, `ipc/runtime_adapters.rs`, and
  `ipc/subsystem_handlers.rs`; Phase 8 should move those domain runtime
  boundaries through explicit owner-crate adapters instead of widening
  `cc-ipc`.
- `cc-mcp` / `cc-browser` ownership is still coupled through root discovery,
  auth, and rendering adapters. Phase 8 must keep browser-specific detection
  and rendering in `cc-browser` while MCP transport/discovery contracts stay in
  `cc-mcp`.
- `cc-ui` still path-includes root UI sources that call command/voice/LSP
  modules. Phase 8/10 should use UI-facing DTOs and adapters before removing
  those residual root-style references.

## Phase 8 Slice Closeout

Phase 8 completed a focused browser/MCP owner-boundary test slice without
moving daemon, web, UI, or broad runtime startup code:

- Added `cc-mcp` boundary coverage proving the MCP crate does not depend on
  `cc-browser` and does not import `cc_browser` from its source tree. The
  `browserMcp` setting remains a data-only config flag on `McpServerConfig`;
  detection and rendering behavior are not pulled into MCP discovery/transport.
- Added `cc-browser` boundary coverage proving browser-specific MCP tool
  detection and result rendering stay in `cc-browser`, while the browser bridge
  may use `cc-mcp` protocol/config DTOs in the allowed dependency direction.
- No platform-specific computer-use, LSP, voice, daemon, web, or TUI code was
  moved in this slice.

Owner-state audit for Phase 8 domains:

- `cc-teams` owns only the scaffold plus team tool specs today. The full
  teammate runtime, coordinator, mailbox, protocol, in-process worker, and team
  memory integration still live primarily in root `teams/` and engine/UI glue.
- `cc-plugins` remains a scaffold; plugin manifest loading, refresh/install
  state, and plugin MCP contribution hooks are still root-hosted.
- `cc-mcp` owns MCP protocol, discovery, config, auth, transport, runtime
  manager, and MCP tool DTOs. Root still installs plugin/IDE hooks and adapts
  subsystem events.
- `cc-browser` owns browser detection, native host, session/state, setup,
  transport, permission categorization, and result rendering. Engine/root still
  contain tool-registry adapters because the `Tool` trait has not fully left
  root-style engine code.
- `cc-lsp-service` is still a scaffold; LSP lifecycle and adapters remain
  root/UI integrated.
- `cc-computer-use` owns screenshot and input platform primitives; detection,
  setup, and tool wrappers still remain in root/engine wrappers.
- `voice/` remains root binary-only for now. Do not introduce `cc-voice` until
  reusable controller/STT/audio contracts need to be consumed by non-root
  crates; current `cc-ui` voice references are still path-included UI closure
  work for Phase 10.

Guard baseline after this slice:

- `rg 'crate::(teams|plugins|mcp|browser|lsp_service|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'`
  still reports root-style hits in `cc-engine` for teams, MCP, browser,
  computer-use, and LSP wrappers, and in `cc-ui` path-included sources for
  teams, voice, and LSP. It also reports `cc-commands` same-crate `browser`
  helper imports and one comment-only `cc-mcp` plugin-discovery note.

Phase 9 blockers:

- Root IPC subsystem bridge still wires MCP/browser/plugin/LSP/IDE/skill
  callbacks and should not be widened into `cc-ipc` while Phase 9 moves daemon,
  gateway, and web ownership.
- `cc-daemon`/`gateway` closure must keep team/plugin/tool interactions behind
  adapters or stable DTOs; `cc-teams` and `cc-plugins` are not ready to own
  daemon-facing runtime behavior.
- Browser/MCP direction is now covered by tests, but engine wrappers still need
  a later adapter cleanup before the root-style import guard can close.

## Phase 9 Slice Closeout

Phase 9 completed a focused daemon worker file-protocol contract slice without
moving the root daemon runtime or changing serialized command/event shape:

- Added `cc-daemon::protocol` as the reusable owner for daemon worker
  command/event DTOs plus pure file-name and NDJSON format helpers. The module
  has no root runtime imports and does not call engine, tools, commands,
  plugins, teams, or plan workflow code.
- Added `cc-daemon` compatibility tests that lock the root worker command JSON
  fields, snake_case enum values, optional gateway payload context, event NDJSON
  line framing, and existing path-component sanitization behavior.
- Left process state, local daemon control token reading, HTTP/SSE routes,
  gateway bridge, webhook, notification, scheduler tick, and web static/SSE
  handlers in root `claude-code-rs` for now. Those remain runtime ownership
  work, not contract ownership work.

Owner-state audit for Phase 9 domains:

- Root `daemon/` still owns daemon process/server/worker implementation,
  supervisor lifecycle, process-state persistence, control token verification,
  local gateway client, route wiring, webhook handling, notification, gateway
  bridge, team-memory proxy, and tick logic.
- Root `web/` still owns reusable-looking static file, SSE, web state, and
  handler code; binary-only port binding/startup remains root as expected.
- `gateway` continues to own remote-control gateway models, auth helpers,
  adapters, persistence, API router foundations, webhook validation/rendering,
  runner, and delivery foundations. Local daemon `/remote-control/v1/*` route
  binding remains root daemon glue.
- `commands/remote_cmd.rs` imports the daemon-owned local gateway client
  through normal root module ownership; there is no `#[path]` shim.
- `cc-daemon` and `gateway` have no direct root-style imports matching
  `crate::(engine|tools|commands|plugins|teams|plan_workflow)::`.

Remaining Phase 9 / Phase 10 blockers:

- Replace root daemon command/event DTO duplication with `cc-daemon::protocol`
  once the process-state path owner and root adapter boundary are ready.
- Move daemon process state, local control-token reader, gateway client, and
  HTTP/SSE route contracts behind `cc-daemon` adapters without importing engine,
  tools, commands, teams, plugins, or plan workflow runtime directly.
- Decide whether reusable `web/` static/SSE/state handlers belong in
  `cc-daemon` or a separate web owner crate before moving root startup glue.
- Keep team/plugin/tool interactions behind adapters until `cc-teams`,
  `cc-plugins`, and tool execution ownership are ready for daemon-facing
  runtime integration.

## Phase 10 Slice Closeout

Phase 10 completed a focused Rust TUI message/render helper ownership slice:

- Moved `shutdown_message` and `wrap` from root
  `crates/claude-code-rs/src/ui/messages/` into
  `crates/cc-ui/src/messages/`, then updated `cc-ui/src/messages.rs` to use
  local module paths for both helpers.
- Deleted the migrated root message helper files so the copied source has a
  single owner for this slice.
- Left terminal raw mode, signal handling, event-channel wiring, engine
  startup handoff, and daemon/headless glue unchanged in root-owned TUI
  runtime code.

Owner-state audit for Phase 10 domains:

- `cc-ui` now owns the migrated shutdown message formatter and message line
  wrapping helper.
- `cc-ui` still path-includes many root `ui/app`, `ui/tui`, and
  `ui/messages` files. The guard currently reports 54
  `cc-ui -> claude-code-rs/src/ui` path hits after this slice.
- Root `ui/mod.rs` still has 7 reverse `#[path]` imports into `cc-ui` for
  extracted facades. These remain compatibility shims until the root facade can
  import the `cc-ui` crate normally.

Remaining Phase 10 / Phase 11 blockers:

- Move the remaining message helpers and `messages/render.rs` to local
  `cc-ui/src/messages/**` ownership, then remove the generated root-path block
  in `cc-ui/src/messages.rs`.
- Split `cc-ui/src/app.rs` and `cc-ui/src/tui.rs` so reusable state/render/input
  modules move into `cc-ui` while terminal lifecycle and runtime startup glue
  stay in root.
- Replace root-style reads of commands, voice, LSP, tools, teams, and IPC
  globals from UI code with DTOs or explicit adapters before deleting broad
  app/tui shims.
- Convert root `ui/mod.rs` reverse `#[path = "../../../cc-ui/src/..."]` shims
  into normal `cc-ui` crate imports during Phase 11 binary thinning.

## Phase 11 Root Engine Duplicate Closeout

Phase 11 completed a safe root binary thinning slice for stale root engine
implementation copies:

- Deleted root duplicate files for `engine/codex_exec.rs`, `engine/effort.rs`,
  `engine/input_processing.rs`, `engine/output_style.rs`,
  `engine/prompt_sections.rs`, `engine/result.rs`, `engine/sdk_types.rs`, and
  `engine/system_prompt.rs`.
- Left root `engine/agent` and `engine/lifecycle` untouched. Root
  `engine/mod.rs` continues to expose compatibility re-exports from
  `cc_engine::{codex_exec, effort, input_processing, output_style,
  prompt_sections, result, sdk_types, system_prompt}`.
- Guarded the deletion with `rg` checks for `#[path]` and module declarations.
  The only module declarations for these names are now in `cc-engine/src/lib.rs`;
  root source no longer declares or path-includes the deleted duplicate files.
- Ran the broader Phase 11 guard set in count form after the deletion. Current
  baseline remains: 54 `cc-ui -> claude-code-rs/src/ui` path shims, 7 root
  `ui/mod.rs -> cc-ui` path shims, 70 root-style import hits in `cc-*`/gateway,
  and 474 `allow(dead_code|unused_imports|unused)` hits across the scanned
  crates.

Remaining Phase 11 blockers:

- Root compatibility re-exports remain in `engine/mod.rs`; call sites still need
  gradual migration to direct `cc_engine::*` imports before those re-exports can
  be removed.
- `cc-ui` path shims remain in both directions: `cc-ui` still path-includes
  root UI files, and root `ui/mod.rs` still path-includes selected `cc-ui`
  facades.
- Broader Phase 11 guards still report expected root-style imports in migration
  areas outside this stale-engine-file slice.

## Phase 12 Verification Snapshot

Phase 12 build/test gates were made green on 2026-05-14 after fixing the
default-test isolation issues found during the first verification attempt. Crate
migration is still not closed because the thin-binary source guards below are
not zero. The active phase plan must remain active; no archive move or
completed-full entry was made.

Validation results:

- `cargo fmt --all --check` passed.
- `cargo check --workspace --all-targets --message-format short` passed. It
  emitted the known local environment warning that `npm` is unavailable, so
  web-ui dependency installation/build was skipped.
- `cargo test --workspace` passed. Live PTY/API tests are ignored by default and
  remain explicit live-verification targets.
- `cargo build --workspace --release` passed with the same known local `npm`
  warning.
- The earlier `cc-mcp` loopback failures were resolved by bypassing ambient
  proxies for loopback OAuth/Streamable HTTP clients.
- Additional default-test fixes covered global env/cwd isolation, task/session
  concurrent persistence, headless JSONL parsing, randomized `/quit` goodbye
  text, and live PTY/UI tests that require real model credentials/network.

Phase 12 guard counts:

```text
rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway -g '*.rs' | wc -l
54
rg '#\[path = ".*cc-' crates/claude-code-rs/src -g '*.rs' | wc -l
7
rg 'crate::(engine|query|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs' | wc -l
70
rg 'allow\((dead_code|unused_imports|unused)\)' crates/cc-* crates/claude-code-rs/src -g '*.rs' | wc -l
474
rg '~/.Codex|\.Codex/|~/.codex|service.*Codex|service.*Claude' crates -g '*.rs' | wc -l
12
```

The 12 path-isolation guard hits are the existing OpenAI Codex compatibility
reader/display surface around `~/.codex/auth.json` and Codex naming in
`cc-api`, `cc-auth`, login UI, and root login command copy. They still require
review as part of final Phase 12 closeout, but this verification pass did not
identify them as unexpected cc-rust write paths.

Concrete blockers preventing full Phase 12 completion:

- Cross-crate UI source shims remain: 54 `cc-ui -> claude-code-rs/src/ui`
  path includes and 7 reverse root `ui/mod.rs -> cc-ui` path includes.
- Root-style imports remain in `cc-*` / `gateway` migration areas, with 70
  guard hits across engine/query/tools/commands/daemon/ipc/teams/plugins/MCP/
  LSP/browser/computer-use/voice namespaces.
- The allow-attribute cleanup is still open, with 474
  `allow(dead_code|unused_imports|unused)` hits in the scanned crates.
- Root binary thinning is incomplete: `claude-code-rs` still depends directly
  on the broad runtime crate set, and source-level guards still show migration
  bridges.

## Phase 0 Minimal Validation

Use the local Rust toolchain from the workspace parent:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

Run:

```bash
rg '#\[path = ' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'
rg 'crate::(engine|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'
cargo check --workspace --all-targets --message-format short
```

If the workspace check fails or times out, record the exact command, exit code,
and first actionable compiler error in the Phase 0 closeout.

### Validation Snapshot

Phase 0 ran the two guard commands above on 2026-05-14. The first command
returned the expected current shim baseline: same-crate test/platform `#[path]`
entries plus the cross-crate migration bridges documented in this file. The
second command returned the expected root-style import baseline, with
production-risk hits concentrated in `cc-engine` and `cc-ui` and additional
comment-only hits in contract crates.

`cargo check --workspace --all-targets --message-format short` completed
successfully in the dev profile. The run emitted the known local environment
warning that `npm` is unavailable and the web UI dependency install was
skipped. It also emitted one existing Rust warning:

```text
crates/claude-code-rs/src/tools/exec/powershell.rs:475:8:
warning: function `parent_message` is never used
```

No Phase 0 code migration was performed to address that existing warning.
