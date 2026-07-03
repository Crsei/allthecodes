# ACP v2 Server Mode

allthecodes can run as an Agent Client Protocol v2 server using `--acp`:

```bash
allthecodes --acp --cwd /path/to/project
```

## Protocol

- JSON-RPC 2.0 over newline-delimited stdin/stdout
- stdout contains only valid JSON-RPC frames
- All diagnostics go to stderr

## Supported Capabilities

| Capability | Status |
|-----------|--------|
| `initialize` | ✅ |
| `auth/login`, `auth/logout` | ✅ (agent-managed, `allthecodes-login` method) |
| `session/new` | ✅ |
| `session/load` | ✅ (replay via `session/update`) |
| `session/resume` | ✅ (no replay) |
| `session/list` | ✅ |
| `session/close` | ✅ |
| `session/prompt` | ✅ (text + resource link content) |
| `session/cancel` (notification) | ✅ |
| `session/delete` | ✅ (archives via `allthecodes_session::storage::archive_session`) |
| `session/set_config_option` | ✅ (model, mode, thought_level) |
| `session/update` notifications | ✅ (streaming agent messages, thoughts, tool calls, state, usage) |
| `$/cancel_request` | ✅ (request-level cancellation) |
| `available_commands_update` | ✅ (sent after session/new, session/load, session/resume) |
| `config_option_update` | ✅ (model, mode, thought_level options) |
| Permission bridge | ⚠️ partial: request/response mapping exists, runtime callback/request dispatch is not wired yet |
| `session/request_permission` client request | ⚠️ partial: schema mapping exists, JSON-RPC client request lifecycle is not wired yet |

## Intentionally Unadvertised Capabilities

## Review Fix Log

2026-07-03:

- Fixed `session/update` stdout frames so ACP updates are emitted as JSON-RPC notifications with `method: "session/update"` and `params`, instead of bare update payloads.
- Fixed JSON-RPC request parsing to preserve `id: null` as a request id, and to execute notifications contained in JSON-RPC batches.
- Fixed `session/delete` dispatch so it is not accepted while the delete capability is not advertised; the handler now serializes the schema `DeleteSessionResponse` shape.
- Fixed `session/new` to reject non-empty ACP `mcpServers` while per-session MCP connection support remains unimplemented.
- Fixed `session/cancel` handling to mark the active turn as cancelled and report final idle state with `stopReason: "cancelled"` when cancellation wins.
- Fixed `session/set_config_option` to reject unknown config ids and invalid `mode` / `thought_level` values instead of silently accepting them.
- Fixed `file://` prompt resource conversion so absolute file links become `@/absolute/path`, not `@//absolute/path`.
- Fixed the ACP root engine factory to reuse startup-discovered tools, the resolved AppState template, model, and CLI overrides for each per-session `QueryEngine`.
- Fixed `auth/login` / `auth/logout` runtime handlers so they return the handler result, and broadened auth-method detection beyond Codex OAuth environment state.

Still open after this pass:

- Full permission bridge runtime wiring is not complete: ACP client requests, response routing, disconnect denial, and `requires_action` transitions still need integration tests.
- `session/load` still needs full conversation replay rather than placeholder state updates.
- `session/list` still needs workspace filtering, cursor serialization, and complete `_meta` mapping.
- Tool-call and thinking/plan update mapping is still incomplete and should not be treated as full ACP parity.

| Capability | Reason |
|-----------|--------|
| `session/prompt.image` | Image content blocks rejected |
| `session/prompt.audio` | Audio content blocks rejected |
| `session/prompt.embeddedContext` | Embedded resource blocks rejected |
| `session.mcp` | Stub only; per-session MCP not yet wired |
| `session.delete` capability flag | Not yet advertised in capability negotiation |

## Client Expectations

- Protocol version must be `2` (v1 rejected with `InvalidParams`)
- Sessions require an absolute, existing `cwd`
- Additional directories must be absolute and exist
- `session/new` returns `sessionId`, `configOptions`, and sends `available_commands_update`
- Prompt content: `Text` and `ResourceLink` (file:// → @/path) are supported
- Image/Audio blocks return `InvalidParams`
- Turn cancellation: send `session/cancel` notification with `sessionId`
- Request cancellation: send `$/cancel_request` notification with `requestId`
- `session/close` aborts active turns and flushes recorder
- `session/delete` closes active sessions and archives them
- Config options: `model`, `thought_level`, `mode`
