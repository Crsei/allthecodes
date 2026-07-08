# Teams Swarm Coordinator Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐 `claude-code-bun/docs/agent/coordinator-and-swarm.mdx` 描述的 Coordinator Mode 与 Agent Teams/Swarm 协作机制，同时保留 allthecodes 的路径隔离和 Rust in-process teammate 决策。

**Architecture:** 以现有 `allthecodes-teams`、`allthecodes-tasks`、`allthecodes-tools`、`allthecodes-engine` 为主线，不引入第二套 agent runtime。Coordinator 是受限工具策略 + 强系统提示词 + worker notification 协议；Swarm 是共享 task list + mailbox + in-process teammates + hook 事件。上游 `.claude` 路径和 `CLAUDE_CODE_*` 环境变量在 allthecodes 中作为兼容 alias 处理，主存储仍落在 `~/.allthecodes` / `ALLTHECODES_HOME`。

**Tech Stack:** Rust 1.91.1, tokio, serde/serde_json, ratatui/crossterm TUI, existing `allthecodes-config`, `allthecodes-teams`, `allthecodes-tasks`, `allthecodes-tools`, `allthecodes-engine`, `allthecodes-session`, `allthecodes-startup`, and `allthecodes` crates.

## Global Constraints

- 当前分支是 Full Build；不得以 Lite 简化为理由跳过 Coordinator/Swarm 行为。
- 持久化路径继续使用 `~/.allthecodes/` 或 `ALLTHECODES_HOME`，不得写入 `~/.claude/`。
- 上游 `CLAUDE_CODE_*` 环境变量只作为兼容 alias；allthecodes 原生变量优先级更高。
- `ALLTHECODES_COORDINATOR_MODE=1` 与 `/coordinator start` 必须同时启用 Coordinator 和 Agent Teams。
- `FEATURE_AGENT_TEAMS=1` / `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS=1` 启用 Swarm；`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` 作为兼容 alias。
- Coordinator top-level 会话不得获得文件读写或命令执行工具。
- Coordinator worker 不得获得 `SendMessage`、`TeamSpawn`、`Agent`/`Task`、`TaskStop`、TeamCreate/TeamDelete 等编排工具。
- `CLAUDE_CODE_SIMPLE=1` 兼容模式下，Coordinator worker 只获得 `Bash`、`Read`、`Edit`。
- Scratchpad 若启用，只能落在 allthecodes 数据目录下，并以额外工作目录方式授权给 worker。
- 外部 tmux/iTerm2 pane backend 仍是 intentional crop；本计划只补齐 in-process backend 的完整语义。
- 任何新增 hook 或 message 协议必须有单元测试覆盖 payload 结构。
- 每个任务结束时运行对应 focused tests；最终运行 workspace release build。
- 使用仓库指定 Cargo 环境：

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
```

---

## Current State

- Coordinator gate 已存在：`crates/allthecodes-teams/src/coordinator.rs` 和 `crates/allthecodes-config/src/features.rs`。
- `/coordinator start|stop|status` 已存在，并会在 start 时启用 Agent Teams。
- Coordinator tool policy 已接入 `crates/allthecodes-startup/src/tool_registry.rs`，但当前暴露工具集比上游更宽。
- Agent Teams 已有 `/team create|spawn|send|kill|leave|delete`、`TeamSpawn`、`SendMessage`、mailbox、in-process runner。
- TaskList/TaskUpdate/TaskStop、claim file lock、TaskCreated/TaskCompleted hook 已存在。
- Teammate idle notification 已通过 mailbox 发送，但没有触发 `TeammateIdle` hook。
- `<task-notification>` 只有 UI 路由识别，没有 worker 完成通知生成链路。
- Scratchpad / `tengu_scratch` 未找到实现。
- `CLAUDE_CODE_SIMPLE` worker 工具简化未找到实现。
- Agent Teams gate 不是严格门控：`TeamSpawn` / `SendMessage` 主要受 `ALLTHECODES_MULTI_AGENT_V2` 影响，`/team create` 可在未开启 feature 时激活 session。

---

## Target Runtime Contract

| Scenario | Expected behavior |
| --- | --- |
| `CLAUDE_CODE_COORDINATOR_MODE=1 allthecodes` | 兼容启用 Coordinator，但数据路径仍为 allthecodes |
| `ALLTHECODES_COORDINATOR_MODE=1 allthecodes` | 原生启用 Coordinator 和 Agent Teams |
| `/coordinator start review` | 创建或绑定 team `review`，设置 session chat mode 为 `coordinator` |
| resume coordinator session | 自动恢复 Coordinator feature override、Coordinator prompt 和 tool policy |
| Coordinator visible tools | `Agent`/`Task` alias, `SendMessage`/alias, `TaskStop`, `subscribe_pr_activity` |
| Coordinator hidden tools | `Bash`, `Read`, `Edit`, `Write`, `TaskList`, `TeamSpawn`, `ListAgents`, `FollowupTask`, `WaitAgent`, `CloseAgent`, `DelegateTask` |
| Coordinator worker full mode | read/search/edit/bash/task progress tools, no message/spawn/stop/internal tools |
| Coordinator worker simple mode | only `Bash`, `Read`, `Edit` |
| Worker completion | parent receives `<task-notification>` with task-id/status/summary/result/usage |
| Scratchpad enabled | workers receive a shared writable scratchpad path under allthecodes data dir |
| `FEATURE_AGENT_TEAMS=1` | `/team` and team tools are usable |
| feature off | `/team create/spawn/send/delete` and `TeamSpawn`/`SendMessage` return feature gate text or are hidden from model-visible schema |
| teammate idle | mailbox idle notification is sent and `TeammateIdle` hook runs with structured payload |
| task claim race | only one claimant owns the task; loser gets `already_claimed` |
| abnormal teammate exit | unfinished owned tasks are unassigned and visible to lead through TaskList |

---

## File Structure

- Modify `crates/allthecodes-config/src/features.rs`: add upstream-compatible env aliases for coordinator and agent teams.
- Create `crates/allthecodes-teams/src/session_mode.rs`: coordinator session mode persistence/sync helpers.
- Modify `crates/allthecodes-teams/src/coordinator.rs`: replace short prompt with full Coordinator prompt builder and policy values.
- Modify `crates/allthecodes-teams/src/lib.rs`: export new modules and helpers.
- Modify `crates/allthecodes-commands/src/coordinator.rs`: persist/clear coordinator session mode and bind team context.
- Modify `crates/allthecodes-session/src/storage.rs`: expose typed helper for coordinator chat mode if existing chat-mode override is not sufficient.
- Modify `crates/allthecodes-engine/src/types/app_state.rs`: add `coordinator_mode` projection if runtime state must cross tool boundaries.
- Modify `crates/allthecodes-tools/src/tool.rs`: mirror `coordinator_mode` in `ToolAppState` when added to `AppState`.
- Modify `crates/allthecodes-types/src/tool_metadata.rs`: tighten Coordinator and CoordinatorWorker visibility.
- Modify `crates/allthecodes-tools/src/registry.rs`: add `CoordinatorWorkerSimple` policy or explicit simple allowlist.
- Modify `crates/allthecodes-startup/src/tool_registry.rs`: update policy tests and active-session tool selection.
- Modify `crates/allthecodes-teams/src/runner.rs`: choose simple/full worker tool policy, inject scratchpad, emit TeammateIdle hook.
- Create `crates/allthecodes-teams/src/scratchpad.rs`: scratchpad gate, path, prompt fragment, permission context helper.
- Create `crates/allthecodes-teams/src/task_notification.rs`: XML task notification builder/parser tests.
- Modify `crates/allthecodes-engine/src/agent/dispatch.rs`: wrap sync worker completion for Coordinator Mode.
- Modify `crates/allthecodes-engine/src/agent/supervisor.rs`: wrap background worker completion for Coordinator Mode.
- Modify `crates/allthecodes-engine/src/query/loop_impl.rs`: inject background `<task-notification>` as user-role message when parent is coordinator.
- Modify `crates/allthecodes-teams/src/team_spawn.rs`: require Agent Teams/Coordinator gate for model-visible team spawn.
- Modify `crates/allthecodes-teams/src/send_message.rs`: require Agent Teams/Coordinator gate for model-visible send.
- Create `crates/allthecodes-teams/src/team_tools.rs`: `TeamCreate` and `TeamDelete` tools if model-visible Swarm team creation is required.
- Modify `crates/allthecodes-startup/src/tool_registry.rs`: register `TeamCreate`/`TeamDelete` behind Agent Teams.
- Modify `crates/allthecodes-tasks/src/lists.rs`: lock task-list id priority and compatibility aliases with tests.
- Modify `crates/allthecodes-tools/src/tasks/mod.rs`: extend hook payload tests and claim-race tests if coverage is missing.
- Modify `development/hided_features/current-hidden-features.md`: update Coordinator/Agent Teams status after implementation.
- Modify `development/archive/IMPLEMENTATION_GAPS.md`: keep pane backend intentional crop and close completed Coordinator/Swarm gaps.
- Modify `docs/WORK_STATUS.md`: add final source-verified completion note.

---

## Implementation Tasks

### Task 1: Feature Gate Compatibility And Session Mode Sync

**Files:**
- Modify: `crates/allthecodes-config/src/features.rs`
- Create: `crates/allthecodes-teams/src/session_mode.rs`
- Modify: `crates/allthecodes-teams/src/lib.rs`
- Modify: `crates/allthecodes-commands/src/coordinator.rs`
- Modify: `crates/allthecodes-session/src/storage.rs`
- Test: `crates/allthecodes-config/src/features.rs`
- Test: `crates/allthecodes-teams/src/session_mode.rs`

**Interfaces:**
- Produces: `FeatureFlags.agent_teams` reads `FEATURE_AGENT_TEAMS`, `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS`, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`.
- Produces: `FeatureFlags.coordinator` reads `ALLTHECODES_COORDINATOR_MODE`, `CLAUDE_CODE_COORDINATOR_MODE`.
- Produces: `allthecodes_teams::session_mode::set_session_coordinator_mode(session_id, cwd, enabled) -> anyhow::Result<()>`.
- Produces: `allthecodes_teams::session_mode::match_session_mode(session_id, cwd) -> anyhow::Result<bool>`.
- Consumes: existing `allthecodes_session::storage::set_session_chat_mode_override(...)`.

- [ ] **Step 1: Add failing feature alias tests**

Add tests to `crates/allthecodes-config/src/features.rs`:

```rust
#[test]
fn agent_teams_reads_upstream_compat_env_var() {
    let f = flags(&[("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1")]);
    assert!(f.agent_teams);
    assert!(f.is_enabled(Feature::AgentTeams));
}

#[test]
fn coordinator_reads_upstream_compat_env_var() {
    let f = flags(&[("CLAUDE_CODE_COORDINATOR_MODE", "true")]);
    assert!(f.coordinator);
    assert!(f.is_enabled(Feature::Coordinator));
}

#[test]
fn allthecodes_env_takes_precedence_when_compat_env_is_disabled() {
    let f = flags(&[
        ("ALLTHECODES_COORDINATOR_MODE", "1"),
        ("CLAUDE_CODE_COORDINATOR_MODE", "0"),
    ]);
    assert!(f.coordinator);
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```bash
cargo test -p allthecodes-config coordinator_reads_upstream_compat_env_var -- --nocapture
cargo test -p allthecodes-config agent_teams_reads_upstream_compat_env_var -- --nocapture
```

Expected before implementation: both compatibility alias tests fail.

- [ ] **Step 3: Implement alias reads**

In `FeatureFlags::from_env_iter`, change reads to:

```rust
let agent_teams = read("FEATURE_AGENT_TEAMS")
    || read("ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS")
    || read("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS");
let coordinator = read("ALLTHECODES_COORDINATOR_MODE")
    || read("CLAUDE_CODE_COORDINATOR_MODE");
```

Keep descriptors pointing at allthecodes primary env vars.

- [ ] **Step 4: Add session mode helper tests**

Create `crates/allthecodes-teams/src/session_mode.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn coordinator_session_mode_roundtrip_uses_chat_mode_override() {
        let temp = tempfile::tempdir().unwrap();
        let _home = allthecodes_config::test_support::EnvGuard::set_path("ALLTHECODES_HOME", temp.path());

        set_session_coordinator_mode("sess-coord", "/repo", true).unwrap();
        assert!(match_session_mode("sess-coord", "/repo").unwrap());
        assert!(crate::coordinator::is_coordinator_mode_enabled());

        set_session_coordinator_mode("sess-coord", "/repo", false).unwrap();
        assert!(!match_session_mode("sess-coord", "/repo").unwrap());
        assert!(!crate::coordinator::is_coordinator_mode_enabled());
    }
}
```

If `allthecodes_config::test_support::EnvGuard` is unavailable in this crate, add a local `EnvGuard` in the test module following the existing style in `mailbox.rs`.

- [ ] **Step 5: Implement session mode helpers**

Use existing chat mode metadata:

```rust
pub const COORDINATOR_CHAT_MODE: &str = "coordinator";

pub fn set_session_coordinator_mode(session_id: &str, cwd: &str, enabled: bool) -> anyhow::Result<()> {
    let value = enabled.then_some(COORDINATOR_CHAT_MODE);
    allthecodes_session::storage::set_session_chat_mode_override(session_id, value, cwd)?;
    crate::coordinator::set_coordinator_mode_enabled(enabled);
    Ok(())
}

pub fn match_session_mode(session_id: &str, _cwd: &str) -> anyhow::Result<bool> {
    let info = allthecodes_session::storage::load_session_info(session_id)?;
    let enabled = info.chat_mode_override.as_deref() == Some(COORDINATOR_CHAT_MODE);
    crate::coordinator::set_coordinator_mode_enabled(enabled);
    Ok(enabled)
}
```

If `load_session_info` returns not-found for new sessions, treat that as `Ok(false)` and leave the feature snapshot unchanged.

- [ ] **Step 6: Wire `/coordinator start|stop`**

In `crates/allthecodes-commands/src/coordinator.rs`:

```rust
crate::session_mode::set_session_coordinator_mode(
    ctx.session_id.as_str(),
    ctx.cwd.to_string_lossy().as_ref(),
    true,
)?;
```

On stop, call the same helper with `false`.

- [ ] **Step 7: Verify**

Run:

```bash
cargo test -p allthecodes-config coordinator_reads_upstream_compat_env_var -- --nocapture
cargo test -p allthecodes-config agent_teams_reads_upstream_compat_env_var -- --nocapture
cargo test -p allthecodes-teams session_mode -- --nocapture
cargo check -p allthecodes-teams
```

Expected: alias gates pass, session mode persists and restores coordinator runtime override.

- [ ] **Step 8: Commit Task 1**

```bash
git add -A -- crates/allthecodes-config/src/features.rs \
              crates/allthecodes-teams/src/session_mode.rs \
              crates/allthecodes-teams/src/lib.rs \
              crates/allthecodes-commands/src/coordinator.rs \
              crates/allthecodes-session/src/storage.rs
git commit -m "feat(teams): sync coordinator session mode"
```

---

### Task 2: Full Coordinator Prompt And Orchestration Contract

**Files:**
- Modify: `crates/allthecodes-teams/src/coordinator.rs`
- Test: `crates/allthecodes-teams/src/coordinator.rs`

**Interfaces:**
- Produces: `coordinator_system_prompt() -> String` containing the full behavior contract.
- Produces: `CoordinatorRunPolicy { max_parallel_workers, max_retry_per_task, stop_after_verification }`.
- Consumes: `is_coordinator_mode_enabled()`.

- [ ] **Step 1: Add prompt coverage tests**

Replace the current narrow prompt test with assertions for the full contract:

```rust
#[test]
fn coordinator_prompt_covers_full_orchestration_contract() {
    let prompt = coordinator_system_prompt();

    for required in [
        "You are the Coordinator",
        "do not read files",
        "do not write files",
        "do not execute shell commands",
        "understand before you delegate",
        "Synthesis is your responsibility",
        "Use Agent",
        "Use SendMessage",
        "Use TaskStop",
        "subscribe_pr_activity",
        "<task-notification>",
        "Scratchpad",
        "Verification",
        "Retry",
        "Cost limits",
        "Stopping conditions",
    ] {
        assert!(prompt.contains(required), "missing prompt clause: {required}");
    }
}
```

- [ ] **Step 2: Implement run policy**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorRunPolicy {
    pub max_parallel_workers: usize,
    pub max_retry_per_task: usize,
    pub stop_after_verification: bool,
}

impl CoordinatorRunPolicy {
    pub fn from_env() -> Self {
        Self {
            max_parallel_workers: read_usize_env("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", 4),
            max_retry_per_task: read_usize_env("ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK", 1),
            stop_after_verification: true,
        }
    }
}
```

Use a private `read_usize_env` that rejects `0` and non-numeric values by returning the default.

- [ ] **Step 3: Replace prompt body**

Keep the function name stable and build a long, explicit prompt. The text must include these sections:

```text
# Coordinator Mode
You are the Coordinator. You own understanding, decomposition, synthesis, verification, and the final answer.

## Tool Boundary
- Do not read files, write files, or execute shell commands yourself.
- Use Agent to start bounded workers.
- Use SendMessage to refine an existing worker's task.
- Use TaskStop to stop stale, duplicate, unsafe, or wrong-direction work.
- Use subscribe_pr_activity when PR review or CI activity matters.

## Understand Before Delegating
...

## Task Decomposition
...

## Scratchpad
...

## Task Notifications
...

## Verification
...

## Retry And Failure Handling
...

## Cost Limits
...

## Stopping Conditions
...
```

The final prompt should be detailed enough that a model can infer:

- start fewer workers than the maximum when tasks are dependent;
- assign exact ownership boundaries;
- ask workers for evidence, not just summaries;
- retry only when the failure mode is understood;
- stop when verification is complete or impossible with a clear blocker.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-teams coordinator_prompt_covers_full_orchestration_contract -- --nocapture
cargo test -p allthecodes-teams coordinator -- --nocapture
```

Expected: prompt tests pass and existing coordinator toggle tests still pass.

- [ ] **Step 5: Commit Task 2**

```bash
git add -A -- crates/allthecodes-teams/src/coordinator.rs
git commit -m "feat(coordinator): expand orchestration prompt"
```

---

### Task 3: Coordinator And Worker Tool Policy Parity

**Files:**
- Modify: `crates/allthecodes-types/src/tool_metadata.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`
- Modify: `crates/allthecodes-startup/src/tool_registry.rs`
- Modify: `crates/allthecodes-teams/src/runner.rs`
- Test: `crates/allthecodes-types/src/tool_metadata.rs`
- Test: `crates/allthecodes-tools/src/registry.rs`
- Test: `crates/allthecodes-startup/src/tool_registry.rs`

**Interfaces:**
- Produces: strict Coordinator allowlist.
- Produces: Coordinator worker internal-tool denylist.
- Produces: simple worker policy for `CLAUDE_CODE_SIMPLE=1`.
- Consumes: `ToolPolicy`.

- [ ] **Step 1: Lock target policies in tests**

In `crates/allthecodes-startup/src/tool_registry.rs`, update tests:

```rust
#[test]
fn coordinator_policy_exposes_only_doc_orchestration_tools() {
    let names = tool_names(get_tools_for_policy(ToolPolicy::Coordinator));

    for allowed in ["Agent", "Task", "SendMessage", "send_message", "TaskStop", "subscribe_pr_activity"] {
        assert!(names.contains(&allowed.to_string()), "{allowed} should be coordinator-visible");
    }

    for forbidden in [
        "Bash", "Read", "Edit", "Write", "TaskList", "TaskUpdate", "TaskOutput",
        "TeamSpawn", "spawn_agent", "ListAgents", "list_agents", "FollowupTask",
        "followup_task", "WaitAgent", "wait_agent", "CloseAgent", "close_agent",
        "DelegateTask", "delegate_task", "unsubscribe_pr_activity",
    ] {
        assert!(!names.contains(&forbidden.to_string()), "{forbidden} should be hidden from coordinator");
    }
}

#[test]
fn worker_policy_removes_internal_orchestration_tools() {
    let names = tool_names(get_tools_for_policy(ToolPolicy::CoordinatorWorker));

    for allowed in ["Read", "Grep", "Glob", "Bash", "Edit", "Write", "TodoWrite", "TaskList", "TaskUpdate", "TaskOutput"] {
        assert!(names.contains(&allowed.to_string()), "{allowed} should be worker-visible");
    }

    for forbidden in ["Agent", "Task", "SendMessage", "send_message", "TeamSpawn", "spawn_agent", "TaskStop"] {
        assert!(!names.contains(&forbidden.to_string()), "{forbidden} should be hidden from worker");
    }
}
```

- [ ] **Step 2: Add simple policy**

In `crates/allthecodes-tools/src/registry.rs`, add:

```rust
pub enum ToolPolicy {
    DefaultAgent,
    Coordinator,
    CoordinatorWorker,
    CoordinatorWorkerSimple,
    InProcessTeammate,
}
```

Update `metadata_allowed_for_policy`:

```rust
ToolPolicy::CoordinatorWorkerSimple => matches!(
    metadata.name,
    "Bash" | "Read" | "Edit"
),
```

Add a test:

```rust
#[test]
fn coordinator_worker_simple_policy_only_allows_bash_read_edit() {
    let names = filter_tools_for_policy(get_all_tools(), ToolPolicy::CoordinatorWorkerSimple)
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect::<Vec<_>>();
    assert!(names.contains(&"Bash".to_string()));
    assert!(names.contains(&"Read".to_string()));
    assert!(names.contains(&"Edit".to_string()));
    assert!(!names.contains(&"Write".to_string()));
    assert!(!names.contains(&"SendMessage".to_string()));
    assert!(!names.contains(&"TaskUpdate".to_string()));
}
```

- [ ] **Step 3: Tighten metadata visibility**

In `tool_metadata.rs`:

- Coordinator visible: `Agent`, `SendMessage`, `TaskStop`, `subscribe_pr_activity`.
- Coordinator worker visible: `Glob`, `Grep`, `Read`, `Bash`, `Edit`, `Write`, `TodoWrite`, `TaskList`, `TaskUpdate`, `TaskOutput`.
- In-process teammate visible remains broader: keep `SendMessage` for teammate-to-teammate direct communication.

Do not add `TeamCreate`/`TeamDelete` to worker visibility.

- [ ] **Step 4: Select simple policy in runner**

In `crates/allthecodes-teams/src/runner.rs`, add:

```rust
fn coordinator_worker_simple_enabled() -> bool {
    ["ALLTHECODES_COORDINATOR_SIMPLE", "CLAUDE_CODE_SIMPLE"]
        .iter()
        .any(|key| std::env::var(key).ok().map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")).unwrap_or(false))
}
```

Update `tool_policy_for_teammate(agent_type)`:

```rust
if agent_type == Some(crate::coordinator::WORKER_AGENT_TYPE) {
    if coordinator_worker_simple_enabled() {
        ToolPolicy::CoordinatorWorkerSimple
    } else {
        ToolPolicy::CoordinatorWorker
    }
} else {
    ToolPolicy::InProcessTeammate
}
```

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-types tool_metadata -- --nocapture
cargo test -p allthecodes-tools coordinator_worker_simple_policy_only_allows_bash_read_edit -- --nocapture
cargo test -p allthecodes-startup coordinator_policy_exposes_only_doc_orchestration_tools -- --nocapture
cargo test -p allthecodes-startup worker_policy_removes_internal_orchestration_tools -- --nocapture
cargo check -p allthecodes-startup
```

Expected: Coordinator has no direct read/write/run tools; worker no longer has `SendMessage`; simple mode is locked.

- [ ] **Step 6: Commit Task 3**

```bash
git add -A -- crates/allthecodes-types/src/tool_metadata.rs \
              crates/allthecodes-tools/src/registry.rs \
              crates/allthecodes-startup/src/tool_registry.rs \
              crates/allthecodes-teams/src/runner.rs
git commit -m "feat(coordinator): tighten tool policies"
```

---

### Task 4: Scratchpad Gate And Worker Injection

**Files:**
- Create: `crates/allthecodes-teams/src/scratchpad.rs`
- Modify: `crates/allthecodes-teams/src/lib.rs`
- Modify: `crates/allthecodes-teams/src/coordinator.rs`
- Modify: `crates/allthecodes-teams/src/runner.rs`
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`
- Test: `crates/allthecodes-teams/src/scratchpad.rs`
- Test: `crates/allthecodes-engine/src/agent/mod.rs`

**Interfaces:**
- Produces: `ScratchpadContext { path: PathBuf, prompt: String }`.
- Produces: `scratchpad_context(team_name, session_id) -> Option<ScratchpadContext>`.
- Produces: `apply_scratchpad_permissions(context: &mut ToolPermissionContext, path: &Path)`.
- Consumes: allthecodes data dir, active team/session id.

- [ ] **Step 1: Add scratchpad tests**

Create tests:

```rust
#[test]
#[serial_test::serial]
fn scratchpad_disabled_by_default() {
    let _guard = EnvGuard::remove("ALLTHECODES_COORDINATOR_SCRATCHPAD");
    assert!(scratchpad_context("team-a", "session-a").is_none());
}

#[test]
#[serial_test::serial]
fn scratchpad_path_is_under_allthecodes_home() {
    let temp = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
    let _gate = EnvGuard::set("ALLTHECODES_COORDINATOR_SCRATCHPAD", "1");

    let ctx = scratchpad_context("Team A", "session-123").expect("scratchpad enabled");
    assert!(ctx.path.starts_with(temp.path()));
    assert!(ctx.path.ends_with("scratchpads/team-a-session-123"));
    assert!(ctx.prompt.contains("Scratchpad"));
    assert!(ctx.prompt.contains(ctx.path.to_string_lossy().as_ref()));
}
```

- [ ] **Step 2: Implement gate aliases**

Treat these as truthy gates:

- `ALLTHECODES_COORDINATOR_SCRATCHPAD`
- `TENGU_SCRATCH`
- `CLAUDE_CODE_TENGU_SCRATCH`

Use sanitized team/session names and create the directory before returning context.

- [ ] **Step 3: Inject into coordinator prompt**

In `coordinator_system_prompt`, include Scratchpad rules unconditionally:

```text
When the runtime provides a Scratchpad path, use it as shared cross-worker memory.
Ask workers to write durable findings there when another worker will need them.
Do not use Scratchpad as a substitute for final synthesis.
```

At runtime, append the actual path through team/worker context rather than hardcoding one in the static prompt.

- [ ] **Step 4: Grant worker access**

When spawning coordinator workers, call:

```rust
if let Some(scratchpad) = crate::scratchpad::scratchpad_context(&identity.team_name, &identity.parent_session_id) {
    crate::scratchpad::apply_scratchpad_permissions(
        &mut app_state.tool_permission_context,
        &scratchpad.path,
    );
    system_context.insert("scratchpad".into(), scratchpad.prompt);
}
```

Use the closest existing system-context injection point in `runner.rs` or `build_child_config`. The implementation must avoid granting the coordinator lead direct file tools; only worker permission contexts get the writable directory.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-teams scratchpad -- --nocapture
cargo test -p allthecodes-engine scratchpad -- --nocapture
cargo check -p allthecodes-teams
cargo check -p allthecodes-engine
```

Expected: scratchpad disabled by default, enabled path under allthecodes home, worker permission context includes the directory only when gate is on.

- [ ] **Step 6: Commit Task 4**

```bash
git add -A -- crates/allthecodes-teams/src/scratchpad.rs \
              crates/allthecodes-teams/src/lib.rs \
              crates/allthecodes-teams/src/coordinator.rs \
              crates/allthecodes-teams/src/runner.rs \
              crates/allthecodes-engine/src/agent/mod.rs
git commit -m "feat(coordinator): add worker scratchpad"
```

---

### Task 5: `<task-notification>` Worker Completion Protocol

**Files:**
- Create: `crates/allthecodes-teams/src/task_notification.rs`
- Modify: `crates/allthecodes-teams/src/lib.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`
- Modify: `crates/allthecodes-engine/src/query/loop_impl.rs`
- Modify: `crates/allthecodes/src/ui/messages/user_agent_notification_message.rs`
- Test: `crates/allthecodes-teams/src/task_notification.rs`
- Test: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Test: `crates/allthecodes-engine/src/agent/supervisor.rs`

**Interfaces:**
- Produces: `TaskNotificationStatus::{Completed, Failed, Killed}`.
- Produces: `TaskNotificationUsage { total_tokens: Option<u64>, tool_uses: Option<u64>, duration_ms: Option<u64> }`.
- Produces: `TaskNotification::to_xml() -> String`.
- Produces: `build_task_notification(...) -> String`.
- Consumes: worker `agent_id`, status, result text, usage/duration metadata.

- [ ] **Step 1: Add XML builder tests**

Create tests:

```rust
#[test]
fn task_notification_xml_matches_doc_contract() {
    let xml = TaskNotification {
        task_id: "agent-a1b".into(),
        status: TaskNotificationStatus::Completed,
        summary: "Agent \"Investigate auth bug\" completed".into(),
        result: "Found null pointer in src/auth/validate.ts:42".into(),
        usage: TaskNotificationUsage {
            total_tokens: Some(1200),
            tool_uses: Some(3),
            duration_ms: Some(4500),
        },
    }
    .to_xml();

    assert!(xml.contains("<task-notification>"));
    assert!(xml.contains("<task-id>agent-a1b</task-id>"));
    assert!(xml.contains("<status>completed</status>"));
    assert!(xml.contains("<summary>Agent &quot;Investigate auth bug&quot; completed</summary>"));
    assert!(xml.contains("<total_tokens>1200</total_tokens>"));
    assert!(xml.contains("<tool_uses>3</tool_uses>"));
    assert!(xml.contains("<duration_ms>4500</duration_ms>"));
}

#[test]
fn task_notification_xml_escapes_result_text() {
    let xml = build_task_notification(
        "agent-x",
        TaskNotificationStatus::Failed,
        "failed <fast>",
        "bad & worse",
        TaskNotificationUsage::default(),
    );
    assert!(xml.contains("failed &lt;fast&gt;"));
    assert!(xml.contains("bad &amp; worse"));
}
```

- [ ] **Step 2: Implement XML escaping**

Implement a small private escaper:

```rust
fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
```

Do not add a new XML dependency for this small one-way formatter.

- [ ] **Step 3: Wrap sync worker completion**

In the sync agent dispatch path, when all conditions are true:

- parent session is coordinator mode;
- `subagent_type == "worker"`;
- worker run has completed, failed, or was killed;

return the worker result in `<task-notification>` form. The tool result can still include structured `data` for UI, but the model-visible text must contain the XML tag so Coordinator can route follow-up through `SendMessage`.

- [ ] **Step 4: Wrap background worker completion**

In `crates/allthecodes-engine/src/agent/supervisor.rs`, when a background coordinator worker finishes:

- store normal task output for UI;
- additionally store or emit the XML notification as the model-visible completion payload.

In `query/loop_impl.rs`, when draining background results for a coordinator parent, inject the XML notification as a user-role informational message, not a system message. Keep non-coordinator background result behavior unchanged.

- [ ] **Step 5: Improve UI renderer**

`user_text_message.rs` already routes `<task-notification>`. Update `user_agent_notification_message.rs` to render a compact summary by extracting:

- `task-id`
- `status`
- `summary`

If parsing fails, render the existing fallback text.

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p allthecodes-teams task_notification -- --nocapture
cargo test -p allthecodes-engine task_notification -- --nocapture
cargo test -p allthecodes user_agent_notification -- --nocapture
cargo check -p allthecodes-engine
```

Expected: XML formatting is stable, sync/background worker completions can produce notification text, UI still routes the tag.

- [ ] **Step 7: Commit Task 5**

```bash
git add -A -- crates/allthecodes-teams/src/task_notification.rs \
              crates/allthecodes-teams/src/lib.rs \
              crates/allthecodes-engine/src/agent/dispatch.rs \
              crates/allthecodes-engine/src/agent/supervisor.rs \
              crates/allthecodes-engine/src/query/loop_impl.rs \
              crates/allthecodes/src/ui/messages/user_agent_notification_message.rs
git commit -m "feat(coordinator): emit task notifications"
```

---

### Task 6: Agent Teams Feature Gate And Team Tools

**Files:**
- Modify: `crates/allthecodes-teams/src/team_spawn.rs`
- Modify: `crates/allthecodes-teams/src/send_message.rs`
- Create: `crates/allthecodes-teams/src/team_tools.rs`
- Modify: `crates/allthecodes-teams/src/lib.rs`
- Modify: `crates/allthecodes-startup/src/tool_registry.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`
- Test: `crates/allthecodes-teams/src/team_spawn.rs`
- Test: `crates/allthecodes-teams/src/send_message.rs`
- Test: `crates/allthecodes-startup/src/tool_registry.rs`

**Interfaces:**
- Produces: model-visible `TeamCreate` and `TeamDelete` tools behind Agent Teams.
- Produces: `teams_tooling_enabled() -> bool`.
- Consumes: `Feature::AgentTeams`, `Feature::Coordinator`.

- [ ] **Step 1: Add gate helper**

In `allthecodes-teams`:

```rust
pub fn teams_tooling_enabled() -> bool {
    allthecodes_config::features::enabled(allthecodes_config::features::Feature::AgentTeams)
        || allthecodes_config::features::enabled(allthecodes_config::features::Feature::Coordinator)
}
```

Keep `is_agent_teams_active(app_state)` as the runtime-session active check, but use `teams_tooling_enabled()` to decide whether model-visible tools should be exposed.

- [ ] **Step 2: Gate `TeamSpawn` and `SendMessage`**

Update `is_enabled()` in both tools:

```rust
fn is_enabled(&self) -> bool {
    crate::teams_tooling_enabled()
}
```

Add tests that:

- disabled flags hide tools;
- `Feature::AgentTeams` enables tools;
- `Feature::Coordinator` enables tools because coordinator start implies teams.

- [ ] **Step 3: Add TeamCreate/TeamDelete tools**

Create `team_tools.rs`:

```rust
pub struct TeamCreateTool;
pub struct TeamDeleteTool;

#[async_trait::async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str { "TeamCreate" }
    fn is_enabled(&self) -> bool { crate::teams_tooling_enabled() }
    async fn call(&self, input: Value, ctx: &ToolUseContext, parent: &AssistantMessage, progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>) -> Result<ToolResult> {
        // Parse { "name": string, "description": optional string }
        // Reuse helpers::create_team and return team context JSON.
    }
}
```

`TeamDeleteTool` parses `{ "name": string }`, reuses the same deletion helpers as `/team delete`, and returns a JSON result containing `deleted: true`, `team_name`, and `removed_members`.

The implementation must not expose either tool to `CoordinatorWorker` or `InProcessTeammate`.

- [ ] **Step 4: Register team tools**

In `root_owned_base_tools()` add:

```rust
Arc::new(allthecodes_teams::team_tools::TeamCreateTool) as _,
Arc::new(allthecodes_teams::team_tools::TeamDeleteTool) as _,
```

In feature-gate tests, assert they are absent when Agent Teams is off and present when on.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-teams team_spawn -- --nocapture
cargo test -p allthecodes-teams send_message -- --nocapture
cargo test -p allthecodes-teams team_tools -- --nocapture
cargo test -p allthecodes-startup team -- --nocapture
cargo check -p allthecodes-startup
```

Expected: Agent Teams tools are no longer visible under only `ALLTHECODES_MULTI_AGENT_V2`; Swarm creation has model-visible tools when the feature is on.

- [ ] **Step 6: Commit Task 6**

```bash
git add -A -- crates/allthecodes-teams/src/team_spawn.rs \
              crates/allthecodes-teams/src/send_message.rs \
              crates/allthecodes-teams/src/team_tools.rs \
              crates/allthecodes-teams/src/lib.rs \
              crates/allthecodes-startup/src/tool_registry.rs \
              crates/allthecodes-tools/src/registry.rs
git commit -m "feat(teams): gate swarm tools"
```

---

### Task 7: Task List Priority And Claim Contract

**Files:**
- Modify: `crates/allthecodes-tasks/src/lists.rs`
- Modify: `crates/allthecodes-tasks/src/store.rs`
- Modify: `crates/allthecodes-tools/src/tasks/mod.rs`
- Test: `crates/allthecodes-tasks/src/lists.rs`
- Test: `crates/allthecodes-tasks/src/store.rs`
- Test: `crates/allthecodes-tools/src/tasks/mod.rs`

**Interfaces:**
- Produces: documented task-list id priority with allthecodes primary aliases and Claude compatibility aliases.
- Produces: claim race behavior locked by tests.
- Consumes: existing file lock in `TaskListLock`.

- [ ] **Step 1: Lock priority contract**

Add tests in `lists.rs`:

```rust
#[test]
#[serial_test::serial]
fn task_list_id_uses_allthecodes_alias_before_claude_alias() {
    let _a = EnvGuard::set("ALLTHECODES_TASK_LIST_ID", "allthecodes-list");
    let _c = EnvGuard::set("CLAUDE_CODE_TASK_LIST_ID", "claude-list");
    let id = task_list_id_from_parts(TaskListScope::default());
    assert_eq!(id, "allthecodes-list");
}

#[test]
#[serial_test::serial]
fn task_list_id_supports_claude_alias_when_allthecodes_absent() {
    let _a = EnvGuard::remove("ALLTHECODES_TASK_LIST_ID");
    let _c = EnvGuard::set("CLAUDE_CODE_TASK_LIST_ID", "claude-list");
    let id = task_list_id_from_parts(TaskListScope::default());
    assert_eq!(id, "claude-list");
}

#[test]
#[serial_test::serial]
fn task_list_id_uses_team_context_before_session_fallback() {
    let scope = TaskListScope {
        explicit_task_list_id: None,
        scoped_team_name: Some("research".into()),
        app_team_name: None,
        session_id: Some("session-x".into()),
    };
    assert_eq!(task_list_id_from_parts(scope), "research");
}
```

- [ ] **Step 2: Preserve allthecodes path isolation**

Do not change the storage root from `.allthecodes/tasks`. Document in comments:

```rust
// allthecodes keeps task storage under ~/.allthecodes for path isolation.
// CLAUDE_CODE_* variables are compatibility aliases only; they do not redirect storage.
```

- [ ] **Step 3: Add claim-race regression**

If no concurrent test exists, add one to `store.rs` using two threads that call `claim_task` on the same task. Expected:

- one `Ok(entry)` with owner A or B;
- one `Err` with `TaskClaimFailureReason::AlreadyClaimed`;
- final persisted task has one owner and `InProgress`.

- [ ] **Step 4: Lock TaskUpdate claim error shape**

In `crates/allthecodes-tools/src/tasks/mod.rs`, add a tool-level test that failed claims return:

```json
{
  "task_error": { "code": "claim_failed" },
  "claim": { "reason": "already_claimed" }
}
```

Use existing task tool test helpers and avoid asserting full pretty text.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-tasks task_list_id -- --nocapture
cargo test -p allthecodes-tasks claim -- --nocapture
cargo test -p allthecodes-tools task_update -- --nocapture
cargo check -p allthecodes-tasks
```

Expected: task-list selection is explicit, claim race is atomic, and tool error payload is stable.

- [ ] **Step 6: Commit Task 7**

```bash
git add -A -- crates/allthecodes-tasks/src/lists.rs \
              crates/allthecodes-tasks/src/store.rs \
              crates/allthecodes-tools/src/tasks/mod.rs
git commit -m "test(tasks): lock swarm claim contract"
```

---

### Task 8: TeammateIdle Hook Runtime

**Files:**
- Modify: `crates/allthecodes-teams/src/runner.rs`
- Modify: `crates/allthecodes-teams/src/types.rs`
- Modify: `crates/allthecodes-teams/src/team_spawn.rs`
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Test: `crates/allthecodes-teams/src/runner.rs`
- Test: `crates/allthecodes-types/src/hooks.rs`

**Interfaces:**
- Produces: TeammateIdle hook payload.
- Consumes: `allthecodes_types::hooks::HookRunner::run_event_hooks`.
- Consumes: existing mailbox idle notification.

- [ ] **Step 1: Add hook payload type**

Add to `types.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeammateIdleHookPayload {
    pub team_name: String,
    pub teammate_name: String,
    pub agent_id: String,
    pub reason: IdleReason,
    pub task_list_id: String,
    pub timestamp: String,
}
```

- [ ] **Step 2: Carry hooks into teammate spawn config**

Extend `TeammateSpawnConfig`:

```rust
pub hooks: std::collections::HashMap<String, serde_json::Value>,
pub hook_runner: Option<std::sync::Arc<dyn allthecodes_types::hooks::HookRunner>>,
```

If `Arc<dyn HookRunner>` prevents `Debug` derive on the config, remove or customize `Debug` for `TeammateSpawnConfig`.

Populate these fields from `ctx.get_app_state()` in `TeamSpawnTool`, `/team spawn`, and Agent tool teammate-spawn path.

- [ ] **Step 3: Run hook after idle notification**

In `runner.rs`, after `send_idle_notification(...)` succeeds or fails, call:

```rust
let configs = allthecodes_types::hooks::load_hook_configs(&config.hooks, "TeammateIdle");
if !configs.is_empty() {
    let payload = serde_json::to_value(TeammateIdleHookPayload { ... })?;
    let _ = hook_runner.run_event_hooks("TeammateIdle", &payload, &configs).await;
}
```

The hook should be best-effort; hook failure must not kill the teammate.

- [ ] **Step 4: Add tests**

Use a fake `HookRunner` that records event names and payloads. Assert:

- `TeammateIdle` event name is used;
- payload contains team name, teammate name, agent id, reason;
- mailbox idle notification remains sent.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-teams teammate_idle -- --nocapture
cargo test -p allthecodes-types TeammateIdle -- --nocapture
cargo check -p allthecodes-teams
```

Expected: idle hook runs once per idle transition and does not replace mailbox notification.

- [ ] **Step 6: Commit Task 8**

```bash
git add -A -- crates/allthecodes-teams/src/runner.rs \
              crates/allthecodes-teams/src/types.rs \
              crates/allthecodes-teams/src/team_spawn.rs \
              crates/allthecodes-engine/src/agent/tool_impl.rs
git commit -m "feat(teams): fire teammate idle hook"
```

---

### Task 9: Coordinator Worker Lifecycle, Retry, Cost, And Stop Conditions

**Files:**
- Create: `crates/allthecodes-teams/src/coordinator_policy.rs`
- Modify: `crates/allthecodes-teams/src/coordinator.rs`
- Modify: `crates/allthecodes-teams/src/task_notification.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Test: `crates/allthecodes-teams/src/coordinator_policy.rs`

**Interfaces:**
- Produces: `CoordinatorPolicySnapshot`.
- Produces: model-visible policy prompt fragment.
- Consumes: env overrides `ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS`, `ALLTHECODES_COORDINATOR_MAX_RETRY_PER_TASK`, `ALLTHECODES_COORDINATOR_MAX_WORKER_TURNS`.

- [ ] **Step 1: Add policy tests**

Create tests:

```rust
#[test]
#[serial_test::serial]
fn coordinator_policy_defaults_are_conservative() {
    let _a = EnvGuard::remove("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS");
    let policy = CoordinatorPolicySnapshot::from_env();
    assert_eq!(policy.max_parallel_workers, 4);
    assert_eq!(policy.max_retry_per_task, 1);
    assert_eq!(policy.max_worker_turns, Some(12));
}

#[test]
#[serial_test::serial]
fn coordinator_policy_rejects_zero_values() {
    let _a = EnvGuard::set("ALLTHECODES_COORDINATOR_MAX_PARALLEL_WORKERS", "0");
    let policy = CoordinatorPolicySnapshot::from_env();
    assert_eq!(policy.max_parallel_workers, 4);
}
```

- [ ] **Step 2: Implement policy snapshot**

Use:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorPolicySnapshot {
    pub max_parallel_workers: usize,
    pub max_retry_per_task: usize,
    pub max_worker_turns: Option<usize>,
}
```

Add:

```rust
pub fn prompt_fragment(&self) -> String {
    format!(
        "- Maximum parallel workers: {}\n- Retry failed worker tasks at most {} time(s) unless the user explicitly asks for more.\n- Worker max turns: {}.\n",
        self.max_parallel_workers,
        self.max_retry_per_task,
        self.max_worker_turns.map(|v| v.to_string()).unwrap_or_else(|| "unlimited".into())
    )
}
```

- [ ] **Step 3: Pass worker turn limit**

When Coordinator spawns worker agents, ensure child `QueryEngineConfig.max_turns` uses `CoordinatorPolicySnapshot.max_worker_turns` unless the tool call explicitly sets a stricter limit.

The expected rule:

- explicit lower max turns wins;
- no explicit max turns uses policy default;
- explicit higher max turns is clamped to policy default in coordinator mode.

- [ ] **Step 4: Add notification usage fields**

Ensure task notification usage includes:

- total tokens when available;
- tool uses when available;
- duration ms;
- retry count if runtime can provide it.

If retry count is unavailable in the agent completion path, omit `<retry_count>` and keep the struct field optional.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p allthecodes-teams coordinator_policy -- --nocapture
cargo test -p allthecodes-engine coordinator_worker_turn_limit -- --nocapture
cargo test -p allthecodes-teams task_notification -- --nocapture
cargo check -p allthecodes-engine
```

Expected: prompt has hard policy values, child worker turn limits are bounded, notification usage stays optional and stable.

- [ ] **Step 6: Commit Task 9**

```bash
git add -A -- crates/allthecodes-teams/src/coordinator_policy.rs \
              crates/allthecodes-teams/src/coordinator.rs \
              crates/allthecodes-teams/src/task_notification.rs \
              crates/allthecodes-engine/src/agent/supervisor.rs \
              crates/allthecodes-engine/src/agent/dispatch.rs
git commit -m "feat(coordinator): add runtime policy limits"
```

---

### Task 10: In-Process Teammate Resume Boundary And Status Diagnostics

**Files:**
- Modify: `crates/allthecodes-teams/src/in_process.rs`
- Modify: `crates/allthecodes-teams/src/command.rs`
- Modify: `crates/allthecodes-teams/src/types.rs`
- Modify: `development/archive/IMPLEMENTATION_GAPS.md`
- Test: `crates/allthecodes-teams/src/in_process.rs`
- Test: `crates/allthecodes-teams/src/command.rs`

**Interfaces:**
- Produces: explicit status text for non-resumable in-process teammate tasks.
- Produces: persisted team config remains recoverable, runtime task registry remains non-resumable by design.
- Consumes: existing `TASK_REGISTRY`.

- [ ] **Step 1: Add status diagnostics tests**

In command tests, assert `/team status` includes:

```text
backend: in-process
resume: unsupported
```

for stopped/stale teammates that exist in team config but are absent from `InProcessBackend::task_snapshots()`.

- [ ] **Step 2: Implement diagnostics**

Add to `TeammateInfo` or status formatter:

```rust
pub resume_supported: bool,
pub runtime_attached: bool,
```

For in-process teammates:

- `resume_supported = false`;
- `runtime_attached = true` only when `TASK_REGISTRY` has the task id.

- [ ] **Step 3: Preserve intentional crop**

Update `IMPLEMENTATION_GAPS.md` to say:

- external pane backend remains intentional crop;
- in-process teammate session resume remains unsupported and now has explicit diagnostics;
- task unassignment on abnormal exit is implemented and should not be listed as missing.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p allthecodes-teams team_status -- --nocapture
cargo test -p allthecodes-teams in_process -- --nocapture
cargo check -p allthecodes-teams
```

Expected: users can see why old in-process teammates cannot be resumed.

- [ ] **Step 5: Commit Task 10**

```bash
git add -A -- crates/allthecodes-teams/src/in_process.rs \
              crates/allthecodes-teams/src/command.rs \
              crates/allthecodes-teams/src/types.rs \
              development/archive/IMPLEMENTATION_GAPS.md
git commit -m "docs(teams): clarify in-process resume boundary"
```

---

### Task 11: Documentation And Hidden Feature Status

**Files:**
- Modify: `development/hided_features/current-hidden-features.md`
- Modify: `development/archive/IMPLEMENTATION_GAPS.md`
- Modify: `docs/WORK_STATUS.md`
- Create: `development/teams_swarm_coordinator/completion-checklist.md`

**Interfaces:**
- Produces: source-verified feature status.
- Produces: final manual verification checklist.

- [ ] **Step 1: Update hidden feature status**

Change Agent Teams / Coordinator entries to list exact gates:

```markdown
- `FEATURE_AGENT_TEAMS` / `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS` / `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`: enables Agent Teams/Swarm.
- `ALLTHECODES_COORDINATOR_MODE` / `CLAUDE_CODE_COORDINATOR_MODE`: enables Coordinator Mode; `/coordinator start` also sets session-local runtime override.
```

Add a note that `ALLTHECODES_MULTI_AGENT_V2` no longer by itself exposes team tools.

- [ ] **Step 2: Update gaps**

Move completed gaps out of active missing list:

- Coordinator prompt parity;
- strict Coordinator tool policy;
- worker simple mode;
- scratchpad;
- `<task-notification>`;
- TeammateIdle hook.

Keep pane backend as intentional crop.

- [ ] **Step 3: Create completion checklist**

Create `development/teams_swarm_coordinator/completion-checklist.md`:

```markdown
# Teams Swarm Coordinator Completion Checklist

- [ ] Feature aliases verified.
- [ ] Coordinator session restore verified.
- [ ] Coordinator tool schema has no file/shell tools.
- [ ] Coordinator worker full policy excludes internal orchestration tools.
- [ ] Coordinator worker simple policy only exposes Bash, Read, Edit.
- [ ] Scratchpad path is under ALLTHECODES_HOME.
- [ ] Worker completion emits task-notification XML.
- [ ] Agent Teams tools are hidden when Agent Teams is disabled.
- [ ] TeamCreate and TeamDelete tools work when Agent Teams is enabled.
- [ ] Task claim race returns exactly one winner.
- [ ] TeammateIdle hook fires.
- [ ] In-process resume limitation is visible in status.
- [ ] Workspace release build passes.
```

- [ ] **Step 4: Verify docs**

Run:

```bash
git diff --check -- development/hided_features/current-hidden-features.md \
                    development/archive/IMPLEMENTATION_GAPS.md \
                    docs/WORK_STATUS.md \
                    development/teams_swarm_coordinator/completion-checklist.md
```

Expected: no trailing whitespace or markdown conflict markers.

- [ ] **Step 5: Commit Task 11**

```bash
git add -A -- development/hided_features/current-hidden-features.md \
              development/archive/IMPLEMENTATION_GAPS.md \
              docs/WORK_STATUS.md \
              development/teams_swarm_coordinator/completion-checklist.md
git commit -m "docs(teams): record coordinator swarm parity"
```

---

### Task 12: End-To-End Verification

**Files:**
- Modify only if tests reveal real bugs in files from prior tasks.
- Test: workspace and focused crates.

**Interfaces:**
- Produces: final verification evidence.

- [ ] **Step 1: Run focused crate tests**

Run:

```bash
cargo test -p allthecodes-config coordinator agent_teams -- --nocapture
cargo test -p allthecodes-teams -- --nocapture
cargo test -p allthecodes-tasks claim task_list -- --nocapture
cargo test -p allthecodes-tools task_update -- --nocapture
cargo test -p allthecodes-startup coordinator_policy worker_policy -- --nocapture
cargo test -p allthecodes-engine coordinator -- --nocapture
```

Expected: all focused tests pass.

- [ ] **Step 2: Run workspace build**

Run:

```bash
cargo build --workspace --release
```

Expected: release build succeeds with no new warnings. Existing known warning about unused Unix `CommandExt` must be resolved if still present in touched code paths; otherwise record it separately before commit.

- [ ] **Step 3: Manual smoke commands**

Run these in a temporary home:

```bash
VERIFY=/tmp/allthecodes-teams-swarm-coordinator-smoke
rm -rf "$VERIFY"
mkdir -p "$VERIFY"
ALLTHECODES_HOME="$VERIFY" ALLTHECODES_COORDINATOR_MODE=1 allthecodes --help >/tmp/allthecodes-coordinator-help.txt
ALLTHECODES_HOME="$VERIFY" FEATURE_AGENT_TEAMS=1 allthecodes --help >/tmp/allthecodes-teams-help.txt
```

Expected:

- commands exit `0`;
- no panic;
- help output generation does not try to write under `~/.claude`.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git diff --check
git diff --stat
```

Expected:

- only planned files changed;
- no whitespace errors;
- no unrelated worktree changes staged.

- [ ] **Step 5: Final commit**

If Task 12 required fixes, commit them:

```bash
git add -A -- <files-fixed-during-verification>
git commit -m "fix(teams): close coordinator swarm verification"
```

If Task 12 only produced evidence, do not create an empty commit.

---

## Acceptance Criteria

- Coordinator can be enabled by allthecodes native gate, upstream compat env, or `/coordinator start`.
- Resumed coordinator sessions restore mode and tool policy.
- Coordinator prompt covers goal decomposition, tool choice, verification, failure retry, cost limits, and stopping conditions.
- Coordinator cannot directly read/write files or run commands.
- Coordinator workers cannot create nested teams, send mailbox messages, or spawn further agents.
- `CLAUDE_CODE_SIMPLE=1` limits coordinator workers to `Bash`, `Read`, and `Edit`.
- Scratchpad is available to workers only when gated and only under allthecodes data root.
- Worker completion emits `<task-notification>` with status/result/usage.
- Swarm team tools require Agent Teams or Coordinator, not only MultiAgentV2.
- Shared task list claim semantics are stable under concurrent claims.
- `TaskCreated`, `TaskCompleted`, and `TeammateIdle` hooks have runtime emission paths.
- In-process teammate resume limitation is explicit, while abnormal-exit unassignment remains implemented.
- External pane backend remains documented as intentional crop.
- `cargo build --workspace --release` passes before final handoff.

## Execution Notes

- Use one commit per task so review can isolate behavior changes.
- Do not use `git add -A` without explicit paths from the task.
- Do not change npm/web packaging.
- Do not introduce a new XML parser dependency for one-way task-notification formatting.
- Do not move team/task data to `.claude`; compatibility env vars must not imply compatibility storage paths.
