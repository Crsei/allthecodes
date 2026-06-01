# Hooks System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire up the existing hooks infrastructure and implement all 26 hook events with `command` type execution, making hooks actually run in the tool execution and query loop pipelines.

**Architecture:** The hooks system has three layers: (1) configuration loading from `settings.json` → `MergedConfig.hooks`, (2) a generic hook executor (`execute_command_hook`) that spawns subprocesses, and (3) event-specific dispatch points throughout the codebase. Layer 1 and 2 are already built; this plan wires them together and adds the missing event dispatch points. Each hook event gets its own stdin schema and fires at the correct lifecycle point.

**Tech Stack:** Rust async (tokio), serde_json, subprocess spawning via `tokio::process`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/tools/hooks.rs` | Modify | Add `HookEvent` enum, generic `run_event_hooks()`, HTTP hook type stub |
| `src/types/app_state.rs` | Modify | Add `hooks: HashMap<String, Value>` field to `AppState` |
| `src/config/settings.rs` | Modify | Propagate `hooks` from `MergedConfig` into `AppState` during init |
| `src/engine/lifecycle/deps.rs` | Modify | Load hook configs and pass to `execute_tool()`, fire lifecycle hooks |
| `src/tools/execution.rs` | Modify | Accept hooks from `StreamingToolExecutor`, stop passing `&[]` |
| `src/query/loop_impl.rs` | Modify | Fire `Stop`/`StopFailure` hooks with real configs |
| `src/query/stop_hooks.rs` | Modify | Wire `run_stop_hooks` to real hook configs from state |
| `src/query/loop_helpers.rs` | Modify | Thread hook configs through `execute_tool_calls` |
| `src/engine/lifecycle/submit_message.rs` | Modify | Fire `UserPromptSubmit` hook |
| `src/main.rs` | Modify | Fire `SessionStart` hook on startup, `SessionEnd` on shutdown |
| `tests/hook_integration.rs` | Create | Integration tests for hook wiring |

---

### Task 1: Add `hooks` field to `AppState` and populate from config

**Files:**
- Modify: `src/types/app_state.rs`
- Modify: `src/engine/lifecycle/deps.rs` (where AppState is constructed)

- [ ] **Step 1: Write the failing test**

```rust
// tests/hook_integration.rs
use std::collections::HashMap;
use serde_json::json;

#[test]
fn app_state_has_hooks_field() {
    let mut state = claude_code_rs::types::app_state::AppState::default();
    state.hooks.insert(
        "PreToolUse".to_string(),
        json!([{"matcher": "*", "hooks": [{"type": "command", "command": "echo hi"}]}]),
    );
    assert!(state.hooks.contains_key("PreToolUse"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test app_state_has_hooks_field 2>&1 | tail -5`
Expected: FAIL — `no field hooks on type AppState`

- [ ] **Step 3: Add `hooks` field to `AppState`**

In `src/types/app_state.rs`, add to `AppState`:

```rust
/// Hook configurations keyed by event name (e.g. "PreToolUse").
/// Loaded from MergedConfig.hooks at startup.
pub hooks: HashMap<String, serde_json::Value>,
```

And in `Default` impl:

```rust
hooks: HashMap::new(),
```

- [ ] **Step 4: Populate hooks from MergedConfig during engine init**

In `src/engine/lifecycle/deps.rs`, find where `AppState` is constructed and add:

```rust
hooks: merged_config.hooks.clone(),
```

If `AppState` is built from `Default`, add a post-init step:

```rust
app_state.hooks = merged_config.hooks.clone();
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test app_state_has_hooks_field`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/types/app_state.rs src/engine/lifecycle/deps.rs tests/hook_integration.rs
git commit -m "feat(hooks): add hooks field to AppState, populate from MergedConfig"
```

---

### Task 2: Add generic `HookEvent` enum and `run_event_hooks()` dispatcher

**Files:**
- Modify: `src/tools/hooks.rs`

The current hooks.rs has separate `run_pre_tool_hooks`, `run_post_tool_hooks`, etc. We need a generic event dispatcher that any lifecycle point can call with an event name and a JSON payload.

- [ ] **Step 1: Write the failing test**

```rust
// In src/tools/hooks.rs #[cfg(test)] mod tests
#[tokio::test]
async fn test_run_event_hooks_empty() {
    let result = run_event_hooks("SessionStart", &serde_json::json!({}), &[]).await;
    assert!(result.is_ok());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_run_event_hooks_empty 2>&1 | tail -5`
Expected: FAIL — `run_event_hooks` not found

- [ ] **Step 3: Implement `run_event_hooks()`**

Add to `src/tools/hooks.rs`:

```rust
/// Generic hook event runner for non-tool lifecycle events.
///
/// Fires all matching hooks for the given event. Returns `Ok(HookOutput)` from
/// the last hook that produced output, or default if none.
/// Fire-and-forget semantics: errors are logged, not propagated.
pub async fn run_event_hooks(
    event_name: &str,
    payload: &Value,
    hook_configs: &[HookEventConfig],
) -> Result<HookOutput> {
    if hook_configs.is_empty() {
        debug!(event = event_name, "event hooks: no hooks configured");
        return Ok(HookOutput::default());
    }

    let mut last_output = HookOutput::default();

    for config in hook_configs {
        // Non-tool events: only match configs with None or "*" matcher
        if config.matcher.is_some() && config.matcher.as_deref() != Some("*") {
            continue;
        }

        for entry in &config.hooks {
            let HookEntry::Command { command, timeout } = entry;

            debug!(event = event_name, command = command, "running event hook");

            match execute_command_hook(command, payload, *timeout).await {
                Ok(output) => {
                    last_output = output;
                }
                Err(e) => {
                    warn!(event = event_name, command = command, error = %e, "event hook error");
                }
            }
        }
    }

    Ok(last_output)
}
```

Also make `HookOutput` public so callers can inspect it:

```rust
pub struct HookOutput { ... }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_run_event_hooks_empty`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/tools/hooks.rs
git commit -m "feat(hooks): add generic run_event_hooks() dispatcher"
```

---

### Task 3: Wire hook configs into tool execution pipeline

This is the critical connection — currently `&[]` is passed everywhere.

**Files:**
- Modify: `src/tools/execution.rs:436-494` (StreamingToolExecutor)
- Modify: `src/query/loop_helpers.rs:108-170` (execute_tool_calls)
- Modify: `src/engine/lifecycle/deps.rs:297-420` (execute_tool)

- [ ] **Step 1: Add hook_configs param to `StreamingToolExecutor::new()` and `execute_all()`**

In `src/tools/execution.rs`, add a field to `StreamingToolExecutor`:

```rust
pub struct StreamingToolExecutor {
    tracked: Vec<TrackedToolUse>,
    has_bash_error: bool,
    hook_configs: Vec<HookEventConfig>,  // NEW
}
```

In `new()`, accept `hook_configs: Vec<HookEventConfig>` and store it.

In `execute_all()`, replace both `&[]` with `&self.hook_configs`:

```rust
let result = run_tool_use(
    &tracked.tool_use_id,
    &tracked.tool_name,
    tracked.input.clone(),
    tools,
    ctx,
    parent_message,
    None,
    &self.hook_configs,  // was &[]
)
.await;
```

- [ ] **Step 2: Thread hook configs through `execute_tool_calls()` in loop_helpers.rs**

Add `hook_configs: &HashMap<String, Value>` param. Use `hooks::load_hook_configs` to load `PreToolUse` and `PostToolUse` configs and pass them to the deps call.

- [ ] **Step 3: Load and pass hook configs in `deps.rs` `execute_tool()`**

In `QueryEngineDeps::execute_tool()`, read hooks from `self.state.read().app_state.hooks`:

```rust
let hooks_map = self.state.read().app_state.hooks.clone();
let pre_hooks = hooks::load_hook_configs(&hooks_map, "PreToolUse");
let post_hooks = hooks::load_hook_configs(&hooks_map, "PostToolUse");
let failure_hooks = hooks::load_hook_configs(&hooks_map, "PostToolUseFailure");
```

Then run pre-hooks before permission check, post-hooks after tool.call(), failure-hooks on error.

- [ ] **Step 4: Run existing tests to verify no regression**

Run: `cargo test -- --skip ignored 2>&1 | tail -10`
Expected: All existing tests pass (hooks are empty by default, behavior unchanged)

- [ ] **Step 5: Write integration test with real command hook**

```rust
// tests/hook_integration.rs
#[tokio::test]
async fn pre_tool_hook_command_runs() {
    use claude_code_rs::tools::hooks::*;
    let configs = vec![HookEventConfig {
        matcher: Some("Bash".to_string()),
        hooks: vec![HookEntry::Command {
            command: "echo '{\"continue\":true}'".to_string(),
            timeout: 10,
        }],
    }];

    let result = run_pre_tool_hooks("Bash", &serde_json::json!({"command": "ls"}), &configs)
        .await
        .unwrap();

    assert!(matches!(result, PreToolHookResult::Continue { .. }));
}
```

- [ ] **Step 6: Run and verify**

Run: `cargo test pre_tool_hook_command_runs`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/tools/execution.rs src/query/loop_helpers.rs src/engine/lifecycle/deps.rs tests/hook_integration.rs
git commit -m "feat(hooks): wire hook configs into tool execution pipeline"
```

---

### Task 4: Wire stop hooks to real config

**Files:**
- Modify: `src/query/stop_hooks.rs`
- Modify: `src/query/loop_impl.rs`

- [ ] **Step 1: Add `hook_configs` param to `run_stop_hooks()`**

In `src/query/stop_hooks.rs`, change signature:

```rust
pub async fn run_stop_hooks(
    assistant_message: &AssistantMessage,
    messages: &[Message],
    stop_hook_active: Option<bool>,
    hook_configs: &[HookEventConfig],  // NEW
) -> Result<StopHookResult> {
```

Replace the stub body with a call to `hooks::run_stop_hooks(hook_configs)` and map the result:

```rust
use crate::tools::hooks::{self, HookEventConfig, PostToolHookResult};

// If stop_hook_active == true, skip to prevent infinite loops
if stop_hook_active == Some(true) {
    return Ok(StopHookResult::AllowStop);
}

match hooks::run_stop_hooks(hook_configs).await {
    Ok(PostToolHookResult::Continue) => Ok(StopHookResult::AllowStop),
    Ok(PostToolHookResult::StopContinuation { message }) => {
        Ok(StopHookResult::PreventStop {
            continuation_message: message,
        })
    }
    Err(e) => Ok(StopHookResult::BlockingError {
        error: e.to_string(),
    }),
}
```

- [ ] **Step 2: Update call site in `loop_impl.rs`**

Load stop hook configs from deps state and pass them:

```rust
let hooks_map = deps.get_hooks();
let stop_configs = crate::tools::hooks::load_hook_configs(&hooks_map, "Stop");

let stop_result = stop_hooks::run_stop_hooks(
    &assistant_message,
    &state.messages,
    state.stop_hook_active,
    &stop_configs,
).await;
```

Add `get_hooks()` method to `QueryDeps` trait:

```rust
fn get_hooks(&self) -> HashMap<String, serde_json::Value>;
```

Implement in `QueryEngineDeps`:

```rust
fn get_hooks(&self) -> HashMap<String, serde_json::Value> {
    self.state.read().app_state.hooks.clone()
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -- --skip ignored 2>&1 | tail -10`
Expected: PASS (default empty hooks → AllowStop, same behavior)

- [ ] **Step 4: Remove `#[allow(dead_code)]` from `PreventStop` and `BlockingError` variants**

They are now reachable through the real implementation.

- [ ] **Step 5: Commit**

```bash
git add src/query/stop_hooks.rs src/query/loop_impl.rs src/engine/lifecycle/deps.rs
git commit -m "feat(hooks): wire stop hooks to real config, remove stubs"
```

---

### Task 5: Fire `UserPromptSubmit` hook

**Files:**
- Modify: `src/engine/lifecycle/submit_message.rs`

- [ ] **Step 1: Identify the submit entry point**

In `submit_message.rs`, find the function that processes user input before starting a query. This is where `UserPromptSubmit` fires.

- [ ] **Step 2: Add hook dispatch**

```rust
// At the start of submit_message, before launching query:
let hooks_map = self.state.read().app_state.hooks.clone();
let configs = hooks::load_hook_configs(&hooks_map, "UserPromptSubmit");
if !configs.is_empty() {
    let payload = serde_json::json!({
        "prompt": user_input,
    });
    let output = hooks::run_event_hooks("UserPromptSubmit", &payload, &configs).await?;
    if !output.should_continue {
        // Hook blocked the prompt — return without starting query
        return Ok(());
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -- --skip ignored`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/engine/lifecycle/submit_message.rs
git commit -m "feat(hooks): fire UserPromptSubmit hook before query start"
```

---

### Task 6: Fire `SessionStart` and `SessionEnd` hooks

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Fire `SessionStart` after config is loaded**

Find the point after `MergedConfig` is loaded and `AppState` is constructed. Add:

```rust
// Fire SessionStart hook
let start_configs = hooks::load_hook_configs(&merged_config.hooks, "SessionStart");
if !start_configs.is_empty() {
    let payload = serde_json::json!({
        "session_id": session_id,
        "cwd": std::env::current_dir().unwrap_or_default().to_string_lossy(),
    });
    let _ = hooks::run_event_hooks("SessionStart", &payload, &start_configs).await;
}
```

- [ ] **Step 2: Fire `SessionEnd` on shutdown**

In `src/shutdown.rs` or at the end of `main()`:

```rust
let end_configs = hooks::load_hook_configs(&merged_config.hooks, "SessionEnd");
if !end_configs.is_empty() {
    let payload = serde_json::json!({
        "session_id": session_id,
    });
    let _ = hooks::run_event_hooks("SessionEnd", &payload, &end_configs).await;
}
```

- [ ] **Step 3: Run tests, verify no startup regression**

Run: `cargo test -- --skip ignored && cargo run -- --version`
Expected: Both pass/run without error

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/shutdown.rs
git commit -m "feat(hooks): fire SessionStart and SessionEnd hooks"
```

---

### Task 7: Fire `Notification` hook

**Files:**
- Modify: `src/tools/hooks.rs` (add helper)
- Modify: Wherever notifications are emitted (search for `Notification` in IPC or UI)

- [ ] **Step 1: Add `fire_notification_hook()` convenience function**

```rust
pub async fn fire_notification_hook(
    title: &str,
    body: &str,
    hooks_map: &HashMap<String, Value>,
) {
    let configs = load_hook_configs(hooks_map, "Notification");
    if configs.is_empty() { return; }
    let payload = serde_json::json!({
        "title": title,
        "body": body,
    });
    let _ = run_event_hooks("Notification", &payload, &configs).await;
}
```

- [ ] **Step 2: Call at notification points**

Grep for where notifications/toasts are sent and add the hook call.

- [ ] **Step 3: Commit**

```bash
git add src/tools/hooks.rs
git commit -m "feat(hooks): fire Notification hook"
```

---

### Task 8: Fire `PermissionRequest` and `PermissionDenied` hooks

**Files:**
- Modify: `src/engine/lifecycle/deps.rs` (permission check section)

- [ ] **Step 1: Fire `PermissionRequest` when permission is asked**

In `execute_tool()`, at the `PermissionResult::Ask` branch, before calling the callback:

```rust
let perm_req_configs = hooks::load_hook_configs(&hooks_map, "PermissionRequest");
if !perm_req_configs.is_empty() {
    let payload = serde_json::json!({
        "tool_name": request.tool_name,
        "tool_input": request.input,
        "message": message,
    });
    let output = hooks::run_event_hooks("PermissionRequest", &payload, &perm_req_configs).await.ok();
    // Check if hook overrides the decision
    if let Some(ref out) = output {
        if let Some(ref decision) = out.permission_decision {
            // "allow" -> proceed, "deny" -> return denied
        }
    }
}
```

- [ ] **Step 2: Fire `PermissionDenied` when denied**

At all `Permission denied` return paths:

```rust
let deny_configs = hooks::load_hook_configs(&hooks_map, "PermissionDenied");
if !deny_configs.is_empty() {
    let payload = serde_json::json!({
        "tool_name": request.tool_name,
        "tool_input": request.input,
        "reason": message,
    });
    let _ = hooks::run_event_hooks("PermissionDenied", &payload, &deny_configs).await;
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -- --skip ignored`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/engine/lifecycle/deps.rs
git commit -m "feat(hooks): fire PermissionRequest and PermissionDenied hooks"
```

---

### Task 9: Fire compact hooks (`PreCompact`, `PostCompact`)

**Files:**
- Modify: `src/compact/pipeline.rs` or wherever compaction is triggered

- [ ] **Step 1: Find the compaction entry point**

Search for `try_reactive_compact` or `auto_compact` in the query loop.

- [ ] **Step 2: Fire `PreCompact` before compaction**

```rust
let compact_configs = hooks::load_hook_configs(&hooks_map, "PreCompact");
if !compact_configs.is_empty() {
    let payload = serde_json::json!({
        "message_count": messages.len(),
    });
    let _ = hooks::run_event_hooks("PreCompact", &payload, &compact_configs).await;
}
```

- [ ] **Step 3: Fire `PostCompact` after compaction**

```rust
let post_configs = hooks::load_hook_configs(&hooks_map, "PostCompact");
if !post_configs.is_empty() {
    let payload = serde_json::json!({
        "message_count": result.messages.len(),
        "tokens_freed": result.tokens_freed,
    });
    let _ = hooks::run_event_hooks("PostCompact", &payload, &post_configs).await;
}
```

- [ ] **Step 4: Commit**

```bash
git add src/compact/pipeline.rs src/query/loop_impl.rs
git commit -m "feat(hooks): fire PreCompact and PostCompact hooks"
```

---

### Task 10: Fire remaining lifecycle hooks (batch)

**Files:**
- Various files where these events occur

The remaining hooks are lower-priority and follow the same pattern. Each one:
1. Loads configs via `load_hook_configs(&hooks_map, "EventName")`
2. Builds a JSON payload
3. Calls `run_event_hooks()`

| Hook | Where to fire | Payload fields |
|------|--------------|----------------|
| `SubagentStart` | Agent tool `call()` | `agent_id`, `agent_type`, `prompt` |
| `SubagentStop` | Agent tool completion | `agent_id`, `result_summary` |
| `TaskCreated` | TaskCreate tool | `task_id`, `description` |
| `TaskCompleted` | TaskUpdate tool (status=completed) | `task_id`, `status` |
| `StopFailure` | Query loop, after stop hook error | `error` |
| `InstructionsLoaded` | CLAUDE.md loading | `source`, `content_length` |
| `ConfigChange` | Settings update command | `key`, `old_value`, `new_value` |
| `CwdChanged` | cd detection in tools | `old_cwd`, `new_cwd` |
| `FileChanged` | FileWrite/FileEdit tool post-hook | `file_path`, `operation` |
| `Elicitation` | Permission prompt creation | `tool_name`, `question` |
| `ElicitationResult` | Permission prompt response | `tool_name`, `decision` |
| `Setup` | First-time setup flow | `step` |

- [ ] **Step 1: Implement `SubagentStart` / `SubagentStop`** in `src/tools/agent.rs`
- [ ] **Step 2: Implement `TaskCreated` / `TaskCompleted`** in `src/tools/task_create.rs` and `src/tools/task_update.rs`
- [ ] **Step 3: Implement `StopFailure`** in `src/query/loop_impl.rs` at the `BlockingError` branch
- [ ] **Step 4: Implement `InstructionsLoaded`** in CLAUDE.md loading code
- [ ] **Step 5: Implement remaining hooks** (`ConfigChange`, `CwdChanged`, `FileChanged`, `Elicitation`, `ElicitationResult`)
- [ ] **Step 6: Run full test suite**

Run: `cargo test -- --skip ignored`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/tools/ src/query/ src/engine/
git commit -m "feat(hooks): fire remaining lifecycle hooks (SubagentStart, TaskCreated, etc.)"
```

---

### Task 11: Remove dead_code allows and compile-verify

**Files:**
- Modify: `src/tools/hooks.rs`
- Modify: `src/query/stop_hooks.rs`

- [ ] **Step 1: Remove all `#[allow(dead_code)]` from hooks.rs and stop_hooks.rs**

Now that everything is wired, these items are reachable. Remove:
- `#[allow(dead_code)]` on `load_hook_configs`
- `#[allow(dead_code)]` on `run_stop_hooks` in hooks.rs
- `#[allow(dead_code)]` on `PreventStop`, `BlockingError`
- `#[allow(dead_code)]` on `has_tool_use`
- `#[allow(unused_variables)]` on `run_stop_hooks` in stop_hooks.rs

- [ ] **Step 2: Compile and fix any warnings**

Run: `cargo build --release 2>&1 | grep warning`
Expected: Zero warnings in hooks/stop_hooks modules

- [ ] **Step 3: Run full test suite**

Run: `cargo test -- --skip ignored`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git add src/tools/hooks.rs src/query/stop_hooks.rs
git commit -m "fix: remove dead_code allows from hooks system, all paths now reachable"
```

---

## Priority Order

| Phase | Tasks | Impact |
|-------|-------|--------|
| **Phase 1: Wiring** | Tasks 1-4 | Enables PreToolUse, PostToolUse, PostToolUseFailure, Stop — the four most important hooks |
| **Phase 2: Core events** | Tasks 5-6 | UserPromptSubmit, SessionStart/End — essential for hook users |
| **Phase 3: Permissions** | Task 8 | PermissionRequest/Denied — critical for security hooks |
| **Phase 4: Remaining** | Tasks 7, 9-10 | Notification, Compact, Subagent, Task, File — completeness |
| **Phase 5: Cleanup** | Task 11 | Remove dead_code allows |
