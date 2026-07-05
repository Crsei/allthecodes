# Daemon Submit Source Mapping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 daemon assistant worker 把所有 submit 都以 `QuerySource::ReplMainThread` 送进 engine 的问题，让 proactive tick、scheduled task、webhook/channel 等后台来源获得正确的 autonomous / non-interactive 语义。

**Architecture:** 在 `allthecodes-daemon` 的 gateway bridge 边界新增一个窄的 payload-to-`QuerySource` 解析函数；所有 daemon `Submit` 命令继续沿用现有 payload 合约，只在执行 submit 前解析 source 并传给 `QueryEngine::submit_message`。事件日志保留原始 payload source，同时新增解析后的 `query_source` 字段，便于测试和诊断。

**Tech Stack:** Rust, serde_json payload parsing, `allthecodes-daemon` durable worker protocol, `allthecodes-engine::types::config::QuerySource`, Cargo workspace tests.

## Global Constraints

- 进入 Full Build 阶段后按上游完整行为对齐，不以 Lite 范围缩减功能。
- 不改变 daemon command 文件 schema；`payload.source` 继续是 producer 写入的字符串。
- 不把普通 `/api/submit` 或未知 submit source 改成 autonomous；默认仍是 `QuerySource::ReplMainThread`，避免破坏交互式用户提交。
- `proactive_tick` 和 `scheduled_task` 必须映射为 autonomous 且 non-interactive 的 engine source。
- `channel` payload 需要根据 `channel.origin.type` 区分 MCP channel 与 webhook channel：`mcp` 映射为 `ChannelNotification`，`webhook` 映射为 `WebhookEvent`。
- 所有测试使用 `ALLTHECODES_HOME` 临时目录，不读写 `~/.Codex/`。
- 修改后使用仓库要求的 Rust 环境变量运行验证：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## Current State

- `crates/allthecodes-engine/src/types/config.rs` 已定义 `QuerySource::ProactiveTick`、`ScheduledTask`、`WebhookEvent`、`ChannelNotification`。
- `QuerySource::is_autonomous()` 已把 proactive、scheduled、webhook、channel 视为 autonomous。
- `QuerySource::is_non_interactive()` 已把 proactive、scheduled、webhook、channel 视为 non-interactive。
- `crates/allthecodes-daemon/src/tick.rs` 已生成 `payload.source = "proactive_tick"`。
- `crates/allthecodes-daemon/src/scheduler_loop.rs` 已生成 `payload.source = "scheduled_task"`。
- `crates/allthecodes-daemon/src/channels.rs` 已生成 `payload.source = "channel"`，并在 `payload.channel.origin.type` 中记录 `mcp` 或 `webhook`。
- `crates/allthecodes-daemon/src/routes.rs` 的 `/api/submit` 已生成 `payload.source = "http"`。
- `crates/allthecodes-daemon/src/gateway_bridge.rs` 的 `AssistantWorkerRuntime::execute_submit()` 目前固定调用：

```rust
self.engine
    .submit_message(text, QuerySource::ReplMainThread);
```

这会丢失 producer 已写入 payload 的 source 信息。

---

## File Changes

### Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`

Responsibility:

- 解析 daemon submit payload 的 source。
- 把解析出的 `QuerySource` 传给 `QueryEngine::submit_message`。
- 在 `submit_started` daemon event 中记录解析后的 `query_source` 诊断字段。
- 添加 focused 单元测试，证明所有已知 daemon submit producer 的 source 被正确映射。

### Modify: `crates/allthecodes-engine/src/types/config.rs`

Responsibility:

- 添加 tests 固化 autonomous / non-interactive source 行为，避免后续 enum 维护破坏 proactive 语义。
- 不改 production enum 行为，除非测试暴露当前实现与计划不一致。

No new files are required.

---

## Source Mapping Contract

Implement this exact mapping in `gateway_bridge.rs`:

| Payload evidence | Engine source |
| --- | --- |
| `payload.source == "proactive_tick"` | `QuerySource::ProactiveTick` |
| `payload.source == "scheduled_task"` | `QuerySource::ScheduledTask` |
| `payload.source == "webhook_event"` | `QuerySource::WebhookEvent` |
| `payload.source == "channel_notification"` | `QuerySource::ChannelNotification` |
| `payload.source == "channel"` and `payload.channel.origin.type == "webhook"` | `QuerySource::WebhookEvent` |
| `payload.source == "channel"` and `payload.channel.origin.type == "mcp"` | `QuerySource::ChannelNotification` |
| `payload.source == "channel"` with missing/unknown origin | `QuerySource::ChannelNotification` |
| `payload.source == "http"` | `QuerySource::ReplMainThread` |
| missing/unknown `payload.source` | `QuerySource::ReplMainThread` |

Do not map plain gateway submit payloads to autonomous unless they carry one of the source values above.

---

## Implementation Tasks

### Task 1: Add Pure Source Mapping Tests

**Files:**

- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`

**Interfaces:**

- Consumes: `serde_json::Value`, existing `QuerySource`.
- Produces: private helper `fn query_source_from_submit_payload(payload: &Value) -> QuerySource`.

- [ ] **Step 1: Add failing unit tests in `gateway_bridge.rs`**

Add these tests inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn submit_payload_source_maps_proactive_tick() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "source": "proactive_tick" })),
        QuerySource::ProactiveTick
    );
}

#[test]
fn submit_payload_source_maps_scheduled_task() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "source": "scheduled_task" })),
        QuerySource::ScheduledTask
    );
}

#[test]
fn submit_payload_source_maps_explicit_webhook_event() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "source": "webhook_event" })),
        QuerySource::WebhookEvent
    );
}

#[test]
fn submit_payload_source_maps_explicit_channel_notification() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "source": "channel_notification" })),
        QuerySource::ChannelNotification
    );
}

#[test]
fn submit_payload_source_maps_mcp_channel_notification() {
    assert_eq!(
        query_source_from_submit_payload(&json!({
            "source": "channel",
            "channel": {
                "origin": { "type": "mcp", "server_name": "slack-mcp" }
            }
        })),
        QuerySource::ChannelNotification
    );
}

#[test]
fn submit_payload_source_maps_webhook_channel_event() {
    assert_eq!(
        query_source_from_submit_payload(&json!({
            "source": "channel",
            "channel": {
                "origin": { "type": "webhook", "endpoint": "/hooks/github" }
            }
        })),
        QuerySource::WebhookEvent
    );
}

#[test]
fn submit_payload_source_maps_unknown_channel_as_channel_notification() {
    assert_eq!(
        query_source_from_submit_payload(&json!({
            "source": "channel",
            "channel": {
                "origin": { "type": "other" }
            }
        })),
        QuerySource::ChannelNotification
    );
}

#[test]
fn submit_payload_source_keeps_http_interactive() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "source": "http" })),
        QuerySource::ReplMainThread
    );
}

#[test]
fn submit_payload_source_keeps_missing_source_interactive() {
    assert_eq!(
        query_source_from_submit_payload(&json!({ "text": "hello" })),
        QuerySource::ReplMainThread
    );
}
```

- [ ] **Step 2: Run the tests and verify they fail to compile**

Run:

```bash
cargo test -p allthecodes-daemon submit_payload_source_maps_ -- --nocapture
```

Expected: compile failure mentioning `query_source_from_submit_payload` is not found.

- [ ] **Step 3: Add the private helper**

Add this helper near `payload_string(...)` in `gateway_bridge.rs`:

```rust
fn query_source_from_submit_payload(payload: &Value) -> QuerySource {
    match payload.get("source").and_then(Value::as_str) {
        Some("proactive_tick") => QuerySource::ProactiveTick,
        Some("scheduled_task") => QuerySource::ScheduledTask,
        Some("webhook_event") => QuerySource::WebhookEvent,
        Some("channel_notification") => QuerySource::ChannelNotification,
        Some("channel") => query_source_from_channel_payload(payload),
        Some("http") | Some("worker") | None => QuerySource::ReplMainThread,
        Some(_) => QuerySource::ReplMainThread,
    }
}

fn query_source_from_channel_payload(payload: &Value) -> QuerySource {
    match payload
        .get("channel")
        .and_then(|channel| channel.get("origin"))
        .and_then(|origin| origin.get("type"))
        .and_then(Value::as_str)
    {
        Some("webhook") => QuerySource::WebhookEvent,
        Some("mcp") | None => QuerySource::ChannelNotification,
        Some(_) => QuerySource::ChannelNotification,
    }
}
```

- [ ] **Step 4: Run the focused mapping tests**

Run:

```bash
cargo test -p allthecodes-daemon submit_payload_source_maps_ -- --nocapture
```

Expected: all `submit_payload_source_maps_*` tests pass.

- [ ] **Step 5: Commit Task 1**

```bash
git add -A -- crates/allthecodes-daemon/src/gateway_bridge.rs
git commit -m "test: cover daemon submit source mapping"
```

---

### Task 2: Wire Mapped Source Into Engine Submit

**Files:**

- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`

**Interfaces:**

- Consumes: `query_source_from_submit_payload(payload: &Value) -> QuerySource`.
- Produces: `AssistantWorkerRuntime::execute_submit()` passes the mapped `QuerySource` into `QueryEngine::submit_message`.

- [ ] **Step 1: Add a failing event-level regression test**

Add this async test in `gateway_bridge.rs` tests:

```rust
#[tokio::test]
#[serial]
async fn execute_submit_records_mapped_query_source_for_proactive_tick() {
    let temp = tempfile::tempdir().unwrap();
    let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
    let runtime = test_runtime(temp.path());
    let command = submit_command(json!({
        "text": "<tick_tag>\nLocal time: 2026-07-05 12:00:00\n</tick_tag>",
        "message_id": "proactive-tick-test",
        "source": "proactive_tick"
    }));

    runtime
        .execute_submit("assistant-session-1", &command)
        .await
        .unwrap();

    let events = crate::protocol_store()
        .read_worker_events("assistant-session-1")
        .unwrap();
    let submit_started = events
        .iter()
        .find(|event| event.event_type == "submit_started")
        .expect("submit_started event should be recorded");

    assert_eq!(submit_started.data["source"], "proactive_tick");
    assert_eq!(submit_started.data["query_source"], "proactive_tick");
}
```

The no-provider test runtime is acceptable here: `execute_submit()` still starts the stream, receives an error result, and records submit events without making a network call.

- [ ] **Step 2: Run the new test and verify it fails**

Run:

```bash
cargo test -p allthecodes-daemon execute_submit_records_mapped_query_source_for_proactive_tick -- --nocapture
```

Expected: failure because `submit_started.data["query_source"]` is missing.

- [ ] **Step 3: Parse source once in `execute_submit()`**

In `AssistantWorkerRuntime::execute_submit()`, after `message_id` is resolved and before `submit_started` is appended, add:

```rust
let query_source = query_source_from_submit_payload(&command.payload);
let query_source_label = query_source.as_label();
```

- [ ] **Step 4: Add parsed source to `submit_started` event**

Change the `submit_started` data to include both raw source and parsed source:

```rust
json!({
    "message_id": message_id,
    "source": command.payload.get("source").cloned().unwrap_or_else(|| json!("worker")),
    "query_source": query_source_label,
    "gateway": command.payload.get("gateway").cloned(),
})
```

- [ ] **Step 5: Pass the mapped source into engine**

Replace the fixed call:

```rust
let stream = self
    .engine
    .submit_message(text, QuerySource::ReplMainThread);
```

with:

```rust
let stream = self.engine.submit_message(text, query_source);
```

This is the behavior change that fixes proactive mode.

- [ ] **Step 6: Run the regression test**

Run:

```bash
cargo test -p allthecodes-daemon execute_submit_records_mapped_query_source_for_proactive_tick -- --nocapture
```

Expected: pass.

- [ ] **Step 7: Commit Task 2**

```bash
git add -A -- crates/allthecodes-daemon/src/gateway_bridge.rs
git commit -m "fix: pass daemon submit source to engine"
```

---

### Task 3: Lock Producer Payload Contracts Against the Mapping

**Files:**

- Modify: `crates/allthecodes-daemon/src/gateway_bridge.rs`
- Modify only if needed: `crates/allthecodes-daemon/src/channels.rs`
- Modify only if needed: `crates/allthecodes-daemon/src/tick.rs`
- Modify only if needed: `crates/allthecodes-daemon/src/scheduler_loop.rs`

**Interfaces:**

- Consumes: existing producer payload helpers:
  - `tick::enqueue_proactive_tick_once(...)`
  - `scheduler_loop::enqueue_due_scheduled_task_once()`
  - `channels::channel_submit_payload(...)`
- Produces: tests that prove producer payloads are compatible with `query_source_from_submit_payload`.

- [ ] **Step 1: Add direct producer compatibility tests for tick and channel payloads**

Add this test in `gateway_bridge.rs` tests:

```rust
#[test]
#[serial]
fn daemon_submit_producer_payloads_resolve_to_expected_query_sources() {
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

    let tick = crate::tick::enqueue_proactive_tick_once(chrono::Local::now(), false)
        .unwrap()
        .expect("proactive tick should enqueue");
    assert_eq!(
        query_source_from_submit_payload(&tick.payload),
        QuerySource::ProactiveTick
    );

    let mcp_event = crate::channels::ChannelEvent {
        source: "slack".into(),
        sender: Some("alice".into()),
        content: "triage incident".into(),
        meta: serde_json::Value::Null,
        origin: crate::channels::ChannelOrigin::Mcp {
            server_name: "slack-mcp".into(),
        },
    };
    assert_eq!(
        query_source_from_submit_payload(&crate::channels::channel_submit_payload(&mcp_event)),
        QuerySource::ChannelNotification
    );

    let webhook_event = crate::channels::ChannelEvent {
        source: "github".into(),
        sender: None,
        content: "pull request opened".into(),
        meta: serde_json::Value::Null,
        origin: crate::channels::ChannelOrigin::Webhook {
            endpoint: "/hooks/github".into(),
        },
    };
    assert_eq!(
        query_source_from_submit_payload(&crate::channels::channel_submit_payload(&webhook_event)),
        QuerySource::WebhookEvent
    );
}
```

- [ ] **Step 2: Run the producer compatibility test**

Run:

```bash
cargo test -p allthecodes-daemon daemon_submit_producer_payloads_resolve_to_expected_query_sources -- --nocapture
```

Expected: pass after Task 1. If it fails because `channel_submit_payload` is not visible to sibling tests, change it from:

```rust
fn channel_submit_payload(event: &ChannelEvent) -> Value
```

to:

```rust
pub(crate) fn channel_submit_payload(event: &ChannelEvent) -> Value
```

No visibility change is needed if it is already `pub(crate)`, which is the current expected state.

- [ ] **Step 3: Keep scheduler compatibility covered by its producer contract**

Do not make `query_source_from_submit_payload(...)` public only so `scheduler_loop.rs` tests can call it. Scheduler compatibility is covered by two independent assertions:

```rust
assert_eq!(command.payload["source"], "scheduled_task");
```

and Task 1's pure mapping test:

```rust
assert_eq!(
    query_source_from_submit_payload(&json!({ "source": "scheduled_task" })),
    QuerySource::ScheduledTask
);
```

- [ ] **Step 4: Run existing producer tests**

Run:

```bash
cargo test -p allthecodes-daemon tick::tests::proactive_tick_enqueues_assistant_submit_command -- --nocapture
cargo test -p allthecodes-daemon scheduler_loop::tests::scheduler_enqueues_due_task -- --nocapture
cargo test -p allthecodes-daemon channels::tests::accepted_channel_event_queues_assistant_submit_command -- --nocapture
```

Expected: all pass. If the exact scheduler test name differs, run:

```bash
cargo test -p allthecodes-daemon scheduler_loop::tests:: -- --nocapture
```

- [ ] **Step 5: Commit Task 3**

```bash
git add -A -- crates/allthecodes-daemon/src/gateway_bridge.rs crates/allthecodes-daemon/src/channels.rs crates/allthecodes-daemon/src/tick.rs crates/allthecodes-daemon/src/scheduler_loop.rs
git commit -m "test: lock daemon submit source producers"
```

---

### Task 4: Guard Engine QuerySource Semantics

**Files:**

- Modify: `crates/allthecodes-engine/src/types/config.rs`

**Interfaces:**

- Consumes: existing `QuerySource::{is_autonomous,is_non_interactive,as_str,as_label}`.
- Produces: tests that fail if proactive/scheduled/webhook/channel stop being autonomous or non-interactive.

- [ ] **Step 1: Add focused tests in `config.rs`**

Add these tests to the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn autonomous_sources_are_non_interactive() {
    let sources = [
        QuerySource::ProactiveTick,
        QuerySource::ScheduledTask,
        QuerySource::WebhookEvent,
        QuerySource::ChannelNotification,
    ];

    for source in sources {
        assert!(source.is_autonomous(), "{source:?} should be autonomous");
        assert!(
            source.is_non_interactive(),
            "{source:?} should be non-interactive"
        );
    }
}

#[test]
fn interactive_sources_are_not_autonomous() {
    let sources = [
        QuerySource::ReplMainThread,
        QuerySource::Sdk,
        QuerySource::Compact,
        QuerySource::SessionMemory,
    ];

    for source in sources {
        assert!(!source.is_autonomous(), "{source:?} should not be autonomous");
    }
}

#[test]
fn autonomous_source_labels_match_daemon_payload_contract() {
    assert_eq!(QuerySource::ProactiveTick.as_label(), "proactive_tick");
    assert_eq!(QuerySource::ScheduledTask.as_label(), "scheduled_task");
    assert_eq!(QuerySource::WebhookEvent.as_label(), "webhook_event");
    assert_eq!(
        QuerySource::ChannelNotification.as_label(),
        "channel_notification"
    );
}
```

- [ ] **Step 2: Run engine config tests**

Run:

```bash
cargo test -p allthecodes-engine types::config::tests::autonomous_sources_are_non_interactive -- --nocapture
cargo test -p allthecodes-engine types::config::tests::interactive_sources_are_not_autonomous -- --nocapture
cargo test -p allthecodes-engine types::config::tests::autonomous_source_labels_match_daemon_payload_contract -- --nocapture
```

Expected: all pass with current engine behavior.

- [ ] **Step 3: Commit Task 4**

```bash
git add -A -- crates/allthecodes-engine/src/types/config.rs
git commit -m "test: guard autonomous query source semantics"
```

---

### Task 5: Run Integration Verification

**Files:**

- No source files changed in this task unless a verification failure requires a fix.

**Interfaces:**

- Consumes: Task 1-4 changes.
- Produces: verified local build/test result with no new warnings.

- [ ] **Step 1: Run daemon crate tests**

```bash
cargo test -p allthecodes-daemon -- --nocapture
```

Expected: all daemon tests pass.

- [ ] **Step 2: Run engine crate tests touched by source semantics**

```bash
cargo test -p allthecodes-engine types::config::tests:: -- --nocapture
```

Expected: all `types::config` tests pass.

- [ ] **Step 3: Run workspace check**

```bash
cargo check --workspace
```

Expected: check succeeds. Existing warnings are not acceptable if they are introduced by this change; remove unused imports or dead code from these tasks before committing.

- [ ] **Step 4: Inspect diff for scope**

```bash
git diff -- crates/allthecodes-daemon/src/gateway_bridge.rs crates/allthecodes-engine/src/types/config.rs
```

Expected:

- `gateway_bridge.rs` contains the mapping helper, tests, and `execute_submit()` passes the mapped `QuerySource`.
- `config.rs` contains only semantic guard tests unless production code genuinely needed adjustment.
- No unrelated formatting churn.

- [ ] **Step 5: Final commit if Task 5 required fixes**

If Task 5 required any fix after earlier commits:

```bash
git add -A -- crates/allthecodes-daemon/src/gateway_bridge.rs crates/allthecodes-engine/src/types/config.rs
git commit -m "fix: stabilize daemon source mapping tests"
```

If no fixes were needed, do not create an empty commit.

---

## Acceptance Criteria

- `proactive_tick` submit commands enter engine as `QuerySource::ProactiveTick`.
- `scheduled_task` submit commands enter engine as `QuerySource::ScheduledTask`.
- MCP channel submit commands enter engine as `QuerySource::ChannelNotification`.
- webhook channel submit commands enter engine as `QuerySource::WebhookEvent`.
- `/api/submit` commands with `source = "http"` remain `QuerySource::ReplMainThread`.
- Unknown source strings remain `QuerySource::ReplMainThread`.
- `submit_started` daemon events include both:
  - raw `source` from payload;
  - parsed `query_source` label passed to engine.
- Focused daemon and engine tests pass.
- `cargo check --workspace` passes without new warnings.

---

## Self-Review Checklist

- [ ] The plan only addresses daemon submit source mapping; terminal focus, plan-mode blocking, and sleep writer duplication remain separate issues.
- [ ] All paths are under this repository and no persistent state path points at `~/.Codex/`.
- [ ] Every source mapping has a test.
- [ ] The plan preserves existing interactive behavior for `http`, missing source, and unknown source.
- [ ] The implementation does not introduce new public daemon command schema fields.
- [ ] The implementation does not require network access.
