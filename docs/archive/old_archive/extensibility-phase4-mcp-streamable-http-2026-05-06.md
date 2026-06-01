# Extensibility Phase 4 - MCP Streamable HTTP Transport

Date: 2026-05-06

## Decision

MCP Streamable HTTP is implemented as the current standard HTTP transport.
Legacy `sse` remains supported as a compatibility transport, but new non-SSE
HTTP behavior uses `type = "streamable-http"`.

The current MCP transport specification lists `stdio` and Streamable HTTP as
standard transports. WebSocket is not implemented in this phase because it is
not a current standard MCP transport; it remains possible only as a future
custom transport decision.

## Implemented

- Added `streamable-http` support in `crates/cc-mcp/src/client.rs`.
- Streamable HTTP POST requests now send JSON-RPC to the configured endpoint
  and accept either `application/json` or `text/event-stream` responses.
- Streamable HTTP initialization uses the current `2025-11-25` MCP protocol
  version without changing legacy stdio/SSE protocol negotiation.
- Session handling captures and reuses `MCP-Session-Id`, sends
  `MCP-Protocol-Version`, opens the optional GET SSE listener after
  initialization, and sends DELETE on disconnect when a session exists.
- Existing OAuth/header handling is shared with Streamable HTTP, while
  transport-managed headers such as `Accept`, `Content-Type`,
  `MCP-Session-Id`, and `MCP-Protocol-Version` cannot be overridden.
- `/mcp add` and `/mcp edit` accept `--transport=streamable-http` and require
  `--url=<url>` for that transport.

## Verification

- `cargo test -p cc-mcp`
- `cargo test -p claude-code-rs mcp_streamable -- --nocapture`

## Notes

- Loopback `http://localhost`, `127.0.0.1`, and `::1` Streamable HTTP URLs are
  accepted for local development and tests. Non-loopback HTTP endpoints must
  use `https://`.
- The optional Streamable HTTP GET listener routes SSE notifications without
  owning request lifecycle cleanup; individual POST response bodies own their
  request/response correlation.
- WebSocket support is explicitly not part of the current standard MCP
  transport matrix.

## References

- MCP Transports specification: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- MCP Lifecycle specification: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
