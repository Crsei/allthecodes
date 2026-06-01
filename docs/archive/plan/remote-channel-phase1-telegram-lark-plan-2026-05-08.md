# Remote Module Phase 1 Plan: Telegram / Lark Channel Connectivity

Date: 2026-05-08
Scope: `crates/claude-code-rs/src/remote/**` or equivalent Rust TUI/daemon integration, plus docs/status updates.
Goal: establish the first usable remote-channel path so cc-rust can connect to Telegram or Lark/Feishu and route inbound text into the assistant channel pipeline.

## Current Completion Status

There is no standalone Rust `remote` module yet. The historical project docs still describe `analytics, remote` as removed from the original Rust port, and active gap docs list remote control / multi-end integration as a deferred Full Build area.

Existing building blocks:

- `crates/claude-code-rs/src/daemon/**` has a usable local daemon skeleton:
  - supervisor state under `~/.cc-rust/daemon/`;
  - worker state files;
  - command/event files;
  - local HTTP/SSE control plane;
  - local control token for mutating endpoints;
  - sleep state and `/daemon` management commands.
- `docs/plan/daemon-usability-plan.md` records that Phase 5 only landed the local control-token security boundary. It explicitly leaves bridge worker network registration and remote submit/abort/permission mapping for later.
- `docs/reference/DAEMON_OPERATIONS.md` says bridge worker registration, remote submit/abort/permission mapping, and result return are still incomplete.
- `crates/claude-code-rs/src/daemon/channels.rs` has a small `ChannelManager`, `ChannelEvent`, allowlist check, and XML wrapping.
- `crates/cc-mcp/src/channel.rs` can detect `capabilities.experimental["claude/channel"]` and parse `notifications/claude/channel`, but it is not a full runtime connection path.
- `crates/claude-code-rs/src/commands/channels.rs` is currently a feature-gated stub. It reports no active connections and has no connect/disconnect/status model.
- `crates/claude-code-rs/src/engine/system_prompt.rs` already has an `external_channels` prompt section behind `FEATURE_KAIROS_CHANNELS`.
- `crates/claude-code-rs/src/services/scheduler/*` and `/schedule remote` intentionally reject remote trigger tasks.
- Task storage supports remote task metadata, but that is background task metadata, not a live Telegram/Lark channel connection.

Upstream/reference behavior:

- TypeScript/Bun uses MCP channel notifications: channel-capable MCP servers declare `experimental["claude/channel"]`, inbound messages use `notifications/claude/channel`, content is wrapped as `<channel ...>...</channel>`, and channel permission replies use a separate permission notification method.
- The upstream channel gate is strict: feature enabled, auth/policy/session allowlist, plugin marketplace allowlist, then handler registration.
- Telegram Bot API is HTTP-based and supports two mutually exclusive update receive modes: `getUpdates` polling and webhooks. Updates include a stable `update_id`; message text appears under `message.text`. `sendMessage` is the basic outbound API.
- Lark/Feishu integration needs an explicit product choice:
  - incoming messages normally require a custom app with Open Platform event subscription / callback or long-connection style runtime;
  - simple custom group bot webhooks are useful for outbound notifications but are not enough for full inbound conversational control.

References:

- Telegram Bot API: https://core.telegram.org/bots/api
- Lark Developer docs entry point: https://open.larksuite.com/
- Existing cc-rust daemon plan: `docs/plan/daemon-usability-plan.md`
- Existing daemon operations doc: `docs/reference/DAEMON_OPERATIONS.md`

## Phase 1 Definition

Phase 1 is not full upstream remote-control parity. It is a narrow remote-channel MVP:

1. Configure one provider connection.
2. Start a daemon-owned connector.
3. Prove the connector can authenticate/connect.
4. Receive a text message from Telegram or Lark/Feishu.
5. Convert that message into a safe `ChannelEvent`.
6. Persist or expose the event through daemon status/events.
7. Inject the channel message into the assistant path as `<channel source="...">...`.
8. Send a basic outbound reply or status/test message through the provider.

Telegram should be the first implementation target because long polling with `getUpdates` avoids public HTTPS callbacks and is easier to test locally on Windows. Lark/Feishu should be designed behind the same provider trait; implement it in Phase 1 only if credentials and event-delivery mode are available. Otherwise ship Lark as a configured but blocked adapter with precise diagnostics.

## Proposed Architecture

Add a new remote module:

```text
crates/claude-code-rs/src/remote/
├── mod.rs
├── config.rs
├── provider.rs
├── runtime.rs
├── event.rs
├── status.rs
├── telegram.rs
├── lark.rs
└── tests.rs
```

`config.rs`

- Reads remote-channel config from cc-rust-owned paths only.
- Suggested file: `~/.cc-rust/remote/channels.json`.
- Environment override support for development:
  - `CC_RUST_REMOTE_TELEGRAM_BOT_TOKEN`
  - `CC_RUST_REMOTE_TELEGRAM_CHAT_ALLOWLIST`
  - `CC_RUST_REMOTE_LARK_APP_ID`
  - `CC_RUST_REMOTE_LARK_APP_SECRET`
  - `CC_RUST_REMOTE_LARK_WEBHOOK_URL` for outbound-only custom bot mode
- Never read or write upstream `~/.claude` paths.
- Present-but-invalid config must fail visibly, following the fail-fast policy already used elsewhere.

`provider.rs`

Defines the common adapter boundary:

```rust
pub trait RemoteProvider {
    fn provider_id(&self) -> &'static str;
    async fn health_check(&self) -> Result<RemoteProviderStatus>;
    async fn poll_once(&mut self) -> Result<Vec<RemoteInboundEvent>>;
    async fn send_text(&self, target: &RemoteTarget, text: &str) -> Result<RemoteOutboundReceipt>;
}
```

Keep the trait small. Avoid generic remote-control/session semantics until the channel path is proven.

`event.rs`

- Defines provider-neutral inbound/outbound event types.
- Maps provider messages to `daemon::channels::ChannelEvent`.
- Sanitizes XML attributes. Current `ChannelEvent::to_xml()` only handles `source` and `sender`; this phase should add safe meta attribute rendering or explicitly preserve meta in event JSON only.
- Dedupes by provider update id/message id before enqueue.

`runtime.rs`

- Owns polling loop lifecycle.
- Supports one active connector per provider in Phase 1.
- Uses exponential backoff for transient provider errors.
- Writes status under `~/.cc-rust/daemon/remote-channels/<provider>.json` or a similarly isolated path.
- Emits daemon events:
  - `remote_channel_connected`
  - `remote_channel_disconnected`
  - `remote_channel_message`
  - `remote_channel_error`
  - `remote_channel_outbound_sent`

`telegram.rs`

Phase 1 preferred implementation:

- Authenticate with `getMe` during `health_check`.
- Receive messages with `getUpdates` long polling.
- Store latest acknowledged `update_id` so restart does not replay old messages.
- Filter chats using an allowlist; default deny unless explicitly configured.
- Map `message.text` to inbound text events.
- Use `sendMessage` for outbound reply/test message.
- Do not call `setWebhook` in Phase 1 unless explicitly configured; polling and webhooks are mutually exclusive in Telegram.

`lark.rs`

Two supported modes, chosen explicitly:

- `outbound_webhook`: custom group bot webhook. Supports status/test send only. It does not satisfy full inbound message receive.
- `app_events`: custom app credentials + event subscription / callback or long-connection mode. This is the path needed for inbound messages.

Phase 1 should implement either:

- real `app_events` if credentials and delivery mode are available; or
- an `outbound_webhook` adapter with a diagnostic saying inbound receive is unavailable in this mode.

Do not silently claim Lark conversational connectivity when only outbound webhook send is configured.

## Command Surface

Extend `/channels` from a stub into a status/control surface:

```text
/channels
/channels status
/channels connect telegram
/channels connect lark
/channels disconnect <provider>
/channels test <provider> <message>
```

CLI daemon equivalent can be added if it matches existing daemon command patterns:

```text
cc-rust remote status
cc-rust remote connect telegram
cc-rust remote test telegram "hello"
cc-rust remote disconnect telegram
```

Keep `/remote-control` out of Phase 1 unless needed for naming compatibility. Telegram/Lark channel connectivity is not yet full remote-control.

## Implementation Plan

### Phase 0 - Discovery And Current-State Guard

Work:

- Confirm no existing `remote` module is present.
- Record current `/channels` behavior and daemon channel stubs.
- Decide final storage path and config schema.
- Verify Telegram and Lark/Feishu current API details against official docs before implementation.

Acceptance:

- A short current-state note exists in the plan or implementation PR.
- No code changes yet.

Verification:

```powershell
rg -n "remote|telegram|lark|feishu|channels" crates docs
```

### Phase 1A - Remote Config And Provider Trait

Work:

- Add `remote::{config,provider,event,status}`.
- Add config parser tests.
- Add fail-fast behavior for malformed existing config.
- Add provider-neutral status and event structs.

Owned files:

- `crates/claude-code-rs/src/remote/**`
- `crates/claude-code-rs/src/main.rs` or `lib.rs` module registration if needed

Do not edit:

- `plugins/**`
- `cc-mcp/**`
- task scheduler remote triggers

Verification:

```powershell
cargo test -p claude-code-rs remote:: -- --nocapture
cargo check -p claude-code-rs --message-format short
```

### Phase 1B - Telegram Connector

Work:

- Implement `RemoteProvider` for Telegram.
- Use `reqwest` already available in workspace.
- Add `getMe`, `getUpdates`, `sendMessage` request/response types.
- Store/update polling offset.
- Add chat allowlist.
- Add tests using a mock HTTP server or local trait-level fake client; do not require real Telegram credentials in unit tests.

Acceptance:

- With token + allowlisted chat configured, connector can:
  - validate bot token;
  - poll one inbound text update;
  - map it to `ChannelEvent`;
  - send a text message.
- Invalid token, denied chat, malformed Telegram response, and transport failure are visible diagnostics.

Verification:

```powershell
cargo test -p claude-code-rs remote::telegram -- --nocapture
cargo check -p claude-code-rs --message-format short
```

Manual smoke, only with real credentials:

```powershell
$env:FEATURE_KAIROS="1"
$env:FEATURE_KAIROS_CHANNELS="1"
$env:CC_RUST_REMOTE_TELEGRAM_BOT_TOKEN="<token>"
$env:CC_RUST_REMOTE_TELEGRAM_CHAT_ALLOWLIST="<chat_id>"
claude-code-rs daemon start
claude-code-rs remote connect telegram
claude-code-rs remote status
claude-code-rs remote test telegram "cc-rust remote channel test"
```

### Phase 1C - Daemon Runtime Integration

Work:

- Add a daemon-managed remote-channel runtime or worker.
- On inbound provider event:
  - dedupe;
  - convert to `ChannelEvent`;
  - write daemon event;
  - enqueue into assistant input path or expose it as pending channel event if assistant injection is not safe yet.
- Extend `/api/status` and daemon status with remote-channel state.
- Decide whether connector starts automatically with daemon or only after explicit `connect`.

Acceptance:

- `daemon status` shows provider connected/disconnected/error state.
- Incoming Telegram text appears in daemon events as `remote_channel_message`.
- Channel content is wrapped as `<channel source="telegram" ...>`.

Verification:

```powershell
cargo test -p claude-code-rs daemon::channels -- --nocapture
cargo test -p claude-code-rs daemon::process_state -- --nocapture
cargo test -p claude-code-rs daemon::routes -- --nocapture
cargo test -p claude-code-rs remote:: -- --nocapture
cargo check -p claude-code-rs --message-format short
```

### Phase 1D - `/channels` And CLI Surface

Work:

- Replace the current `/channels` stub with real status output.
- Add connect/disconnect/test commands.
- If adding CLI `remote`, wire it through existing command parsing patterns, not through a separate binary.
- Update system prompt only if inbound injection is working; otherwise keep it gated.

Acceptance:

- `/channels status` reflects configured providers.
- `/channels test telegram ...` sends a real outbound test when configured.
- Clear diagnostics for disabled feature flags, missing config, denied chat, or Lark outbound-only mode.

Verification:

```powershell
cargo test -p claude-code-rs commands::channels -- --nocapture
cargo test -p claude-code-rs remote:: -- --nocapture
cargo check -p claude-code-rs --message-format short
```

### Phase 1E - Lark/Feishu Adapter Decision And Minimal Support

Work:

- Add Lark config schema and status diagnostics.
- If using custom app events:
  - implement token fetch / app auth;
  - implement message send;
  - implement event receive mode selected by the project.
- If only custom group bot webhook is available:
  - implement outbound test send;
  - explicitly report `inbound=unsupported_in_webhook_mode`.

Acceptance:

- A configured Lark provider can at least validate/send test messages.
- If inbound event mode is not implemented, status says why and what configuration is required.
- No misleading "connected conversational channel" status for outbound-only webhook mode.

Verification:

```powershell
cargo test -p claude-code-rs remote::lark -- --nocapture
cargo test -p claude-code-rs remote:: -- --nocapture
cargo check -p claude-code-rs --message-format short
```

### Phase 1F - Docs And Gap Updates

Work:

- Update `docs/WORK_STATUS.md` remote/channel row or add a precise active status.
- Update `docs/IMPLEMENTATION_GAPS.md`:
  - remove "remote module absent" wording if introduced elsewhere;
  - keep remote-control, remote task poller, and full bridge parity as open gaps.
- Update `docs/KNOWN_ISSUES.md` only if a user-visible limitation remains.
- Add operator docs for Telegram/Lark setup.

Acceptance:

- Docs distinguish:
  - Telegram/Lark channel connectivity;
  - full remote-control;
  - scheduler remote triggers;
  - remote/background task poller parity.

Verification:

```powershell
git diff --check
cargo check -p claude-code-rs --message-format short
```

## Security And Policy Requirements

- Default off: require `FEATURE_KAIROS=1` and `FEATURE_KAIROS_CHANNELS=1`.
- Default deny: require explicit provider config and chat/conversation allowlist.
- Store secrets only under `~/.cc-rust/` or environment variables. Do not write Telegram/Lark secrets to project files.
- Never log full bot tokens or app secrets.
- Deduplicate inbound messages by provider update id/message id.
- Do not interpret free-form chat text as permission approvals in Phase 1.
- Permission reply over remote channels requires a separate typed event protocol in a later phase.
- Remote channel messages must be clearly marked as external with source metadata.
- Outbound tool usage must stay explicit; the model should not assume it can message arbitrary chats.

## Non-Goals For Phase 1

- Full `/remote-control` parity.
- Mobile/web remote session attach.
- Remote submit/abort/permission control.
- Scheduler remote triggers.
- Remote task poller/reconnect parity.
- Plugin marketplace channel allowlist parity.
- Rich cards, images, voice, file attachments.
- Multi-provider simultaneous production hardening beyond one Telegram + one Lark config.

## Acceptance Criteria

- A new remote/channel module exists with provider-neutral config, event, status, and provider abstractions.
- Telegram can connect via bot token, poll inbound text, dedupe updates, and send a test message.
- Lark is either implemented for real inbound events or explicitly limited to outbound webhook/test-send with clear diagnostics.
- `/channels status` reports actual provider state rather than "No channels connected."
- Daemon status/events expose remote-channel state.
- Inbound channel text is safely represented as `<channel source="...">`.
- Tests cover config parsing, bad credentials diagnostics, denied chat/conversation, provider response parsing, event mapping, and command status output.
- Docs update active gaps without claiming full remote-control parity.

## Recommended Commit Split

1. Add remote config/provider/event/status core with tests.
2. Add Telegram provider with mocked HTTP tests.
3. Wire daemon remote-channel runtime/status/events.
4. Replace `/channels` stub with real status/connect/test surface.
5. Add Lark adapter or explicit outbound-only diagnostic adapter.
6. Update docs and archive any replaced "remote absent" notes.

## Open Decisions Before Implementation

- Should Telegram be the required first adapter, with Lark following after the core path is proven?
- For Lark/Feishu, do we have custom app credentials and an inbound event delivery mode, or only a custom bot webhook?
- Should connectors run only under daemon, or should a foreground TUI session be able to run a connector without daemon?
- Should inbound channel events immediately submit to `QueryEngine`, or first appear as pending external messages requiring local confirmation?
- What is the intended operator setup path: slash command, CLI command, config file, or environment-only MVP?

