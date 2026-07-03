# ACP Review Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `test-driven-development` before each implementation task and `verification-before-completion` before claiming any task is complete. Steps use checkbox (`- [ ]`) syntax for tracking. Keep ACP stdout protocol-pure: only JSON-RPC frames may be written to stdout in ACP mode.

**Goal:** Close the ACP adapter review gaps so the implementation matches the ACP v2 adaptation plan, advertises only verified capabilities, has deterministic tests for protocol, session, prompt, permission, config, auth, and stdout behavior, and includes a binary ACP smoke test that runs a real model turn through the normal allthecodes runtime configuration.

**Architecture:** Keep `crates/allthecodes-acp` responsible for ACP transport, method dispatch, session state, schema conversion, and runtime tests. Keep root-only engine construction inside `crates/allthecodes/src/full_init.rs` or a future `crates/allthecodes/src/acp_runtime_bridge.rs`, but pass the same startup-owned dependencies into each ACP session engine. Add test harnesses around the ACP runtime so method ordering, notifications, client requests, and disconnect behavior are verified without a real API provider. Binary smoke tests must launch the real `allthecodes --acp` path and use the same effective config, model, auth, provider, and credential resolution as normal allthecodes startup; do not add an ACP-specific credential store or alternate model-selection path.

**Tech Stack:** Rust, Tokio, Serde JSON, JSON-RPC 2.0 over newline-delimited stdio, `agent-client-protocol-schema = "=1.2.0"` with `unstable_protocol_v2`, allthecodes `QueryEngine`, `allthecodes-session`, `allthecodes-commands`, `allthecodes-permissions`, and the Cargo environment from `AGENTS.md`.

## Global Constraints

- Work only in `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/.worktrees/acp-adapter` unless explicitly rebasing or merging.
- Do not change legacy `--headless` JSONL behavior except for shared helpers with tests.
- Do not touch Rust TUI code outside `crates/allthecodes/src/ui/`.
- Do not change npm packaging behavior.
- Do not advertise an ACP capability until its method path and tests pass.
- Fully implement `session/delete` in this branch; do not leave it intentionally unsupported after Task 6.
- Keep `session.mcp.*`, `session.prompt.image`, `session.prompt.audio`, and `session.prompt.embeddedContext` out of this branch's implementation scope. Leave explicit parser/API boundaries and capability gates, but keep them unadvertised and rejected until separate feature work lands.
- The binary ACP smoke must exercise a real model turn using the normal allthecodes configuration and auth resolution. It may require configured credentials and network access; failures must be explicit rather than silently falling back to a slash-command or mock path.
- ACP stdout must contain only JSON-RPC messages; diagnostics must go to stderr.
- Use `agent-client-protocol-schema` v1.2.0 ACP v2 schema as the wire contract.
- Add or update docs in `development/acp/README.md`, `docs/WORK_STATUS.md`, and `docs/archive/COMPLETED_FULL.md` only when the corresponding implementation is actually verified.

## Review Failure Status

1. Fixed 2026-07-03: `session/prompt` no longer emits `session/update` before `PromptResponse`; accepted prompt turns are gated until the response is enqueued.
2. Permission bridge only maps structs; engine callbacks and client request lifecycle are not wired.
3. `session/load` sends placeholder running/idle updates instead of replaying conversation.
4. `session/list` ignores workspace filtering, cursor, cwd, and `_meta`.
5. Tool, thinking, plan, and tombstone update mapping is incomplete.
6. Config options and per-turn overrides are incomplete; no `config_option_update` is sent.
7. Auth login does not return the required login instructions and auth detection bypasses normal resolution.
8. Documentation claims support for capabilities that are not implemented or not advertised; binary E2E smoke target is missing.

## File Structure

- `crates/allthecodes-acp/src/runtime.rs`: JSON-RPC dispatch, ACK ordering, notification routing, request cancellation, prompt task orchestration.
- `crates/allthecodes-acp/src/session.rs`: session map, active turn handle, loaded transcript replay, close/delete helpers.
- `crates/allthecodes-acp/src/updates.rs`: stateful `SdkMessage` to ACP `SessionUpdate` conversion.
- `crates/allthecodes-acp/src/tool_calls.rs`: tool context cache, kind/title/location/status/content/diff conversion.
- `crates/allthecodes-acp/src/permissions.rs`: engine callback installation, ACP client request manager, outcome routing.
- `crates/allthecodes-acp/src/config_options.rs`: model/mode/thought config construction, validation, and update application.
- `crates/allthecodes-acp/src/auth.rs`: credential status resolution and login/logout response shaping.
- `crates/allthecodes-acp/src/jsonrpc.rs` and `transport.rs`: JSON-RPC envelope and stdio purity tests.
- `crates/allthecodes-acp/tests/*.rs`: protocol, session, prompt/update, permission, config/auth tests.
- `crates/allthecodes/tests/acp_stdio_smoke.rs`: binary-level ACP stdio smoke tests.
- `crates/allthecodes/src/full_init.rs` or `crates/allthecodes/src/acp_runtime_bridge.rs`: root-owned engine factory wiring, including classifier and callbacks.
- `development/acp/README.md`: conservative operator status and link to this plan.

---

### Task 1: Runtime Test Harness And Prompt ACK Ordering

**Files:**
- Create: `crates/allthecodes-acp/tests/support/mod.rs`
- Create: `crates/allthecodes-acp/tests/protocol_methods.rs`
- Create: `crates/allthecodes-acp/tests/prompt_updates.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`
- Modify: `crates/allthecodes-acp/src/transport.rs`

**Interfaces:**
- Produces: `support::RuntimeHarness` with `send_request`, `send_notification`, `next_response`, and `drain_notifications`.
- Produces: an internal runtime entry point that can run against in-memory channels, not only real stdin/stdout.
- Consumes: existing `AcpRuntimeConfig`, `AcpEngineFactory`, and JSON-RPC builders.

- [x] **Step 1: Add failing ACK-order test**

Add `prompt_updates::prompt_acks_before_first_update`.

Required behavior:

```text
Input order:
1. initialize request
2. session/new request
3. session/prompt request

Expected output order:
1. response for session/prompt id
2. first session/update notification for that prompt
```

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update
```

Expected before fix: FAIL because the spawned task can send `session/update` before the request response is written.

- [x] **Step 2: Add an ACK gate**

Change `handle_session_prompt` so it creates the turn task in a paused state and releases it only after the response is enqueued.

Implementation shape:

```rust
struct PendingPromptStart {
    response: serde_json::Value,
    start_tx: tokio::sync::oneshot::Sender<()>,
}
```

`dispatch_request` should enqueue the JSON-RPC response first, then send `start_tx`. If keeping the current return type, add a small enum:

```rust
enum DispatchOutcome {
    Response(Result<serde_json::Value, v2::Error>),
    ResponseThenStart(PendingPromptStart),
}
```

- [x] **Step 3: Verify request cancellation before ACK**

Add `protocol_methods::cancel_request_cancels_pending_prompt_before_ack`. It must assert that cancelling the prompt request before the ACK returns JSON-RPC `RequestCancelled` and does not start the engine stream.

- [x] **Step 4: Run focused verification**

```bash
cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update
cargo test -p allthecodes-acp --test protocol_methods cancel_request_cancels_pending_prompt_before_ack
cargo check -p allthecodes-acp
```

Verified 2026-07-03:
- `cargo test -p allthecodes-acp --test prompt_updates prompt_acks_before_first_update`
- `cargo test -p allthecodes-acp --test protocol_methods cancel_request_cancels_pending_prompt_before_ack`
- `cargo test -p allthecodes-acp`
- `cargo check -p allthecodes-acp`

- [ ] **Step 5: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/runtime.rs crates/allthecodes-acp/src/transport.rs crates/allthecodes-acp/tests
git commit -m "Fix ACP prompt acknowledgement ordering"
```

### Task 2: Session Lifecycle Parity

**Files:**
- Create: `crates/allthecodes-acp/tests/session_lifecycle.rs`
- Modify: `crates/allthecodes-acp/src/session.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`
- Modify: `crates/allthecodes-acp/src/updates.rs`

**Interfaces:**
- Produces: `session::replay_loaded_messages(session_id, messages, sink) -> anyhow::Result<()>`.
- Produces: `session::list_sessions(params) -> Result<v2::ListSessionsResponse, v2::Error>`.
- Consumes: `allthecodes_session::resume::resume_session_detail`.
- Consumes: `allthecodes_session::storage::{list_sessions_page, list_workspace_sessions_page}`.

- [ ] **Step 1: Add failing `session/load` replay test**

Test name: `session_lifecycle::load_replays_before_response`.

Assertions:
- `session/load` creates the session with loaded messages.
- Every visible user and assistant message is sent via `session/update`.
- The replay notifications are observed before `LoadSessionResponse`.
- The placeholder running/idle pair is not emitted as a substitute for content.

- [ ] **Step 2: Implement replay conversion**

Map loaded `allthecodes_types::message::Message` values to ACP updates:
- user text -> `SessionUpdate::UserMessage`
- assistant text -> `SessionUpdate::AgentMessage`
- assistant thinking -> `SessionUpdate::AgentThought`
- existing tool use/result blocks -> same tool-call mapper used for live updates

Keep replay deterministic by deriving message ids from loaded message order:

```text
loaded-user-<index>
loaded-agent-<index>
loaded-thought-<index>
loaded-tool-<tool_use_id>
```

- [ ] **Step 3: Add failing `session/list` workspace/cursor/meta tests**

Test names:
- `session_lifecycle::list_uses_workspace_filter_when_cwd_present`
- `session_lifecycle::list_serializes_cursor`
- `session_lifecycle::list_includes_workspace_meta`

Assertions:
- With `cwd`, call workspace-filtered storage.
- Without `cwd`, call global storage.
- Response entries include real `cwd` or workspace root, not `PathBuf::from("")`.
- Cursor round-trips through JSON serialization of the storage cursor.
- `_meta.messageCount`, `_meta.workspaceKey`, and `_meta.workspaceRoot` are present when storage provides them.

- [ ] **Step 4: Implement list mapping**

Move `session/list` logic out of the large runtime match into `session.rs`. Preserve schema `_meta` by using the schema builder when available; if the schema type lacks a direct setter, serialize to `serde_json::Value`, insert `_meta`, then deserialize back only if the schema supports it. If deserialization drops `_meta`, return the JSON value directly from the handler.

- [ ] **Step 5: Close waits for turn idle**

Add `session_lifecycle::close_waits_for_cancelled_idle_before_removal`. `session/close` must:
- mark active turn cancelled
- call `engine.abort()`
- wait with a bounded timeout for final idle/cancelled update
- flush session recorder
- remove session from memory

- [ ] **Step 6: Run focused verification**

```bash
cargo test -p allthecodes-acp --test session_lifecycle
cargo check -p allthecodes-acp
```

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/session.rs crates/allthecodes-acp/src/runtime.rs crates/allthecodes-acp/src/updates.rs crates/allthecodes-acp/tests/session_lifecycle.rs
git commit -m "Align ACP session lifecycle"
```

### Task 3: Complete Prompt Update And Tool Call Mapping

**Files:**
- Create or expand: `crates/allthecodes-acp/tests/prompt_updates.rs`
- Modify: `crates/allthecodes-acp/src/updates.rs`
- Modify: `crates/allthecodes-acp/src/tool_calls.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`

**Interfaces:**
- Produces: `updates::AcpUpdateMapper`.
- Produces: `tool_calls::ToolCallContextCache`.
- Consumes: `allthecodes_tool_display::ToolClassifier`.
- Consumes: `SdkMessage` stream events and assistant content blocks.

- [ ] **Step 1: Add failing stream mapping tests**

Test names:
- `prompt_updates::state_running_precedes_agent_content`
- `prompt_updates::stream_text_delta_becomes_agent_message_chunk`
- `prompt_updates::thinking_delta_becomes_agent_thought_chunk`
- `prompt_updates::result_sends_usage_and_idle`
- `prompt_updates::max_turns_maps_to_max_turn_requests`
- `prompt_updates::cancel_sends_idle_cancelled`

- [ ] **Step 2: Replace stateless counter with mapper**

Implement:

```rust
pub struct AcpUpdateMapper {
    session_id: v2::SessionId,
    message_ids: MessageIdState,
    tool_cache: ToolCallContextCache,
}
```

The mapper owns deterministic ids for one turn and reuses the same id across chunks of the same stream message.

- [ ] **Step 3: Add failing tool tests**

Test names:
- `prompt_updates::assistant_tool_use_starts_tool_call`
- `prompt_updates::tool_result_completes_tool_call`
- `prompt_updates::tool_error_fails_tool_call`
- `prompt_updates::read_tool_location_is_absolute`
- `prompt_updates::edit_tool_without_old_new_does_not_fabricate_diff`
- `prompt_updates::tool_progress_appends_content_chunk`

- [ ] **Step 4: Implement tool mapping**

Tool call updates must include:
- `kind` from `ToolClassifier` and fallback name classification
- `status: in_progress` for tool use
- `status: completed` for successful tool result
- `status: failed` for tool errors
- absolute `locations`
- `rawInput`
- `rawOutput`
- text result content
- diff content only when old/new text exists

- [ ] **Step 5: Map non-content engine events**

Map:
- `SdkMessage::ApiRetry` -> `AgentThought` with `_meta.kind = "api_retry"`
- `SdkMessage::CompactBoundary` -> `AgentThought` with `_meta.kind = "compact_boundary"`
- `SdkMessage::ToolUseSummary` -> `AgentThought` with `_meta.kind = "tool_use_summary"`
- `SdkMessage::GoalUpdated` -> `PlanUpdate` when parseable, otherwise `AgentThought` with `_meta.kind = "goal_updated"`
- `SdkMessage::Tombstone` -> `AgentThought` with `_meta.kind = "tombstone"` and abandoned id

- [ ] **Step 6: Run focused verification**

```bash
cargo test -p allthecodes-acp --test prompt_updates
cargo check -p allthecodes-acp
```

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/updates.rs crates/allthecodes-acp/src/tool_calls.rs crates/allthecodes-acp/src/runtime.rs crates/allthecodes-acp/tests/prompt_updates.rs
git commit -m "Map ACP prompt updates fully"
```

### Task 4: Permission Bridge Runtime Wiring

**Files:**
- Create: `crates/allthecodes-acp/tests/permission_bridge.rs`
- Modify: `crates/allthecodes-acp/src/permissions.rs`
- Modify: `crates/allthecodes-acp/src/session.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`
- Modify: `crates/allthecodes/src/full_init.rs` or create `crates/allthecodes/src/acp_runtime_bridge.rs`

**Interfaces:**
- Produces: `permissions::AcpPermissionManager`.
- Produces: `permissions::install_permission_callbacks(session, sink, permission_manager)`.
- Produces: JSON-RPC client request routing for `session/request_permission`.
- Consumes: engine permission, ask-user, permission-event, and tool-progress callbacks.

- [ ] **Step 1: Add failing permission request lifecycle test**

Test name: `permission_bridge::permission_request_uses_client_request`.

Expected sequence:
1. engine callback receives `PermissionRequestPayload`
2. runtime emits `session/update` state `requires_action`
3. runtime sends JSON-RPC request with method `session/request_permission`
4. client response resolves the engine callback
5. runtime emits `session/update` state `running`

- [ ] **Step 2: Implement client request id management**

Add monotonic ACP-originated request ids:

```text
allthecodes-permission-<session-id>-<n>
```

Maintain:
- `request_id -> pending permission`
- `(session_id, tool_use_id) -> pending permission`

- [ ] **Step 3: Add outcome mapping tests**

Test names:
- `permission_bridge::allow_once_maps_to_allow`
- `permission_bridge::always_allow_maps_to_always_allow`
- `permission_bridge::cancelled_maps_to_deny`
- `permission_bridge::disconnect_denies_pending_permission`
- `permission_bridge::cancel_request_denies_pending_permission`

- [ ] **Step 4: Install callbacks per session**

Install callbacks immediately after session engine creation. The callback must:
- build `RequestPermissionRequest`
- emit `requires_action`
- wait for ACP client response
- emit `running`
- return deny if client disconnects, request is cancelled, or session closes

- [ ] **Step 5: Wire tool progress callback**

Map engine tool progress events to ACP `tool_call_content_chunk` using the same tool context cache from Task 3.

- [ ] **Step 6: Run focused verification**

```bash
cargo test -p allthecodes-acp --test permission_bridge
cargo check -p allthecodes
```

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/permissions.rs crates/allthecodes-acp/src/session.rs crates/allthecodes-acp/src/runtime.rs crates/allthecodes/src/full_init.rs crates/allthecodes-acp/tests/permission_bridge.rs
git commit -m "Bridge ACP permission requests"
```

### Task 5: Config Options, Auth, And Per-Turn Overrides

**Files:**
- Create: `crates/allthecodes-acp/tests/config_options.rs`
- Add to: `crates/allthecodes-acp/tests/protocol_methods.rs`
- Modify: `crates/allthecodes-acp/src/config_options.rs`
- Modify: `crates/allthecodes-acp/src/auth.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`

**Interfaces:**
- Produces: `config_options::SessionConfigState`.
- Produces: `config_options::submit_overrides_for_session(session) -> SubmitMessageOverrides`.
- Produces: `auth::resolve_acp_auth_methods(settings) -> Vec<AuthMethod>`.
- Consumes: `settings.available_models`, session `AppState`, and existing auth resolvers.

- [ ] **Step 1: Add failing config option tests**

Test names:
- `config_options::new_session_returns_model_mode_thought_options`
- `config_options::model_options_come_from_available_models`
- `config_options::set_model_updates_app_state`
- `config_options::set_unknown_config_rejects`
- `config_options::set_invalid_value_rejects`
- `config_options::set_option_sends_config_option_update`
- `config_options::prompt_uses_session_submit_overrides`

- [ ] **Step 2: Implement config state**

Use:
- `model`: values from `settings.available_models`, current from `app_state.main_loop_model`
- `mode`: known permission/chat modes supported by this repo, current from permission context
- `thought_level`: `low`, `medium`, `high`, current from `app_state.effort_value` or settings fallback

Reject any selected value not in the option set.

- [ ] **Step 3: Apply config to prompt overrides**

Before `submit_message_with_overrides`, build:

```rust
SubmitMessageOverrides {
    model: selected_model,
    effort: selected_thought_level,
    ..Default::default()
}
```

Also update permission mode in `AppState` before tool execution.

- [ ] **Step 4: Add failing auth tests**

Test names:
- `protocol_methods::initialize_advertises_agent_login_when_unauthenticated`
- `protocol_methods::initialize_omits_agent_login_when_authenticated`
- `protocol_methods::auth_login_returns_login_instructions`
- `protocol_methods::auth_login_rejects_unknown_method`
- `protocol_methods::auth_logout_is_idempotent`

- [ ] **Step 5: Implement auth status and login instructions**

Auth resolution must use the same effective rules as normal startup:
- native provider credentials via `auth::resolve_auth()`
- Codex credentials via `auth::resolve_codex_auth_token()`
- env and credential files under allthecodes paths

`auth/login` response without a session must include `_meta` instructions:

```text
API key: send /login sk-ant-... or /login openai-api sk-...
Claude OAuth: send /login claude-ai, then /login-code <code>
Console OAuth: send /login console, then /login-code <code>
Codex OAuth: send /login codex-oauth, then /login-code <code>
```

- [ ] **Step 6: Run focused verification**

```bash
cargo test -p allthecodes-acp --test config_options
cargo test -p allthecodes-acp --test protocol_methods auth
cargo check -p allthecodes-acp
```

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/config_options.rs crates/allthecodes-acp/src/auth.rs crates/allthecodes-acp/src/runtime.rs crates/allthecodes-acp/tests/config_options.rs crates/allthecodes-acp/tests/protocol_methods.rs
git commit -m "Expose ACP config and auth correctly"
```

### Task 6: Capability Gates, Session Delete, And MCP Policy

**Files:**
- Add to: `crates/allthecodes-acp/tests/session_lifecycle.rs`
- Modify: `crates/allthecodes-acp/src/runtime.rs`
- Modify: `crates/allthecodes-acp/src/session.rs`
- Modify: `crates/allthecodes-acp/src/mcp.rs`
- Modify: `development/acp/README.md`

**Interfaces:**
- Produces: capability builder that reflects only tested method support.
- Produces: tested `session/delete` behavior and advertises it only after the archive/list/active-close tests pass.
- Consumes: `allthecodes_session::storage::archive_session`.

- [ ] **Step 1: Add capability serialization tests**

Test names:
- `protocol_methods::initialize_advertises_session_delete_after_delete_tests_pass`
- `protocol_methods::initialize_omits_session_mcp_until_enabled`
- `protocol_methods::initialize_omits_prompt_multimodal_until_enabled`

- [ ] **Step 2: Implement full delete policy for this branch**

This branch must implement and advertise `session.delete` once its method path and tests pass.

Required delete tests:
- `session_lifecycle::delete_existing_session_archives_it`
- `session_lifecycle::delete_nonexistent_session_succeeds`
- `session_lifecycle::delete_active_session_closes_first`
- `session_lifecycle::deleted_session_no_longer_lists`

- [ ] **Step 3: Implement delete fully**

- close active in-memory session first
- archive persistent session
- treat nonexistent session as success
- ensure archived sessions are filtered from `session/list`
- enable `session_delete` in capabilities only after tests pass

- [ ] **Step 4: Keep MCP and multimodal policy honest**

Until per-session MCP and multimodal prompt blocks are implemented in separate feature work:
- reject non-empty `mcpServers` in `session/new`
- omit `session.mcp` capabilities
- reject image, audio, and embedded-context prompt blocks with explicit `InvalidParams`
- omit image/audio/embedded-context prompt capabilities
- keep conversion modules structured so these APIs can be added later without changing the text/resource-link path
- document MCP as unadvertised

Do not leave stub functions described as implemented behavior.

- [ ] **Step 5: Run focused verification**

```bash
cargo test -p allthecodes-acp --test protocol_methods capability
cargo test -p allthecodes-acp --test session_lifecycle delete
cargo check -p allthecodes-acp
```

- [ ] **Step 6: Commit**

```bash
git add -A -- crates/allthecodes-acp/src/runtime.rs crates/allthecodes-acp/src/session.rs crates/allthecodes-acp/src/mcp.rs crates/allthecodes-acp/tests/session_lifecycle.rs crates/allthecodes-acp/tests/protocol_methods.rs development/acp/README.md
git commit -m "Correct ACP capability gates"
```

### Task 7: Binary ACP Stdio Smoke Tests

**Files:**
- Create: `crates/allthecodes/tests/acp_stdio_smoke.rs`
- Add fixtures under: `crates/allthecodes-acp/tests/fixtures/`
- Modify: `crates/allthecodes-acp/src/runtime.rs` only if testability hooks need a small public API

**Interfaces:**
- Produces: binary-level test target required by the original plan.
- Consumes: `assert_cmd`, a temp project cwd, the normal allthecodes auth/config resolution path, and newline-delimited JSON-RPC frames.
- Consumes: the user's configured real model/provider credentials from allthecodes settings, credential files, keychain, or environment. Do not introduce ACP-only credentials.

- [ ] **Step 1: Add stdout purity smoke**

Test name: `acp_stdio_stdout_contains_only_jsonrpc_frames`.

Spawn:

```bash
allthecodes --acp --cwd <temp-project>
```

Send:
1. `initialize`
2. `session/new`
3. `session/close`

Assert every stdout line parses as JSON and contains `jsonrpc: "2.0"`.

- [ ] **Step 2: Add real-model prompt smoke**

This smoke must call a real model through the same engine path used by normal allthecodes prompts. It must not use slash commands, mock engines, synthetic assistant responses, or local-only command output as a substitute for the model-backed path.

Use the existing allthecodes effective configuration:
- preserve normal `ALLTHECODES_HOME` / credential lookup unless the test caller explicitly supplies a test home
- use startup-resolved model/provider/auth settings
- do not read, print, copy, or persist secret values
- fail with an actionable auth/config error when no usable credentials are available

Sequence:
1. `initialize`
2. `session/new`
3. `session/prompt` with a short text prompt for the real model, for example `Reply with one short sentence for ACP smoke verification.`
4. receive `PromptResponse`
5. receive at least one non-empty model-backed `session/update`
6. receive final idle
7. `session/list`
8. `session/close`

- [ ] **Step 3: Add cancellation coverage**

Cover deterministic cancellation in `prompt_updates::cancel_sends_idle_cancelled` with a mock engine stream. Add a real-model binary cancellation smoke only if the repository already has a reliable long-running prompt or provider-neutral delay hook; otherwise document that binary cancellation is covered at the runtime harness layer while the binary smoke covers stdout shape and a real model turn.

- [ ] **Step 4: Run focused verification**

```bash
cargo test -p allthecodes --test acp_stdio_smoke
cargo test -p allthecodes-acp --test prompt_updates cancel_sends_idle_cancelled
```

- [ ] **Step 5: Commit**

```bash
git add -A -- crates/allthecodes/tests/acp_stdio_smoke.rs crates/allthecodes-acp/tests/fixtures crates/allthecodes-acp/src/runtime.rs
git commit -m "Add ACP stdio smoke tests"
```

### Task 8: Documentation Reconciliation And Final Verification

**Files:**
- Modify: `development/acp/README.md`
- Modify: `docs/WORK_STATUS.md`
- Modify: `docs/archive/COMPLETED_FULL.md`
- Modify: `docs/KNOWN_ISSUES.md` only if a user-visible limitation remains

**Interfaces:**
- Consumes: verified test results from Tasks 1-7.
- Produces: user-facing ACP status that matches capability negotiation.

- [ ] **Step 1: Rewrite ACP README status**

The README must have three sections:
- Verified supported methods
- Intentionally unadvertised capabilities
- Known remaining limitations

No method may be marked supported unless:
- capability is advertised when relevant
- method path exists
- focused tests pass
- E2E smoke covers stdout shape

- [ ] **Step 2: Update project status docs**

Add ACP status to `docs/WORK_STATUS.md` with:
- exact completed scope
- exact unimplemented scope
- verification commands run
- date of verification

Move ACP entry to `docs/archive/COMPLETED_FULL.md` only after full baseline verification passes.

- [ ] **Step 3: Add known issues only for remaining user-visible limitations**

Examples that require `docs/KNOWN_ISSUES.md`:
- image/audio prompt blocks intentionally rejected
- per-session MCP not supported
- real-model ACP smoke requires configured allthecodes credentials, provider access, and network availability

- [ ] **Step 4: Run full verification set**

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-acp
cargo test -p allthecodes --test acp_stdio_smoke
cargo check --workspace
cargo build --workspace --release
```

- [ ] **Step 5: Commit docs separately**

```bash
git add -A -- development/acp/README.md docs/WORK_STATUS.md docs/archive/COMPLETED_FULL.md docs/KNOWN_ISSUES.md
git commit -m "Document ACP adapter support status"
```

## Execution Order

1. Task 1: make protocol ordering testable and fix prompt ACK ordering.
2. Task 2: make session lifecycle truthful.
3. Task 3: complete stream and tool updates.
4. Task 4: wire permission bridge.
5. Task 5: complete config and auth.
6. Task 6: correct capability gates and delete/MCP policy.
7. Task 7: add binary smoke.
8. Task 8: reconcile docs after verified behavior.

## Acceptance Gates

The branch is not complete until these pass in the `acp-adapter` worktree:

```bash
cargo test -p allthecodes-acp
cargo test -p allthecodes --test acp_stdio_smoke
cargo check --workspace
cargo build --workspace --release
```

Expected final state:
- ACP stdout smoke confirms every stdout line is a JSON-RPC frame.
- ACP binary smoke completes a real model-backed prompt through normal allthecodes config/auth resolution.
- `PromptResponse` is always observed before prompt updates.
- `session/load` replays actual conversation content before response.
- `session/list` honors workspace filtering and cursor/meta mapping.
- `session/delete` closes active sessions, archives persisted sessions, filters archived sessions from lists, and is advertised only after tests pass.
- Permission requests round-trip through ACP client requests.
- Tool calls, thinking, plan, result, tombstone, and cancellation updates map to ACP updates.
- Config options validate choices, send update notifications, and affect turn overrides.
- Auth login exposes concrete allthecodes login instructions.
- README and capability negotiation agree.

## Explicitly Out Of Scope For This Fix Plan

The following remain separate feature work and must not be implemented in this branch:
- `session.prompt.image`
- `session.prompt.audio`
- `session.prompt.embeddedContext`
- `session.mcp.stdio`
- `session.mcp.http`

Leave clear parser/API seams for these capabilities where the current code already receives their schema shapes, but keep the methods/capabilities unadvertised and return explicit unsupported/invalid-params errors until their own method paths and tests are implemented.
