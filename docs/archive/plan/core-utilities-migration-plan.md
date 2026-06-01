# Core Utilities and Team/Swarm Migration Plan

Date: 2026-05-18

Scope:
- Source inventory: `claude-code-bun/src/utils/`
- Rust utility doc: `docs/utils/core-utilities.md`
- Team/Swarm doc: `docs/utils/teams-swarm.md`
- Ownership boundary: `docs/reference/CRATE_DEPENDENCY_TARGETS.md`

## Summary

`core-utilities.md` should not be treated as a direct file-by-file order to
move every Bun `src/utils/` file into `cc-utils`. In the Rust workspace, many
capabilities that were historically placed under Bun `utils` already have
owner crates such as `cc-config`, `cc-session`, `cc-permissions`, `cc-sandbox`,
`cc-tasks`, `cc-teams`, `cc-mcp`, `cc-plugins`, `cc-observability`,
`cc-keybindings`, `cc-models`, `cc-browser`, and `cc-computer-use`.

The correct execution strategy is:
- Keep `cc-utils` limited to pure helper code.
- Move domain behavior to the crate that owns the runtime boundary.
- Treat Team/Swarm code as `cc-teams` work, not generic utility work.
- In Full Build mode, do not skip Bun behavior as a Lite simplification unless
  the decision is explicitly marked Intentional in docs and implementation.

## Currently Used Rust `cc-utils` Modules

| Rust module | Current status | Runtime usage |
|---|---|---|
| `bash.rs` | Active, core path | `cc-permissions`, `cc-sandbox`, `cc-engine` exec paths |
| `cwd.rs` | Active | `cc-tools/src/observable_input.rs` |
| `git.rs` | Active, partially used | root startup, `cc-session`, `cc-engine/system_prompt.rs`, `cc-daemon/team_memory_proxy.rs` |
| `git_operation_tracking.rs` | Active | Bash/PowerShell exec tracking in `cc-engine` |
| `messages.rs` | Partial | query loop, TUI previews, message rendering via `truncate_text` |
| `shell.rs` | Active | shell detection/env construction in exec tools |
| `tokens.rs` | Active, core path | `cc-compact`, `cc-session`, `cc-api`, `cc-engine` |
| `abort.rs` | Implemented but not clearly wired | Needs cancellation/timeout wiring or explicit TODO/cleanup decision |
| `file_state_cache.rs` | Implemented but duplicated | Current runtime uses `cc-tools::tool::FileStateCache`; decide merge or delete |

## Owner Mapping for Bun Utility Areas

| Bun utility area | Rust owner | Plan |
|---|---|---|
| `settings/**`, `config*.ts`, `cachePaths.ts`, `userAgent.ts`, `systemDirectories.ts` | `cc-config` | Do not move to `cc-utils`; audit coverage in config crate |
| `session*.ts`, `conversationRecovery.ts`, `crossProjectResume.ts` | `cc-session` or future `cc-state` | Continue under session/state ownership |
| `permissions/**`, `shell/readOnlyCommandValidation.ts` | `cc-permissions`, `cc-sandbox`, future `cc-execpolicy` / `cc-shell-command` | Keep risk/policy logic out of `cc-utils` |
| `task/**`, `tasks.ts`, `todo/**` | `cc-tasks` | Continue under tasks owner |
| `swarm/**`, `team*.ts`, `teammate*.ts` | `cc-teams` | Main focus for Team/Swarm gap closure |
| `mcp*.ts` | `cc-mcp` | Continue under MCP owner |
| `plugins/**` | `cc-plugins` | Audit separately by plugin behavior |
| `model/**`, `modelCost.ts`, `effort.ts`, `fastMode.ts` | `cc-models` and command/runtime adapters | Continue under model owner |
| `browser.ts`, `claudeInChrome/**` | `cc-browser` | Continue under browser owner |
| `computerUse/**` | `cc-computer-use` | Continue under computer-use owner |
| `telemetry/**`, `stats*.ts`, profiler files | `cc-observability` or profiler-specific module | Keep out of `cc-utils` |
| `keybindings*.ts`, `hotkeys.ts` | `cc-keybindings` and TUI | Continue under input/UI owner |
| `suggestions/**`, completion files | TUI/commands or future `cc-completions` | Rust has prompt suggestions but shell/directory completion needs audit |

## Team/Swarm Gap Matrix

| Bun capability | Rust counterpart | Status |
|---|---|---|
| `swarm/inProcessRunner.ts` | `cc-teams/src/runner.rs` | Main path migrated |
| `swarm/spawnInProcess.ts`, `InProcessBackend.ts` | `in_process.rs`, `team_spawn.rs` | Main path migrated |
| `swarm/reconnection.ts` | `reconnection.rs` | Wired into CLI startup and `/resume`; self-session persistence still has `TEAMS-001` |
| `swarm/teammateLayoutManager.ts` | `layout_manager.rs` | Color assignment wired; pane layout missing |
| `teammateContext.ts` | `context.rs` | Task-local semantics migrated |
| `teammate.ts` | `identity.rs`, `context.rs` | Partial; dynamic team context needs audit |
| `teammateMailbox.ts` | `mailbox.rs`, `protocol.rs`, `send_message.rs` | Basic mailbox migrated; full protocol/limits/compaction missing |
| `swarm/teamHelpers.ts` | `helpers.rs` | Basic create/read/write/active migrated; member removal, mode sync, hidden pane, cleanup incomplete |
| `swarm/permissionSync.ts` | partial plan-approval handling in `send_message.rs` | Full permission/sandbox sync missing |
| `swarm/leaderPermissionBridge.ts` | no full equivalent found | Needs migration or Intentional decision |
| `teamDiscovery.ts` | `/agents`, `/status`, dashboard snippets | Partial; needs unified discovery API |
| `directMemberMessage.ts` | no clear TUI input equivalent found | Needs migration |
| `teamMemoryOps.ts` | `team_memory_proxy.rs`, `cc_config::paths::team_memory_dir` | Proxy exists; search/write summary helpers missing |
| `TmuxBackend.ts`, `ITermBackend.ts`, `WindowsTerminalBackend.ts` | `backend.rs` abstraction only | Major gap |
| `PaneBackendExecutor.ts`, backend `registry.ts`, `detection.ts` | no full equivalent found | Major gap |

## Execution Phases

### P0: Rebuild the utility gap inventory

Write scope:
- `docs/utils/core-utilities.md`
- Optional new detailed matrix under `docs/plan/` or `docs/utils/`

Tasks:
- Replace the flat missing-file list with an owner-aware matrix.
- Mark every Bun utility as one of: `cc-utils`, existing owner crate, future
  owner crate, Intentional, or removed/not applicable.
- Link Team/Swarm rows to `docs/utils/teams-swarm.md` instead of duplicating
  details.

Exit criteria:
- The doc no longer implies all Bun `utils` files belong in `cc-utils`.
- Each high-level utility family has an owner and next action.

### P1: Clean up `cc-utils` scope and duplication

Write scope:
- `crates/cc-utils/src/*`
- Callers that import utility helpers
- Potentially `crates/cc-tools/src/tool.rs`

Tasks:
- Decide whether `abort.rs` should be wired into cancellation/timeout paths or
  left as explicit TODO.
- Resolve duplication between `cc-utils/src/file_state_cache.rs` and
  `cc-tools::tool::FileStateCache`.
- Keep public exports minimal and purely helper-oriented.

Exit criteria:
- `cc-utils` exports only helpers that are used or intentionally staged.
- No duplicate `FileStateCache` implementation remains without explanation.

### P2: Port low-risk pure helpers into `cc-utils`

Write scope:
- `crates/cc-utils/src/string.rs`
- `crates/cc-utils/src/json.rs`
- `crates/cc-utils/src/hash.rs`
- `crates/cc-utils/src/format.rs`
- `crates/cc-utils/src/semantic.rs`
- `crates/cc-utils/src/lib.rs`

Candidate Bun sources:
- `stringUtils.ts`
- `truncate.ts`
- `words.ts`
- `format.ts`
- `formatBriefTimestamp.ts`
- `semanticBoolean.ts`
- `semanticNumber.ts`
- `hash.ts`
- `uuid.ts`
- `json.ts`
- `jsonRead.ts`

Exit criteria:
- Pure helper tests exist for the migrated behavior.
- Callers stop carrying local one-off versions for these helpers.

### P3: Split shell, process, and filesystem utilities into owners

Write scope:
- `cc-utils` for pure parsing helpers only
- `cc-permissions` for allow/deny/read-only policy
- `cc-sandbox` for sandbox policy and summaries
- Future `cc-shell-command` or `cc-execpolicy` if created

Tasks:
- Extend shell parsing only where it has no permission semantics.
- Move command-risk behavior to permission/sandbox owner crates.
- Avoid making `cc-utils` depend on runtime state.

Exit criteria:
- Shell parsing, shell display, and permission decision logic have clear owners.

### P4: Complete Team mailbox protocol

Write scope:
- `crates/cc-teams/src/mailbox.rs`
- `crates/cc-teams/src/protocol.rs`
- `crates/cc-teams/src/send_message.rs`
- `crates/cc-teams/src/runner.rs`
- `crates/cc-teams/src/types.rs`

Candidate Bun sources:
- `teammateMailbox.ts`
- `inProcessTeammateHelpers.ts`

Tasks:
- Add structured protocol coverage for shutdown, plan approval, permission,
  sandbox permission, task assignment, team permission update, and mode set.
- Add mailbox limits, retained byte management, and compaction semantics.
- Add mark-as-read by identity/predicate equivalents.

Exit criteria:
- Protocol parsing/formatting is centralized.
- `send_message.rs` and `runner.rs` use typed helpers instead of ad hoc JSON.

### P5: Complete Team helper lifecycle

Write scope:
- `crates/cc-teams/src/helpers.rs`
- `crates/cc-teams/src/reconnection.rs`
- `crates/cc-teams/src/types.rs`

Candidate Bun sources:
- `swarm/teamHelpers.ts`
- `swarm/reconnection.ts`

Tasks:
- Add member removal by name/agent id.
- Add member mode sync and multi-member mode updates.
- Add session cleanup registration semantics where relevant.
- Persist teammate self-session IDs so `TEAMS-001` can be closed or narrowed.
- Add hidden pane fields only if pane backend migration proceeds.

Exit criteria:
- Team file lifecycle supports cleanup, restore, and active state transitions
  without relying on in-memory-only state.

### P6: Port Team permission bridge and synchronization

Write scope:
- New `crates/cc-teams/src/permission_sync.rs` or equivalent
- `crates/cc-teams/src/send_message.rs`
- `crates/cc-teams/src/runner.rs`
- `crates/cc-permissions` adapter points if needed

Candidate Bun sources:
- `swarm/permissionSync.ts`
- `swarm/leaderPermissionBridge.ts`

Tasks:
- Implement leader/worker permission request flow.
- Implement sandbox permission request flow.
- Route approvals through mailbox or direct in-process bridge as appropriate.
- Keep UI prompt implementation outside `cc-teams`; use stable DTOs.

Exit criteria:
- In-process teammates can request tool and sandbox permissions from the
  leader without bypassing the normal permission model.

### P7: Add Team discovery and direct member message input

Write scope:
- `crates/cc-teams`
- `crates/cc-commands`
- `crates/claude-code-rs/src/ui/input/*`
- relevant IPC adapters if needed

Candidate Bun sources:
- `teamDiscovery.ts`
- `directMemberMessage.ts`
- Prompt input call sites using direct team message syntax

Tasks:
- Add a unified team discovery API for member status, unread count, active
  state, backend type, and task state.
- Wire direct member message parsing from TUI input into team mailbox send.
- Reuse the same discovery output for `/agents`, `/status`, and dashboard.

Exit criteria:
- Team status surfaces are not separately parsing team files in incompatible
  ways.
- Direct teammate messaging works from the input path.

### P8: Add pane backend architecture

Write scope:
- `crates/cc-teams/src/backend.rs`
- New backend modules for registry/detection/pane execution
- Potentially `crates/cc-teams/src/constants.rs` and `types.rs`

Candidate Bun sources:
- `swarm/backends/types.ts`
- `swarm/backends/registry.ts`
- `swarm/backends/detection.ts`
- `swarm/backends/PaneBackendExecutor.ts`

Tasks:
- Define pane backend trait and detection result DTOs.
- Implement backend registry and mode resolution.
- Preserve InProcess as fallback, but do not silently reject Full Build pane
  modes unless Intentional is documented.

Exit criteria:
- `BackendType::Tmux`, `BackendType::ITerm2`, and
  `BackendType::WindowsTerminal` have architecture-level support.

### P9: Implement concrete pane backends

Write scope:
- `crates/cc-teams/src/tmux.rs`
- `crates/cc-teams/src/iterm.rs`
- `crates/cc-teams/src/windows_terminal.rs`
- backend registry/detection modules

Candidate Bun sources:
- `swarm/backends/TmuxBackend.ts`
- `swarm/backends/ITermBackend.ts`
- `swarm/backends/WindowsTerminalBackend.ts`
- `swarm/backends/it2Setup.ts`

Tasks:
- Implement tmux pane create/show/hide/kill/send-command/rebalance behavior.
- Implement iTerm2 setup and pane control where platform support exists.
- Implement Windows Terminal backend where platform support exists.
- Provide explicit unsupported messages per platform.

Exit criteria:
- `/team spawn --backend tmux|iterm2|windows-terminal` either works or fails
  with a precise platform/setup error.

### P10: Process remaining owner-crate utility families

Write scope:
- One owner crate per batch.

Suggested order:
- `cc-session`: session restore, title, URL, activity, storage portability.
- TUI/commands or `cc-completions`: shell/directory/slash suggestions.
- `cc-plugins`: plugin utility parity.
- `cc-observability`: profiler/stats/cache diagnostics.
- `cc-mcp`: validation/output storage/websocket transport.

Exit criteria:
- Each owner crate has its own parity checklist and completion tests.

### P11: Documentation closeout

Write scope:
- `docs/utils/core-utilities.md`
- `docs/utils/teams-swarm.md`
- `docs/IMPLEMENTATION_GAPS.md`
- `docs/archive/COMPLETED_FULL.md`

Tasks:
- Move completed items from gap docs to completed docs.
- Mark retained differences as Intentional.
- Keep user-visible issues in `docs/KNOWN_ISSUES.md`.

Exit criteria:
- Docs match runtime behavior and no longer describe already-wired code as
  missing.

## Recommended First Batch for Agents

Batch 1:
- P0 owner-aware inventory update.
- P1 `cc-utils` scope cleanup decision.
- Do not implement pane backends in this batch.

Batch 2:
- P4 mailbox protocol typed helpers.
- P5 team helper lifecycle, especially self-session persistence.

Batch 3:
- P6 permission bridge.
- P7 team discovery/direct member message.

Batch 4:
- P8 backend architecture.
- P9 concrete pane backends.

## Validation Guidance

Do not run broad workspace builds until a batch is ready.

Recommended targeted checks when implementation begins:
- `cargo test -p cc-utils`
- `cargo test -p cc-teams`
- `cargo test -p cc-commands team`
- targeted `cargo check -p claude-code-rs` only after runtime wiring changes

Use the repository-local Rust toolchain:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"
```
