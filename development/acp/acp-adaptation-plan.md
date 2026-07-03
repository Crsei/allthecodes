# ACP v2 Adaptation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `test-driven-development` before implementation, and use `verification-before-completion` before claiming completion. Keep ACP stdout protocol-pure: only JSON-RPC frames may be written to stdout in ACP mode.

**Goal:** Add an Agent Client Protocol v2 server mode to allthecodes so ACP clients can initialize the agent, create/load/resume/list/close sessions, submit prompts, receive streamed `session/update` notifications, answer permission requests, manage config options, use slash-command metadata, authenticate, cancel turns, and optionally delete sessions.

**Architecture:** Add a new Rust crate `crates/allthecodes-acp` that owns ACP JSON-RPC transport, method dispatch, session management, protocol-schema conversion, and client request handling. The root binary performs the existing full initialization, then dispatches `--acp` to the ACP stdio runtime. ACP sessions use `QueryEngine` as the execution boundary and `allthecodes-session` as the durable history boundary. The existing legacy `--headless` JSONL protocol remains unchanged.

**Tech Stack:** Rust 2024, Tokio, Serde, JSON-RPC 2.0 over newline-delimited stdio, `agent-client-protocol-schema = "=1.2.0"` with `unstable_protocol_v2`, allthecodes `QueryEngine`, `allthecodes-session`, `allthecodes-commands`, `allthecodes-permissions`, `allthecodes-mcp`, and the existing Cargo toolchain configuration from `AGENTS.md`.

## Protocol Baseline

ACP source of truth:

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/agent-client-protocol/docs/protocol/v2/*.mdx`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/agent-client-protocol/agent-client-protocol-schema/src/v2/*.rs`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/agent-client-protocol/agent-client-protocol-schema/src/rpc.rs`

Target version:

- Implement ACP `protocolVersion: 2`.
- Use `agent-client-protocol-schema` v1.2.0 with feature `unstable_protocol_v2`.
- Treat v2 as the adapter contract even though the schema crate gates it as experimental.
- Preserve `_meta` fields and unknown extension variants where schema types expose them.

JSON-RPC rules:

- Transport is newline-delimited UTF-8 JSON over stdin/stdout.
- stdout contains only ACP JSON-RPC messages.
- logs, diagnostics, and existing auth warnings go to stderr via `tracing`/`eprintln`.
- Support single requests, notifications, and non-empty batches.
- Notifications produce no response.
- Method errors use ACP/JSON-RPC error codes from `v2::error`.
- Request-level cancellation uses `$/cancel_request`; turn-level cancellation uses `session/cancel`.

## Capability Policy

Only advertise a capability after its method path and tests are implemented.

Initial stable advertised capability set after all required baseline tasks in this plan:

- `session`: enabled with baseline session methods:
  - `session/new`
  - `session/load`
  - `session/list`
  - `session/resume`
  - `session/close`
  - `session/prompt`
  - `session/cancel`
  - `session/update`
- `session.prompt`: supports baseline `ContentBlock::Text` and `ContentBlock::ResourceLink`.
- `session.prompt.image`: omitted until image prompt conversion is implemented.
- `session.prompt.audio`: omitted until audio prompt conversion is implemented.
- `session.prompt.embeddedContext`: omitted until embedded resource blocks are mapped.
- `session.additionalDirectories`: enabled after ACP `additionalDirectories` are applied to `ToolPermissionContext.additional_working_directories`.
- `session.delete`: enabled after `session/delete` maps to archive/delete behavior and passes tests.
- `session.mcp`: enabled after client-supplied ACP `mcpServers` are connected per session.

Authentication methods:

- If current settings/env/keychain credentials resolve to a usable API client, return `authMethods: []`.
- If no usable credentials resolve, return one `AuthMethod::Agent`:
  - `methodId`: `allthecodes-login`
  - `name`: `allthecodes login`
  - `description`: `Run allthecodes authentication using existing /login and /login-code flows.`

## Repository Changes

Create:

- `crates/allthecodes-acp/Cargo.toml`
- `crates/allthecodes-acp/src/lib.rs`
- `crates/allthecodes-acp/src/jsonrpc.rs`
- `crates/allthecodes-acp/src/transport.rs`
- `crates/allthecodes-acp/src/runtime.rs`
- `crates/allthecodes-acp/src/session.rs`
- `crates/allthecodes-acp/src/engine_factory.rs`
- `crates/allthecodes-acp/src/updates.rs`
- `crates/allthecodes-acp/src/content.rs`
- `crates/allthecodes-acp/src/tool_calls.rs`
- `crates/allthecodes-acp/src/permissions.rs`
- `crates/allthecodes-acp/src/config_options.rs`
- `crates/allthecodes-acp/src/commands.rs`
- `crates/allthecodes-acp/src/auth.rs`
- `crates/allthecodes-acp/src/mcp.rs`
- `crates/allthecodes-acp/src/errors.rs`
- `crates/allthecodes-acp/tests/jsonrpc_stdio.rs`
- `crates/allthecodes-acp/tests/protocol_methods.rs`
- `crates/allthecodes-acp/tests/session_lifecycle.rs`
- `crates/allthecodes-acp/tests/prompt_updates.rs`
- `crates/allthecodes-acp/tests/permission_bridge.rs`
- `crates/allthecodes-acp/tests/config_options.rs`
- `crates/allthecodes-acp/tests/fixtures/*.json`
- `crates/allthecodes/src/acp_runtime_bridge.rs`

Modify:

- `Cargo.toml`
- `crates/allthecodes/Cargo.toml`
- `crates/allthecodes/src/cli.rs`
- `crates/allthecodes/src/main.rs`
- `crates/allthecodes/src/full_init.rs`
- `crates/allthecodes/src/app_runtime_adapters/mod.rs`
- `crates/allthecodes-engine/src/types/config.rs` only if the engine factory needs a first-class field not already expressible through `AppState`.
- `docs/WORK_STATUS.md` after implementation lands.
- `docs/KNOWN_ISSUES.md` only when testing exposes a user-visible limitation.

Do not modify:

- Existing `crates/allthecodes-ipc` legacy JSONL behavior except shared helper extraction with tests.
- Rust TUI code outside `crates/allthecodes/src/ui/`.
- npm packaging behavior.

## Implementation Tasks

### 1. Add ACP Crate And Dependency Boundary

- [ ] Add workspace dependency entries in root `Cargo.toml`:

```toml
agent-client-protocol-schema = { version = "=1.2.0", features = ["unstable_protocol_v2"] }
```

- [ ] Add `crates/allthecodes-acp/Cargo.toml`:

```toml
[package]
name = "allthecodes-acp"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
agent-client-protocol-schema.workspace = true
anyhow.workspace = true
chrono.workspace = true
futures.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
uuid.workspace = true

allthecodes-auth.workspace = true
allthecodes-commands.workspace = true
allthecodes-engine.workspace = true
allthecodes-mcp.workspace = true
allthecodes-session.workspace = true
allthecodes-tool-display.workspace = true
allthecodes-types.workspace = true
```

- [ ] Add `allthecodes-acp` to `crates/allthecodes/Cargo.toml`.
- [ ] Keep the new crate free of any dependency on the root binary crate to avoid a cycle.
- [ ] Expose a public `run_stdio(config: AcpRuntimeConfig) -> anyhow::Result<()>` from `crates/allthecodes-acp/src/lib.rs`.

Acceptance:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo check -p allthecodes-acp
```

Expected result: the new crate compiles with an empty runtime shell and no warnings.

### 2. Implement ACP JSON-RPC Stdio Transport

- [ ] Implement `jsonrpc.rs` around schema crate `rpc::{JsonRpcMessage, JsonRpcBatch, Request, Response, Notification, RequestId}`.
- [ ] Implement `transport.rs` with:
  - `AcpStdioReader`
  - `AcpStdioWriter`
  - `read_frame()`
  - `write_message()`
  - `write_batch()`
- [ ] Parse one JSON value per newline.
- [ ] Reject empty batch with `InvalidRequest`.
- [ ] Return `ParseError` with `id: null` for malformed JSON.
- [ ] Return `MethodNotFound` for unsupported methods.
- [ ] Preserve request id type: number, string, or null.
- [ ] For batch input, send a batch response containing only responses for requests that require a response.
- [ ] Flush stdout after every written JSON-RPC message.

Tests:

- `jsonrpc_stdio::parse_single_request`
- `jsonrpc_stdio::parse_notification_no_response`
- `jsonrpc_stdio::parse_non_empty_batch`
- `jsonrpc_stdio::reject_empty_batch`
- `jsonrpc_stdio::malformed_json_returns_parse_error`
- `jsonrpc_stdio::stdout_writer_appends_single_newline`

Acceptance:

```bash
cargo test -p allthecodes-acp jsonrpc_stdio
```

### 3. Add CLI ACP Mode Without Touching Legacy Headless

- [ ] Add hidden CLI flag in `crates/allthecodes/src/cli.rs`:

```rust
/// Agent Client Protocol mode: JSON-RPC 2.0 over stdio.
#[arg(long = "acp", hide = true)]
pub acp: bool,
```

- [ ] In `full_init.rs`, include `cli.acp` in `source_mode` as `"acp"`.
- [ ] Dispatch ACP after engine creation and before server/TUI/headless routing:

```rust
if cli.acp {
    let result = allthecodes_acp::run_stdio(
        crate::acp_runtime_bridge::build_acp_runtime_config(crate::acp_runtime_bridge::AcpBridgeInputs {
            engine,
            model,
            cwd: cwd.clone(),
            tools: tools.clone(),
            app_state_template: app_state.clone(),
            detected_client: detected_client.clone(),
            merged_config: merged_config.clone(),
            cli_overrides: crate::acp_runtime_bridge::AcpCliOverrides::from_cli(&cli),
        }),
    )
    .await
    .map(|()| ExitCode::SUCCESS);
    persist_skill_usage();
    return result;
}
```

- [ ] Ensure ACP mode does not also start TUI, `--headless`, web, or daemon server.
- [ ] Add a conflict policy in Clap if needed:
  - `--acp` conflicts with `--headless`
  - `--acp` conflicts with `--print`
  - `--acp` conflicts with `--web`
  - `--acp` conflicts with `--daemon`

Acceptance:

```bash
cargo check -p allthecodes
```

Expected result: `allthecodes --acp` routes into ACP runtime; existing `--headless` still routes into `allthecodes_ipc::headless::run_headless`.

### 4. Extract Engine Factory For Per-ACP Sessions

ACP supports multiple active sessions. A single mutable `QueryEngine` with `active_session_id` is not a sufficient concurrency boundary. Use one `QueryEngine` per ACP session.

- [ ] Add `crates/allthecodes-acp/src/engine_factory.rs`:

```rust
pub struct AcpEngineParams {
    pub session_id: Option<String>,
    pub cwd: std::path::PathBuf,
    pub additional_directories: Vec<std::path::PathBuf>,
    pub initial_messages: Option<Vec<allthecodes_types::message::Message>>,
}

pub trait AcpEngineFactory: Send + Sync {
    fn create_engine(&self, params: AcpEngineParams) -> anyhow::Result<std::sync::Arc<allthecodes_engine::lifecycle::QueryEngine>>;
}
```

- [ ] Add `crates/allthecodes/src/acp_runtime_bridge.rs` with root-owned factory implementation.
- [ ] Refactor root engine construction in `full_init.rs` so the same helper can create:
  - the initial TUI/headless engine
  - each ACP session engine
- [ ] The factory must apply the same root-owned setup as normal startup:
  - `set_hook_runner(ShellHookRunner)`
  - `set_command_dispatcher(DefaultCommandDispatcher::for_full_registry())`
  - auto classifier setup when an API client exists
  - cloned `AppState`
  - current tools list, including MCP and computer-use tools already discovered during startup
  - `persist_session: true`
  - `auto_save_session: true`
  - `replay_user_messages: true` for ACP sessions
- [ ] If `params.session_id` is present, call `engine.set_current_session_id(SessionId::from_string(...))`.
- [ ] Apply `params.additional_directories` by inserting canonical paths into:

```rust
engine.update_app_state(|state| {
    state.tool_permission_context.additional_working_directories.insert(
        display_name,
        allthecodes_types::permissions::AdditionalWorkingDirectory {
            path: canonical_path,
            read_only: false,
        },
    );
});
```

Tests:

- `session_lifecycle::new_session_creates_distinct_engine`
- `session_lifecycle::load_session_sets_requested_session_id`
- `session_lifecycle::additional_directories_are_added_to_permission_context`

Acceptance:

```bash
cargo test -p allthecodes-acp session_lifecycle
cargo check -p allthecodes
```

### 5. Implement Initialization And Auth Methods

- [ ] Implement `initialize` in `runtime.rs`.
- [ ] Validate `protocolVersion == 2`; return `InvalidParams` for other versions.
- [ ] Return:
  - `protocolVersion: 2`
  - `agentCapabilities`
  - `implementation.name: "allthecodes"`
  - `implementation.version: env!("CARGO_PKG_VERSION")`
  - `authMethods`
- [ ] Build capabilities from actual implemented feature flags in `runtime.rs`; do not hard-code future methods as enabled.
- [ ] Implement `auth/login` in `auth.rs`:
  - Accept only `methodId == "allthecodes-login"`.
  - Return `MethodNotFound` or `InvalidParams` for unknown method ids.
  - For agent-managed login, send an ACP `session/update` style informational message only when a session exists; otherwise return `LoginAuthResponse` with `_meta` containing the login instructions.
  - Include exact instructions for existing flows:
    - API key: send `/login sk-ant-...` or `/login openai-api sk-...` as a prompt.
    - Claude OAuth: send `/login claude-ai`, then `/login-code <code>`.
    - Console OAuth: send `/login console`, then `/login-code <code>`.
    - Codex OAuth: send `/login codex-oauth`, then `/login-code <code>`.
- [ ] Implement `auth/logout` by calling `allthecodes_auth::oauth_logout()` and returning the schema response.
- [ ] Recompute auth status after login/logout for subsequent `initialize` responses.

Tests:

- `protocol_methods::initialize_rejects_protocol_v1`
- `protocol_methods::initialize_returns_allthecodes_implementation`
- `protocol_methods::initialize_advertises_agent_login_when_unauthenticated`
- `protocol_methods::auth_login_rejects_unknown_method`
- `protocol_methods::auth_logout_is_idempotent`

Acceptance:

```bash
cargo test -p allthecodes-acp protocol_methods::initialize protocol_methods::auth
```

### 6. Implement Session Lifecycle

- [ ] Add `session.rs`:

```rust
pub struct AcpSession {
    pub session_id: agent_client_protocol_schema::v2::SessionId,
    pub cwd: std::path::PathBuf,
    pub additional_directories: Vec<std::path::PathBuf>,
    pub engine: std::sync::Arc<allthecodes_engine::lifecycle::QueryEngine>,
    pub active_turn: tokio::sync::Mutex<Option<AcpTurnHandle>>,
}

pub struct AcpSessionManager {
    sessions: tokio::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<AcpSession>>>,
}
```

- [ ] `session/new`:
  - require absolute `cwd`
  - require every `additionalDirectories` path to be absolute, existing, and a directory
  - accept empty `mcpServers`
  - reject non-empty `mcpServers` until Task 14 is complete
  - create a per-session `QueryEngine`
  - return `NewSessionResponse { sessionId, configOptions }`
  - send `available_commands_update`
  - send `config_option_update`
- [ ] `session/list`:
  - if `cwd` is present, call `allthecodes_session::storage::list_workspace_sessions_page`
  - otherwise call `allthecodes_session::storage::list_sessions_page`
  - map cursor through JSON serialization of `SessionListCursor`
  - map `updatedAt` from `last_modified` as RFC 3339 UTC
  - include `_meta.messageCount`, `_meta.workspaceKey`, `_meta.workspaceRoot`
- [ ] `session/load`:
  - require absolute `cwd`
  - load messages with `allthecodes_session::resume::resume_session_detail`
  - create a session engine using loaded messages
  - replay the full visible conversation via `session/update` before responding
  - after replay finishes, return `LoadSessionResponse { configOptions }`
- [ ] `session/resume`:
  - load messages like `session/load`
  - do not replay conversation
  - return `ResumeSessionResponse { configOptions }`
- [ ] `session/close`:
  - if a turn is active, call `engine.abort()`
  - wait for the turn task to send final idle/cancelled update, with a bounded timeout
  - remove the session from the in-memory map
  - flush session recorder if present
  - return `{}` schema response
- [ ] Reject prompt/list/load/resume/close requests with `ResourceNotFound` when the session id is unknown.

Tests:

- `session_lifecycle::new_rejects_relative_cwd`
- `session_lifecycle::new_rejects_relative_additional_directory`
- `session_lifecycle::list_uses_workspace_filter_when_cwd_present`
- `session_lifecycle::load_replays_before_response`
- `session_lifecycle::resume_does_not_replay`
- `session_lifecycle::close_removes_session`

Acceptance:

```bash
cargo test -p allthecodes-acp session_lifecycle
```

### 7. Implement Prompt Content Conversion

- [ ] Add `content.rs`.
- [ ] Convert ACP prompt blocks to a single allthecodes prompt string for `QueryEngine::submit_message_with_overrides`.
- [ ] `ContentBlock::Text`: append text exactly, preserving user line breaks.
- [ ] `ContentBlock::ResourceLink`:
  - accept `file://` URIs whose decoded path is absolute
  - append `@/absolute/path` so existing allthecodes input processing can resolve file mentions
  - for non-file URI schemes, append a readable marker:

```text
[resource_link: <name> <uri>]
```

- [ ] Reject `ContentBlock::Image` until image prompt capability is advertised.
- [ ] Reject `ContentBlock::Audio` until audio prompt capability is advertised.
- [ ] Reject embedded `ContentBlock::Resource` until embedded-context capability is advertised.
- [ ] Preserve prompt block `_meta` by copying it into turn-local metadata for diagnostics; do not send `_meta` to the model.
- [ ] Empty prompt after conversion returns `InvalidParams`.

Tests:

- `prompt_updates::text_prompt_preserves_lines`
- `prompt_updates::resource_link_file_uri_maps_to_at_path`
- `prompt_updates::unsupported_image_is_invalid_params`
- `prompt_updates::empty_prompt_is_invalid_params`

Acceptance:

```bash
cargo test -p allthecodes-acp prompt_updates::prompt_conversion
```

### 8. Implement Prompt Lifecycle And Streaming Updates

- [ ] Add `updates.rs` with `SdkMessage -> SessionUpdate` mapping.
- [ ] `session/prompt`:
  - validate session exists
  - reject if the session has an active turn with `InvalidRequest`
  - convert content blocks
  - spawn a turn task
  - return `PromptResponse {}` immediately after accepting the prompt
  - the turn task sends updates asynchronously
- [ ] Before polling the engine stream, send:

```json
{"sessionUpdate":"state_update","state":"running"}
```

- [ ] Use `QueryEngine::submit_message_with_overrides(prompt, QuerySource::Sdk, SubmitMessageOverrides::default())`.
- [ ] Map `SdkMessage::UserReplay` to `SessionUpdate::UserMessage`.
- [ ] Map `SdkMessage::StreamEvent`:
  - text deltas to `agent_message_chunk`
  - thinking deltas to `agent_thought_chunk`
  - maintain deterministic message ids per engine stream message
- [ ] Map `SdkMessage::Assistant`:
  - text blocks to `agent_message`
  - thinking blocks to `agent_thought`
  - tool-use blocks to `tool_call_update` with `status: in_progress`
- [ ] Map `SdkMessage::ApiRetry` to an `agent_thought` with `_meta.kind = "api_retry"`.
- [ ] Map `SdkMessage::CompactBoundary` to an `agent_thought` with `_meta.kind = "compact_boundary"`.
- [ ] Map `SdkMessage::ToolUseSummary` to an `agent_thought` with `_meta.kind = "tool_use_summary"`.
- [ ] Map `SdkMessage::GoalUpdated` to `plan_update` when the payload can be interpreted as plan/goal items; otherwise send an `agent_thought` with `_meta.kind = "goal_updated"`.
- [ ] Map `SdkMessage::Tombstone` to an `agent_thought` with `_meta.kind = "tombstone"` and the abandoned message id.
- [ ] Map `SdkMessage::Result`:
  - send `usage_update`
  - send final `state_update idle`
  - set stop reason:
    - `cancelled` when `engine.abort_reason().is_some()`
    - `max_turn_requests` for `ResultSubtype::ErrorMaxTurns`
    - `max_tokens` when `stop_reason == "max_tokens"`
    - `refusal` when result subtype is an execution error and no more specific ACP stop reason applies
    - `end_turn` for successful completion
- [ ] On internal turn task failure, send an `agent_message` containing the failure summary and final `state_update idle` with `refusal`.

Tests:

- `prompt_updates::prompt_acks_before_first_update`
- `prompt_updates::state_running_precedes_agent_content`
- `prompt_updates::stream_text_delta_becomes_agent_message_chunk`
- `prompt_updates::thinking_delta_becomes_agent_thought_chunk`
- `prompt_updates::result_sends_usage_and_idle`
- `prompt_updates::max_turns_maps_to_max_turn_requests`

Acceptance:

```bash
cargo test -p allthecodes-acp prompt_updates
```

### 9. Implement Tool Call Mapping

- [ ] Add `tool_calls.rs`.
- [ ] Reuse `allthecodes_tool_display::ToolClassifier` for kind/title/status classification.
- [ ] Maintain `(session_id, tool_use_id) -> tool context` cache like `sdk_mapper.rs`.
- [ ] Map internal tool names to ACP `ToolCallKind`:
  - read tools: `read`
  - edit/write/multiedit tools: `edit`
  - delete/remove tools: `delete`
  - move/rename tools: `move`
  - grep/glob/search tools: `search`
  - bash/shell/process tools: `execute`
  - think/plan/internal reasoning tools: `think`
  - web fetch/search tools: `fetch`
  - all other tools: `other`
- [ ] For tool input paths, convert relative paths against session `cwd` and emit absolute `locations`.
- [ ] For completed tool results:
  - success -> `status: completed`
  - tool error -> `status: failed`
  - include `rawInput`
  - include `rawOutput`
  - include text output as `content`
- [ ] For edit tools with old/new text available, include ACP `diff` content with absolute path.
- [ ] If old/new text is not available from current tool result data, still emit `locations` and `rawOutput`; add a test that verifies the adapter does not fabricate diffs.
- [ ] Map `ToolProgress` callback to `tool_call_content_chunk`.

Tests:

- `prompt_updates::assistant_tool_use_starts_tool_call`
- `prompt_updates::tool_result_completes_tool_call`
- `prompt_updates::tool_error_fails_tool_call`
- `prompt_updates::read_tool_location_is_absolute`
- `prompt_updates::tool_progress_appends_content_chunk`

Acceptance:

```bash
cargo test -p allthecodes-acp prompt_updates::tool
```

### 10. Implement Permission Bridge

- [ ] Add `permissions.rs`.
- [ ] Install per-session callbacks on each ACP session engine:
  - permission callback
  - ask-user callback
  - permission-event callback
  - tool-progress callback
- [ ] On `PermissionRequestPayload`, send client request `session/request_permission`.
- [ ] Emit `state_update requires_action` before waiting for the client response.
- [ ] Emit `state_update running` after the response is received.
- [ ] Map request fields:
  - `tool_use_id` -> `_meta.allthecodes.toolUseId`
  - `tool_name` -> permission `title`
  - `message` -> permission `description`
  - `operation` -> structured `subject`
  - `options` -> ACP `PermissionOption`
- [ ] Map options:
  - `allow` -> `kind: allow_once`
  - `always_allow` -> `kind: allow_always`
  - `deny` -> `kind: reject_once`
  - `always_deny` -> `kind: reject_always`
  - `auto_review` -> custom option `_allthecodes_auto_review`
- [ ] Map client outcomes:
  - `selected` with `optionId` -> `PermissionResponsePayload::new(optionId, None)`
  - `cancelled` -> `PermissionResponsePayload::deny()`
- [ ] If the ACP client disconnects while permission is pending, return `deny`.
- [ ] If the pending client request is cancelled with `$/cancel_request`, return `deny`.

Tests:

- `permission_bridge::permission_request_uses_client_request`
- `permission_bridge::permission_requires_action_then_running`
- `permission_bridge::allow_once_maps_to_allow`
- `permission_bridge::cancelled_maps_to_deny`
- `permission_bridge::disconnect_denies_pending_permission`

Acceptance:

```bash
cargo test -p allthecodes-acp permission_bridge
```

### 11. Implement Cancellation

- [ ] `session/cancel`:
  - find session
  - call `engine.abort()`
  - mark active turn as cancel requested
  - return no response because it is an ACP notification
  - active turn sends final `state_update idle` with `stopReason: cancelled`
- [ ] `$/cancel_request`:
  - if request id maps to an in-flight JSON-RPC method, cancel that method's future
  - if the cancelled request is `session/prompt` before ACK, return `RequestCancelled`
  - do not treat request-level cancellation as closing the session
- [ ] On `session/close`, cancel active turn as in Task 6.

Tests:

- `protocol_methods::session_cancel_is_notification`
- `prompt_updates::cancel_sends_idle_cancelled`
- `protocol_methods::cancel_request_cancels_pending_method`

Acceptance:

```bash
cargo test -p allthecodes-acp cancel
```

### 12. Implement Config Options

- [ ] Add `config_options.rs`.
- [ ] Build config options from each session engine `AppState`.
- [ ] Use stable `SessionConfigOption::select`; do not use unstable boolean config until `unstable_boolean_config` is intentionally enabled.
- [ ] Provide:
  - `configId: "model"`, category `model`, options from `settings.available_models`, current value from `app_state.main_loop_model`
  - `configId: "mode"`, category `mode`, options from known permission/chat modes, current value from permission mode
  - `configId: "thought_level"`, category `thought_level`, options `low`, `medium`, `high`, current value from `app_state.effort_value` or settings fallback
- [ ] `session/set_config_option`:
  - validate session id
  - validate config id
  - validate selected value is in the option set
  - update session engine `AppState`
  - return full current config option list
  - send `config_option_update`
- [ ] Update per-turn submit overrides from session config:
  - selected model -> `SubmitMessageOverrides.model`
  - selected thought level -> `SubmitMessageOverrides.effort`
  - mode changes update permission context before tool execution

Tests:

- `config_options::new_session_returns_model_mode_thought_options`
- `config_options::set_model_updates_app_state`
- `config_options::set_unknown_config_rejects`
- `config_options::set_invalid_value_rejects`
- `config_options::set_option_returns_complete_list`

Acceptance:

```bash
cargo test -p allthecodes-acp config_options
```

### 13. Implement Slash Command Updates

- [ ] Add `commands.rs`.
- [ ] Build ACP `available_commands_update` from:

```rust
allthecodes_commands::runtime::command_metadata_snapshot()
```

- [ ] Filter `allthecodes_commands::is_hidden_command(name)` from the advertised list.
- [ ] Include command name with slash prefix in ACP command fields.
- [ ] Include aliases in `_meta.allthecodes.aliases`.
- [ ] Send command update:
  - after `session/new`
  - after `session/load`
  - after `session/resume`
- [ ] Slash command execution uses normal `session/prompt` with text beginning `/`.
- [ ] Ensure command output from `CommandResult::Output` maps to ACP agent/system-visible update and does not invoke the model.

Tests:

- `protocol_methods::available_commands_update_contains_visible_commands`
- `protocol_methods::hidden_commands_are_not_advertised`
- `prompt_updates::slash_command_output_is_sent_without_model_turn`

Acceptance:

```bash
cargo test -p allthecodes-acp commands
```

### 14. Implement ACP MCP Server Inputs

- [ ] Add `mcp.rs`.
- [ ] Map ACP `McpServer` variants to `allthecodes_mcp::McpServerConfig`.
- [ ] For `stdio` servers:
  - command
  - args
  - env
  - display name
- [ ] For `http` servers:
  - url
  - headers
  - auth/bearer token env var when represented by ACP schema
- [ ] Connect client-supplied servers with a per-session `McpManager`.
- [ ] Merge returned MCP tools into that session engine only.
- [ ] Discover MCP skill resources for the session and register them for that session before first prompt.
- [ ] Preserve existing startup-discovered MCP behavior.
- [ ] Advertise `session.mcp.stdio` and `session.mcp.http` only after corresponding tests pass.

Tests:

- `session_lifecycle::new_with_stdio_mcp_adds_tools_to_session`
- `session_lifecycle::mcp_tools_do_not_leak_between_sessions`
- `session_lifecycle::new_with_http_mcp_adds_tools_to_session`
- `session_lifecycle::mcp_connection_failure_returns_invalid_params_with_details`

Acceptance:

```bash
cargo test -p allthecodes-acp mcp
```

### 15. Implement Session Delete

- [ ] Implement `session/delete` after baseline session lifecycle is stable.
- [ ] If session is active in memory, close it first.
- [ ] Delete semantics:
  - active JSON/SQLite session is archived using `allthecodes_session::storage::archive_session`
  - deleting a nonexistent session returns success, matching ACP docs
  - deleted/archived sessions no longer appear in `session/list`
- [ ] Advertise `session.delete` after tests pass.

Tests:

- `session_lifecycle::delete_existing_session_archives_it`
- `session_lifecycle::delete_nonexistent_session_succeeds`
- `session_lifecycle::delete_active_session_closes_first`

Acceptance:

```bash
cargo test -p allthecodes-acp session_lifecycle::delete
```

### 16. Add End-To-End ACP Smoke Tests

- [ ] Add integration helper that spawns:

```bash
allthecodes --acp --cwd <temp-project>
```

- [ ] Use temp `ALLTHECODES_HOME` for isolated credentials/session files.
- [ ] Feed JSON-RPC frames through stdin.
- [ ] Assert stdout contains only valid JSON-RPC frames.
- [ ] Smoke sequence:
  1. `initialize`
  2. `session/new`
  3. `session/prompt`
  4. receive `state_update running`
  5. receive at least one content update
  6. receive final `state_update idle`
  7. `session/list`
  8. `session/close`
- [ ] Add cancellation smoke:
  1. `session/prompt`
  2. `session/cancel`
  3. receive final idle with `cancelled`
- [ ] Add permission smoke with a fake permission-requiring tool if a deterministic in-process fixture exists; otherwise use a mock engine factory in crate tests.

Acceptance:

```bash
cargo test -p allthecodes-acp --test protocol_methods
cargo test -p allthecodes --test acp_stdio_smoke
```

### 17. Documentation And Status Updates

- [ ] Add a concise operator note under `development/acp/README.md` after implementation:
  - how to run `allthecodes --acp`
  - supported ACP capabilities
  - intentionally unadvertised capabilities
  - known client expectations
- [ ] Update `docs/WORK_STATUS.md` with ACP implementation status.
- [ ] Update `docs/archive/COMPLETED_FULL.md` after the adapter is verified.
- [ ] If any user-visible ACP limitation remains, add it to `docs/KNOWN_ISSUES.md` with concrete reproduction steps.

Acceptance:

```bash
rg -n "ACP|Agent Client Protocol|--acp" development docs crates/allthecodes/src/cli.rs
```

## Mapping Reference

### ACP Methods To allthecodes Components

| ACP method | allthecodes target |
| --- | --- |
| `initialize` | `allthecodes-acp::runtime`, `allthecodes_auth`, startup model/version data |
| `auth/login` | existing `/login` and `/login-code` flow surfaced through `AuthMethod::Agent` |
| `auth/logout` | `allthecodes_auth::oauth_logout()` |
| `session/new` | `AcpEngineFactory`, `QueryEngine::new`, `ToolPermissionContext.additional_working_directories` |
| `session/load` | `allthecodes_session::resume::resume_session_detail`, replay through ACP updates |
| `session/resume` | `allthecodes_session::resume::resume_session_detail`, no replay |
| `session/list` | `allthecodes_session::storage::{list_sessions_page,list_workspace_sessions_page}` |
| `session/close` | `QueryEngine::abort`, session recorder flush, in-memory removal |
| `session/delete` | `allthecodes_session::storage::archive_session` |
| `session/prompt` | `QueryEngine::submit_message_with_overrides(..., QuerySource::Sdk, ...)` |
| `session/cancel` | `QueryEngine::abort()` |
| `session/set_config_option` | per-session `AppState` updates and submit overrides |
| `session/request_permission` | ACP client request from installed permission callback |
| `session/update` | `SdkMessage` and callback events converted to ACP notifications |

### Internal Events To ACP Updates

| allthecodes event | ACP update |
| --- | --- |
| `SdkMessage::SystemInit` | `config_option_update`, `available_commands_update`, `_meta.system_init` |
| `SdkMessage::UserReplay` | `user_message` |
| `StreamEvent::ContentBlockDelta(text_delta)` | `agent_message_chunk` |
| `StreamEvent::ContentBlockDelta(thinking_delta)` | `agent_thought_chunk` |
| `SdkMessage::Assistant` text | `agent_message` |
| `SdkMessage::Assistant` thinking | `agent_thought` |
| `ContentBlock::ToolUse` | `tool_call_update` `in_progress` |
| `ContentBlock::ToolResult` replay | `tool_call_update` `completed` or `failed` |
| `ToolProgress` callback | `tool_call_content_chunk` |
| `PermissionRequestPayload` | client request `session/request_permission` |
| `SdkMessage::ApiRetry` | `agent_thought` with `_meta.kind = "api_retry"` |
| `SdkMessage::CompactBoundary` | `agent_thought` with `_meta.kind = "compact_boundary"` |
| `SdkMessage::ToolUseSummary` | `agent_thought` with `_meta.kind = "tool_use_summary"` |
| `SdkMessage::Result` | `usage_update`, then `state_update idle` |

## Full Verification Command Set

Use the project-local Rust environment:

```bash
cd /data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes

export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-acp
cargo test -p allthecodes --test acp_stdio_smoke
cargo check --workspace
cargo build --workspace --release
```

Expected final state:

- No build warnings from ACP code.
- Existing legacy headless tests still pass.
- ACP stdio smoke confirms stdout contains only JSON-RPC frames.
- `allthecodes --acp` supports the advertised ACP v2 session lifecycle.

## Commit Strategy

Suggested commit grouping:

1. `Add ACP protocol crate skeleton`
2. `Implement ACP JSON-RPC stdio transport`
3. `Wire ACP CLI mode`
4. `Add ACP session lifecycle`
5. `Map engine events to ACP updates`
6. `Bridge ACP permissions and cancellation`
7. `Expose ACP config and commands`
8. `Add ACP MCP and delete support`
9. `Document ACP support`

Only stage paths touched by each commit. Follow the explicit-path commit process in `AGENTS.md`.
