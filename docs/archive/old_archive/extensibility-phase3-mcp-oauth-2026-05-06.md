# Extensibility Phase 3 - MCP OAuth And Interactive Auth

Date: 2026-05-06

## Decision

MCP OAuth is implemented as an MCP-owned auth contract instead of embedding
tokens in `settings.json` or IPC config entries.

The settings/config layer stores only OAuth metadata on `McpServerConfig.oauth`.
Access tokens, refresh tokens, and pending PKCE state are persisted under the
cc-rust data root:

- `{CC_RUST_HOME|~/.cc-rust}/mcp-oauth.json`
- `{CC_RUST_HOME|~/.cc-rust}/mcp-oauth-pending.json`

No original Codex paths are used.

## Implemented

- Added `McpOAuthConfig` to `crates/cc-mcp/src/lib.rs`.
- Added `crates/cc-mcp/src/auth.rs` for:
  - authorization-server metadata discovery;
  - manual PKCE authorization start/complete;
  - token response parsing;
  - expiry and refresh handling;
  - credential clear/status;
  - token redaction.
- Remote SSE now injects stored bearer credentials through
  `normalized_sse_headers_with_auth()` when no explicit `Authorization` header
  is configured.
- `/mcp auth start|complete|status|clear` exposes the interactive auth path
  without echoing tokens.
- IPC `McpCommand` now supports auth start/complete/query/clear and emits
  redacted `AuthStarted` / `AuthStatus` events.

## Verification

- `cargo test -p cc-mcp`
- `cargo test -p claude-code-rs mcp`
- `cargo check -p claude-code-rs --message-format short`

## Notes

- Dynamic OAuth client registration is not implemented in this phase; configured
  `clientId` is used when present and otherwise defaults to `cc-rust`.
- Loopback `http://` OAuth endpoints are accepted for local development and
  tests. Non-loopback OAuth endpoints must use `https://`.
- Streamable HTTP, WebSocket, and IDE-specific transports remain Phase 4 work.

## References

- MCP Authorization specification: https://modelcontextprotocol.io/specification/2025-03-26/basic/authorization
- MCP Transports specification: https://modelcontextprotocol.io/specification/2025-03-26/basic/transports
- Claude Code MCP docs: https://docs.anthropic.com/en/docs/claude-code/mcp
