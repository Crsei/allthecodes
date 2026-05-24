# commands-00 inventory checkpoint

Date: 2026-05-11

Scope: inventory only. No Rust code was moved or changed.

## Command module inventory

- Command ownership root: `crates/claude-code-rs/src/commands/`.
- Current command source footprint: 82 Rust files, including 9 files under `commands/mcp/`.
- Registry owner: `crates/claude-code-rs/src/commands/mod.rs`.
- Shared command contract:
  - `Command`, `CommandHandler`, `CommandContext`, and `CommandResult` live in `commands/mod.rs`.
  - `get_all_commands()` builds the registry, then `sort_commands_for_display()` keeps `/init` first and all other command names sorted.
  - `find_command()` and `parse_command_input()` own slash-name/alias lookup.
  - `DefaultCommandDispatcher` adapts the registry to `cc_types::commands::CommandDispatcher`.
- Cross-crate command trait owner: `crates/cc-types/src/commands.rs`; default no-op dispatcher is used by contexts that must not parse slash commands.
- Command UI surfaces are separate from command execution:
  - Command palette: `crates/claude-code-rs/src/ui/components/command_palette/`.
  - Command surface shell/adapters/surfaces: `crates/claude-code-rs/src/ui/components/command_surface/`.
  - Slash metadata for UI input: `crates/claude-code-rs/src/ui/input/slash_command.rs`.

## Dispatcher call sites

- TUI command executor: `crates/claude-code-rs/src/ui/tui/commands.rs` parses with `commands::parse_command_input()`, loads `get_all_commands()`, and executes `cmd.handler.execute(...)`.
- Headless IPC slash command path: `crates/claude-code-rs/src/ipc/ingress.rs` handles `FrontendMessage::SlashCommand`, parses, executes the handler, and emits backend protocol events.
- Daemon route path: `crates/claude-code-rs/src/daemon/routes.rs` parses slash commands from route input and executes handlers.
- Web command API: `crates/claude-code-rs/src/web/handlers.rs` lists commands via `get_all_commands()` and executes selected command handlers.
- Engine input-processing path: `crates/claude-code-rs/src/engine/input_processing.rs` depends only on `cc_types::commands::CommandDispatcher`.
- Engine bootstrap wiring:
  - `crates/claude-code-rs/src/main.rs` exposes command metadata in startup state and installs `DefaultCommandDispatcher`.
  - `crates/claude-code-rs/src/daemon/gateway_bridge.rs` exposes command metadata and installs `DefaultCommandDispatcher`.
  - `crates/claude-code-rs/src/teams/runner.rs` installs `DefaultCommandDispatcher` for team runner engines.
- Child/forked agents propagate an existing dispatcher through engine agent contexts; many test/tool contexts intentionally use `NoopCommandDispatcher`.

## Source-layout tests

Current source-layout checks exist, but two are stale against the helper-based command registry shape:

- `crates/claude-code-rs/tests/e2e_plan_cmd.rs`
  - Checks `pub mod plan;`, `plan::PlanHandler`, `/plan` subcommands, plan workflow storage, and cc-config path helper exposure.
  - Stale assertion: `plan_command_is_registered_in_get_all_commands` expects literal `name: "plan".into(),`; registry now uses `command("plan", ..., plan::PlanHandler)`.
- `crates/claude-code-rs/tests/e2e_context_cmd.rs`
  - Checks cross-crate `cc_compact::context_analysis`, context handler service delegation, JSON/raw subcommands, and command registry wiring.
  - Stale assertion: `context_command_is_still_registered` expects literal `name: "context"`; registry now uses `command("context", ..., context::ContextHandler)`.
- These are test-shape defects, not missing command registrations. `commands/mod.rs` currently wires both `context::ContextHandler` and `plan::PlanHandler`.

## Baseline command/e2e tests

Focused baselines run during this checkpoint:

| Command | Result | Notes |
| --- | --- | --- |
| `cargo test -p claude-code-rs commands:: -- --nocapture` | WARNING | 420 passed, 2 failed in parallel run: `permissions_cmd::test_permissions_allow_user_persist` hit Windows temp-file rename `os error 5`; `tasks_cmd::delete_removes_tool_task` expected deleted persisted task output. Both failed tests passed when rerun individually. |
| `cargo test -p claude-code-rs commands:: -- --test-threads=1 --nocapture` | WARNING | 421 passed, 1 failed: `commands::compact::tests::test_partial_compact_from_by_uuid_prefix`; that single test passed when rerun individually. Baseline shows command unit tests have order/isolation sensitivity. |
| `cargo test -p claude-code-rs --test e2e_terminal commands -- --nocapture` | WARNING | 11 passed, 2 failed: `/clear` path receives a `ready` event where tests expect `system_info` after `conversation_replaced`. |
| `cargo test -p claude-code-rs --test e2e_terminal commands::slash_clear -- --nocapture` | WARNING | Failed with `ready` event after `/clear`. |
| `cargo test -p claude-code-rs --test e2e_terminal commands::multiple_slash_commands_in_session -- --nocapture` | WARNING | Failed with `ready` event after `/clear`. |
| `cargo test -p claude-code-rs --test e2e_plan_cmd -- --nocapture` | WARNING | 5 passed, 1 stale source-layout assertion failed. |
| `cargo test -p claude-code-rs --test e2e_context_cmd -- --nocapture` | WARNING | 7 passed, 1 stale source-layout assertion failed. |

Recommended baseline set for future command edits:

- `cargo test -p claude-code-rs commands::<changed_module> -- --nocapture`
- `cargo test -p claude-code-rs commands::tests -- --nocapture`
- `cargo test -p claude-code-rs --test e2e_terminal commands -- --nocapture`
- `cargo test -p claude-code-rs --test e2e_plan_cmd -- --nocapture`
- `cargo test -p claude-code-rs --test e2e_context_cmd -- --nocapture`
- For UI command surfaces: `cargo test -p claude-code-rs ui::components::command_palette ui::components::command_surface -- --nocapture`

## File-size hot spots

Command-focused Rust files over 450 lines:

| Lines | File |
| ---: | --- |
| 938 | `crates/claude-code-rs/src/commands/config_cmd.rs` |
| 894 | `crates/claude-code-rs/src/commands/memory.rs` |
| 850 | `crates/claude-code-rs/src/commands/permissions_cmd.rs` |
| 809 | `crates/claude-code-rs/src/commands/mod.rs` |
| 754 | `crates/claude-code-rs/src/commands/plugin_cmd.rs` |
| 659 | `crates/claude-code-rs/src/commands/doctor.rs` |
| 628 | `crates/claude-code-rs/src/commands/statusline_cmd.rs` |
| 612 | `crates/claude-code-rs/src/commands/team_cmd.rs` |
| 607 | `crates/claude-code-rs/src/commands/team_onboarding.rs` |
| 601 | `crates/claude-code-rs/src/commands/compact.rs` |
| 534 | `crates/claude-code-rs/src/commands/login.rs` |
| 525 | `crates/claude-code-rs/src/commands/terminal_setup.rs` |
| 508 | `crates/claude-code-rs/src/commands/tasks_cmd.rs` |
| 505 | `crates/claude-code-rs/src/commands/agents_cmd.rs` |
| 498 | `crates/claude-code-rs/src/commands/rewind.rs` |
| 487 | `crates/claude-code-rs/src/commands/plan.rs` |
| 482 | `crates/claude-code-rs/src/commands/mcp/tests.rs` |
| 471 | `crates/claude-code-rs/src/commands/voice_cmd.rs` |
| 454 | `crates/claude-code-rs/src/commands/schedule.rs` |

Command UI hot spots:

| Lines | File |
| ---: | --- |
| 566 | `crates/claude-code-rs/src/ui/components/command_surface/surfaces/config.rs` |
| 514 | `crates/claude-code-rs/src/ui/components/command_surface/tests.rs` |
| 344 | `crates/claude-code-rs/src/ui/components/command_palette/render.rs` |
| 326 | `crates/claude-code-rs/src/ui/components/command_surface/surfaces/remote.rs` |
| 325 | `crates/claude-code-rs/src/ui/components/command_palette/tests.rs` |
| 310 | `crates/claude-code-rs/src/ui/components/command_palette/metadata.rs` |

Repository-wide Rust hot spots from this checkpoint's scan:

| Lines | File |
| ---: | --- |
| 2432 | `crates/cc-engine/src/lifecycle/deps.rs` |
| 2403 | `crates/cc-query/src/loop_tests.rs` |
| 2273 | `crates/claude-code-rs/src/ipc/subsystem_handlers.rs` |
| 1965 | `crates/cc-mcp/src/client.rs` |
| 1938 | `crates/cc-permissions/src/dangerous.rs` |
| 1645 | `crates/cc-config/src/settings.rs` |
| 1614 | `crates/cc-session/src/memdir.rs` |
| 1572 | `crates/claude-code-rs/src/tools/tasks/tests.rs` |
| 1458 | `crates/claude-code-rs/src/tools/tool_search.rs` |
| 1405 | `crates/cc-engine/src/lifecycle/submit_message.rs` |

Follow-up constraint: future command edits in files above guard thresholds should split along existing command/helper/test boundaries instead of growing those files.

## Effects, defects, follow-ups

Effects:

- Added this checkpoint inventory artifact only.
- No Rust source, command registry behavior, dispatcher path, or test code changed.

Defects:

- Source-layout tests for `/plan` and `/context` still assert an older literal registry shape.
- Headless `/clear` e2e tests observe a `ready` event before the expected `system_info` confirmation.
- Broad `commands::` unit tests show isolation/order sensitivity; failed cases passed when rerun individually.

Follow-ups:

- Update source-layout tests to detect helper-based `command("name", ..., Handler)` registrations.
- Investigate whether headless `/clear` should suppress/interleave `ready` differently or whether the e2e should consume `ready` before asserting `system_info`.
- Add serial isolation or stronger temp-home isolation for command tests that mutate shared settings/session/task state.
