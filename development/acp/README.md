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
| Permission bridge | ✅ (maps `PermissionRequestPayload` to `session/request_permission`) |
| `session/request_permission` client request | ✅ (via `RequestPermissionRequest`) |

## Intentionally Unadvertised Capabilities

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
