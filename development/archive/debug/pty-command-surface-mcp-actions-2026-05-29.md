# PTY command surface MCP actions - 2026-05-29

## Scope

Expanded the PTY command-surface coverage for `/mcp` beyond opening the panel.
The target actions are the MCP list-surface actions:

- Status
- Edit
- Reconnect
- Remove

## Findings

### Status action was inconsistent

`McpSurface::selected_mcp_action()` mapped action index `0` to `/mcp status`,
but `McpSurface::handle_key()` special-cased `Enter` on action index `0` to
open server details instead. This made the visible `Status` tab behave like a
details action and made the generated `/mcp status` action unreachable from the
main list.

Fix applied:

- `Enter` now always dispatches `selected_mcp_action()` on the main list.
- The list command hint now shows `Enter: /mcp status`.
- Server details remain reachable through the existing `v` shortcut, now shown
  in the list command hints.

Recommendation:

- Keep the action label and generated command aligned. If server detail should
  be a primary list action later, add a separate `Details` action instead of
  overloading `Status`.

### Remove action direct-executes

The `Remove` action submits `/mcp remove <server>` directly. That is current
surface behavior, but it is destructive from a project/user settings
perspective.

Mitigation applied in tests:

- Each PTY remove-action test creates an isolated temporary workspace.
- The server fixture is project-scoped under that temp workspace.
- `ALLTHECODES_HOME` is pointed at a temp data root so user settings are not
  touched.

Recommendation:

- Consider adding a confirmation surface for MCP remove, or changing the surface
  action to fill `/mcp remove <server> ` in the prompt for review before submit.

## Test coverage added

`crates/allthecodes/tests/pty_tui_e2e/tests/commands_surface.rs` now seeds an
isolated MCP fixture and adds item-level PTY scenarios for:

- list rendering of `Status`, `Edit`, `Reconnect`, `Remove`, and the fixture
  server
- Status action submitting `/mcp status`
- Edit action filling `/mcp edit db`
- Reconnect action submitting `/mcp reconnect db`
- Remove action submitting `/mcp remove db`

`crates/allthecodes/src/ui/command_surface/tests.rs` now covers all four MCP
action outcomes at the Rust surface level.

## Verification notes

Passed:

- `cargo test -p allthecodes ui::command_surface::tests::mcp_surface_action_tabs_apply_to_selected_server -- --nocapture`
- direct `rustfmt` on the touched Rust files

Blocked:

- `cargo test -p allthecodes --test pty_tui_e2e surface_mcp -- --nocapture`
- `cargo fmt`

Both blocked commands currently fail before this task's PTY tests run because
`crates/allthecodes-tools/src/lib.rs` and `crates/allthecodes-tools/src/registry.rs`
refer to `crate::product_tools`, but no
`crates/allthecodes-tools/src/product_tools.rs` or
`crates/allthecodes-tools/src/product_tools/mod.rs` file exists.
