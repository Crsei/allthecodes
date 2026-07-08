# KAIROS Next Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring allthecodes KAIROS from the current daemon/worker MVP to the upstream resident-assistant behavior described in `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/docs/features/kairos.md`.

**Architecture:** Keep the existing Rust daemon supervisor and `assistant-session-1` worker as the execution owner. Add the missing KAIROS prompt semantics, durable interactive DTOs, bridge worker, notification/channel ingress, and dream-memory loop around that worker model rather than reintroducing a TypeScript-style single-process REPL bridge.

**Tech Stack:** Rust 1.91.1, tokio, axum, SSE, existing `allthecodes-daemon`, `allthecodes-engine`, `allthecodes-tools`, `allthecodes-gateway`, and `allthecodes-services` crates.

## Global Constraints

- KAIROS remains gated by `FEATURE_KAIROS=1`; child gates remain `FEATURE_KAIROS_BRIEF`, `FEATURE_KAIROS_CHANNELS`, `FEATURE_KAIROS_PUSH_NOTIFICATION`, and `FEATURE_KAIROS_GITHUB_WEBHOOKS`.
- KAIROS implies proactive behavior; do not require users to set `FEATURE_PROACTIVE=1` when `FEATURE_KAIROS=1` is already enabled.
- Do not write to upstream Claude/Codex locations. Persistent state must stay under `ALLTHECODES_HOME` or `~/.allthecodes/`.
- Do not regress current daemon capabilities: `daemon start/status/stop/restart`, control token auth, assistant worker submit/abort ownership, SSE replay, and durable command/event files.
- Keep the npm release backend-only policy unchanged; do not add a web SPA build to daemon/KAIROS work.
- Use the repository cargo environment:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## Upstream-To-Current Mapping

- Already present: feature gate tree, daemon process, worker command/event protocol, `BriefTool`, `SleepTool`, `/assistant`, `/daemon`, `/brief`, `/sleep`, webhook route skeletons, daily log append path, supervisor state, control token.
- Partially present: proactive tick loop and scheduler loop run in the daemon process, not as supervisor-managed workers; sleep state exists but has duplicate writer contracts between tool and daemon command paths.
- Missing parity: KAIROS system-prompt sections, bridge session discovery/history, bridge poll/ack worker, live permission/ask-user replay, real history/resize DTOs, notification consumer startup, channel ingress, Dream memory distillation, long-running soak tests.

---

## File Structure

- Modify `crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs`: add KAIROS Brief and Proactive prompt sections.
- Modify `crates/allthecodes-engine/src/system_prompt/mod.rs`: register KAIROS sections in the dynamic system prompt sequence.
- Modify `crates/allthecodes-engine/src/system_prompt/tests.rs`: cover KAIROS/Brief/Proactive prompt injection and feature gating.
- Modify `crates/allthecodes-daemon/src/protocol.rs`: add durable interaction DTOs for permission, ask-user, resize, history, and automation state.
- Modify `crates/allthecodes-daemon/src/gateway_bridge.rs`: make assistant worker consume permission/ask-user responses and publish worker-owned history.
- Modify `crates/allthecodes-daemon/src/routes.rs`: replace permission/history/resize stubs with worker-backed behavior.
- Modify `crates/allthecodes-daemon/src/supervisor.rs`: add `bridge-sync`, `proactive`, and `scheduler` worker specs after the assistant DTO work is stable.
- Create `crates/allthecodes-daemon/src/bridge_worker.rs`: durable bridge session poll/ack loop.
- Create `crates/allthecodes-daemon/src/automation_state.rs`: expose `standby`, `sleeping`, `running`, `blocked`, and `needs_input` states for daemon/gateway surfaces.
- Modify `crates/allthecodes-daemon/src/notification.rs` and `crates/allthecodes/src/full_init.rs`: start the notification consumer when enabled.
- Modify `crates/allthecodes-daemon/src/channels.rs` and `crates/allthecodes-daemon/src/webhook.rs`: route external channel messages into assistant worker commands.
- Create `crates/allthecodes-daemon/src/dream.rs`: daily log distillation and memory write loop.
- Modify `development/reference/DAEMON_OPERATIONS.md`, `docs/WORK_STATUS.md`, and `development/hided_features/current-hidden-features.md`: keep current status accurate as tasks land.

---

### Task 1: KAIROS Prompt Parity

**Files:**
- Modify: `crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs`
- Modify: `crates/allthecodes-engine/src/system_prompt/mod.rs`
- Test: `crates/allthecodes-engine/src/system_prompt/tests.rs`

**Interfaces:**
- Produces: `kairos_brief_section() -> Option<String>`
- Produces: `kairos_proactive_section() -> Option<String>`
- Consumes: `allthecodes_config::features::{enabled, Feature}`

- [ ] **Step 1: Add failing prompt tests**

Add tests that set runtime feature overrides and assert:
- `FEATURE_KAIROS=1` injects proactive instructions.
- `FEATURE_KAIROS_BRIEF=1` plus KAIROS injects Brief output instructions.
- KAIROS off does not inject either section.
- The proactive section mentions `<tick_tag>`, `Sleep`, terminal focus, and avoiding "still waiting" style no-op updates.

Run:

```bash
cargo test -p allthecodes-engine system_prompt::tests::kairos -- --nocapture
```

Expected before implementation: tests fail because no KAIROS prompt section exists.

- [ ] **Step 2: Implement dynamic sections**

Add two dynamic section builders. Keep wording concise and Rust-native, but preserve upstream semantics:
- Brief messages are structured user-facing output.
- Proactive work is tick-driven.
- The model should use `Sleep` for waiting.
- The model should act autonomously when terminal focus is false.

- [ ] **Step 3: Register sections**

Register these sections after coordinator mode and before memory context injection in `system_prompt/mod.rs`, using uncached sections where feature or runtime state can change during a session.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-engine system_prompt
cargo check -p allthecodes-engine
```

Expected: tests pass and no warnings are introduced.

- [ ] **Step 5: Commit**

```bash
git add crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs \
        crates/allthecodes-engine/src/system_prompt/mod.rs \
        crates/allthecodes-engine/src/system_prompt/tests.rs
git commit -m "feat(kairos): add resident assistant prompt sections"
```

---

### Task 2: Worker-Owned Interaction DTOs

**Files:**
- Modify: `crates/allthecodes-daemon/src/protocol.rs`
- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`
- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Test: `crates/allthecodes-daemon/src/protocol.rs`
- Test: `crates/allthecodes-daemon/src/routes.rs`

**Interfaces:**
- Produces: `DaemonEventKind::PermissionRequest`
- Produces: `DaemonEventKind::AskUserQuestion`
- Produces: `DaemonEventKind::HistorySnapshot`
- Produces: `DaemonCommandKind::Resize`
- Consumes: existing durable command files under `daemon/commands/<worker-id>/`.

- [ ] **Step 1: Add failing protocol tests**

Add JSON contract tests for permission request, permission response, ask-user question, ask-user response, resize, and history snapshot. Include corrupted/unknown event handling so stale or partial files do not panic the daemon.

Run:

```bash
cargo test -p allthecodes-daemon protocol
```

Expected before implementation: tests fail for missing variants.

- [ ] **Step 2: Extend command/event schema**

Add typed command/event variants without breaking existing JSON names. Keep existing `submit`, `abort`, `permission_response`, `ask_user_response`, `shutdown`, and `reload_config` stable.

- [ ] **Step 3: Connect permission and ask-user replay**

Update `AssistantWorkerRuntime` so permission and ask-user response commands are routed into the live worker runtime instead of only being marked handled. If there is no live waiter, store the response as replayable and consume it when the matching waiter appears.

- [ ] **Step 4: Replace route stubs**

Update:
- `POST /api/permission`: queues and replays against the live worker waiter.
- `POST /api/resize`: writes a real resize command or returns an explicit unsupported response only when no worker is running.
- `GET /api/history`: returns worker-owned history snapshots plus durable worker events.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-daemon protocol
cargo test -p allthecodes-daemon routes
cargo test -p allthecodes-daemon gateway_bridge
```

Expected: permission, resize, and history tests pass without hitting live model APIs.

- [ ] **Step 6: Commit**

```bash
git add crates/allthecodes-daemon/src/protocol.rs \
        crates/allthecodes-daemon/src/gateway_bridge.rs \
        crates/allthecodes-daemon/src/routes.rs
git commit -m "feat(kairos): route daemon interactions through worker DTOs"
```

---

### Task 3: Automation State And Sleep Contract Unification

**Files:**
- Create: `crates/allthecodes-daemon/src/automation_state.rs`
- Modify: `crates/allthecodes-daemon/src/lib.rs`
- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Modify: `crates/allthecodes-daemon/src/tick.rs`
- Modify: `crates/allthecodes-daemon/src/scheduler_loop.rs`
- Modify: `crates/allthecodes-tools/src/exec/sleep_tool.rs`
- Test: `crates/allthecodes-daemon/src/automation_state.rs`
- Test: `crates/allthecodes-tools/src/exec/sleep_tool.rs`

**Interfaces:**
- Produces: `AutomationState { status, sleeping_until, reason, terminal_focus, query_running, pending_input }`
- Produces: `automation_state::snapshot(state: &DaemonState) -> AutomationState`
- Consumes: daemon sleep state at `allthecodes_config::paths::daemon_dir().join("sleep-state.json")`.

- [ ] **Step 1: Add failing automation-state tests**

Test the following states:
- no query and no sleep => `standby`
- sleep state active => `sleeping`
- active submit command => `running`
- pending permission/ask-user event => `needs_input`

- [ ] **Step 2: Normalize SleepTool writer**

Make `SleepTool` write the same JSON contract read by `allthecodes-daemon::process_state::active_sleep_state()`. Avoid adding an `allthecodes-daemon` dependency to `allthecodes-tools`; move the shared sleep DTO to a neutral crate if needed.

- [ ] **Step 3: Expose automation state**

Add `automation_state` to `GET /api/status` and to gateway-facing status surfaces. This must include terminal focus because upstream KAIROS uses it to tune autonomy.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-daemon automation_state
cargo test -p allthecodes-tools sleep_tool
cargo test -p allthecodes-daemon routes
```

Expected: sleep state from `/sleep`, `SleepTool`, and `daemon sleep` all produce the same `sleeping` automation state.

- [ ] **Step 5: Commit**

```bash
git add crates/allthecodes-daemon/src/automation_state.rs \
        crates/allthecodes-daemon/src/lib.rs \
        crates/allthecodes-daemon/src/routes.rs \
        crates/allthecodes-daemon/src/tick.rs \
        crates/allthecodes-daemon/src/scheduler_loop.rs \
        crates/allthecodes-tools/src/exec/sleep_tool.rs
git commit -m "feat(kairos): expose unified automation state"
```

---

### Task 4: Bridge Worker MVP

**Files:**
- Create: `crates/allthecodes-daemon/src/bridge_worker.rs`
- Modify: `crates/allthecodes-daemon/src/supervisor.rs`
- Modify: `crates/allthecodes-daemon/src/runtime.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/types.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/storage.rs`
- Test: `crates/allthecodes-daemon/src/bridge_worker.rs`
- Test: `crates/allthecodes-daemon/src/supervisor.rs`

**Interfaces:**
- Produces: `WorkerKind::BridgeSync`
- Produces: `BridgeWorkerRuntime::poll_once() -> Result<PollOutcome>`
- Consumes: existing gateway command sink and assistant worker durable command queue.

- [ ] **Step 1: Add failing worker-kind tests**

Assert `WorkerKind::parse("bridge-sync")` succeeds and default worker specs include bridge-sync only when the bridge feature/config is enabled.

- [ ] **Step 2: Add bridge session store**

Persist bridge session metadata under `~/.allthecodes/daemon/bridge/`. Store session id, account id/profile, cwd, assistant worker id, last poll cursor, and last successful ack time.

- [ ] **Step 3: Implement poll/ack adapter**

Use existing allthecodes gateway abstractions where possible. Map upstream work units to durable assistant commands:
- remote message => `Submit`
- remote abort => `Abort`
- remote permission response => `PermissionResponse`
- remote ask-user response => `AskUserResponse`

- [ ] **Step 4: Add supervisor ownership**

Run bridge-sync as a supervised worker. It must heartbeat independently and restart without losing the last acknowledged work cursor.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-daemon bridge_worker
cargo test -p allthecodes-daemon supervisor
```

Expected: bridge work is acknowledged once, replays are idempotent, and assistant commands are queued with a gateway context.

- [ ] **Step 6: Commit**

```bash
git add crates/allthecodes-daemon/src/bridge_worker.rs \
        crates/allthecodes-daemon/src/supervisor.rs \
        crates/allthecodes-daemon/src/runtime.rs \
        crates/allthecodes-daemon/src/process_state/types.rs \
        crates/allthecodes-daemon/src/process_state/storage.rs
git commit -m "feat(kairos): add bridge sync worker"
```

---

### Task 5: Notification And Channel Ingress

**Files:**
- Modify: `crates/allthecodes-daemon/src/notification.rs`
- Modify: `crates/allthecodes-daemon/src/channels.rs`
- Modify: `crates/allthecodes-daemon/src/webhook.rs`
- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Modify: `crates/allthecodes/src/full_init.rs`
- Test: `crates/allthecodes-daemon/src/notification.rs`
- Test: `crates/allthecodes-daemon/src/channels.rs`
- Test: `crates/allthecodes-daemon/src/webhook.rs`

**Interfaces:**
- Produces: `ChannelEvent -> DaemonCommandKind::Submit`
- Produces: notification consumer startup path guarded by `FEATURE_KAIROS_PUSH_NOTIFICATION`
- Consumes: daemon control token and existing webhook route validation.

- [ ] **Step 1: Add failing tests**

Cover:
- notification consumer receives and dispatches a task-complete payload without a connected client.
- channel allowlist accepts configured source and rejects unknown source.
- declarative webhook route queues a submit command with channel/gateway metadata.

- [ ] **Step 2: Start notification consumer**

In daemon startup, take `notification_rx` from `DaemonState` and spawn `notification_consumer` when push notification is enabled. Keep non-Windows toast behavior as a no-op.

- [ ] **Step 3: Route channel events**

Convert accepted channel messages into assistant worker submit commands with source metadata. This implements the upstream "external channel messages to CLI" behavior without bypassing worker ownership.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-daemon notification
cargo test -p allthecodes-daemon channels
cargo test -p allthecodes-daemon webhook
```

Expected: channel/webhook ingress queues durable commands; notification consumer is covered without network calls.

- [ ] **Step 5: Commit**

```bash
git add crates/allthecodes-daemon/src/notification.rs \
        crates/allthecodes-daemon/src/channels.rs \
        crates/allthecodes-daemon/src/webhook.rs \
        crates/allthecodes-daemon/src/routes.rs \
        crates/allthecodes/src/full_init.rs
git commit -m "feat(kairos): connect notifications and channel ingress"
```

---

### Task 6: Dream Memory Distillation

**Files:**
- Create: `crates/allthecodes-daemon/src/dream.rs`
- Modify: `crates/allthecodes-daemon/src/lib.rs`
- Modify: `crates/allthecodes-commands/src/dream.rs`
- Modify: `crates/allthecodes-daemon/src/scheduler_loop.rs`
- Test: `crates/allthecodes-daemon/src/dream.rs`
- Test: `crates/allthecodes-commands/src/dream.rs`

**Interfaces:**
- Produces: `dream::distill_daily_log(date) -> Result<DreamSummary>`
- Produces: `dream::write_memory(summary) -> Result<PathBuf>`
- Consumes: current daily log path from `allthecodes_config::paths::daily_log_path`.

- [ ] **Step 1: Add failing dream tests**

Use a temp `ALLTHECODES_HOME` and a synthetic daily log. Assert distillation writes a summary under allthecodes memory/log paths and does not use upstream Claude paths.

- [ ] **Step 2: Implement deterministic local distillation first**

Start with rule-based distillation that extracts tasks, decisions, blockers, and follow-ups from the daily log. Do not call a model in tests.

- [ ] **Step 3: Wire `/dream`**

Make `/dream` run the distillation path when `FEATURE_KAIROS=1`. Keep the command output explicit about the memory file written.

- [ ] **Step 4: Wire scheduler trigger**

Add a daemon scheduler hook that can run Dream once per day when KAIROS is enabled. It must be idempotent for the same date.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-daemon dream
cargo test -p allthecodes-commands dream
```

Expected: deterministic summaries are written under `ALLTHECODES_HOME`, and reruns for the same date do not duplicate entries.

- [ ] **Step 6: Commit**

```bash
git add crates/allthecodes-daemon/src/dream.rs \
        crates/allthecodes-daemon/src/lib.rs \
        crates/allthecodes-commands/src/dream.rs \
        crates/allthecodes-daemon/src/scheduler_loop.rs
git commit -m "feat(kairos): add dream memory distillation"
```

---

### Task 7: Workerize Proactive And Scheduler Loops

**Files:**
- Modify: `crates/allthecodes-daemon/src/supervisor.rs`
- Modify: `crates/allthecodes-daemon/src/tick.rs`
- Modify: `crates/allthecodes-daemon/src/scheduler_loop.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/types.rs`
- Test: `crates/allthecodes-daemon/src/supervisor.rs`

**Interfaces:**
- Produces: `WorkerKind::Proactive`
- Produces: `WorkerKind::Scheduler`
- Consumes: automation state and assistant worker command queue from earlier tasks.

- [ ] **Step 1: Add failing worker tests**

Assert proactive and scheduler workers are managed by supervisor, heartbeat independently, and stop cleanly.

- [ ] **Step 2: Move loops out of supervisor process**

Proactive and scheduler workers should enqueue assistant commands rather than directly calling the daemon process engine. This prevents the daemon process from owning a second model-running `QueryEngine`.

- [ ] **Step 3: Preserve sleep behavior**

Both workers must read unified sleep state and skip work while sleeping.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-daemon supervisor
cargo test -p allthecodes-daemon tick
cargo test -p allthecodes-daemon scheduler_loop
```

Expected: no daemon-process model execution remains in proactive/scheduler paths.

- [ ] **Step 5: Commit**

```bash
git add crates/allthecodes-daemon/src/supervisor.rs \
        crates/allthecodes-daemon/src/tick.rs \
        crates/allthecodes-daemon/src/scheduler_loop.rs \
        crates/allthecodes-daemon/src/process_state/types.rs
git commit -m "feat(kairos): supervise proactive scheduler workers"
```

---

### Task 8: KAIROS E2E And Documentation Closeout

**Files:**
- Create or modify: `crates/allthecodes/tests/e2e_cli.rs`
- Modify: `development/reference/DAEMON_OPERATIONS.md`
- Modify: `development/hided_features/current-hidden-features.md`
- Modify: `docs/WORK_STATUS.md`
- Modify: `development/archive/IMPLEMENTATION_GAPS.md`

**Interfaces:**
- Consumes: all tasks above.
- Produces: release-check evidence for KAIROS daemon behavior.

- [ ] **Step 1: Add e2e smoke tests**

Cover:
- no daemon => `daemon status` reports stopped.
- `FEATURE_KAIROS=1 daemon start --port <temp>` starts supervisor and workers.
- `daemon submit "hello"` queues and worker handles command in mock/no-model mode.
- `/api/status` returns automation state, workers, command root, and history fields.
- `daemon sleep` changes automation state to `sleeping`.
- `daemon stop` stops supervisor and workers.

- [ ] **Step 2: Add a live optional smoke script**

Create an ignored/on-demand live smoke that requires credentials and network:
- start daemon
- submit over HTTP with token
- read `/events`
- assert stream reaches result or explicit model error
- stop daemon

- [ ] **Step 3: Update docs**

Document:
- KAIROS gate tree and how it maps to current env vars.
- Current allthecodes paths: `ALLTHECODES_HOME` / `~/.allthecodes`.
- Which upstream items are complete.
- Any intentional deviations from upstream Bridge/GrowthBook behavior.

- [ ] **Step 4: Final verification**

Run:

```bash
cargo fmt --all -- --check
cargo test -p allthecodes-config partition_functions_all_root_under_data_root --lib
cargo test -p allthecodes-daemon protocol
cargo test -p allthecodes-daemon routes
cargo test -p allthecodes-daemon supervisor
cargo test -p allthecodes-daemon gateway_bridge
cargo test -p allthecodes-engine system_prompt
cargo test -p allthecodes-tools sleep_tool
cargo check --workspace
cargo build --workspace --release
```

Expected: all commands exit 0. If live KAIROS smoke is skipped, record the missing credential/network reason in `development/reference/DAEMON_OPERATIONS.md`.

- [ ] **Step 5: Commit**

```bash
git add crates/allthecodes/tests/e2e_cli.rs \
        development/reference/DAEMON_OPERATIONS.md \
        development/hided_features/current-hidden-features.md \
        docs/WORK_STATUS.md \
        development/archive/IMPLEMENTATION_GAPS.md
git commit -m "docs(kairos): close resident assistant parity plan"
```

---

## Recommended Execution Order

1. Task 1 first: it is low risk and makes KAIROS model behavior match the upstream spec.
2. Tasks 2 and 3 next: they remove the current permission/history/resize/sleep inconsistencies that block reliable resident mode.
3. Task 4 after DTOs: bridge work should only target durable worker commands, not daemon-process state.
4. Tasks 5 and 6 after bridge foundations: notification, channels, and Dream rely on reliable durable events.
5. Task 7 last among implementation tasks: workerizing proactive/scheduler is easier once automation state and assistant queue semantics are stable.
6. Task 8 closes verification and documentation.

## Self-Review Notes

- The plan covers every upstream KAIROS section: feature gates, prompt sections, Sleep/Proactive, bridge integration, notifications, daily memory, channels, and Brief output.
- The plan preserves current allthecodes implementation that is already ahead of the upstream stub list in daemon/worker lifecycle.
- The main unresolved design choice is Bridge API endpoint fidelity. If direct claude.ai Bridge API is not available for allthecodes auth, Task 4 must document the intentional deviation and map the behavior onto the allthecodes gateway protocol instead.
