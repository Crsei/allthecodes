# UI Parity Difference Report and Update Plan

> Date: 2026-05-01
>
> Scope:
> - Baseline: `crates/claude-code-rs/src/ui`
> - Local OpenTUI frontend: `ui/src` (reference only; not an update target for this plan)
> - Upstream Codex Rust TUI reference: `F:\AIclassmanager\cc\codex\codex-rs\tui`
>
> Execution boundary: this plan updates only the Rust TUI under
> `crates/claude-code-rs/src/ui`. `ui/src` may be read for comparison, but it
> should not be modified, promoted as the default frontend, or included in the
> required verification gate for this plan.

> Update 2026-05-02:
> `crates/claude-code-rs/src/ui` no longer contains comment-only Rust module
> placeholders. The Rust TUI now has compiled foundation modules for approval
> overlays, bottom pane state, composer state, event routing, feature panels,
> frame requests, typed history cells, selection surfaces, status widgets,
> streaming control, terminal policy, tool activity, clipboard text/paste
> support, and an `insta` snapshot covering the foundation output. These
> modules are foundation APIs; most still need incremental runtime wiring into
> `App` before they count as full user-facing parity.

## Agent Team Method

The requested `agent teams` workflow was attempted first. The OMX tmux team runtime could not be launched in this Windows shell because `tmux` is not installed and `$TMUX` is not available. To keep the work parallel and bounded, this report used three Codex native subagents:

| Lane | Responsibility | Output used here |
| --- | --- | --- |
| Baseline explorer | Inventory `crates/claude-code-rs/src/ui` | Current Rust UI capabilities and must-have boundary |
| OpenTUI comparer | Compare `ui/src` to the baseline | Reference-only feature gaps; no `ui/src` implementation work |
| Codex TUI comparer | Compare `codex-rs/tui` to the baseline | Rust TUI architecture and UX patterns to reuse |

## Baseline: `crates/claude-code-rs/src/ui`

Current `claude-code-rs` UI is a direct Rust TUI built with `ratatui` + `crossterm`.

Key files:

| Area | Baseline file(s) | Current role |
| --- | --- | --- |
| Runtime loop | `crates/claude-code-rs/src/ui/tui.rs` | Initializes terminal, reads `crossterm` events, bridges `QueryEngine`, handles permission callbacks, ticks spinner, draws dirty frames. |
| App state | `crates/claude-code-rs/src/ui/app.rs` | Monolithic state holder for messages, prompt, scroll, permission dialog, command palette, keybindings, vim, transcript, trust prompt, status line, voice placeholders. |
| Messages | `crates/claude-code-rs/src/ui/messages.rs` | Converts `Message` variants into wrapped `ratatui` lines. |
| Transcript | `crates/claude-code-rs/src/ui/transcript.rs` | Prompt/transcript/focus modes, search, navigation, markdown export. |
| Prompt input | `crates/claude-code-rs/src/ui/prompt_input.rs` | Basic prompt editing, cursor movement, submit, insertion. |
| Keybindings and vim | `crates/claude-code-rs/src/ui/keybindings.rs`, `crates/claude-code-rs/src/ui/vim.rs` | Shortcut resolution and vim-like editing state machine. |
| Slash palette | `crates/claude-code-rs/src/ui/command_palette.rs` | Command filtering, selection, argument hints/help. |
| Permissions | `crates/claude-code-rs/src/ui/permissions.rs` | Single modal prompt with allow/deny/always-allow decisions. |
| Markdown/diff | `crates/claude-code-rs/src/ui/markdown.rs`, `crates/claude-code-rs/src/ui/diff.rs` | Markdown line conversion and basic unified diff rendering. |
| Performance/status | `crates/claude-code-rs/src/ui/virtual_scroll.rs`, `crates/claude-code-rs/src/ui/status_line_resolver.rs`, `crates/claude-code-rs/src/ui/spinner.rs` | Height cache, scriptable status payload, spinner state. |
| Terminal environment | `crates/claude-code-rs/src/ui/terminal_env.rs`, `crates/claude-code-rs/src/ui/notifications/` | No-flicker/mouse/scroll env, bell/OSC9 notification paths. |
| Foundation surfaces | `approval_overlay.rs`, `bottom_pane.rs`, `chat_composer.rs`, `event_router.rs`, `feature_panels.rs`, `frame_requester.rs`, `history_cell.rs`, `selection_surface.rs`, `status_widget.rs`, `streaming_controller.rs`, `terminal_integration.rs`, `tool_activity.rs`, `capability_contract.rs` | Typed Rust primitives and renderable snapshots for Codex-TUI-inspired surfaces; ready for incremental `App` wiring. |
| Clipboard helpers | `clipboard_text.rs`, `clipboard_paste.rs` | Text copy policy, path normalization, image-format metadata, optional image-paste paths behind existing feature gates. |
| Snapshot coverage | `crates/claude-code-rs/src/ui/visual_regression.rs`, `crates/claude-code-rs/src/ui/snapshots/*foundation*.snap` | `insta` coverage for foundation module behavior plus existing command-palette snapshots. |
| PTY tests | `crates/claude-code-rs/tests/pty_ui/*` | End-to-end terminal smoke coverage for input, commands, resize, streaming, welcome. |

Baseline strengths:

- Has a working in-process Rust TUI with direct engine integration.
- Has transcript/focus modes, command palette, vim mode, workspace trust prompt, status line, virtual scroll, PTY tests.
- Keeps the UI path simple enough to debug.
- Has first-pass typed foundation modules for the main Codex TUI architectural seams and snapshot coverage that proves they render deterministic output.

Baseline limits:

- Most user-facing UI domains still flow through `app.rs`; the new foundation modules are compiled and snapshot-covered but not yet the primary runtime path.
- The active permission UI is still generic even though `approval_overlay.rs` now has category-aware foundation types.
- Tool progress, background agents, teams, MCP, LSP, settings, sandbox, plugins, and rich status surfaces have foundation panel/status/activity primitives but are not all first-class interactive screens.
- Streaming and history rendering still use existing runtime message mutations; `streaming_controller.rs` and `history_cell.rs` are ready for incremental migration.
- Snapshot-style visual regression coverage has started for the foundation modules and command palette, but remains much lighter than the Codex Rust TUI reference.

## Reference Only: `ui/src` vs `claude-code-rs`

`ui/src` is a separate TypeScript/OpenTUI frontend that runs `claude-code-rs --headless` and communicates through JSONL IPC. Compared with the Rust baseline, it already contains many full-build surfaces that are useful as product and behavior references. This section is informational only: the update plan below does not change `ui/src`.

| Area | `claude-code-rs` Rust TUI baseline | `ui/src` reference behavior | Rust TUI implication |
| --- | --- | --- | --- |
| Runtime model | In-process Rust TUI directly owns terminal and engine channel in `tui.rs`. | Separate OpenTUI process in `ui/src/main.tsx`, spawning the backend through `ui/src/ipc/client.ts`; frontend/backend responsibilities are split. | Keep Rust-native ownership; use this only to identify missing lifecycle state. |
| State model | Monolithic `App` fields in `app.rs`; no reducer/view-model boundary. | Typed protocol, reducer store, and view-model layer in `ui/src/ipc/protocol.ts`, `ui/src/store/app-state.ts`, `ui/src/store/app-store.tsx`, `ui/src/view-model`. | Extract Rust state boundaries into modules instead of editing TS reducers. |
| Message model | `messages.rs` renders backend `Message` variants directly. | Componentized message taxonomy, content-block adapters, tool grouping, streaming items, thinking blocks, image/tool result normalization in `ui/src/components/messages`, `ui/src/store/message-model.ts`, `ui/src/adapters`. | Port equivalent taxonomy into Rust history cells and message renderers. |
| Prompt/composer | `prompt_input.rs` plus `App::handle_key_event` handles editing and submit. | Layered prompt modules under `ui/src/components/PromptInput`, queued submissions, paste truncation, busy status, slash submodes, vim integration, composer hooks. | Wire Rust `chat_composer.rs` into `App`; do not update OpenTUI hooks. |
| Slash commands | `command_palette.rs` provides Rust-side palette and argument hints. | `ui/src/commands.ts` has richer command metadata, aliases, usage fragments, dynamic model options, and frontend hinting. | Keep Rust command registry as source of truth for Rust TUI metadata. |
| Permissions | Generic 3-choice modal in `permissions.rs`. | Category-aware dialogs under `ui/src/components/permissions`, including bash/file edit/web fetch/fallback request components and hotkey tests. | Wire Rust `approval_overlay.rs` into permission callbacks. |
| Feature panels | Baseline UI mostly covers chat, transcript, prompt, permissions, status. | Dedicated panels/dialogs for MCP, agents, teams, LSP recommendations, subsystem status, background tasks, settings, sandbox, help, status line, plugins/skills. | Build Rust panels with `feature_panels.rs` and existing backend snapshots. |
| IPC capabilities | Rust TUI does not need IPC internally. | `ui/src/ipc/protocol.ts` defines broad backend/frontend events: tool progress, agent/team events, LSP/MCP/plugin/skill/subsystem/status-line updates, resize, permission/question requests. | Use as a checklist for Rust-visible backend state, not as a TS protocol change request. |
| Diff | Basic Rust diff renderer exists. | Structured diff views, file lists, hunk parsing, and dialog-oriented UX in `ui/src/components/StructuredDiff`, `ui/src/components/diff`. | Improve Rust diff rendering only after core Rust TUI parity. |
| Markdown/code | Rust markdown rendering exists but is line-oriented. | Component Markdown plus highlighted-code fallback path and OpenTUI syntax bridge. | Improve Rust markdown/code rendering without touching OpenTUI. |
| Testing | Rust unit tests plus PTY smoke tests. | Bun tests cover OpenTUI reducers/components. | Add Rust `insta`, unit, and PTY tests; Bun tests are out of scope. |

Interpretation:

- `ui/src` is not just a skin, but this plan treats it only as a reference.
- Full Rust TUI functionality requires porting equivalent surfaces into `crates/claude-code-rs/src/ui`, not changing OpenTUI components.
- Do not make `ui/src` the default delivery path as part of this plan.

## Difference: `codex-rs/tui` vs `claude-code-rs`

The Codex Rust TUI is a mature Rust TUI reference. It is closer to the desired long-term Rust-native architecture than the current baseline.

| Area | `claude-code-rs` baseline | `codex-rs/tui` difference | Stage |
| --- | --- | --- | --- |
| Runtime and frame scheduling | Direct event loop in `tui.rs`, dirty flag, 16 ms tick, terminal guard. | `tui/src/tui.rs` has `TuiEvent`, event broker, frame requester, draw broadcast, focus tracking, enhanced key detection, alt-screen control, restored-terminal execution. | Immediate full functionality |
| App architecture | `app.rs` owns most state directly. | `tui/src/app.rs` separates app-server session, active threads, pending requests, agent navigation, event handling, feedback, config refresh, thread replay, and UI state. | Immediate full functionality |
| Bottom pane/composer | `bottom_pane.rs` and `chat_composer.rs` now provide foundation state, but the active prompt path still lives mostly in `App`/`prompt_input.rs`. | `tui/src/bottom_pane` owns `ChatComposer`, view stack, approval overlays, request-user-input overlays, MCP elicitation, selection lists, skill popups, pending input preview, unified exec footer. | Immediate full functionality |
| Chat/history rendering | `history_cell.rs` now provides typed cells; active message rendering still uses `messages.rs`/`transcript.rs`. | `tui/src/chatwidget.rs`, `tui/src/history_cell.rs`, `tui/src/exec_cell.rs` use typed cells and richer chat event handling for approvals, tool calls, collaboration, streaming, status. | Immediate full functionality |
| Streaming | `streaming_controller.rs` now provides deterministic foundation state; active streaming still uses `StreamingState` in `tui.rs`. | `tui/src/streaming` has a dedicated controller/chunking/commit-tick pipeline. | Immediate full functionality |
| Approvals/permissions | `approval_overlay.rs` now has typed overlay primitives; active permission prompt remains generic. | Approval requests distinguish exec, apply patch, permissions, MCP elicitation, network policy, session decisions, guardian review states. | Immediate full functionality |
| Command and selection UX | Slash palette exists and `selection_surface.rs` now provides a reusable picker primitive. | Dedicated slash command, command popup, file search, skill popup, selection list, multi-select, model/status setup views. | Immediate full functionality |
| Markdown/diff | Basic markdown/diff modules. | `markdown_render`, `markdown_stream`, `diff_render`, `line_truncation`, `live_wrap`, syntax highlighting, snapshot-backed layout behavior. | Experience optimization after core parity |
| Status/account | Baseline status line payload and simple status bar. | `tui/src/status`, `status_indicator_widget`, `terminal_title`, `theme_picker`, status setup views, rate limit/account display. | Immediate full functionality |
| Collaboration/agents | Not first-class in baseline UI. | `multi_agents`, agent picker/navigation, loaded subagent thread backfill, collab spawn/wait/resume/close events in chat history. | Immediate full functionality |
| Test coverage | PTY smoke tests and in-module tests. | Extensive `insta` snapshots, vt100 suites, chatwidget/bottom-pane/status tests, startup/no-panic/layout regressions. | Immediate full functionality for migration safety |

Interpretation:

- `codex-rs/tui` should be treated as the Rust-native architecture reference, not copied wholesale.
- Its strongest reusable ideas are: event broker + frame requester, bottom-pane view stack, typed history cells, dedicated streaming controller, rich approval overlays, and snapshot/vt100 testing. The first-pass Rust foundation modules for these ideas now exist in `crates/claude-code-rs/src/ui`; the remaining work is runtime integration and parity depth.
- Direct porting will be expensive because its dependencies and app-server protocol are Codex-specific. Extract patterns and adapt them to `cc-rust` types/IPC.

## Priority Classification

Immediate full functionality means users can access all currently implemented backend features from the terminal UI, even if the UX is not perfect.

Experience optimization means the feature exists and works, but layout, polish, performance, discoverability, visual fidelity, or edge-case ergonomics still lag.

| Priority | Include now | Defer to optimization |
| --- | --- | --- |
| Runtime | Terminal restore, resize, input routing, draw scheduling, abort/quit safety, event backpressure. | Fine-grained animation, advanced terminal palette/theme tuning. |
| Conversation | User/assistant/system/tool/progress/thinking/image/error messages, streaming, transcript/focus, export. | Rich cell decoration, dense visual polish, smoother streaming chunk animation. |
| Composer | Multi-line edit, paste handling, queue while busy, history, vim, slash commands, model/effort/config commands. | Advanced vim parity, mouse-heavy editing, highly polished hint placement. |
| Permissions | Bash/file edit/write/web/MCP/question prompts with category-specific labels and hotkeys. | Guardian-style explanation polish and secondary review presentation. |
| Domain panels | MCP, agents, teams, LSP, settings, sandbox, status, background tasks, plugins/skills/subsystems. | Multi-pane layout polish, filters, sort controls, dense dashboard refinements. |
| Rendering | Markdown/code/diff/tool output must be readable and structurally correct. | Full syntax fidelity, hunk navigation, collapsible sections, visual density tuning. |
| Tests | Rust contract/state tests, PTY/e2e, vt100/snapshot coverage for core flows. | Large snapshot matrix and terminal-specific fixture expansion. |

## Update Plan

### Phase 1: Immediate Full Functionality

Goal: every implemented backend capability has a reachable, understandable terminal UI path.

1. Define the canonical UI capability contract.
   - Status: started in `crates/claude-code-rs/src/ui/capability_contract.rs`.
   - Extend the checked matrix mapping backend events/commands to Rust TUI surfaces until it covers the backend capabilities that should be visible in `crates/claude-code-rs/src/ui`.
   - Use `ui/src/ipc/protocol.ts` only as a reference checklist for missing state; do not update the OpenTUI protocol in this plan.
   - Do not add new UI dependencies; use existing `ratatui`/`crossterm` dependencies.

2. Choose the default delivery path.
   - Target `crates/claude-code-rs/src/ui` as the only implementation path for this plan.
   - Keep OpenTUI behavior unchanged; do not promote, demote, or re-route `ui/src`.
   - If a startup/CLI decision point is needed, limit it to documenting or selecting existing modes without changing OpenTUI internals.

3. Lock behavior with tests before broad edits.
   - Status: foundation snapshot added in `crates/claude-code-rs/src/ui/visual_regression.rs`.
   - Add/extend IPC contract tests for ready, stream, tool, permission, question, usage, status-line, MCP/LSP/agent/team/subsystem events.
   - Add PTY tests for workspace trust, prompt submit, abort, permission response, slash command, transcript toggle, resize.
   - Add Rust snapshot/unit tests for every newly wired Rust TUI surface.

4. Harden runtime and event delivery.
   - Ensure backend emits complete ready snapshots: cwd, session id, model, permission mode, keybindings, available models, view mode, status-line state.
   - Ensure Rust TUI handles `Event::Resize` reliably and redraws through the Rust frame path.
   - Add consistent busy/idle semantics so queued submissions, abort, permission prompts, and streaming state cannot conflict.

5. Complete conversation and tool rendering.
   - Align message conversion so content blocks preserve text, thinking, redacted thinking, tool use, tool result, image blocks, system levels, usage/cost.
   - Group read/search tool activity in prompt view while keeping full detail in transcript view.
   - Ensure long output, binary/image placeholders, and tool errors render without layout breakage.

6. Complete composer parity.
   - Make queue-while-busy, multi-line paste, truncation display, history navigation, vim mode, slash submodes, and dynamic model choices work against backend state.
   - Keep slash command metadata in sync with Rust command registry.
   - Add command execution responses for `/mcp`, `/agents-ui`, `/permissions`, `/status`, `/diff`, `/config`, `/model`, `/team`.

7. Complete permission and question flows.
   - Status: typed overlay primitives exist in `approval_overlay.rs`; active runtime wiring remains open.
   - Replace generic-only permission handling with typed UI payloads for bash, file edit/write, web fetch, MCP approval/elicitation, and generic fallback.
   - Preserve hotkeys and session-level decisions.
   - Ensure unanswered prompt teardown defaults safely and reports a visible system message.

8. Surface full-build domains.
   - MCP: server list, config list, tools/resources, reconnect/edit/remove states.
   - Agents: agent tree, background agent progress, generated/editable agent settings.
   - Teams: active team members, routed messages, pending counts, status snapshots.
   - LSP: server states, diagnostics, recommendation prompt, completion command errors.
   - Settings/sandbox/status: current mode, permission mode, config warnings, sandbox status, usage/rate/cost.
   - Plugins/skills/subsystems: status rows and error notices.

9. Bring Rust-native TUI to minimum parity where fallback remains.
   - Status: first-pass foundation APIs exist for bottom-pane view stack, typed approval overlay, status widget, message cell model, frame requester, event router, streaming controller, selection surface, terminal policy, and tool activity.
   - Wire those modules into the active `App` path one surface at a time.
   - Avoid wholesale Codex protocol imports; adapt to `cc-rust` types.
   - Keep each port behind tests and small modules instead of expanding `app.rs`.

10. Verification gate for Phase 1.
    - `cargo test -p claude-code-rs pty_ui -- --nocapture` or the closest available PTY subset.
    - `cargo build --release`.
    - `cargo test -p claude-code-rs ui:: --no-default-features` plus targeted feature-gated Rust UI tests.
    - Manual smoke: start Rust TUI, trust workspace, submit prompt, stream response, run slash command, approve/deny a permission, open MCP/agents/team/status surfaces, resize terminal, abort a running turn, exit cleanly.

### Phase 2: Experience Optimization

Goal: make the complete UI feel stable, fast, discoverable, and maintainable.

1. Modularize the Rust TUI around Codex TUI patterns.
   - Status: foundation modules now exist for event routing, bottom pane, chat history, status, overlays, command/selection surfaces, and frame requests.
   - Migrate active `App` behavior into those modules incrementally with snapshot/PTY coverage.
   - Introduce the `FrameRequester` into the runtime redraw path if Rust-native TUI remains strategic.

2. Improve rendering fidelity.
   - Status: `history_cell.rs` defines typed cells; active render migration remains open.
   - Move from direct message-line rendering to typed history cells.
   - Add better markdown wrapping, code block rendering, diff hunk layout, file path links, and collapsible tool-output sections.
   - Add truncation rules that preserve useful file paths, commands, and error tails.

3. Improve large-session performance.
   - Strengthen virtual scroll invalidation and height caching.
   - Render only visible history cells and keep streaming commits incremental.
   - Add stress tests for long transcript, many tool calls, large paste, and rapid resize.

4. Refine composer UX.
   - Polish slash ranking, argument help, skill/file/model mentions, queued draft previews, pending steer display, and transcript read-only hints.
   - Expand vim behavior only where it is covered by tests and matches expected Claude Code behavior.

5. Refine modal and dashboard UX.
   - Make MCP/agents/teams/LSP/settings panels keyboard-consistent.
   - Add selection-list filtering, persistent cursor position, sensible empty/error states, and narrow-terminal fallbacks.
   - Avoid nested-card style layouts; keep terminal surfaces dense and scan-friendly.

6. Refine terminal integration.
   - Add terminal title/status setup, notification backend selection, focus-aware notifications, external editor restore, mouse selection/copy, no-flicker fallback, and zellij/tmux caveats where supported.

7. Expand visual and regression coverage.
   - Status: `foundational_ui_surfaces` snapshot covers the first foundation modules; command-palette snapshots already exist.
   - Add vt100/insta-style snapshots for active bottom pane, permission overlays, slash palette, status line, transcript, markdown, diff, and dashboards.
   - Keep PTY tests for real Rust terminal behavior.

8. Documentation and cleanup.
   - Update `docs/WORK_STATUS.md` only after implementation changes land.
   - Move completed items from gap lists to completed-full records.
   - Document intentional crops explicitly; do not rely on historic `rust-lite` semantics.

## Recommended Execution Order

1. Stabilize IPC and test contracts.
2. Backfill missing backend state/events needed by the Rust TUI for MCP/agents/teams/LSP/status/permissions.
3. Wire the new Rust TUI foundation modules into runtime parity for permission overlays, composer queue, message cells, and status surfaces.
4. Add Rust snapshots/PTY coverage for each newly wired surface.
5. Run full build/test verification.
6. Start optimization passes only after the Phase 1 smoke checklist is green.

## Main Risks

| Risk | Mitigation |
| --- | --- |
| Rust TUI diverges from backend capabilities | Define Rust-visible capability contracts and test them from Rust. |
| `app.rs` grows further | New Rust-native surfaces should be extracted into modules before wiring into `App`. |
| Reference behavior is copied without fitting Rust constraints | Use `ui/src` and `codex-rs/tui` as references, but adapt to current Rust types and tests. |
| Permission decisions regress | Add typed permission contract tests and fail closed on missing responses. |
| Large sessions become slow | Keep virtual scroll and incremental streaming as Phase 1 acceptance criteria, not optional polish. |
| Copying Codex TUI creates dependency drag | Port patterns and small modules, not protocol-specific implementation wholesale. |
