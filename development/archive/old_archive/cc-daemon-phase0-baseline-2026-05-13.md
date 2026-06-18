# cc-daemon Phase 0 Baseline

Date: 2026-05-13

Scope: freeze the pre-migration daemon baseline before moving implementation from `crates/claude-code-rs/src/daemon/**` into `crates/cc-daemon/**`.

Phase 0 made no runtime behavior changes.

## File Inventory

Current root-crate daemon implementation:

| File | Lines |
| --- | ---: |
| `crates/claude-code-rs/src/daemon/channels.rs` | 153 |
| `crates/claude-code-rs/src/daemon/gateway_bridge.rs` | 429 |
| `crates/claude-code-rs/src/daemon/gateway_client.rs` | 448 |
| `crates/claude-code-rs/src/daemon/gateway_routes.rs` | 170 |
| `crates/claude-code-rs/src/daemon/gateway_run_events.rs` | 67 |
| `crates/claude-code-rs/src/daemon/memory_log.rs` | 85 |
| `crates/claude-code-rs/src/daemon/mod.rs` | 17 |
| `crates/claude-code-rs/src/daemon/notification.rs` | 178 |
| `crates/claude-code-rs/src/daemon/process_state.rs` | 1070 |
| `crates/claude-code-rs/src/daemon/protocol.rs` | 449 |
| `crates/claude-code-rs/src/daemon/routes.rs` | 751 |
| `crates/claude-code-rs/src/daemon/server.rs` | 34 |
| `crates/claude-code-rs/src/daemon/sse.rs` | 86 |
| `crates/claude-code-rs/src/daemon/state.rs` | 233 |
| `crates/claude-code-rs/src/daemon/supervisor.rs` | 414 |
| `crates/claude-code-rs/src/daemon/team_memory_proxy.rs` | 197 |
| `crates/claude-code-rs/src/daemon/tick.rs` | 95 |
| `crates/claude-code-rs/src/daemon/webhook.rs` | 370 |

Current `cc-daemon` scaffold:

| File | Lines |
| --- | ---: |
| `crates/cc-daemon/src/lib.rs` | 5 |
| `crates/cc-daemon/Cargo.toml` | 8 |

Total recorded daemon/scaffold size: 5259 lines.

## Entry Points

`crates/claude-code-rs/src/main.rs` currently owns all daemon entry wiring:

| Lines | Entry |
| --- | --- |
| 100-101 | Declares root-crate `mod daemon;`. |
| 297-311 | `--daemon-worker` fast path; calls `daemon::supervisor::run_worker_mode`. |
| 321-327 | Management command fast path; calls `daemon::process_state::try_run_management_command`. |
| 876-948 | `--daemon` mode; writes started/stopped state, builds `daemon::state::DaemonState`, starts team-memory proxy, HTTP server, tick loop, supervisor loop, and worker termination. |

`crates/claude-code-rs/src/cli.rs` currently defines daemon CLI flags:

| Lines | Flag |
| --- | --- |
| 86-88 | Hidden `--daemon`. |
| 90-92 | Hidden internal `--daemon-worker <KIND>`. |
| 94-96 | Hidden internal `--worker-id <ID>`. |
| 98-100 | Public `--port <PORT>`, default `19836`. |

## Test Baseline

Full logs were captured under `target/phase0-daemon-baseline/`.

| Command | Result |
| --- | --- |
| `cargo test -p claude-code-rs daemon::process_state` | Passed: 8 tests |
| `cargo test -p claude-code-rs daemon::protocol` | Passed: 4 tests |
| `cargo test -p claude-code-rs daemon::routes` | Passed: 5 tests |
| `cargo test -p claude-code-rs daemon::sse` | Passed command, but 0 tests matched |
| `cargo test -p claude-code-rs daemon::supervisor` | Passed: 2 tests |
| `cargo test -p claude-code-rs commands::daemon_cmd` | Passed: 2 tests |
| `cargo test -p claude-code-rs tools::exec::sleep` | Passed: 7 tests |

No Phase 0 target test failures were observed.

## Existing Warnings

The Phase 0 test runs repeatedly emitted these pre-migration warnings:

- `crates/cc-browser/src/mcp_bridge.rs`: unused import `Context`.
- `crates/claude-code-rs/src/tools/exec/process_control.rs`: unused import `std::os::unix::process::CommandExt`.
- `crates/claude-code-rs/src/tools/exec/powershell.rs`: unused function `parent_message`.
- `claude-code-rs` build script attempted the web-ui build and reported `npm install error: No such file or directory (os error 2), skipping web build`.

These warnings are baseline issues for this migration unless changed by a later phase.

## Known Coverage Gaps

- `cargo test -p claude-code-rs daemon::sse` currently matches no tests, so SSE behavior has no direct unit-test baseline under that filter.
- Phase 0 did not run an end-to-end live daemon HTTP/SSE session; it only ran the daemon-focused commands listed above.
