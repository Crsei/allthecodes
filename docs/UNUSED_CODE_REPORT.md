# Unused Code Report

Updated: 2026-05-08

This report records the current unused-code scan for the Rust workspace. It is
an audit artifact only; no code was removed as part of this pass.

## Scope

Commands run from `F:\AIclassmanager\cc\rust`:

```powershell
$env:RUSTUP_TOOLCHAIN='stable-x86_64-pc-windows-msvc'
cargo metadata --no-deps --format-version 1
cargo clippy --workspace --all-targets --all-features -- -W dead-code -W unused-imports -W unused-variables
rg -n "#\!?\[allow\((dead_code|unused|unused_imports|unused_variables)" crates -g '*.rs'
```

The stable toolchain override was used because the workspace toolchain
(`1.91.1`) was not installed locally and rustup downloads from
`static.rust-lang.org` were failing in this environment.

## File-Level Findings

A module-tree scan was built from the `cargo metadata` target roots and Rust
`mod` / `#[path]` declarations.

| Metric | Count |
| --- | ---: |
| Rust source files scanned | 805 |
| Files reachable from crate/test/build roots | 802 |
| Files not reachable from any Rust target | 3 |

Unreachable Rust files:

| File | Evidence | Notes |
| --- | --- | --- |
| `crates/claude-code-rs/src/ui/notifications/mod.rs` | Not declared by `src/ui/mod.rs`; no `crate::ui::notifications` references found | Contains `DesktopNotificationBackend` and terminal notification backend selection. |
| `crates/claude-code-rs/src/ui/notifications/bel.rs` | Only referenced by the unreachable `notifications/mod.rs` | Emits BEL notification command. |
| `crates/claude-code-rs/src/ui/notifications/osc9.rs` | Only referenced by the unreachable `notifications/mod.rs` | Emits OSC 9 notification command. |

Related documentation still mentions these files:

- `docs/RATATUI_UI_PARITY.md` lists BEL and OSC 9 notifications as present.
- `docs/tmp/ui-parity-update-plan.md` references `src/ui/notifications/`.

Recommended follow-up: either wire `ui::notifications` into `src/ui/mod.rs` and
connect it to the terminal notification flow, or mark the docs entry as planned
/ stale before deleting the files.

## Function / Type-Level Findings

`cargo clippy --workspace --all-targets --all-features -- -W dead-code
-W unused-imports -W unused-variables` completed successfully.

Clippy emitted no `dead_code`, `unused_imports`, or `unused_variables`
warnings for connected code that is visible to those lints. Under this scan,
there are no compiler-reported unused functions or types inside files that are
already part of the Rust module graph.

Important limitation: the workspace still contains 212
`allow(dead_code)`, `allow(unused)`, `allow(unused_imports)`, or
`allow(unused_variables)` annotations. Those suppress some unused-code signals,
so the clean Clippy result should be treated as a lower bound, not proof that
every retained item is actively used.

Largest areas with broad unused/dead-code allowances include:

| Area | Examples | Follow-up |
| --- | --- | --- |
| UI / runtime compatibility surfaces | `crates/claude-code-rs/src/voice/mod.rs`, `crates/claude-code-rs/src/ide/mod.rs`, `crates/claude-code-rs/src/ipc/agent_tree.rs` | Revisit once corresponding full-build wiring is complete. |
| IPC and subsystem extension types | `crates/claude-code-rs/src/ipc/subsystem_types.rs`, `crates/claude-code-rs/src/ipc/subsystem_events.rs` | Keep only if tied to an active IPC extension plan. |
| Team runtime scaffolding | `crates/claude-code-rs/src/teams/{types.rs,protocol.rs,mailbox.rs,identity.rs,helpers.rs}` | Convert broad module-level allows into item-level allows as team features stabilize. |
| Split-out utility crates | `crates/cc-keybindings/src/*.rs`, `crates/cc-utils/src/*.rs`, `crates/cc-compact/src/*.rs` | Audit crate by crate before deleting; many are API-parity placeholders. |

## Other Clippy Warnings

The Clippy run found maintainability warnings, but not unused function/type
warnings. These are worth separate cleanup passes:

| Category | Examples |
| --- | --- |
| Simplifiable expressions | `cc-mcp/src/auth.rs:831`, `engine/agent/mod.rs:251`, `api/streaming.rs:317`, `ipc/sdk_mapper.rs:296` |
| Function shape | `engine/agent/supervisor.rs:571`, `engine/lifecycle/submit_message.rs:365`, `engine/system_prompt.rs:388` |
| Large enum variants | `ui/components/command_surface/mod.rs:41`, `ipc/subsystem_events.rs:83`, `ipc/subsystem_events.rs:329` |
| Test-only lock/default issues | `commands/memory.rs:753`, `commands/memory.rs:797`, `commands/memory.rs:834`, `tools/tasks/tests.rs:109` |
| Platform-sensitive permissions warning | `tools/fs/file_edit.rs:961` |

## Current Conclusion

Confirmed unused code at file granularity is limited to
`crates/claude-code-rs/src/ui/notifications/**`.

No connected functions or types were reported as unused by Clippy in the current
workspace/all-targets/all-features scan, but broad lint allowances remain a
known source of blind spots.
