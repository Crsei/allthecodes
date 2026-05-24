# UI Skeleton Surface Implementation Audit

Date: 2026-05-03
Scope: `crates/claude-code-rs/src/ui`

## Requirement Coverage

- Implement newly added Rust TUI skeleton files one file at a time.
  - Covered by concrete helpers/renderers under `agents`, `diff`, `hooks`, `lsp_recommendation`, `mcp`, `memory`, `messages`, `permissions`, `skills`, `tasks`, and `teams`.
  - `rg -n "skeleton|scaffold-only|Port rendering|future full-build|This file preserves" crates/claude-code-rs/src/ui` returns no matches.
- Use subagents where helpful.
  - Used focused subagents for diff, messages, mapping, and test strategy. Broad permission/rules attempts were abandoned after context failures; remaining permissions work was completed locally in smaller patches.
- Add snapshot tests for each feature.
  - Each implemented surface has deterministic snapshot coverage. Message and permission root snapshots call every helper in those surfaces at least once.
- Main-agent planning/checking.
  - Final checks were run locally after subagent integration, including formatting, typecheck, clippy, UI snapshots, e2e permissions, and release build.
- Create a human inspection document.
  - This file is the inspection artifact.
- Commit with Lore protocol.
  - Completed in commit `7a4e61b52359b18d75245c2eda4b81368eb733a5`, with Lore trailers for constraints, rejected alternatives, tested commands, and known gaps.

## Implemented Surface Groups

- `ui/agents/**`: agent definitions, validation, file paths, selectors, menus, editor, wizard steps, and snapshots.
- `ui/diff/**`: file list/detail/dialog rendering and snapshots.
- `ui/hooks/**`: hook config menu, event/matcher selection, prompt dialog, view mode, and snapshots.
- `ui/lsp_recommendation/**`: LSP recommendation menu and snapshot.
- `ui/mcp/**`: server/tool/capability views, reconnect, settings, elicitation, stdio/remote/agent menus, and snapshot.
- `ui/memory/**`: memory file selector/update notification and snapshot.
- `ui/messages/**`: assistant/system/user/team/tool-result helper surfaces and snapshots.
- `ui/permissions/**`: generic permission request models plus ask-user-question, shell, file, filesystem, sandbox, monitor, notebook, skill, web fetch, worker, hook, and rule-management surfaces with snapshot coverage.
- `ui/skills/**`: skills menu and snapshot.
- `ui/tasks/**`: background task, shell/remote/session/detail/workflow renderers and snapshot.
- `ui/teams/**`: team status/dialog renderers and snapshot.

## Verification Evidence

- `cargo fmt -p claude-code-rs`
- `cargo check -p claude-code-rs`
- `cargo clippy -p claude-code-rs --all-targets -- -D warnings`
- `cargo test -p claude-code-rs ui:: -- --nocapture --test-threads=1`
  - Result: 148 passed, 0 failed.
- `cargo test -p claude-code-rs --test e2e_permissions -- --nocapture`
  - Result: 2 passed, 0 failed.
- `cargo build --release`
- `git diff --check`
  - Result: exit 0; Git reported only CRLF normalization warnings for touched files.
- `Get-ChildItem -Recurse crates/claude-code-rs/src/ui -Filter *.snap.new`
  - Result: no temporary snapshot files.

## Follow-up Snapshot Audit

Follow-up audit date: 2026-05-03

Purpose: confirm that the latest Rust TUI updates were exercised by snapshot tests, with explicit checks for agent surfaces and command selection behavior.

Commands rerun:

- `git status --short`
  - Result: clean before the audit.
- `cargo test -p claude-code-rs ui:: -- --list`
  - Result: 148 UI tests listed, including agent snapshots, task/async-agent snapshots, command-palette snapshots, and App-level command selection behavior tests.
- `cargo test -p claude-code-rs ui:: -- --nocapture --test-threads=1`
  - Result: 148 passed, 0 failed, 0 ignored.
- `Get-ChildItem -Recurse crates/claude-code-rs/src/ui -Filter *.snap.new`
  - Result: no `.snap.new` files.
- `git status --short`
  - Result: clean after the audit.

Interpretation:

- The latest implemented UI surface groups are covered by deterministic snapshot tests at the surface/module level.
- Coverage is not one snapshot per changed file. Several small helper files are intentionally exercised through aggregate surface snapshots, for example `agents`, `messages`, `permissions`, and `tasks`.
- Command selection has both snapshot coverage for rendered command-palette states and regular unit coverage for interactive state transitions.
- Agent execution display is covered as Rust TUI task rendering state, not by launching a real background agent process during the UI snapshot suite.

## Snapshot Coverage Checklist

| Area | Snapshot or behavior test | Evidence | Status |
| --- | --- | --- | --- |
| Agents list/detail/menu | `ui::agents::tests::snapshot_agents_core_surfaces` | `crates/claude-code-rs/src/ui/agents/snapshots/claude_code_rs__ui__agents__tests__agents_core_surfaces.snap` | Passed |
| Agent generation and create wizard | `ui::agents::tests::snapshot_agent_generation_and_wizard` | `crates/claude-code-rs/src/ui/agents/snapshots/claude_code_rs__ui__agents__tests__agent_generation_and_wizard.snap` | Passed |
| Agent path isolation | Agent snapshots include `.cc-rust/agents` project paths | `agents_core_surfaces.snap` shows `./.cc-rust/agents/reviewer.md` | Passed |
| Agent runtime/task display | `ui::tasks::tests::snapshot_task_surfaces` | `crates/claude-code-rs/src/ui/tasks/snapshots/claude_code_rs__ui__tasks__tests__task_surfaces.snap` includes `async-agent` and `[agent]` state | Passed |
| Command palette root and filtered views | `ui::command_palette::tests::snapshot_command_palette_root_and_filtered_views` | root, page-down, MCP-filtered, and plugin-filtered snapshots under `crates/claude-code-rs/src/ui/snapshots/` | Passed |
| Command argument help for all commands | `ui::command_palette::tests::snapshot_all_command_argument_help_views` | `claude_code_rs__ui__command_palette__tests__command_argument_help_all_commands_110w.snap` | Passed |
| Command palette App placement | `ui::app::tests::command_palette_renders_below_prompt_input` | Regular behavior test confirms the palette renders below the prompt input | Passed |
| Slash command open/select flow | `ui::app::tests::slash_opens_command_palette_and_selection_keeps_argument_entry` | Regular behavior test confirms `/` opens palette and selected commands keep argument entry | Passed |
| Edit target picker flow | `ui::app::tests::ctrl_e_opens_edit_target_picker_for_supported_commands` and `ui::app::tests::picker_enter_inserts_selected_target_into_prompt` | Regular behavior tests confirm picker activation and insertion | Passed |
| Command argument help near prompt | `ui::app::tests::argument_entry_renders_parameter_help_near_input` | Regular behavior test confirms contextual help placement | Passed |
| Diff surfaces | `ui::diff::*::tests::snapshot_*` | diff file list, detail, binary, untracked, and dialog snapshots | Passed |
| Hooks surfaces | `ui::hooks::tests::snapshot_hooks_surfaces` | `hooks_surfaces.snap` | Passed |
| LSP recommendation surface | `ui::lsp_recommendation::tests::snapshot_lsp_recommendation_menu` | `lsp_recommendation_menu.snap` | Passed |
| MCP surfaces | `ui::mcp::tests::snapshot_mcp_surfaces` | `mcp_surfaces.snap` includes stdio, remote, and agent server menu rendering | Passed |
| Memory surfaces | `ui::memory::tests::snapshot_memory_surfaces` | `memory_surfaces.snap` | Passed |
| Message helper surfaces | `ui::messages::tests::snapshot_message_component_helpers` | `message_component_helpers.snap` | Passed |
| User tool-result messages | `ui::messages::user_tool_result_message::tests::snapshot_user_tool_result_messages` | `user_tool_result_messages.snap` | Passed |
| Permission helper surfaces | `ui::permissions::tests::snapshot_permission_component_helpers` | `permission_component_helpers.snap` | Passed |
| Skills menu | `ui::skills::tests::snapshot_skills_menu` | `skills_menu.snap` | Passed |
| Teams surfaces | `ui::teams::tests::snapshot_team_surfaces` | `team_surfaces.snap` | Passed |
| Broad visual regression surfaces | `ui::visual_regression::tests::snapshot_foundational_ui_surfaces` and `ui::visual_regression::tests::snapshot_interactive_ui_surfaces` | foundational and interactive UI snapshots under `crates/claude-code-rs/src/ui/snapshots/` | Passed |

## Agent And Command Selection Notes

Agent-specific verification:

- `agents_core_surfaces` snapshots the visible agent list, source grouping, built-in/project/plugin metadata, detail rendering, save-change summary, validation feedback, model selector, tool selector, color picker, and navigation footer.
- `agent_generation_and_wizard` snapshots generated-agent draft rendering plus create-agent wizard steps, including method, type, description, prompt, tools, model, color, memory, location, and confirmation output.
- `task_surfaces` snapshots async-agent display through the TUI task model. The snapshot includes an async agent task named `review worker`, `[agent]` task kind labeling, succeeded state, summary text, and output rendering.
- The UI snapshot suite does not start a real subagent process. Process launch, IPC, and backend agent execution should remain covered by backend/e2e tests or manual runs outside this TUI helper snapshot layer.

Command-specific verification:

- `snapshot_command_palette_root_and_filtered_views` snapshots the root command list, selected row layout, pagination behavior, `/mcp` filtering, and `/plugins` filtering.
- `snapshot_all_command_argument_help_views` renders every command's argument-help view at 110 columns, including examples and edit-target hints where available.
- `slash_opens_command_palette_and_selection_keeps_argument_entry` confirms `/` opens command selection and choosing a command leaves the prompt ready for arguments.
- `ctrl_e_opens_edit_target_picker_for_supported_commands` confirms supported commands expose the edit-target picker.
- `picker_enter_inserts_selected_target_into_prompt` confirms selecting a target mutates the prompt input as expected.
- `argument_entry_renders_parameter_help_near_input` confirms command help follows the prompt area during argument entry.

## Manual Inspection Checklist

Use this checklist after pulling commit `7a4e61b52359b18d75245c2eda4b81368eb733a5` or any follow-up change that touches these UI surfaces.

Environment checks:

- Confirm the working tree is clean or only contains the intentional manual-test changes.
- Confirm the binary being tested was built from the expected commit.
- Confirm cc-rust project data is written under `.cc-rust` paths, not original `.Codex` or `.Claude` paths.

Command palette checks:

- Launch the Rust TUI and press `/`.
- Confirm the command palette opens below the prompt input, not over the prompt history.
- Type `agents` or `/agents` and confirm the `/agents` command is visible with agent-related help text.
- Select `/agents` and press Enter. Confirm the prompt switches into argument entry and keeps the selected command prefix.
- Move selection beyond the first visible page. Confirm the list scrolls without losing the selected row.
- Filter for `/mcp` and `/plugins`. Confirm filtered lists are readable and do not show stale unfiltered commands.
- On a command that supports edit targets, press `Ctrl+E`. Confirm the target picker opens.
- Choose a target and press Enter. Confirm the target text is inserted into the prompt.
- Press Escape from the palette and from the target picker. Confirm both close cleanly without leaving partial UI artifacts.

Agent UI checks:

- Open the agents surface, for example through `/agents list` or the agents menu.
- Confirm built-in, project, plugin, managed, and flag sources display with clear source labels when present.
- Confirm project agent paths use `.cc-rust/agents`.
- Select an agent and open detail view.
- Confirm the detail view shows description, tools, model, effort, permission mode, memory scope, hooks, skills, and system prompt where applicable.
- Start the create-agent flow.
- Walk through method, type, description, prompt, tools, model, color, memory, location, and confirm steps.
- Confirm validation errors appear for empty, duplicate, or invalid agent names.
- Confirm generated-agent preview text is readable and maps to the requested goal.

Agent task/runtime display checks:

- Start or load a background/async agent task if the current test environment supports it.
- Confirm the task list marks the task kind as `agent`.
- Open task detail and confirm result summary, output lines, elapsed time, and state labels render correctly.
- Check success, running, and failed states when available.
- Confirm task detail rendering stays readable on narrow terminal widths.

Other surface checks:

- Open diff views for tracked, untracked, and binary-like changes. Confirm list/detail/dialog modes match the snapshot expectations.
- Open hooks configuration views. Confirm event, matcher, prompt, and view modes are reachable.
- Open MCP views. Confirm stdio, remote, and agent-owned servers display distinct labels and status text.
- Open memory file selector and update notification surfaces. Confirm project/global memory labels are clear.
- Trigger permission prompts for shell, PowerShell, file edit/write, filesystem, web fetch, skill, sandbox, and fallback cases where feasible.
- Review message transcript examples for assistant text/thinking/tool-use, user command/bash output, team memory, plan approval, and tool-result states.
- Open skills and teams surfaces and confirm menu/status rows remain aligned at narrow and normal terminal widths.

Expected manual result:

- No stale placeholder/skeleton text appears in any inspected UI.
- No `.Codex` or `.Claude` path appears for cc-rust-owned project agent, skill, memory, or permission surfaces.
- Command palette selection, edit-target picker, and agent flows are keyboard reachable.
- Text remains aligned and readable at common terminal widths.
- Any mismatch should be logged in `docs/KNOWN_ISSUES.md` before further implementation.

## Snapshot Note

The command-palette snapshot `command_argument_help_all_commands_110w.snap` needed metadata refresh because the assertion line shifted. The parallel `ui::` test run can also produce padding-only drift in that shared snapshot; the final accepted verification uses `--test-threads=1`, where the suite is stable.

## Remaining Risks

- These are deterministic Rust TUI render/state helpers, not a full interactive port of every upstream React event path.
- Module names intentionally mirror upstream file names; narrow `clippy::module_inception` allows preserve that mapping for reviewability.
- Snapshot coverage locks text output and helper state, but does not replace manual end-to-end inspection of every terminal interaction path.
