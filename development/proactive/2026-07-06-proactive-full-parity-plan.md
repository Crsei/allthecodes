# FEATURE_PROACTIVE Full Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐 `FEATURE_PROACTIVE` 的完整主动模式行为，使 `FEATURE_PROACTIVE=1` 与 `FEATURE_KAIROS=1` 都能提供上游 `claude-code-bun/docs/features/proactive.md` 描述的 tick 驱动自主工作、Sleep 节奏控制、状态展示和远端/daemon 状态镜像。

**Architecture:** 保留 Rust 当前的双路径架构：普通 TUI 使用进程内 `QueryEngine`，daemon/web/remote 使用 supervisor + worker 队列。新增一个共享 proactive 状态与 tick payload 服务，让 TUI 本地 tick driver 和 daemon proactive worker 使用同一套状态、Sleep、早醒和 payload 语义；不把 TypeScript REPL 状态机原样搬进 daemon worker。

**Tech Stack:** Rust 1.91.1, tokio, clap, ratatui/crossterm TUI, axum daemon API, serde_json, existing `allthecodes-services`, `allthecodes-commands`, `allthecodes-engine`, `allthecodes-daemon`, `allthecodes-tools`, and `allthecodes` crates.

## Global Constraints

- 当前分支是 Full Build；不要以 Lite 精简为理由省略 proactive 行为。
- 所有持久化路径必须使用 `~/.allthecodes/` 或 `ALLTHECODES_HOME`，不得读写 `~/.Codex/`。
- `FEATURE_KAIROS=1` 必须继续隐含启用 proactive。
- `FEATURE_PROACTIVE=1` 必须可单独启用 proactive；不要求用户同时设置 `FEATURE_KAIROS=1`。
- daemon/web/remote 路径继续由 assistant worker 执行模型 turn；proactive/scheduler worker 只负责排队，不直接拥有第二个 `QueryEngine`。
- 普通用户输入和 remote 用户输入必须打断 proactive sleep；proactive tick 自身不能打断 sleep。
- `Sleep` 工具名、`duration_seconds` 参数、1-3600 秒范围保持稳定。
- 不改变 npm release backend-only 策略；此计划不引入 sibling web build。
- 修改 UI 代码时只触碰 `crates/allthecodes/src/ui/` 端的 Rust TUI 代码。
- 使用仓库指定 Cargo 环境：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## Current State

- 已有 `FEATURE_PROACTIVE` feature gate，且 `FEATURE_KAIROS` 会把 `FeatureFlags.proactive` 置为 true。
- 已有 proactive system prompt section，包含 `<tick_tag>`、`Sleep`、`terminalFocus` 和禁止空转文本。
- 已有 `SleepTool` 与 `/sleep`，两者写入 daemon sleep state。
- 已有 daemon `proactive-1` worker，每 30 秒排队 `source=proactive_tick` 的 submit command。
- 已有 `QuerySource::ProactiveTick`，并已标记为 autonomous + non-interactive。
- 已有 `/api/status.automation_state`，但只覆盖 daemon API。

## Verified Gaps

- 没有 `/proactive` toggle 命令。
- 没有 `--proactive` CLI 启动参数，也没有 allthecodes 命名空间的启动环境变量。
- `FEATURE_PROACTIVE=1` 单独开启时不能启动 daemon proactive worker，因为 `--daemon` 当前要求 `FEATURE_KAIROS=1`。
- TUI 默认路径没有本地 proactive tick driver；普通 `allthecodes` TUI 不会因 `FEATURE_PROACTIVE=1` 自主 tick。
- Sleep 早醒语义不完整；新用户输入不会清理 active sleep state，后续 proactive tick 仍会等到 sleep 过期。
- daemon proactive worker 固定传 `terminal_focus=false`，没有使用 daemon `DaemonState::terminal_focus()`。
- Rust TUI 没有 proactive footer/next tick 状态。
- Rust remote/gateway 没有上游 CCR `external_metadata.automation_state` 等价镜像；当前只有 daemon `/api/status` 查询式状态。

---

## File Structure

- Create `crates/allthecodes-services/src/proactive.rs`: shared proactive state machine, tick scheduling, sleep state helpers, and tick payload builder.
- Modify `crates/allthecodes-services/src/lib.rs`: export the proactive service module.
- Modify `crates/allthecodes-config/src/features.rs`: add tests that lock standalone proactive and Kairos-implies-proactive behavior.
- Create `crates/allthecodes-commands/src/proactive_cmd.rs`: `/proactive` toggle command.
- Modify `crates/allthecodes-commands/src/lib.rs`: register `/proactive`.
- Modify `crates/allthecodes/src/command_runtime_bridge.rs`: install a command runtime adapter for proactive state.
- Modify `crates/allthecodes/src/cli.rs`: add `--proactive`.
- Modify `crates/allthecodes/src/startup/runtime_composition.rs` and `crates/allthecodes/src/startup/startup_context.rs`: activate proactive from CLI/env before tools/system prompt are built.
- Create `crates/allthecodes/src/ui/tui/proactive.rs`: TUI-local tick driver and footer state helpers.
- Modify `crates/allthecodes/src/ui/tui.rs`: drive proactive ticks and clear sleep on user input.
- Modify `crates/allthecodes/src/ui/tui/engine_events.rs`: allow proactive tick submit with `QuerySource::ProactiveTick`.
- Modify `crates/allthecodes/src/ui/tui/tests.rs`: cover TUI tick behavior and footer status.
- Modify `crates/allthecodes-daemon/src/tick.rs`: use shared tick payload builder and accept real terminal focus.
- Modify `crates/allthecodes-daemon/src/supervisor.rs`: pass terminal focus into proactive worker and keep standalone proactive worker under `FEATURE_PROACTIVE`.
- Modify `crates/allthecodes-daemon/src/process_state/management.rs`: allow daemon start with `FEATURE_KAIROS=1` or `FEATURE_PROACTIVE=1`.
- Modify `crates/allthecodes-daemon/src/routes.rs`: clear sleep on user submits and expose richer automation status.
- Modify `crates/allthecodes-daemon/src/gateway_bridge.rs`: clear sleep on non-proactive queued work and publish automation metadata events.
- Modify `crates/allthecodes-daemon/src/automation_state.rs`: consume shared proactive snapshot and expose `next_tick_at`.
- Modify `crates/allthecodes-tools/src/exec/sleep_tool.rs`: delegate sleep writes to shared service contract.
- Modify `development/hided_features/current-hidden-features.md`, `development/reference/DAEMON_OPERATIONS.md`, and `docs/WORK_STATUS.md`: update the source-verified proactive status after implementation.

---

## Target Runtime Contract

| Scenario | Expected behavior |
| --- | --- |
| `FEATURE_PROACTIVE=1 allthecodes --daemon` | daemon starts with assistant worker + proactive worker, without bridge/scheduler-only KAIROS workers |
| `FEATURE_KAIROS=1 allthecodes --daemon` | daemon starts KAIROS workers and proactive worker |
| `FEATURE_PROACTIVE=1 allthecodes` | TUI starts with local proactive active and emits first tick after startup interval |
| `/proactive` when inactive | activates proactive, schedules next tick, emits a system notice |
| `/proactive` when active | deactivates proactive, clears `next_tick_at`, emits a system notice |
| model calls `Sleep` | writes shared sleep state, automation state becomes `sleeping` |
| user submits work while sleeping | clears sleep state and runs the user turn immediately |
| proactive tick arrives while a turn is active | skipped and rescheduled; no duplicate submit command |
| terminal focused | tick payload includes `terminalFocus: true` semantics |
| terminal unfocused | tick payload includes `terminalFocus: false` semantics |
| daemon `/api/status` | returns `automation_state.status`, `sleeping_until`, `next_tick_at`, `terminal_focus`, `query_running`, `pending_input` |

---

### Task 1: Shared Proactive State And Tick Payload Service

**Files:**
- Create: `crates/allthecodes-services/src/proactive.rs`
- Modify: `crates/allthecodes-services/src/lib.rs`
- Test: `crates/allthecodes-services/src/proactive.rs`

**Interfaces:**
- Produces: `ProactiveStatus`
- Produces: `ProactiveSnapshot`
- Produces: `ProactiveController`
- Produces: `global_controller() -> &'static ProactiveController`
- Produces: `build_tick_payload(now: DateTime<Local>, terminal_focus: bool, daily_log: Option<&str>) -> serde_json::Value`
- Produces: `write_sleep_state(duration_seconds: u64, reason: &str) -> anyhow::Result<SleepState>`
- Produces: `active_sleep_state() -> anyhow::Result<Option<SleepState>>`
- Produces: `clear_sleep_state(reason: &str) -> anyhow::Result<bool>`
- Consumes: `allthecodes_config::paths::daemon_dir()`

- [ ] **Step 1: Add the module export**

In `crates/allthecodes-services/src/lib.rs`, add:

```rust
pub mod proactive;
```

- [ ] **Step 2: Write failing state-machine tests**

Create `crates/allthecodes-services/src/proactive.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn controller_activate_pause_resume_and_deactivate_updates_snapshot() {
        let controller = ProactiveController::new();

        assert_eq!(controller.snapshot().status, ProactiveStatus::Inactive);

        controller.activate("test");
        let active = controller.snapshot();
        assert_eq!(active.status, ProactiveStatus::Active);
        assert_eq!(active.source.as_deref(), Some("test"));
        assert!(active.next_tick_at.is_some());

        controller.pause("user_input");
        let paused = controller.snapshot();
        assert_eq!(paused.status, ProactiveStatus::Paused);
        assert!(paused.next_tick_at.is_none());

        controller.resume("turn_complete");
        let resumed = controller.snapshot();
        assert_eq!(resumed.status, ProactiveStatus::Active);
        assert!(resumed.next_tick_at.is_some());

        controller.deactivate("slash_command");
        let inactive = controller.snapshot();
        assert_eq!(inactive.status, ProactiveStatus::Inactive);
        assert!(inactive.next_tick_at.is_none());
    }

    #[test]
    fn tick_payload_contains_upstream_contract_fields() {
        let now = Utc.with_ymd_and_hms(2026, 7, 6, 12, 30, 0).unwrap().with_timezone(&chrono::Local);
        let payload = build_tick_payload(now, false, Some("previous work item"));

        assert_eq!(payload["source"], "proactive_tick");
        assert_eq!(payload["terminal_focus"], false);
        assert!(payload["text"].as_str().unwrap().contains("<tick_tag>"));
        assert!(payload["text"].as_str().unwrap().contains("Local time:"));
        assert!(payload["text"].as_str().unwrap().contains("Terminal focus: false"));
        assert!(payload["text"].as_str().unwrap().contains("<daily_log>"));
    }
}
```

- [ ] **Step 3: Run the failing tests**

Run:

```bash
cargo test -p allthecodes-services proactive -- --nocapture
```

Expected before implementation: compile failure because the new types and functions are missing.

- [ ] **Step 4: Implement the state types**

Add these public types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProactiveStatus {
    Inactive,
    Active,
    Paused,
    ContextBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProactiveSnapshot {
    pub status: ProactiveStatus,
    pub source: Option<String>,
    pub next_tick_at: Option<chrono::DateTime<chrono::Utc>>,
    pub paused_reason: Option<String>,
    pub context_blocked: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SleepState {
    pub schema_version: u32,
    pub sleeping_until: chrono::DateTime<chrono::Utc>,
    pub reason: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
```

Implement `ProactiveController` with an internal `parking_lot::RwLock<ProactiveSnapshot>`. Use a 30 second default next tick:

```rust
pub const DEFAULT_TICK_INTERVAL_MS: u64 = 30_000;
pub const SLEEP_STATE_SCHEMA_VERSION: u32 = 2;
```

- [ ] **Step 5: Implement payload and sleep helpers**

Implement `build_tick_payload` with this shape:

```rust
serde_json::json!({
    "text": tick_prompt,
    "message_id": format!("proactive-tick-{}", now.timestamp_millis()),
    "source": "proactive_tick",
    "terminal_focus": terminal_focus,
    "proactive": {
        "time": now.to_rfc3339(),
        "terminal_focus": terminal_focus
    }
})
```

Implement sleep state helpers using `allthecodes_config::paths::daemon_dir().join("sleep-state.json")`, a temporary file, and `std::fs::rename` for atomic replacement.

- [ ] **Step 6: Run focused verification**

Run:

```bash
cargo test -p allthecodes-services proactive -- --nocapture
cargo check -p allthecodes-services
```

Expected: proactive service tests pass and no warnings are introduced.

- [ ] **Step 7: Commit Task 1**

```bash
git add -A -- crates/allthecodes-services/src/lib.rs \
              crates/allthecodes-services/src/proactive.rs
git commit -m "feat(proactive): add shared runtime state"
```

---

### Task 2: Feature Gate And Startup Activation Contract

**Files:**
- Modify: `crates/allthecodes-config/src/features.rs`
- Modify: `crates/allthecodes/src/cli.rs`
- Modify: `crates/allthecodes/src/startup/startup_context.rs`
- Modify: `crates/allthecodes/src/startup/runtime_composition.rs`
- Modify: `crates/allthecodes/src/startup/mode_router.rs`
- Test: `crates/allthecodes-config/src/features.rs`
- Test: `crates/allthecodes/src/startup/mode_router.rs`

**Interfaces:**
- Produces: `Cli.proactive: bool`
- Produces: startup activation from `--proactive`
- Produces: startup activation from `ALLTHECODES_PROACTIVE=1`
- Consumes: `allthecodes_services::proactive::global_controller()`

- [ ] **Step 1: Add feature and daemon gate tests**

In `crates/allthecodes-config/src/features.rs`, add a regression test if not already present:

```rust
#[test]
fn kairos_and_standalone_proactive_contracts_are_locked() {
    let kairos = flags(&[("FEATURE_KAIROS", "1")]);
    assert!(kairos.kairos);
    assert!(kairos.proactive);

    let proactive = flags(&[("FEATURE_PROACTIVE", "1")]);
    assert!(!proactive.kairos);
    assert!(proactive.proactive);
}
```

In `crates/allthecodes/src/startup/mode_router.rs`, add a pure helper test around a new helper:

```rust
fn daemon_allowed_by_features() -> bool {
    use allthecodes_config::features::{self, Feature};
    features::enabled(Feature::Kairos) || features::enabled(Feature::Proactive)
}
```

Test:

```rust
#[test]
#[serial_test::serial]
fn daemon_mode_accepts_standalone_proactive() {
    let mut flags = allthecodes_config::features::FeatureFlags::all_disabled();
    flags.proactive = true;
    allthecodes_config::features::set_runtime_override(flags);
    assert!(daemon_allowed_by_features());
    allthecodes_config::features::clear_runtime_override();
}
```

- [ ] **Step 2: Run tests and verify current daemon helper gap**

Run:

```bash
cargo test -p allthecodes-config proactive_contracts -- --nocapture
cargo test -p allthecodes daemon_mode_accepts_standalone_proactive -- --nocapture
```

Expected before implementation: config test passes if existing behavior is intact; startup test fails until helper and gate are added.

- [ ] **Step 3: Add CLI flag**

In `crates/allthecodes/src/cli.rs`, add:

```rust
/// Start this session in proactive autonomous mode.
#[arg(long = "proactive")]
pub proactive: bool,
```

Do not hide the flag; upstream exposes `--proactive`.

- [ ] **Step 4: Activate proactive during startup composition**

In startup composition, before tool registry and system prompt construction are finalized, compute:

```rust
fn startup_proactive_requested(cli: &crate::cli::Cli) -> bool {
    cli.proactive
        || std::env::var("ALLTHECODES_PROACTIVE")
            .ok()
            .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
        || allthecodes_config::features::enabled(allthecodes_config::features::Feature::Proactive)
}
```

When true:

```rust
allthecodes_services::proactive::global_controller().activate("startup");
```

This must happen before `allthecodes_tools::registry::get_all_tools()` is used so `SleepTool::is_enabled()` and system prompt feature checks are consistent.

- [ ] **Step 5: Allow standalone proactive daemon mode**

In `mode_router.rs`, replace:

```rust
if !features::enabled(Feature::Kairos) {
    eprintln!("error: --daemon requires FEATURE_KAIROS=1");
    return Ok(ExitCode::FAILURE);
}
```

with:

```rust
if !daemon_allowed_by_features() {
    eprintln!("error: --daemon requires FEATURE_KAIROS=1 or FEATURE_PROACTIVE=1");
    return Ok(ExitCode::FAILURE);
}
```

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p allthecodes-config proactive
cargo test -p allthecodes daemon_mode_accepts_standalone_proactive -- --nocapture
cargo check -p allthecodes
```

Expected: standalone proactive daemon gate passes and `Cli` compiles with the new flag.

- [ ] **Step 7: Commit Task 2**

```bash
git add -A -- crates/allthecodes-config/src/features.rs \
              crates/allthecodes/src/cli.rs \
              crates/allthecodes/src/startup/startup_context.rs \
              crates/allthecodes/src/startup/runtime_composition.rs \
              crates/allthecodes/src/startup/mode_router.rs
git commit -m "feat(proactive): support standalone startup"
```

---

### Task 3: `/proactive` Toggle Command

**Files:**
- Create: `crates/allthecodes-commands/src/proactive_cmd.rs`
- Modify: `crates/allthecodes-commands/src/lib.rs`
- Modify: `crates/allthecodes/src/command_runtime_bridge.rs`
- Test: `crates/allthecodes-commands/src/proactive_cmd.rs`
- Test: `crates/allthecodes-commands/src/lib.rs`

**Interfaces:**
- Produces: `ProactiveCmdHandler`
- Produces: `/proactive` command metadata
- Consumes: `allthecodes_services::proactive::global_controller()`

- [ ] **Step 1: Add command module and failing tests**

Create `crates/allthecodes-commands/src/proactive_cmd.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandContext, CommandHandler, CommandResult};
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/tmp/proactive-test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn proactive_command_toggles_active_state() {
        let controller = allthecodes_services::proactive::global_controller();
        controller.deactivate("test-reset");

        let handler = ProactiveCmdHandler;
        let mut ctx = test_ctx();
        let first = handler.execute("", &mut ctx).await.unwrap();
        match first {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode enabled")),
            _ => panic!("expected output"),
        }
        assert_eq!(controller.snapshot().status, allthecodes_services::proactive::ProactiveStatus::Active);

        let second = handler.execute("", &mut ctx).await.unwrap();
        match second {
            CommandResult::Output(text) => assert!(text.contains("Proactive mode disabled")),
            _ => panic!("expected output"),
        }
        assert_eq!(controller.snapshot().status, allthecodes_services::proactive::ProactiveStatus::Inactive);
    }
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p allthecodes-commands proactive_command_toggles_active_state -- --nocapture
```

Expected before implementation: compile failure because `ProactiveCmdHandler` is missing.

- [ ] **Step 3: Implement command handler**

Add:

```rust
pub struct ProactiveCmdHandler;

#[async_trait::async_trait]
impl crate::CommandHandler for ProactiveCmdHandler {
    async fn execute(
        &self,
        _args: &str,
        _ctx: &mut crate::CommandContext,
    ) -> anyhow::Result<crate::CommandResult> {
        use allthecodes_services::proactive::{global_controller, ProactiveStatus};

        let controller = global_controller();
        let snapshot = controller.snapshot();
        if matches!(snapshot.status, ProactiveStatus::Active | ProactiveStatus::Paused | ProactiveStatus::ContextBlocked) {
            controller.deactivate("slash_command");
            Ok(crate::CommandResult::Output("Proactive mode disabled.".to_string()))
        } else {
            controller.activate("slash_command");
            Ok(crate::CommandResult::Output("Proactive mode enabled. The assistant will continue working from periodic ticks when idle.".to_string()))
        }
    }
}
```

- [ ] **Step 4: Register the command**

In `crates/allthecodes-commands/src/lib.rs`, add:

```rust
pub mod proactive_cmd;
```

Register in `get_all_commands()`:

```rust
command(
    "proactive",
    &[],
    "Toggle proactive autonomous mode",
    proactive_cmd::ProactiveCmdHandler,
),
```

- [ ] **Step 5: Add metadata regression test**

In `lib.rs` tests:

```rust
#[test]
fn builtin_registry_includes_proactive_command() {
    let metadata = command_metadata(&get_all_commands());
    assert!(metadata.iter().any(|cmd| cmd.name == "proactive"));
}
```

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p allthecodes-commands proactive -- --nocapture
cargo check -p allthecodes-commands
```

Expected: `/proactive` toggles the shared state and appears in command metadata.

- [ ] **Step 7: Commit Task 3**

```bash
git add -A -- crates/allthecodes-commands/src/proactive_cmd.rs \
              crates/allthecodes-commands/src/lib.rs \
              crates/allthecodes/src/command_runtime_bridge.rs
git commit -m "feat(proactive): add slash toggle"
```

---

### Task 4: TUI Local Proactive Tick Driver

**Files:**
- Create: `crates/allthecodes/src/ui/tui/proactive.rs`
- Modify: `crates/allthecodes/src/ui/tui.rs`
- Modify: `crates/allthecodes/src/ui/tui/engine_events.rs`
- Test: `crates/allthecodes/src/ui/tui/tests.rs`
- Test: `crates/allthecodes/src/ui/tui/proactive.rs`

**Interfaces:**
- Produces: `ProactiveTickDriver`
- Produces: `ProactiveTickDecision`
- Produces: `spawn_engine_query_with_source(engine, prompt, source, tx)`
- Consumes: `allthecodes_services::proactive::global_controller()`
- Consumes: `allthecodes_services::proactive::build_tick_payload(...)`

- [ ] **Step 1: Add focused tick decision tests**

Create `crates/allthecodes/src/ui/tui/proactive.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn tick_decision_requires_active_idle_and_due() {
        let due = Utc::now() - Duration::seconds(1);
        let snapshot = allthecodes_services::proactive::ProactiveSnapshot {
            status: allthecodes_services::proactive::ProactiveStatus::Active,
            source: Some("test".into()),
            next_tick_at: Some(due),
            paused_reason: None,
            context_blocked: false,
        };

        assert_eq!(
            decide_tick(&snapshot, false, false, false, Utc::now()),
            ProactiveTickDecision::Submit
        );
        assert_eq!(
            decide_tick(&snapshot, true, false, false, Utc::now()),
            ProactiveTickDecision::BlockedByRunningTurn
        );
        assert_eq!(
            decide_tick(&snapshot, false, true, false, Utc::now()),
            ProactiveTickDecision::BlockedByPendingInput
        );
    }
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p allthecodes tui::proactive -- --nocapture
```

Expected before implementation: compile failure because the module is not wired.

- [ ] **Step 3: Implement tick decision and driver**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProactiveTickDecision {
    Submit,
    Inactive,
    NotDue,
    BlockedByRunningTurn,
    BlockedByPendingInput,
    Sleeping,
}

pub(super) fn decide_tick(
    snapshot: &allthecodes_services::proactive::ProactiveSnapshot,
    streaming: bool,
    pending_permission: bool,
    pending_question: bool,
    now: chrono::DateTime<chrono::Utc>,
) -> ProactiveTickDecision {
    use allthecodes_services::proactive::ProactiveStatus;
    if snapshot.status != ProactiveStatus::Active {
        return ProactiveTickDecision::Inactive;
    }
    if streaming {
        return ProactiveTickDecision::BlockedByRunningTurn;
    }
    if pending_permission || pending_question {
        return ProactiveTickDecision::BlockedByPendingInput;
    }
    if allthecodes_services::proactive::active_sleep_state().ok().flatten().is_some() {
        return ProactiveTickDecision::Sleeping;
    }
    match snapshot.next_tick_at {
        Some(next) if next <= now => ProactiveTickDecision::Submit,
        _ => ProactiveTickDecision::NotDue,
    }
}
```

- [ ] **Step 4: Add source-aware engine submit helper**

In `engine_events.rs`, keep the existing helper and add:

```rust
pub(super) fn spawn_engine_query_with_source(
    engine: Arc<QueryEngine>,
    prompt: String,
    source: QuerySource,
    tx: mpsc::UnboundedSender<EngineEvent>,
) {
    tokio::spawn(async move {
        let overrides = chat_mode_submit_overrides(&engine);
        let stream = engine.submit_message_with_overrides(&prompt, source, overrides);
        futures::pin_mut!(stream);
        while let Some(msg) = stream.next().await {
            if tx.send(EngineEvent::Sdk(Box::new(msg))).is_err() {
                break;
            }
        }
        let _ = tx.send(EngineEvent::Done);
    });
}
```

Then make the existing `spawn_engine_query(...)` call `spawn_engine_query_with_source(..., QuerySource::ReplMainThread, ...)`.

- [ ] **Step 5: Wire tick driver into TUI loop**

In `tui.rs`:

- Add module import:

```rust
#[path = "tui/proactive.rs"]
mod proactive;
```

- Add a 1 second proactive interval next to the draw tick:

```rust
let mut proactive_interval = tokio::time::interval(Duration::from_secs(1));
proactive_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
```

- Add a select branch:

```rust
_ = proactive_interval.tick() => {
    let snapshot = allthecodes_services::proactive::global_controller().snapshot();
    let decision = proactive::decide_tick(
        &snapshot,
        app.is_streaming(),
        pending_permission_response.is_some(),
        pending_question_response.is_some(),
        chrono::Utc::now(),
    );
    if decision == proactive::ProactiveTickDecision::Submit {
        let payload = allthecodes_services::proactive::build_tick_payload(
            chrono::Local::now(),
            true,
            None,
        );
        let prompt = payload["text"].as_str().unwrap_or_default().to_string();
        app.set_streaming(true);
        engine.reset_abort();
        engine_events::spawn_engine_query_with_source(
            engine.clone(),
            prompt,
            allthecodes_engine::types::config::QuerySource::ProactiveTick,
            engine_tx.clone(),
        );
        allthecodes_services::proactive::global_controller().mark_tick_submitted();
    }
}
```

If the TUI later gains a reliable terminal focus signal, replace the hardcoded `true` with that signal. For this task, `true` is correct for local TUI because the event loop is running in an attached terminal.

- [ ] **Step 6: Clear sleep on user submit**

Before normal user submit starts a new turn in `submit_prompt_to_engine(...)`, call:

```rust
let _ = allthecodes_services::proactive::clear_sleep_state("user_submit");
```

Do not call this for proactive tick submits.

- [ ] **Step 7: Verify**

Run:

```bash
cargo test -p allthecodes tui::proactive -- --nocapture
cargo test -p allthecodes tui -- --nocapture
cargo check -p allthecodes
```

Expected: local TUI proactive tick decision tests pass and no UI compile warnings are introduced.

- [ ] **Step 8: Commit Task 4**

```bash
git add -A -- crates/allthecodes/src/ui/tui/proactive.rs \
              crates/allthecodes/src/ui/tui.rs \
              crates/allthecodes/src/ui/tui/engine_events.rs \
              crates/allthecodes/src/ui/tui/tests.rs
git commit -m "feat(proactive): drive local TUI ticks"
```

---

### Task 5: Sleep Early Wake And Unified Sleep Writers

**Files:**
- Modify: `crates/allthecodes-tools/src/exec/sleep_tool.rs`
- Modify: `crates/allthecodes-commands/src/sleep_cmd.rs`
- Modify: `crates/allthecodes/src/command_runtime_bridge.rs`
- Modify: `crates/allthecodes-daemon/src/process_state/storage.rs`
- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`
- Test: `crates/allthecodes-tools/src/exec/sleep_tool.rs`
- Test: `crates/allthecodes-commands/src/sleep_cmd.rs`
- Test: `crates/allthecodes-daemon/src/routes.rs`
- Test: `crates/allthecodes-daemon/src/gateway_bridge.rs`

**Interfaces:**
- Produces: single sleep state writer via `allthecodes_services::proactive::write_sleep_state`
- Produces: `clear_sleep_state("user_submit" | "remote_submit" | "gateway_command")`
- Consumes: `allthecodes_services::proactive::active_sleep_state`

- [ ] **Step 1: Add SleepTool contract test**

Update `sleep_tool.rs` tests to assert the exact shared schema:

```rust
#[test]
#[serial_test::serial]
fn sleep_tool_writes_shared_sleep_contract() {
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

    let state = write_sleep_state(60, " waiting ").unwrap();
    let shared = allthecodes_services::proactive::active_sleep_state()
        .unwrap()
        .expect("shared sleep state");

    assert_eq!(state.schema_version, allthecodes_services::proactive::SLEEP_STATE_SCHEMA_VERSION);
    assert_eq!(shared.schema_version, state.schema_version);
    assert_eq!(shared.reason.as_deref(), Some("waiting"));
}
```

- [ ] **Step 2: Replace local SleepTool writer**

Remove `ToolSleepState` and local JSON writing from `sleep_tool.rs`. Use:

```rust
let sleep_state = allthecodes_services::proactive::write_sleep_state(
    duration_seconds as u64,
    &reason,
)?;
```

Return `sleep_until` from `sleep_state.sleeping_until`.

- [ ] **Step 3: Route `/sleep` through the same writer**

In `command_runtime_bridge.rs`, make `sleep_state_for_commands` call:

```rust
let state = allthecodes_services::proactive::write_sleep_state(secs, reason)?;
Ok(allthecodes_commands::sleep_cmd::DaemonSleepState {
    sleeping_until: state.sleeping_until,
})
```

- [ ] **Step 4: Clear sleep on new non-proactive work**

In daemon submit paths:

- `routes.rs` `/api/submit`: clear with `"http_submit"`.
- `gateway_bridge.rs` before dispatching any submit whose payload `source` is not `"proactive_tick"`: clear with `"gateway_submit"`.
- `process_state/management.rs` `daemon submit`: clear with `"daemon_submit"`.

Use this guard:

```rust
fn should_wake_sleep_for_payload(payload: &serde_json::Value) -> bool {
    payload.get("source").and_then(serde_json::Value::as_str) != Some("proactive_tick")
}
```

- [ ] **Step 5: Add early-wake regression tests**

In daemon route tests, write a sleep state, submit a user request, and assert sleep is cleared:

```rust
#[tokio::test]
#[serial_test::serial]
async fn submit_endpoint_clears_active_sleep_state() {
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());
    let token = crate::process_state::write_control_token().unwrap();
    allthecodes_services::proactive::write_sleep_state(300, "waiting").unwrap();
    let app = api_routes().with_state(make_daemon_state());

    let (_status, body) = post_json(
        app,
        "/api/submit",
        &token.token,
        json!({ "text": "wake up" }),
    ).await;

    assert_eq!(body["status"], "ok");
    assert!(allthecodes_services::proactive::active_sleep_state().unwrap().is_none());
}
```

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p allthecodes-tools sleep_tool -- --nocapture
cargo test -p allthecodes-commands sleep -- --nocapture
cargo test -p allthecodes-daemon submit_endpoint_clears_active_sleep_state -- --nocapture
cargo test -p allthecodes-daemon gateway_bridge -- --nocapture
```

Expected: all sleep writers use one schema, and user/remote work clears proactive sleep.

- [ ] **Step 7: Commit Task 5**

```bash
git add -A -- crates/allthecodes-tools/src/exec/sleep_tool.rs \
              crates/allthecodes-commands/src/sleep_cmd.rs \
              crates/allthecodes/src/command_runtime_bridge.rs \
              crates/allthecodes-daemon/src/process_state/storage.rs \
              crates/allthecodes-daemon/src/routes.rs \
              crates/allthecodes-daemon/src/gateway_bridge.rs
git commit -m "feat(proactive): wake sleep on new work"
```

---

### Task 6: Daemon Proactive Standalone And Terminal Focus

**Files:**
- Modify: `crates/allthecodes-daemon/src/tick.rs`
- Modify: `crates/allthecodes-daemon/src/supervisor.rs`
- Modify: `crates/allthecodes-daemon/src/state.rs`
- Modify: `crates/allthecodes-daemon/src/automation_state.rs`
- Test: `crates/allthecodes-daemon/src/tick.rs`
- Test: `crates/allthecodes-daemon/src/supervisor.rs`
- Test: `crates/allthecodes-daemon/src/automation_state.rs`

**Interfaces:**
- Produces: daemon tick payloads from shared `build_tick_payload`
- Produces: real terminal focus propagation into daemon tick payload
- Produces: standalone proactive daemon worker set when only `FEATURE_PROACTIVE=1`
- Consumes: `DaemonState::terminal_focus()`

- [ ] **Step 1: Add supervisor spec tests**

Add tests:

```rust
#[test]
#[serial]
fn default_worker_specs_include_proactive_without_kairos() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let _features = FeatureOverrideGuard::set(FeatureFlags {
        proactive: true,
        ..FeatureFlags::all_disabled()
    });

    let specs = default_worker_specs(temp.path());
    let kinds = specs.iter().map(|spec| spec.kind.as_str()).collect::<Vec<_>>();

    assert!(kinds.contains(&"assistant-session"));
    assert!(kinds.contains(&"proactive"));
    assert!(!kinds.contains(&"bridge-sync"));
    assert!(!kinds.contains(&"scheduler"));
}
```

- [ ] **Step 2: Replace daemon tick payload builder**

In `tick.rs`, replace local payload formatting with:

```rust
let today_log = super::memory_log::read_today_log();
let payload = allthecodes_services::proactive::build_tick_payload(
    now,
    terminal_focus,
    (!today_log.is_empty()).then_some(today_log.as_str()),
);
```

Keep the existing `skill_discovery` enrichment by inserting it into `payload["proactive"]` after the shared payload is created.

- [ ] **Step 3: Pass terminal focus into worker ticks**

Change `run_proactive_worker_mode` to receive a terminal focus provider. In worker process mode, read focus from persisted daemon client state if present; until a durable client-focus file exists, use a conservative helper:

```rust
fn daemon_terminal_focus_for_worker() -> bool {
    crate::process_state::status_snapshot()
        .ok()
        .and_then(|snapshot| match snapshot {
            crate::process_state::DaemonStatusSnapshot::Running(state) => Some(!state.workers.is_empty()),
            _ => None,
        })
        .unwrap_or(false)
}
```

Then pass:

```rust
super::tick::enqueue_proactive_tick_once(Local::now(), daemon_terminal_focus_for_worker())?
```

When a better SSE client focus state is persisted, this helper is the only replacement point.

- [ ] **Step 4: Extend automation state with next tick**

Add to `AutomationState`:

```rust
pub next_tick_at: Option<DateTime<Utc>>,
pub proactive_active: bool,
```

Populate from:

```rust
let proactive = allthecodes_services::proactive::global_controller().snapshot();
```

Daemon worker mode should publish `proactive_active=true` when `features.proactive` is true even if the in-process global controller is inactive in the supervisor process.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-daemon proactive_tick -- --nocapture
cargo test -p allthecodes-daemon default_worker_specs_include_proactive_without_kairos -- --nocapture
cargo test -p allthecodes-daemon automation_state -- --nocapture
cargo check -p allthecodes-daemon
```

Expected: daemon proactive works as standalone feature and tick payloads still include `source=proactive_tick`.

- [ ] **Step 6: Commit Task 6**

```bash
git add -A -- crates/allthecodes-daemon/src/tick.rs \
              crates/allthecodes-daemon/src/supervisor.rs \
              crates/allthecodes-daemon/src/state.rs \
              crates/allthecodes-daemon/src/automation_state.rs
git commit -m "feat(proactive): complete daemon tick parity"
```

---

### Task 7: TUI Footer And Status Surface

**Files:**
- Modify: `crates/allthecodes/src/ui/app/domain.rs`
- Modify: `crates/allthecodes/src/ui/app/render.rs`
- Modify: `crates/allthecodes/src/ui/components/status_widget.rs`
- Modify: `crates/allthecodes/src/ui/tui.rs`
- Modify: `crates/allthecodes/src/ui/tui/tests.rs`
- Test: `crates/allthecodes/src/ui/components/snapshots/`
- Test: `crates/allthecodes/src/ui/runtime/snapshots/`

**Interfaces:**
- Produces: `App::set_proactive_status(snapshot)`
- Produces: footer display for inactive/standby/sleeping/running/paused
- Consumes: `allthecodes_services::proactive::ProactiveSnapshot`
- Consumes: `allthecodes_services::proactive::active_sleep_state()`

- [ ] **Step 1: Add render state tests**

In TUI tests, create a snapshot test with proactive active:

```rust
#[test]
fn status_widget_shows_proactive_standby() {
    let mut app = App::new();
    app.set_proactive_status(Some(ProactiveUiStatus {
        label: "proactive standby".to_string(),
        next_tick_text: Some("next tick in 30s".to_string()),
    }));

    let output = render_app_for_test(&app, 100, 24);
    assert!(output.contains("proactive standby"));
    assert!(output.contains("next tick in 30s"));
}
```

- [ ] **Step 2: Add App state fields**

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProactiveUiStatus {
    pub label: String,
    pub next_tick_text: Option<String>,
}
```

Store `Option<ProactiveUiStatus>` in App runtime state.

- [ ] **Step 3: Update TUI loop status every second**

In the proactive interval branch, compute:

```rust
let snapshot = allthecodes_services::proactive::global_controller().snapshot();
let sleep = allthecodes_services::proactive::active_sleep_state().ok().flatten();
app.set_proactive_status(proactive::ui_status_from_snapshot(&snapshot, sleep.as_ref()));
```

`ui_status_from_snapshot` returns:

- `None` for inactive.
- `proactive sleeping` with wake time when sleep exists.
- `proactive standby` with next tick countdown when active.
- `proactive paused` when paused.
- `proactive blocked` when context blocked.

- [ ] **Step 4: Render footer without layout shift**

Render the status as compact text in the existing footer/status area. Keep the text under 32 visible characters before the optional countdown:

```text
proactive standby
proactive sleeping
proactive paused
```

Do not create a new floating card or panel.

- [ ] **Step 5: Verify snapshots**

Run:

```bash
cargo test -p allthecodes status_widget_shows_proactive_standby -- --nocapture
cargo test -p allthecodes ui:: -- --nocapture
```

Expected: proactive status renders in the existing footer without overlapping prompt input.

- [ ] **Step 6: Commit Task 7**

```bash
git add -A -- crates/allthecodes/src/ui/app/domain.rs \
              crates/allthecodes/src/ui/app/render.rs \
              crates/allthecodes/src/ui/components/status_widget.rs \
              crates/allthecodes/src/ui/tui.rs \
              crates/allthecodes/src/ui/tui/tests.rs \
              crates/allthecodes/src/ui/components/snapshots \
              crates/allthecodes/src/ui/runtime/snapshots
git commit -m "feat(proactive): show TUI automation status"
```

---

### Task 8: Daemon Status And Gateway Metadata Mirror

**Files:**
- Modify: `crates/allthecodes-daemon/src/automation_state.rs`
- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`
- Modify: `crates/allthecodes-gateway/src/run.rs`
- Modify: `crates/allthecodes-gateway/src/store.rs`
- Modify: `crates/allthecodes-gateway/src/api.rs`
- Test: `crates/allthecodes-daemon/src/routes.rs`
- Test: `crates/allthecodes-gateway/src/store.rs`
- Test: `crates/allthecodes-gateway/src/api.rs`

**Interfaces:**
- Produces: `AutomationState::external_metadata() -> serde_json::Value`
- Produces: gateway run metadata key `automation_state`
- Consumes: existing `RunMeta` persistence

- [ ] **Step 1: Add daemon status test**

Extend `status_endpoint_embeds_automation_state`:

```rust
assert_eq!(body["automation_state"]["status"], "standby");
assert!(body["automation_state"].get("proactive_active").is_some());
assert!(body["automation_state"].get("next_tick_at").is_some());
```

- [ ] **Step 2: Add gateway metadata storage**

In `RunMeta`, add:

```rust
pub metadata: serde_json::Map<String, serde_json::Value>,
```

If `RunMeta` already has a metadata-like field by the time this task is executed, extend that field instead of adding a second one. The serialized JSON key must be exactly `"metadata"`.

- [ ] **Step 3: Add store update helper**

In gateway store:

```rust
pub fn merge_run_metadata(
    &self,
    run_id: &RunId,
    patch: serde_json::Value,
) -> Result<RunMeta, GatewayError>
```

Merge object keys shallowly. If `patch` is not an object, return a `GatewayDiagnostic` with code `"invalid_run_metadata_patch"`.

- [ ] **Step 4: Mirror automation state when daemon state changes**

In daemon bridge submit start, submit completion, sleep write, and sleep clear paths, call:

```rust
let automation = crate::automation_state::snapshot_from_process_state();
let patch = serde_json::json!({
    "automation_state": automation.external_metadata()
});
```

For command paths without a run id, append a daemon event:

```rust
event_type = "automation_state"
data = patch
```

- [ ] **Step 5: Expose metadata in gateway API**

In gateway API run response, include:

```json
"metadata": {
  "automation_state": {
    "status": "standby"
  }
}
```

This is the Rust equivalent of upstream `external_metadata.automation_state` without inventing a CCR PUT path that does not exist in this repository.

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p allthecodes-daemon status_endpoint_embeds_automation_state -- --nocapture
cargo test -p allthecodes-gateway metadata -- --nocapture
cargo test -p allthecodes-gateway api -- --nocapture
```

Expected: daemon and gateway surfaces both carry automation state.

- [ ] **Step 7: Commit Task 8**

```bash
git add -A -- crates/allthecodes-daemon/src/automation_state.rs \
              crates/allthecodes-daemon/src/routes.rs \
              crates/allthecodes-daemon/src/gateway_bridge.rs \
              crates/allthecodes-gateway/src/run.rs \
              crates/allthecodes-gateway/src/store.rs \
              crates/allthecodes-gateway/src/api.rs
git commit -m "feat(proactive): mirror automation metadata"
```

---

### Task 9: Context Blocking And Compaction Resume Semantics

**Files:**
- Modify: `crates/allthecodes-services/src/proactive.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/mod.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/submit_message.rs`
- Modify: `crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs`
- Modify: `crates/allthecodes-engine/src/system_prompt/tests.rs`
- Test: `crates/allthecodes-engine/src/lifecycle/tests.rs`
- Test: `crates/allthecodes-engine/src/system_prompt/tests.rs`

**Interfaces:**
- Produces: `ProactiveController::set_context_blocked(blocked: bool, reason: &str)`
- Produces: proactive compact resume reminder
- Consumes: existing compaction boundary and `QuerySource::ProactiveTick`

- [ ] **Step 1: Add state tests for context blocked**

In `proactive.rs`:

```rust
#[test]
fn context_blocked_clears_next_tick_and_resume_reschedules() {
    let controller = ProactiveController::new();
    controller.activate("test");
    controller.set_context_blocked(true, "compact_required");
    let blocked = controller.snapshot();
    assert_eq!(blocked.status, ProactiveStatus::ContextBlocked);
    assert!(blocked.next_tick_at.is_none());

    controller.set_context_blocked(false, "compact_complete");
    let active = controller.snapshot();
    assert_eq!(active.status, ProactiveStatus::Active);
    assert!(active.next_tick_at.is_some());
}
```

- [ ] **Step 2: Block proactive while context is unsafe**

When engine lifecycle detects context overflow, compaction-required, or plan-mode block that prevents safe autonomous work, call:

```rust
allthecodes_services::proactive::global_controller()
    .set_context_blocked(true, "context_limit");
```

When compaction completes or `/clear` starts a clean session, call:

```rust
allthecodes_services::proactive::global_controller()
    .set_context_blocked(false, "context_ready");
```

- [ ] **Step 3: Add proactive compact resume prompt**

In system prompt dynamic section, if proactive is active and this is not the first wake-up after compaction, include:

```text
You are running in autonomous/proactive mode. This is not a first wake-up after compaction. Continue the existing work loop from the summary instead of greeting the user again.
```

Trigger this from a compact metadata flag stored in `AppState` or session state. Use the existing compact metadata path; do not add a second compact tracking file.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-services context_blocked -- --nocapture
cargo test -p allthecodes-engine proactive -- --nocapture
cargo test -p allthecodes-engine compact -- --nocapture
```

Expected: proactive ticks stop while context is blocked and resume after compaction without first-wake greeting instructions.

- [ ] **Step 5: Commit Task 9**

```bash
git add -A -- crates/allthecodes-services/src/proactive.rs \
              crates/allthecodes-engine/src/lifecycle/mod.rs \
              crates/allthecodes-engine/src/lifecycle/submit_message.rs \
              crates/allthecodes-engine/src/system_prompt/dynamic_sections.rs \
              crates/allthecodes-engine/src/system_prompt/tests.rs
git commit -m "feat(proactive): handle context blocking"
```

---

### Task 10: End-To-End Proactive Tests

**Files:**
- Create: `crates/allthecodes/tests/proactive_tui.rs`
- Create: `crates/allthecodes-daemon/tests/proactive_daemon.rs`
- Modify: `development/test/README.md`

**Interfaces:**
- Produces: CLI-level smoke for `/proactive`
- Produces: daemon-level smoke for standalone `FEATURE_PROACTIVE=1`
- Consumes: cargo test harness and temp `ALLTHECODES_HOME`

- [ ] **Step 1: Add daemon standalone smoke test**

Create `crates/allthecodes-daemon/tests/proactive_daemon.rs`:

```rust
#[test]
fn proactive_daemon_mode_accepts_standalone_feature() {
    let temp = tempfile::tempdir().unwrap();
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    std::env::set_var("FEATURE_PROACTIVE", "1");
    std::env::remove_var("FEATURE_KAIROS");

    let flags = allthecodes_config::features::FeatureFlags::from_env();
    assert!(flags.proactive);
    assert!(!flags.kairos);

    std::env::remove_var("FEATURE_PROACTIVE");
    std::env::remove_var("ALLTHECODES_HOME");
}
```

This test is deliberately process-local and does not start a long-running daemon.

- [ ] **Step 2: Add command smoke test**

Create `crates/allthecodes/tests/proactive_tui.rs` with a command registry assertion:

```rust
#[test]
fn proactive_command_is_registered_for_tui() {
    let metadata = allthecodes_commands::command_metadata(&allthecodes_commands::get_all_commands());
    let proactive = metadata
        .iter()
        .find(|command| command.name == "proactive")
        .expect("/proactive command");
    assert_eq!(proactive.description, "Toggle proactive autonomous mode");
}
```

- [ ] **Step 3: Add a no-provider tick submit test**

Use existing engine mock patterns to submit a synthetic tick with `QuerySource::ProactiveTick` and assert no interactive-only hooks run. If a direct integration point is unavailable, add this to `crates/allthecodes-engine/src/types/config.rs` tests by extending the autonomous source tests.

- [ ] **Step 4: Document local test commands**

In `development/test/README.md`, add:

````markdown
## Proactive

Focused verification:

```bash
cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
```
````

- [ ] **Step 5: Verify focused suites**

Run:

```bash
cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
```

Expected: all focused proactive tests pass without live model credentials.

- [ ] **Step 6: Commit Task 10**

```bash
git add -A -- crates/allthecodes/tests/proactive_tui.rs \
              crates/allthecodes-daemon/tests/proactive_daemon.rs \
              development/test/README.md
git commit -m "test(proactive): add parity smoke coverage"
```

---

### Task 11: Documentation And Status Cleanup

**Files:**
- Modify: `development/hided_features/current-hidden-features.md`
- Modify: `development/reference/DAEMON_OPERATIONS.md`
- Modify: `docs/WORK_STATUS.md`
- Create: `development/proactive/current-state-after-implementation.md`

**Interfaces:**
- Produces: source-verified proactive status docs
- Consumes: final implementation behavior from Tasks 1-10

- [ ] **Step 1: Update hidden feature status**

Change the `FEATURE_PROACTIVE` entry from:

```markdown
- `FEATURE_PROACTIVE`: enables proactive sleep/tick surfaces such as `/sleep` and `SleepTool`; also implied by `FEATURE_KAIROS`.
```

to:

```markdown
- `FEATURE_PROACTIVE`: enables standalone proactive mode, `/proactive`, `/sleep`, `SleepTool`, local TUI ticks, and daemon proactive worker ticks; also implied by `FEATURE_KAIROS`.
```

- [ ] **Step 2: Update daemon operations**

In `development/reference/DAEMON_OPERATIONS.md`, add a standalone proactive section:

```markdown
## Standalone Proactive Daemon

`FEATURE_PROACTIVE=1 allthecodes --daemon` starts the daemon with `assistant-session-1` and `proactive-1`. It does not start KAIROS bridge or scheduler workers unless `FEATURE_KAIROS=1` is also set.

Sleep state lives at `~/.allthecodes/daemon/sleep-state.json` or under `ALLTHECODES_HOME`. User submit paths clear active sleep state before queueing work.
```

- [ ] **Step 3: Update work status**

In `docs/WORK_STATUS.md`, replace any proactive partial-status sentence with:

```markdown
- FEATURE_PROACTIVE parity: standalone feature gate, `/proactive`, local TUI tick driver, daemon proactive worker, Sleep early-wake, automation status, and focused no-provider test coverage are implemented. Live provider soak remains an operator-run validation because it requires credentials.
```

- [ ] **Step 4: Add post-implementation state note**

Create `development/proactive/current-state-after-implementation.md`:

```markdown
# Proactive Current State After Full Parity

Source-verified after implementing `development/proactive/2026-07-06-proactive-full-parity-plan.md`.

## Implemented

- `FEATURE_PROACTIVE=1` can run standalone.
- `FEATURE_KAIROS=1` implies proactive.
- `/proactive` toggles session proactive state.
- TUI local tick driver submits `QuerySource::ProactiveTick`.
- Daemon proactive worker queues `source=proactive_tick`.
- `Sleep` and `/sleep` share one sleep-state schema.
- New user/remote work clears active sleep.
- `/api/status` and gateway metadata expose automation state.

## Verification

```bash
cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
```

Live provider soak is run only when credentials are available.
```

- [ ] **Step 5: Commit Task 11**

```bash
git add -A -- development/hided_features/current-hidden-features.md \
              development/reference/DAEMON_OPERATIONS.md \
              docs/WORK_STATUS.md \
              development/proactive/current-state-after-implementation.md
git commit -m "docs(proactive): record full parity status"
```

---

## Final Verification

After all tasks land, run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-engine proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
cargo check --workspace
cargo build --workspace --release
```

Expected:

- All focused proactive tests pass.
- `cargo check --workspace` has no new warnings.
- `cargo build --workspace --release` succeeds.
- Existing known warning policy remains satisfied; unused imports introduced by these tasks are removed.

Manual smoke, no live model call:

```bash
FEATURE_PROACTIVE=1 allthecodes daemon status
FEATURE_PROACTIVE=1 allthecodes --help | rg -- '--proactive'
```

Manual smoke, requires configured model credentials:

```bash
FEATURE_PROACTIVE=1 allthecodes --proactive
```

Expected live behavior:

- TUI shows proactive status in the footer.
- `/proactive` disables and re-enables the tick loop.
- `Sleep` causes sleeping status.
- Submitting a user prompt clears sleeping status.

---

## Self-Review

- Spec coverage: this plan covers feature gating, standalone activation, `/proactive`, local TUI ticks, daemon ticks, SleepTool, early wake, terminal focus, automation status, gateway metadata, context-blocking, tests, and docs.
- Placeholder scan: this plan avoids prohibited placeholder markers, deferred implementation bullets, and unowned file references.
- Type consistency: `ProactiveStatus`, `ProactiveSnapshot`, `ProactiveController`, `SleepState`, and `build_tick_payload` are defined in Task 1 and reused consistently by later tasks.
- Architecture consistency: TUI and daemon share services but keep execution ownership in their existing paths; daemon proactive worker continues to queue assistant commands rather than running the model directly.
