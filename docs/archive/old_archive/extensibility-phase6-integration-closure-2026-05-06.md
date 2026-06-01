# Extensibility Phase 6 - Integration Closure

Date: 2026-05-06

## Decision

Phase 6 closes the active Extensibility runtime by removing the last MCP
client-side gap: tools from MCP servers connected after startup now reach the
model on the next turn.

The implementation does not infer dynamic MCP tools from `mcp__*` name
prefixes. Native tools can intentionally use MCP-shaped names, especially
Computer Use (`mcp__computer-use__*`), so the `Tool` trait now exposes an
optional `mcp_server_name()` marker. `McpToolWrapper` implements that marker,
and `QueryEngineDeps::refresh_tools()` removes/rebuilds only marked dynamic
MCP wrappers from the shared runtime `McpManager`.

The query loop refreshes tools before each model call, not only after a tool
turn, so a `/mcp connect` or IPC connect made between user turns is visible to
the first subsequent model request. ToolSearch is refreshed with the same
merged registry.

## Scope

- Added a dynamic MCP ownership marker to `Tool`.
- Marked `McpToolWrapper` with its server name.
- Rebuilt runtime MCP tool wrappers from the live manager during
  `QueryEngineDeps::refresh_tools()`.
- Refreshed tools before model calls in the query loop.
- Added regression coverage for prefix-safe MCP merge behavior and pre-call
  tool refresh.
- Fixed a skill baseline test race by serializing tests that mutate the global
  skill registry.
- Updated the implementation map, work status, gap register, and phase plan.

## Residuals

- WebSocket remains unsupported/custom because it is outside the current
  standard MCP transport matrix used for this phase.
- Custom-agent `skills`, `hooks`, `plugin`, and `mcpServers` fields remain
  parsed/round-tripped future fields, not active runtime behavior. The active
  custom-agent runtime now has an explicit safety contract.

## Verification

- `cargo test -p claude-code-rs refreshed_mcp_merge_replaces_wrapped_tools_without_prefix_guessing`
- `cargo test -p claude-code-rs query_refreshes_tools_before_first_model_call`
- `cargo check -p cc-engine --message-format short`
- `cargo check -p claude-code-rs --message-format short`
- `cargo test -p cc-mcp`
- `cargo test -p cc-skills`
- `cargo test -p cc-permissions`
- `cargo test -p claude-code-rs mcp`
- `cargo test -p claude-code-rs tools::skill`
- `cargo test -p claude-code-rs tools::hooks`
- `cargo test -p claude-code-rs engine::agent`
- `cargo test -p claude-code-rs agent_settings`
- `cargo test --workspace --lib`
- `cargo build --release`

## Notes

`cargo test -p claude-code-rs tools::skill` initially exposed a pre-existing
parallel-test race between tests that register skills and tests that clear the
global registry. The runtime code was unchanged; the tests now serialize only
the registry-mutating cases.
