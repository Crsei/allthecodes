# Extensibility Phase 1 - MCP Lifecycle Integration

Date: 2026-05-06

## Decision

Phase 1 closes the nearest MCP management gap by routing user-facing and IPC lifecycle commands through the same runtime `McpManager` that startup uses.

The implementation intentionally keeps transport/auth scope unchanged. Remote HTTPS SSE, OAuth / interactive auth, Streamable HTTP, and WebSocket remain later phases.

## Changed Runtime Behavior

- Startup registers the session MCP manager through `crate::mcp::runtime::install_manager`.
- `/mcp connect`, `/mcp disconnect`, and `/mcp reconnect` call `run_mcp_runtime_operation` instead of returning queued intent text.
- Headless IPC `McpCommand::{ConnectServer, DisconnectServer, ReconnectServer}` uses the same runtime operation helper.
- MCP status snapshots read live manager state when available, then fall back to disabled config state or the last observed runtime state.
- Runtime MCP state-change events are recorded while still being forwarded to the existing IPC event bus.

## Preserved Boundaries

- Config editing remains scope-aware; plugin and IDE scopes stay read-only for mutation.
- Disabled MCP servers are still skipped by the manager and do not retain a live client.
- Manager-level reconnect still drops the old client before reconnecting and leaves no stale client after failed reconnect.
- No new dependencies were added.

## Verification

- `cargo test -p claude-code-rs mcp`
- `cargo test -p cc-mcp`
- `cargo check -p claude-code-rs --message-format short`

All passed on 2026-05-06.

## Remaining Risks

- `/mcp connect` can connect a newly discovered server at runtime, but Phase 1 does not rebuild the active model tool registry for tools that were not present at startup.
- In-flight MCP tool calls during reconnect still follow the existing disconnect semantics; Phase 2/4 should decide whether to cancel, drain, or reject them explicitly.
