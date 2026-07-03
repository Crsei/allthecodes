# ACP v2 Server Mode

allthecodes can run as an Agent Client Protocol v2 server using `--acp`:

```bash
allthecodes --acp --cwd /path/to/project
```

## Protocol

- JSON-RPC 2.0 over newline-delimited stdin/stdout
- stdout contains only valid JSON-RPC frames
- All diagnostics go to stderr

## Current Status

The ACP adapter currently compiles and has a protocol/runtime skeleton, but it
does not yet satisfy the full ACP v2 adaptation plan. Use
[`acp-review-fix-plan.md`](acp-review-fix-plan.md) as the execution plan for the
remaining review fixes.

## Fix Plan Decisions

- `session/delete` must be fully implemented in this branch, including active-session close, persisted-session archive, list filtering, tests, and capability advertisement only after verification.
- `session.mcp.*`, `session.prompt.image`, `session.prompt.audio`, and `session.prompt.embeddedContext` are out of scope for this branch. Keep their API/capability seams explicit, but keep them unadvertised and rejected until separate feature work lands.
- The binary ACP smoke test must run a real model-backed prompt through the normal allthecodes config/auth/model resolution path. It must not use slash commands, mock engines, or ACP-specific credentials as substitutes.

## Verified So Far

The current branch has passed:

```bash
cargo check -p allthecodes-acp
cargo test -p allthecodes-acp
cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update
cargo test -p allthecodes-acp --test protocol_methods cancel_request_cancels_pending_prompt_before_ack
cargo test -p allthecodes-acp --test config_options
cargo test -p allthecodes-acp --test protocol_methods auth
cargo test -p allthecodes-acp --test protocol_methods capability
cargo test -p allthecodes-acp --test session_lifecycle delete
```

The binary smoke target required by the original plan is not present yet:

```bash
cargo test -p allthecodes --test acp_stdio_smoke
```

Current result: fails with `no test target named acp_stdio_smoke`.

## Capability Status

| Capability or method | Status |
|-----------|--------|
| `initialize` | Verified for protocol version, auth methods, and capability negotiation |
| `auth/login`, `auth/logout` | Implemented for ACP login instructions and normal auth resolver detection |
| `session/new` | Implemented with cwd/additional-directory validation, config options, command update, and MCP rejection |
| `session/load` | Implemented with persisted transcript replay before response |
| `session/resume` | Implemented for persisted session resume |
| `session/list` | Implemented with global/workspace filtering, cursor serialization, cwd, and `_meta` mapping |
| `session/close` | Implemented with active-turn cancellation, bounded idle wait, recorder flush, and removal |
| `session/prompt` | Implemented for text/resource-link content, ACK-before-update ordering, config overrides, and mapped runtime updates |
| `session/cancel` | Implemented for active turn abort and cancelled idle state |
| `session/set_config_option` | Implemented for model/mode/thought validation, config notifications, and per-turn overrides |
| `session/update` notifications | Implemented for state, usage, text, thinking, tool calls, tool progress, plans, tombstones, retries, and summaries |
| `$/cancel_request` | Implemented for pending prompt cancellation and pending permission cancellation |
| `available_commands_update` | Sent after session creation/load/resume |
| `config_option_update` | Wired for `session/set_config_option` changes |
| Permission bridge | Implemented with engine callbacks, ACP client requests, response routing, cancellation, and disconnect denial |
| `session/request_permission` client request | Implemented and tested through JSON-RPC request/response lifecycle |
| `session/delete` | Implemented and advertised after archive/list/active-close tests passed |

## Intentionally Unadvertised Capabilities

| Capability | Reason |
|-----------|--------|
| `session/prompt.image` | Out of scope for this branch; image content blocks are rejected until separate implementation and tests land |
| `session/prompt.audio` | Out of scope for this branch; audio content blocks are rejected until separate implementation and tests land |
| `session/prompt.embeddedContext` | Out of scope for this branch; embedded resource blocks are rejected until separate implementation and tests land |
| `session.mcp` | Out of scope for this branch; per-session MCP is not wired |

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
- Fixed `session/prompt` ACK ordering by pausing accepted prompt turns until the JSON-RPC response is enqueued.
- Fixed pre-ACK request cancellation for accepted prompts so it returns `RequestCancelled` and does not start the engine stream.
- Fixed config options so model/mode/thought choices are advertised from session state, validated on update, sent through `config_option_update`, and applied to prompt submit overrides.
- Fixed ACP auth initialization and login responses so unauthenticated clients receive agent login instructions and authenticated clients do not see redundant login methods.
- Fixed `session/delete` so it closes active sessions first, archives persisted sessions, treats missing ids as idempotent success, filters archived sessions from `session/list`, and advertises `session.delete`.
- Clarified MCP policy in code and capability negotiation: per-session MCP remains unadvertised and non-empty `mcpServers` requests are rejected with `InvalidParams`.

Still open after this pass:

- Binary stdio smoke coverage is still missing and tracked in `acp-review-fix-plan.md`.
- `session.prompt.image`, `session.prompt.audio`, `session.prompt.embeddedContext`, and `session.mcp` remain intentionally out of scope for this branch.

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
- `session/delete` closes active sessions first, archives persisted sessions, and treats missing ids as success
- Config options: `model`, `thought_level`, `mode`
