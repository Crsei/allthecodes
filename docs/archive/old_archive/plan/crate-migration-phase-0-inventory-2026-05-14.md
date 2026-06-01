# Crate Migration Phase 0 Inventory

> 日期：2026-05-14
> 阶段：Phase 0 - Baseline And Ownership Inventory
> 上游计划：[`crate-migration-phase-plan-2026-05-14.md`](crate-migration-phase-plan-2026-05-14.md)

本文冻结当前 root 一级模块 owner、迁移 guard、后续 phase blocker 和 focused
验证命令。后续 phase worker 应先更新对应行，再移动代码；不要用历史
`rust-lite` 边界作为保留理由。

## Baseline Commands

运行 Phase 0 inventory 时使用本地 Rust 工具链：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```

当前 root 一级面来自：

```bash
find crates/claude-code-rs/src -mindepth 1 -maxdepth 1 -printf '%f\n' | sort
```

当前 root top-level 包含：

```text
browser cli.rs commands computer_use daemon dashboard.rs engine ide ipc
lsp_service main.rs mcp plan_workflow.rs plugins safety services shutdown.rs
startup teams tools types ui voice web worktree_hooks.rs
```

## Risk Legend

Owner matrix 中 `Risks` 列使用以下标记：

| 标记 | 含义 |
| --- | --- |
| `P` | persistence / filesystem write or durable state |
| `IPC` | IPC/headless/wire protocol shape |
| `Sess` | session transcript or session metadata |
| `Set` | settings schema, settings read/write, or config discovery |
| `Cred` | credentials, OAuth token, keychain, auth fallback |
| `Dmn` | daemon worker/server/event/supervisor state |
| `Aud` | audit/observability output or diagnostic event shape |
| `Path` | cc-rust path isolation risk: must remain under `~/.cc-rust/`, `.cc-rust/`, or service `cc-rust` |

## Owner Matrix

| Root module | Current shape | Target crate owner | Root retained glue | Public type / contract owner | Risks |
| --- | --- | --- | --- | --- | --- |
| `browser/` | Chrome/browser prompt, detection facade, runtime bridge pieces | `cc-browser` | CLI flag handoff, native-host binary entry, MCP bridge subprocess startup | Browser config/status DTOs in `cc-browser`; shared MCP/browser DTOs in `cc-types` if needed | `IPC`, `Set`, `Path` |
| `cli.rs` | Clap argument shape | Root thin binary, possibly `cc-bootstrap` only for reusable startup helpers | CLI parse and mode selection stays root | `Cli` remains root until a dedicated CLI crate is approved | `Dmn`, `Set`, `Path` |
| `commands/` | Slash command implementations and runtime bridge | `cc-commands` | Binary-only command adapters for engine, daemon, teams, plugins, browser, voice, UI | Command metadata/result DTOs in `cc-commands`; shared UI/IPC payloads in `cc-types` or `cc-ipc-protocol` | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `computer_use/` | Detection, setup, native tools facade | `cc-computer-use` | CLI enablement and permission adapter wiring | Computer-use tool/capability DTOs in `cc-computer-use`; shared permission contracts in `cc-types` | `P`, `Set`, `Aud`, `Path` |
| `daemon/` | Server, routes, supervisor, worker state, protocol duplicate | `cc-daemon` | Daemon binary entry, engine/command/team/webhook adapter install | Daemon DTOs in `cc-daemon::protocol` or `cc-ipc-protocol`; no root duplicate DTO | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `dashboard.rs` | Subagent dashboard companion, event log writer, child process | `cc-services` for event service, `cc-observability` for event DTO if reused | Optional dashboard process launch and browser open may stay binary glue | `SubagentEvent` should move to `cc-types` or `cc-observability` before cross-crate use | `P`, `Sess`, `Aud`, `Path` |
| `engine/` | Root facade over engine modules and aliases | `cc-engine` and `cc-query` | Adapter registration only | `QueryEngine`, lifecycle, SDK output in `cc-engine`; query loop in `cc-query` | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Aud`, `Path` |
| `ide/` | IDE detection and selected IDE persistence | `cc-services` or a future `cc-ide` if the domain grows | Startup/UI command handoff only | IDE config/status DTOs in target service crate; settings write API in `cc-config` | `P`, `Set`, `Path` |
| `ipc/` | Headless runtime, SDK mapping, subsystem handlers, host adapters | `cc-ipc`, `cc-ipc-client`, `cc-ipc-protocol` | stdio headless binary entry, process signal glue, adapter install | Wire DTOs in `cc-ipc-protocol`; client bridge in `cc-ipc-client`; runtime orchestration in `cc-ipc` | `P`, `IPC`, `Sess`, `Set`, `Dmn`, `Aud`, `Path` |
| `lsp_service/` | LSP client/transport/types/default config | `cc-lsp-service` | CLI/UI status adapter install only | LSP server/config/status DTOs in `cc-lsp-service`; settings schema in `cc-config` | `P`, `IPC`, `Set`, `Aud`, `Path` |
| `main.rs` | Application bootstrap and compatibility aliases | Root thin binary | CLI parse, startup mode selection, runtime adapter wiring, final process lifecycle | No reusable public DTO owner; remove compatibility aliases in Phase 11 | `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `mcp/` | MCP facade and tool wrapper still coupled to root tools | `cc-mcp` | Startup manager construction and runtime adapter install only | MCP config/tool DTOs in `cc-mcp`; cross-runtime event sink in `cc-types` if shared | `P`, `IPC`, `Set`, `Aud`, `Path` |
| `plan_workflow.rs` | Plan-mode persistent workflow service and engine state transitions | `cc-commands` for workflow commands, `cc-types` for record/status, adapter into `cc-engine` | Engine-specific mutation adapter may stay root until Phase 5/6 boundary is explicit | `PlanWorkflowRecord`/status already in `cc-types`; transition service target should not own root `QueryEngine` | `P`, `IPC`, `Sess`, `Set`, `Aud`, `Path` |
| `plugins/` | Plugin manifests, cache/materialization, refresh, plugin tools | `cc-plugins` | Startup refresh trigger and command adapter install | Plugin manifest/install/tool metadata in `cc-plugins`; settings schema in `cc-config` | `P`, `Set`, `Cred`, `Aud`, `Path` |
| `safety/` | Auto-mode safety classifier | `cc-permissions` if policy-only; `cc-engine` adapter if model call remains engine-owned | Runtime model invocation adapter | Classifier request/decision DTOs in `cc-permissions` or `cc-types` | `P`, `Sess`, `Set`, `Aud` |
| `services/` | Onboarding and session analytics service glue | `cc-services` | Startup scheduling and adapter install | Service DTOs in `cc-services`; analytics/audit DTOs in `cc-observability` if shared | `P`, `Sess`, `Set`, `Aud`, `Path` |
| `shutdown.rs` | SIGINT registration, graceful shutdown, session flush, audit | Root glue plus `cc-observability` / `cc-session` APIs | Signal handling and terminal reset stay root | Shutdown audit DTOs in `cc-observability`; session flush APIs in `cc-session` | `P`, `Sess`, `Aud`, `Path` |
| `startup/` | logging, modes, fast paths, runtime config | Root thin binary plus `cc-bootstrap` for reusable startup helpers | Mode selection, adapter order, binary entry composition | Settings/auth/session contracts in owner crates, not startup-local DTOs | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `teams/` | Team runtime, coordinator, mailbox, in-process workers | `cc-teams` | CLI/session adapter install and engine handoff | Team context/protocol DTOs in `cc-teams` or `cc-types` when shared | `P`, `IPC`, `Sess`, `Set`, `Dmn`, `Aud`, `Path` |
| `tools/` | Tool registry and many real tool implementations | `cc-tools`, `cc-tasks`, `cc-teams`, `cc-lsp-service`, domain crates by tool | Runtime tool registry adapter and root-only binary integrations | Tool metadata/contracts in `cc-tools` or `cc-types`; task DTOs in `cc-tasks`; permissions in `cc-permissions` | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `types/` | Root facade/re-export of shared types | `cc-types`; wire pieces in `cc-ipc-protocol` | None except short migration re-export until Phase 11 | `AppState`, `ToolUseContext`, `Tool`, `SdkMessage` ownership must be explicit before runtime moves | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `ui/` | Rust TUI source; intentional root owner in current plan | Root-owned Rust TUI; `cc-ui` remains empty boundary crate | Entire TUI state/render/input surface remains root for now | UI-facing DTOs should come from owner crates; root UI must not become DTO owner for library crates | `P`, `IPC`, `Sess`, `Set`, `Cred`, `Dmn`, `Aud`, `Path` |
| `voice/` | Voice capability, controller, audio/STT unsupported surfaces | Future `cc-voice` or intentional root compatibility surface | CLI/UI command bridge until owner is approved | Voice capability/settings DTOs in target crate or `cc-types`; unsupported rationale in gaps if retained | `P`, `Set`, `Aud`, `Path` |
| `web/` | Web UI static files, handlers, SSE, state | `cc-daemon` or `cc-services` for reusable HTTP/SSE state; root for port binding | Web binary entry, port binding, browser open | Web route/state DTOs in owner crate; no root-only persisted DTO | `P`, `IPC`, `Sess`, `Set`, `Dmn`, `Aud`, `Path` |
| `worktree_hooks.rs` | Worktree hook policy and path validation | `cc-sandbox` for path policy, `cc-types` for hook DTOs, possibly `cc-tools` for tool integration | Root adapter only until tool/agent owners are separated | Hook DTOs already in `cc-types`; worktree path policy must use `cc-config::paths` | `P`, `Set`, `Aud`, `Path` |

## Guard Matrix

| Guard | Command | Current baseline meaning |
| --- | --- | --- |
| All `#[path]` attrs | `rg '#\[path = ' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'` | 110 matches. They are local UI/internal module splits, platform module selection, or test modules; cross-crate migration shims are zero by the stricter guards below. |
| Library crate reads root source | `rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway -g '*.rs'` | 0 matches expected. Any match blocks Phase 11/12. |
| Root reads `cc-*` source by path | `rg '#\[path = ".*cc-' crates/claude-code-rs/src -g '*.rs'` | 0 matches expected. Any match is a new migration bridge. |
| Root-style runtime imports inside library crates | `rg 'crate::(engine|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'` | 37 matches at this baseline; includes comments plus real references in `cc-engine`, `cc-commands`, and related crates. Real references are blockers for Phases 1, 5, 8, and 11. |
| Path isolation | `rg '~/.Codex|\.Codex/|~/.codex|service.*Codex|service.*Claude' crates -g '*.rs'` | Current hits include documented/read-only Codex compatibility and UI text. Each migration touching persistence/auth must classify hits as read-only compatibility or fix them. |
| Migration allow debt | `rg 'allow\((dead_code|unused_imports|unused)\)' crates/cc-* crates/claude-code-rs/src -g '*.rs'` | Nonzero baseline. Each phase must remove allows in its touched owner area rather than add new ones. |
| Root compatibility aliases | `rg '^use cc_.* as ' crates/claude-code-rs/src/main.rs` | Tracks aliases preserving old root module paths. Phase 11 deletes them after call sites use owner crates directly. |
| Daemon DTO duplicate | `rg 'struct .*Command|enum .*Event|struct .*Payload' crates/claude-code-rs/src/daemon crates/cc-daemon/src -g '*.rs'` | Used before Phase 9 to ensure root daemon protocol is not the canonical duplicate owner. |
| IPC/session/settings wire DTO drift | `cargo test -p cc-ipc-protocol && cargo test -p cc-session && cargo test -p cc-config` | Roundtrip/schema tests should be added before behavior-affecting moves. |

### `#[path]` Baseline Details

The unfiltered `rg '#\[path = '` command is the reproducible known list. As of
this baseline it reports 110 matches:

```text
crates/cc-computer-use/src/input/mod.rs:3
crates/cc-computer-use/src/screenshot/mod.rs:3
crates/cc-engine/src/query/loop_impl.rs:1
crates/cc-mcp/src/client.rs:1
crates/cc-query/src/loop_impl.rs:1
crates/claude-code-rs/src/ui/app.rs:1
crates/claude-code-rs/src/ui/command_surface/surfaces/sandbox.rs:1
crates/claude-code-rs/src/ui/messages.rs:37
crates/claude-code-rs/src/ui/mod.rs:54
crates/claude-code-rs/src/ui/selection_surface.rs:2
crates/claude-code-rs/src/ui/tui.rs:6
```

Result semantics:

- Cross-crate migration bridge count is zero:
  `rg '#\[path = ".*claude-code-rs/src' crates/cc-* crates/gateway -g '*.rs'`
  and `rg '#\[path = ".*cc-' crates/claude-code-rs/src -g '*.rs'` both return
  no matches.
- `cc-computer-use` uses same-crate platform module selection.
- `cc-query`, `cc-engine`, and `cc-mcp` use same-crate test module paths.
- `claude-code-rs/src/ui/**` uses intentional root-owned Rust TUI internal
  module layout. It is not a migration bridge while Phase 10 keeps UI in root.

## Decycle Input Index

The current [`workspace-decycle-plan-2026-05-14.md`](workspace-decycle-plan-2026-05-14.md)
state is:

| Cut | Status | Phase input |
| --- | --- | --- |
| Cut 1 `cc-engine -> cc-ipc` | Completed first cut | Preserve `AgentTreeRuntime` adapter install order in root startup. |
| Cut 2 `cc-engine -> cc-ipc-client` | Open | Phase 1 blocker: move callback/query host contracts or install helpers out of direct engine dependency. |
| Cut 3 `engine <-> tools` | Open | Phase 1/4/5 blocker: tool execution and registry callbacks need explicit contracts. |
| Cut 4 `browser <-> mcp` | Open | Phase 8 blocker: browser rendering/detection must not be owned by `cc-mcp`. |
| Cut 5 `commands/ui/ipc` | Open | Phase 2/6/7 blocker: command results, status payload, subsystem snapshots, and UI-facing DTOs need shared owners. |

## Phase Blockers, Guards, And Focused Verification

| Phase | Primary blockers before code movement | Guard | Focused verification |
| --- | --- | --- | --- |
| 1 Cargo graph decycle | `cc-engine -> cc-ipc-client`; engine/tools runtime callbacks; browser/MCP ownership; commands/UI/IPC DTO ownership | `cargo tree -p cc-engine -e normal --depth 1`; `cargo tree -p cc-tools -e normal --depth 1`; root-style import guard | `cargo check -p cc-engine --message-format short`; `cargo check -p cc-tools --message-format short`; `cargo check -p cc-mcp --message-format short`; `cargo check -p cc-browser --message-format short`; `cargo check -p cc-commands --message-format short`; `cargo check -p cc-ui --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 2 Contracts and wire DTOs | `Tool`, `ToolUseContext`, `AppState`, `SdkMessage`, command/status/subsystem DTO final owners unclear | Root DTO duplicate guard: `rg 'struct .*Message|enum .*Event|enum .*Command|struct .*Payload' crates/claude-code-rs/src -g '*.rs'` | `cargo test -p cc-types`; `cargo test -p cc-ipc-protocol`; `cargo check -p cc-engine --message-format short`; `cargo check -p cc-ipc --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 3 Leaf/domain closure | Path isolation and schema tests must exist before moving config/auth/session/settings/credentials behavior | Path guard: `rg '~/.Codex|\.Codex/|~/.codex|service.*Codex|service.*Claude' crates -g '*.rs'` | `cargo check -p cc-config --message-format short`; `cargo check -p cc-auth --message-format short`; `cargo check -p cc-api --message-format short`; `cargo check -p cc-session --message-format short`; `cargo check -p cc-compact --message-format short`; `cargo check -p cc-permissions --message-format short`; `cargo check -p cc-sandbox --message-format short`; `cargo check -p cc-skills --message-format short`; `cargo check -p cc-observability --message-format short` |
| 4 Tools/tasks/hooks | Root `tools/` owns real execution; task/tool/hook contracts still mixed with runtime state | `rg 'crate::tools::' crates/cc-* crates/gateway -g '*.rs'` | `cargo check -p cc-tools --message-format short`; `cargo check -p cc-tasks --message-format short`; `cargo check -p cc-permissions --message-format short`; `cargo check -p cc-sandbox --message-format short`; `cargo test -p cc-tools`; `cargo test -p cc-tasks`; `cargo check -p claude-code-rs --message-format short` |
| 5 Engine/query/agent | `cc-engine` still has root-style runtime references; command/tool/MCP/team/browser/computer-use adapters are incomplete | `rg 'crate::(engine|tools|teams|mcp|browser|computer_use)::' crates/cc-engine crates/cc-query -g '*.rs'`; `rg 'crate::engine::lifecycle|#\[path = .*cc-engine' crates/claude-code-rs/src -g '*.rs'` | `cargo check -p cc-engine --message-format short`; `cargo test -p cc-engine`; `cargo check -p cc-query --message-format short`; `cargo test -p cc-query`; `cargo check -p claude-code-rs --message-format short` |
| 6 Commands/plan workflow | Root command implementations rely on engine/daemon/plugin/browser/voice/private state; plan workflow mutates root engine state | `rg '#\[path = "../daemon/gateway_client.rs"\]|crate::commands::' crates/claude-code-rs/src crates/cc-* -g '*.rs'` | `cargo check -p cc-commands --message-format short`; `cargo test -p cc-commands`; `cargo check -p cc-daemon --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 7 IPC/headless/client | Root `ipc/` owns runtime handlers and SDK mapping; protocol changes need roundtrip tests | `rg 'crate::(engine|tools|commands)::' crates/cc-ipc crates/cc-ipc-client -g '*.rs'` | `cargo check -p cc-ipc-protocol --message-format short`; `cargo test -p cc-ipc-protocol`; `cargo check -p cc-ipc-client --message-format short`; `cargo test -p cc-ipc-client`; `cargo check -p cc-ipc --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 8 Horizontal runtimes | `cc-teams`, `cc-plugins`, `cc-lsp-service` are still partial; MCP/browser/computer-use need shared tool contracts; voice owner unresolved | `rg 'crate::(teams|plugins|mcp|browser|lsp_service|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'` | `cargo check -p cc-teams --message-format short`; `cargo check -p cc-plugins --message-format short`; `cargo check -p cc-mcp --message-format short`; `cargo check -p cc-browser --message-format short`; `cargo check -p cc-lsp-service --message-format short`; `cargo check -p cc-computer-use --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 9 Daemon/gateway/web | Root `daemon/protocol.rs` duplicate DTO; server/supervisor/routes still root-owned; web reusable handlers not owned | `rg 'crate::(engine|tools|commands|plugins|teams|plan_workflow)::' crates/cc-daemon crates/gateway -g '*.rs'`; daemon DTO duplicate guard | `cargo check -p cc-daemon --message-format short`; `cargo test -p cc-daemon`; `cargo check -p gateway --message-format short`; `cargo check -p claude-code-rs --message-format short` |
| 10 UI/TUI boundary | UI is intentional root-owned; ensure `cc-ui` stays empty boundary and no cross-crate UI path bridge returns | `rg '#\[path = ".*claude-code-rs/src/ui' crates/cc-ui -g '*.rs'`; `rg '#\[path = ".*cc-ui' crates/claude-code-rs/src -g '*.rs'` | `cargo check -p claude-code-rs --message-format short` |
| 11 Root thinning/shim deletion | Compatibility aliases, re-export facades, duplicate implementations, and allow debt remain | Cross-crate path guards; root-style import guard; `rg 'claude[-_]code[-_]rs' crates/cc-* crates/gateway -g '*.rs' -g '*.toml'`; allow guard | `cargo check --workspace --all-targets --message-format short` |
| 12 Full verification/docs | Any nonzero migration bridge, unclassified path-isolation hit, or final gate failure blocks archive closeout | Final guard suite from active phase plan | `cargo fmt --all --check`; `cargo check --workspace --all-targets --message-format short`; `cargo test --workspace`; `cargo build --workspace --release`; selected `cargo tree -p ... -e normal --depth 1` checks |

## Phase 0 Minimal Verification

Required commands:

```bash
rg '#\[path = ' crates/claude-code-rs/src crates/cc-* crates/gateway -g '*.rs'
rg 'crate::(engine|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' crates/cc-* crates/gateway -g '*.rs'
cargo check --workspace --all-targets --message-format short
```

The first two commands are guard-producing commands, not pass/fail zero-match
commands at Phase 0. Their result sets are the baseline documented above.

Observed result in this workspace:

- `rg '#\[path = ' ...`: exit 0, 110 baseline matches; no cross-crate
  migration bridge by the stricter guards.
- `rg 'crate::(engine|tools|commands|daemon|ipc|teams|plugins|mcp|lsp_service|browser|computer_use|voice)::' ...`:
  exit 0, 37 baseline matches; includes comments plus real owner-boundary
  blockers.
- `cargo check --workspace --all-targets --message-format short`: exit 0.
  It emitted the known environment warning from `claude-code-rs` build script:
  web-ui dependency installation was skipped because `npm` is not installed
  (`No such file or directory (os error 2)`). No Phase 0 doc change introduced
  Rust warnings.
