# Persistent Bridge Session Reuse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` to execute this plan.

**Goal:** make KAIROS bridge sessions durable and resumable across terminal restarts, daemon restarts, and separate terminal windows in the same workspace, without mixing state with upstream Codex paths.

**Architecture:** keep bridge state under `~/.allthecodes/daemon/bridge/`, introduce a stable bridge-session identity and lease, route bridge worker startup through a selector instead of the current hard-coded `default` session, and persist the assistant runtime session id needed to continue the same conversation.

**Technology Stack:** Rust, serde JSON state files, daemon worker queue, existing `GatewayDaemonBridge`, existing `DaemonBridgeSessionState`, existing `QueryEngineConfig { persist_session: true, auto_save_session: true }`.

---

## Current State

Implemented now:

- `DaemonBridgeSessionState` is persisted by `crates/allthecodes-daemon/src/process_state/storage.rs`.
- State path is `~/.allthecodes/daemon/bridge/sessions/<session_id>.json`.
- Bridge inbox path is `~/.allthecodes/daemon/bridge/sessions/<session_id>/inbox.ndjson`.
- `BridgeWorkerRuntime::poll_once()` persists `last_poll_cursor` and `last_ack_at`.
- Daemon commands already support durable idempotency via `DaemonCommand::idempotency_key`.
- Background bridge worker is supervised when `Feature::Kairos` is enabled.

Missing for cross-terminal restart reuse:

- Bridge worker always uses `DEFAULT_BRIDGE_SESSION_ID = "default"`.
- There is no stable workspace/account/profile/terminal identity for selecting a reusable bridge session.
- There is no CLI/API to list, resume, or explicitly select a bridge session.
- The state does not store `remote_session_key`, `last_run_id`, or the assistant runtime `session_id`.
- There is no lease/owner metadata, so two terminals cannot safely reason about whether a session is active or stale.
- The assistant worker is configured to persist sessions, but bridge state does not bind to the saved assistant session that should be resumed.
- There is no compatibility/migration layer for future bridge session schema changes.

---

## Desired Behavior

1. Starting KAIROS in the same workspace reuses the previous bridge session by default.
2. Restarting a terminal or daemon continues from the last processed bridge cursor and does not re-enqueue completed work.
3. Opening a second terminal can either attach to the same bridge session or create a new one explicitly.
4. The selected bridge session records enough identity to continue the same assistant conversation.
5. Stale leases can be taken over after a short TTL, while active leases prevent accidental double ownership.
6. All persistent files stay under `~/.allthecodes/`, never `~/.Codex/`.

---

## File Changes

### 1. Extend Bridge Session State

Modify `crates/allthecodes-daemon/src/process_state/types.rs`.

Add fields to `DaemonBridgeSessionState`:

```rust
pub struct DaemonBridgeSessionState {
    pub schema_version: u32,
    pub session_id: String,
    pub account_id: Option<String>,
    pub profile: Option<String>,
    pub cwd: PathBuf,
    pub assistant_worker_id: String,
    pub last_poll_cursor: Option<String>,
    pub last_ack_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,

    pub workspace_key: String,
    pub terminal_id: Option<String>,
    pub remote_session_key: Option<String>,
    pub assistant_session_id: Option<String>,
    pub last_run_id: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
}
```

Rules:

- `schema_version` becomes `2`.
- `workspace_key` is derived from canonical cwd plus account/profile.
- `terminal_id` is optional and comes from env or CLI only when the user wants per-terminal sessions.
- `remote_session_key` stores the external bridge/channel session key when available.
- `assistant_session_id` stores the local allthecodes assistant session id that should be resumed.
- `lease_owner` is the current daemon process identity.
- `lease_expires_at` uses a short TTL, refreshed by the worker loop.

### 2. Add Storage APIs and Migration

Modify `crates/allthecodes-daemon/src/process_state/storage.rs`.

Add:

```rust
pub fn list_bridge_session_states() -> Result<Vec<DaemonBridgeSessionState>>;

pub fn find_bridge_session_by_workspace_key(
    workspace_key: &str,
    account_id: Option<&str>,
    profile: Option<&str>,
) -> Result<Option<DaemonBridgeSessionState>>;

pub fn migrate_bridge_session_state(
    raw: serde_json::Value,
) -> Result<DaemonBridgeSessionState>;
```

Migration behavior:

- Version 1 state without `workspace_key` derives it from `cwd`, `account_id`, and `profile`.
- Missing lease fields default to `None`.
- Missing assistant/remote fields default to `None`.
- Unknown future fields are ignored by serde.

Also normalize the split file layout:

- Keep state at `bridge/sessions/<session_id>.json` for compatibility.
- Keep inbox at `bridge/sessions/<session_id>/inbox.ndjson`.
- Add tests proving both paths resolve under `~/.allthecodes/daemon/bridge/`.

### 3. Introduce Session Selection Module

Create `crates/allthecodes-daemon/src/bridge_session.rs`.

Public types:

```rust
pub struct BridgeSessionIdentity {
    pub cwd: PathBuf,
    pub account_id: Option<String>,
    pub profile: Option<String>,
    pub terminal_id: Option<String>,
    pub remote_session_key: Option<String>,
}

pub enum BridgeSessionReusePolicy {
    ReuseWorkspace,
    NewSession,
    ExplicitSession(String),
}

pub struct BridgeSessionLease {
    pub owner: String,
    pub ttl: Duration,
    pub allow_stale_takeover: bool,
}
```

Public functions:

```rust
pub fn derive_workspace_key(identity: &BridgeSessionIdentity) -> Result<String>;

pub fn select_or_create_bridge_session(
    identity: BridgeSessionIdentity,
    policy: BridgeSessionReusePolicy,
    lease: BridgeSessionLease,
) -> Result<DaemonBridgeSessionState>;

pub fn refresh_bridge_session_lease(
    session_id: &str,
    owner: &str,
    ttl: Duration,
) -> Result<DaemonBridgeSessionState>;

pub fn release_bridge_session_lease(
    session_id: &str,
    owner: &str,
) -> Result<()>;
```

Selection rules:

- `ExplicitSession(id)` loads that exact session and refreshes its lease.
- `NewSession` creates a new UUID session and leases it.
- `ReuseWorkspace` finds the newest session with matching `workspace_key`, `account_id`, and `profile`.
- If a matching session has an unexpired lease owned by another process, return a typed conflict error.
- If the lease is stale and `allow_stale_takeover` is true, take it over.
- If no matching session exists, create one.

### 4. Wire Bridge Worker to Selected Session

Modify `crates/allthecodes-daemon/src/bridge_worker.rs`.

Replace:

```rust
BridgeWorkerRuntime::new(cwd)
```

with:

```rust
BridgeWorkerRuntime::new(
    cwd,
    BridgeSessionIdentity { ... },
    BridgeSessionReusePolicy::ReuseWorkspace,
    BridgeSessionLease { ... },
)
```

Add an internal constructor for tests:

```rust
pub fn from_session_state(state: DaemonBridgeSessionState) -> Self;
```

Worker loop behavior:

- Refresh lease before or after each successful poll.
- Persist `remote_session_key` from `GatewayWorkItem.session_key`.
- Persist `last_run_id` from `GatewayWorkItem.run_id`.
- Keep current `last_poll_cursor` behavior.
- Preserve existing idempotency key behavior so old inbox items are skipped and duplicate daemon commands are not created.

### 5. Resume Assistant Runtime Session

Modify the assistant worker path that receives bridge-submitted commands. Candidate files to inspect during implementation:

- `crates/allthecodes-daemon/src/worker.rs`
- `crates/allthecodes-daemon/src/gateway_bridge.rs`
- `crates/allthecodes-daemon/src/services.rs`
- `crates/allthecodes-daemon/src/process_state/types.rs`

Required behavior:

- When a bridge-selected assistant starts a new runtime session, write its allthecodes session id into `DaemonBridgeSessionState.assistant_session_id`.
- When a bridge session with `assistant_session_id` exists, pass that session id into the assistant/query engine resume path.
- If the referenced assistant session file is missing or invalid, create a new assistant session and update bridge state.

Implementation should prefer existing session APIs. If none exist, add a narrow resume field to the daemon worker command payload:

```rust
pub struct GatewayContext {
    pub run_id: String,
    pub session_key: Option<String>,
    pub bridge_session_id: String,
    pub assistant_session_id: Option<String>,
}
```

### 6. Add CLI/API Surface

Modify `crates/allthecodes-commands/src/daemon_cmd.rs` and daemon routes.

Add commands:

```text
allthecodes daemon bridge sessions
allthecodes daemon bridge status
allthecodes daemon bridge resume <session-id>
allthecodes daemon bridge new
allthecodes daemon bridge release <session-id>
```

Add environment overrides:

```text
ALLTHECODES_BRIDGE_SESSION_ID=<session-id>
ALLTHECODES_BRIDGE_SESSION_POLICY=reuse-workspace|new|explicit
ALLTHECODES_BRIDGE_TERMINAL_ID=<terminal-id>
```

API endpoints:

```text
GET  /daemon/bridge/sessions
GET  /daemon/bridge/sessions/:id
POST /daemon/bridge/sessions/:id/resume
POST /daemon/bridge/sessions/:id/release
```

Output fields:

- `session_id`
- `workspace_key`
- `cwd`
- `account_id`
- `profile`
- `assistant_session_id`
- `remote_session_key`
- `last_run_id`
- `last_ack_at`
- `lease_owner`
- `lease_expires_at`

### 7. Documentation

Update:

- `development/reference/DAEMON_OPERATIONS.md`
- `development/hided_features/current-hidden-features.md`
- `docs/WORK_STATUS.md`

Document:

- Default reuse behavior.
- Explicit new session behavior.
- How stale lease takeover works.
- Where state files live.
- The guarantee that all state is under `~/.allthecodes/`.

---

## Test Plan

### Storage Tests

File: `crates/allthecodes-daemon/src/process_state/storage.rs`

Add tests:

```rust
#[test]
fn bridge_session_v1_migrates_to_v2_with_workspace_key() { ... }

#[test]
fn list_bridge_session_states_returns_all_valid_sessions() { ... }

#[test]
fn find_bridge_session_by_workspace_key_returns_newest_match() { ... }
```

Expected before implementation: fail to compile because APIs do not exist.

### Selector Tests

File: `crates/allthecodes-daemon/src/bridge_session.rs`

Add tests:

```rust
#[test]
fn reuse_workspace_selects_existing_session() { ... }

#[test]
fn new_session_always_creates_distinct_session() { ... }

#[test]
fn explicit_session_loads_exact_session() { ... }

#[test]
fn active_foreign_lease_blocks_reuse() { ... }

#[test]
fn stale_foreign_lease_can_be_taken_over() { ... }
```

Expected before implementation: fail to compile because module does not exist.

### Worker Restart Tests

File: `crates/allthecodes-daemon/src/bridge_worker.rs`

Add or extend tests:

```rust
#[tokio::test]
async fn bridge_worker_reuses_selected_session_after_restart() { ... }

#[tokio::test]
async fn bridge_worker_persists_remote_session_key_and_run_id() { ... }

#[tokio::test]
async fn bridge_worker_refreshes_lease_on_poll() { ... }
```

Expected behavior:

- First runtime processes inbox item and persists cursor.
- Second runtime uses the same session id.
- Second runtime does not enqueue the same item again.
- State contains `last_run_id`, `remote_session_key`, and refreshed lease.

### Assistant Resume Tests

Add tests around the worker or gateway path where assistant sessions are created.

Required assertions:

- First bridge command records `assistant_session_id`.
- Restarted bridge session includes that id in the next command context.
- Missing assistant session file causes a clean new session id to be stored.

### CLI/API Tests

Add command/parser tests in `crates/allthecodes-commands`.

Required assertions:

- `daemon bridge sessions` parses and calls list endpoint.
- `daemon bridge resume <session-id>` parses explicit session selection.
- Env override `ALLTHECODES_BRIDGE_SESSION_ID` maps to explicit policy.
- Env override `ALLTHECODES_BRIDGE_SESSION_POLICY=new` maps to new-session policy.

---

## Implementation Sequence

1. Add failing storage and selector tests.
2. Extend `DaemonBridgeSessionState` and implement migration/list/lookup.
3. Add `bridge_session.rs` with selection, lease, and workspace-key derivation.
4. Update `BridgeWorkerRuntime` to accept selected session state.
5. Persist `remote_session_key`, `last_run_id`, and lease refresh from worker polling.
6. Bind bridge state to assistant runtime session id.
7. Add CLI/API commands for list/status/resume/new/release.
8. Update docs and work-status notes.
9. Run targeted tests.
10. Run workspace verification.

Verification commands:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-daemon bridge_session
cargo test -p allthecodes-daemon bridge_worker
cargo test -p allthecodes-commands daemon_bridge
cargo check --workspace
```

Final verification before commit:

```bash
cargo build --workspace --release
```

---

## Acceptance Criteria

- A bridge session created in one terminal is selected automatically after restarting in the same workspace.
- A restarted bridge worker resumes from the stored cursor and does not duplicate submitted daemon commands.
- `allthecodes daemon bridge sessions` shows the reusable session and its lease state.
- `allthecodes daemon bridge resume <session-id>` attaches to the selected session.
- The bridge state records `assistant_session_id` after first assistant use.
- Restarted bridge work carries the previous `assistant_session_id` into the assistant resume path.
- Stale leases are takeover-safe and active leases are conflict-safe.
- Tests cover migration, selection, lease, worker restart, and CLI/API parsing.
- No persistent bridge/session state is written outside `~/.allthecodes/`.
