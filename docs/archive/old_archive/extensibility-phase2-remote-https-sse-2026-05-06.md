# Extensibility Phase 2 - Remote HTTPS SSE

Date: 2026-05-06

## Decision

Phase 2 implements remote `https://` support for the existing `type = "sse"` MCP transport.

The current MCP spec has moved the recommended remote transport to Streamable HTTP, while the 2024-11-05 HTTP+SSE transport remains the backwards-compatible path. Claude Code docs also describe remote SSE as deprecated but still configurable. This phase therefore completes secure remote SSE compatibility without absorbing OAuth or Streamable HTTP into the same change.

Sources checked:

- https://modelcontextprotocol.io/specification/2024-11-05/basic/transports
- https://modelcontextprotocol.io/specification/2025-03-26/basic/transports
- https://docs.anthropic.com/en/docs/claude-code/mcp
- `F:\AIclassmanager\cc\src\services\mcp\client.ts`
- `F:\AIclassmanager\cc\src\services\mcp\types.ts`

## Implementation

- `cc-mcp` now depends on existing workspace HTTP/TLS primitives: `reqwest`, `tokio-util`, `futures`, `bytes`, and `url`.
- Loopback `http://localhost`, `127.0.0.1`, and `::1` SSE still use the existing raw `TcpStream` request path.
- Remote SSE must use `https://`.
- Remote SSE GET uses reqwest with redirects disabled and `Accept: text/event-stream`.
- Remote SSE POST reuses the same endpoint event and JSON-RPC pending-request dispatch as local SSE.
- Endpoint events for remote SSE may be same-origin absolute paths or same-origin `https://` URLs.
- Remote endpoint events that downgrade to `http://` or cross origins are rejected.
- User-supplied headers still allow authentication headers, but protocol-managed headers such as `Host`, `Connection`, `Content-Length`, and `Transfer-Encoding` are rejected.
- URL logging removes query strings and fragments.
- HTTP 401/403 is classified as `auth-needed`, but token acquisition/storage is left to Phase 3.

## Verification

- `cargo test -p cc-mcp`
- `cargo test -p claude-code-rs mcp`
- `cargo check -p claude-code-rs --message-format short`

## Remaining Risk

- No live remote HTTPS MCP server fixture is checked into this repository; coverage uses target parsing, endpoint validation, status classification, and the existing loopback SSE integration fixture.
- OAuth / interactive auth is still intentionally incomplete until Phase 3.
- Streamable HTTP and WebSocket transport support remain Phase 4 work.
