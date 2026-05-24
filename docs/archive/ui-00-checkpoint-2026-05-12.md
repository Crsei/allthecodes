# ui-00 Checkpoint: Rust UI Baseline

Date: 2026-05-12
Scope: record-only checkpoint for Rust IPC/client prerequisites and Rust TUI baselines. No Rust code was moved or changed. TypeScript UI files were not inspected.

## Boundary Status

- Rust-only UI scope confirmed from `docs/scripts/achieve/workspace-crate-extraction-omx-tasks-2026-05-10.txt` and `docs/scripts/workspace-crate-extraction-omx-remaining-from-api-models-03.txt`.
- Task ownership is `crates/claude-code-rs/src/ui/` plus related Rust IPC/client, snapshot, and PTY test surfaces.
- TypeScript UI/Web paths remain out of scope for this checkpoint.

## Rust IPC And Client Prerequisites

Passed:

- `cargo test -p cc-ipc-protocol` -> 84 passed.
- `cargo test -p cc-ipc` -> 7 passed.
- `cargo test -p claude-code-rs ipc::` -> 68 passed, 1541 filtered.
- `cargo test -p claude-code-rs --test e2e_terminal` -> 27 passed, 19 ignored.

Client note:

- `e2e_terminal` confirms the headless JSONL client boundary starts, emits `ready`, accepts resize/slash/quit/permission mode messages, reports invalid JSON/unknown messages explicitly, and exits cleanly on stdin close.

## Rust TUI Snapshot And PTY Baseline

Passed:

- `cargo test -p claude-code-rs ui:: -- --test-threads=1` -> 242 passed, 1367 filtered.
- `cargo test -p claude-code-rs --test e2e_pty -- --skip live_pty_print_mode --skip live_pty_simple_chat` -> 4 passed, 2 filtered.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\export-ui-snapshots.ps1 -CheckOnly -SkipTests` -> exported 34 Rust TUI snapshots to `target/ui-snapshots/index.md`.

PTY artifacts:

- Offline PTY run generated current logs under `crates/claude-code-rs/logs/202605121602/`.
- Snapshot export produced a flat review bundle under `target/ui-snapshots/`.

Warnings:

- Running `cargo test -p claude-code-rs --test e2e_pty` without filtering live tests ran provider-backed tests in this environment. `live_pty_print_mode` failed with DeepSeek HTTP 402 and local plugin/plan config warnings; this is an environment/API-key defect, not an offline Rust TUI baseline failure.
- Snapshot export warned that existing `.env` could not be loaded as UTF-8: `stream did not contain valid UTF-8`.

## Rust UI Import Map

Public facade: `crates/claude-code-rs/src/ui/mod.rs` keeps stable `crate::ui::*` paths and maps responsibility folders with `#[path]`.

Primary module roots:

- Entry/runtime: `tui`, `app`.
- Feature domains: `agents`, `diff`, `hooks`, `lsp_recommendation`, `mcp`, `memory`, `messages`, `permissions`, `skills`, `tasks`, `teams`.
- Components: `approval_overlay`, `bottom_pane`, `chat_composer`, `chatwidget`, `command_palette`, `command_surface`, `cwd_prompt`, `feature_panels`, `fuzzy_match`, `history_search_dialog`, `keyboard_shortcut`, `pager_overlay`, `prompt_input`, `resume_picker`, `search_box`, `selection_surface`, `status_icon`, `status_widget`, `tabs`, `tooltops`, `welcome`.
- Input: `clipboard_paste`, `clipboard_text`, `file_search`, `form_navigation`, `insert_history`, `keybindings`, `mention_codec`, `slash_command`, `vim`.
- Rendering: `get_git_diff`, `history_cell`, `markdown`, `markdown_render`, `markdown_stream`, `progress_bar`, `shimmer`, `spinner`, `theme`, `tool_activity`, `virtual_scroll`.
- Runtime: `capability_contract`, `event_router`, `frame_requester`, `persistent_history`, `session_log`, `snapshot_export`, `streaming_controller`, `transcript`, `visual_regression`.
- Platform/status/helpers: `audio_device`, `browser`, `custom_terminal`, `debug_config`, `terminal_env`, `terminal_integration`, `skills_helpers`, `status_line`, `status_line_resolver`.

Important cross-crate imports observed in Rust UI:

- `cc_ipc_protocol`: app events, app server adapter, command surfaces, subsystem/TUI events.
- `cc_engine::status_line`: status line facade and resolver support.
- Root crate services/types/config: `crate::commands`, `crate::config`, `crate::engine`, `crate::services`, `crate::types`, `crate::voice`.
- Terminal/UI stack: `ratatui`, `crossterm`, `tokio`, `tokio_util`.
- Supporting libraries: `serde`, `serde_json`, `anyhow`, `chrono`, `uuid`, `unicode_width`, `lru`, `pulldown_cmark`, `gateway`.

Guard-threshold hotspots:

| File | Lines |
| --- | ---: |
| `crates/claude-code-rs/src/ui/messages/render.rs` | 883 |
| `crates/claude-code-rs/src/ui/input/vim.rs` | 796 |
| `crates/claude-code-rs/src/ui/app/input.rs` | 742 |
| `crates/claude-code-rs/src/ui/app/render.rs` | 599 |
| `crates/claude-code-rs/src/ui/runtime/transcript.rs` | 591 |
| `crates/claude-code-rs/src/ui/components/command_surface/surfaces/config.rs` | 532 |

## Effects, Defects, Follow-Ups

Effects:

- Added this checkpoint note only.
- Captured Rust UI snapshot export and PTY log artifacts in ignored/generated output locations.

Defects:

- Full PTY test target can execute live provider-backed tests unless live tests are explicitly skipped.
- The local `.env` file is not valid UTF-8 for snapshot export.

Follow-ups:

- Future Rust UI extraction tasks should split the hotspot files before adding substantial behavior.
- Keep offline PTY baseline commands explicit with `--skip live_pty_print_mode --skip live_pty_simple_chat`, or mark live PTY tests ignored if that is the intended contract.
- Fix or isolate the local `.env` encoding issue before treating snapshot export warnings as actionable Rust UI failures.
