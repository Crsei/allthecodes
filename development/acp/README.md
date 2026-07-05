# ACP v2 Server Mode

allthecodes can run as an Agent Client Protocol v2 server using `--acp`:

```bash
allthecodes --acp --cwd /path/to/project
```

## Protocol

- JSON-RPC 2.0 over newline-delimited stdin/stdout.
- stdout contains only valid JSON-RPC frames.
- Diagnostics go to stderr.
- Wire contract uses `agent-client-protocol-schema = "=1.2.0"` with ACP v2.

## Verified Supported Methods

Verified on 2026-07-03 in `.worktrees/acp-adapter`:

| Method or update | Verified behavior |
| --- | --- |
| `initialize` | Protocol version validation, auth method negotiation, and capability serialization. |
| `auth/login` | Returns agent-managed allthecodes login instructions in `_meta.instructions`. |
| `auth/logout` | Idempotent logout response. |
| `session/new` | Absolute cwd validation, additional directory validation, config option return, available command notification, and rejection of non-empty `mcpServers`. |
| `session/load` | Loads persisted sessions and replays visible transcript content before the response. |
| `session/resume` | Resumes persisted sessions through the normal session factory and sends command updates. |
| `session/list` | Supports global and workspace-filtered listing, cursor serialization, cwd mapping, and `_meta` fields. |
| `session/close` | Cancels active turns, waits boundedly for cancelled idle, flushes the recorder, and removes the session. |
| `session/delete` | Closes active sessions first, archives persisted sessions, treats missing ids as success, filters archived sessions from lists, and is advertised. |
| `session/prompt` | Supports text and file/resource-link prompt blocks, sends `PromptResponse` before updates, applies per-session config overrides, and maps runtime updates. |
| `session/cancel` | Aborts active turns and emits final idle state with `stopReason: "cancelled"`. |
| `session/set_config_option` | Validates model, mode, and thought-level choices, sends `config_option_update`, and applies prompt overrides. |
| `$/cancel_request` | Cancels pending prompt ACKs and pending permission requests. |
| `session/request_permission` client request | Round-trips engine permission callbacks through ACP client requests, including cancellation and disconnect denial. |
| `session/update` notifications | Covers state, usage, text chunks, thinking, tool calls, tool progress, plan updates, tombstones, retries, summaries, available commands, and config option updates. |
| Binary stdio smoke | `cargo test -p allthecodes --test acp_stdio_smoke` verifies `allthecodes --acp` emits only JSON-RPC frames on stdout for initialize/new/close. |

Focused verification run during the review-fix pass:

```bash
cargo test -p allthecodes-acp
cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update
cargo test -p allthecodes-acp --test protocol_methods cancel_request_cancels_pending_prompt_before_ack
cargo test -p allthecodes-acp --test config_options
cargo test -p allthecodes-acp --test protocol_methods auth
cargo test -p allthecodes-acp --test protocol_methods capability
cargo test -p allthecodes-acp --test session_lifecycle delete
cargo test -p allthecodes-acp --test prompt_updates cancel_sends_idle_cancelled
cargo test -p allthecodes --test acp_stdio_smoke
cargo check -p allthecodes-acp
cargo check --workspace
cargo build --workspace --release
```

## Intentionally Unadvertised Capabilities

These capabilities are out of scope for this branch and must remain absent from `initialize` until separate method paths and tests exist:

| Capability | Current behavior |
| --- | --- |
| `session.prompt.image` | Image content blocks are rejected with `InvalidParams`. |
| `session.prompt.audio` | Audio content blocks are rejected with `InvalidParams`. |
| `session.prompt.embeddedContext` | Embedded resource/context blocks are rejected with `InvalidParams`. |
| `session.mcp.*` | Per-session MCP is unadvertised; `session/new` and `session/load` reject non-empty `mcpServers`. |

## Known Remaining Limitations

- The real-model binary smoke exists as `acp_stdio_real_model_prompt_smoke` and uses the normal allthecodes config, auth, model, and provider path. It is ignored by default because it requires usable credentials, provider access, and network. On this machine, explicit runs against the current `backend=codex` config timed out after 300 seconds after only `available_commands_update` and `state_update: running`; no model content or idle frame arrived.
- Structured KAIROS Brief output currently degrades to plain `AgentMessage` text over ACP because the ACP wire mapping does not preserve Brief metadata. Track the required protocol extension in [structured-brief-wire-protocol-plan.md](structured-brief-wire-protocol-plan.md).
- `session.prompt.image`, `session.prompt.audio`, `session.prompt.embeddedContext`, and `session.mcp.*` remain separate feature work, not partial support.
- ACP mode uses the Rust backend/TUI-era runtime only. It does not alter legacy `--headless` JSONL IPC behavior.

## Client Expectations

- Protocol version must be `2`; v1 is rejected with `InvalidParams`.
- Sessions require an absolute, existing `cwd`.
- Additional directories must be absolute and exist.
- Prompt content supports text and `file://` resource links (`file://` becomes `@/path`).
- Request cancellation uses `$/cancel_request` with `requestId`; active turn cancellation uses `session/cancel` with `sessionId`.
- Config options currently exposed are `model`, `thought_level`, and `mode`.
