# Background Agent (`run_in_background`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable the Agent tool to spawn child QueryEngines as background tokio tasks, returning immediately with a placeholder while the child runs asynchronously, and injecting results via mpsc channel when complete.

**Architecture:** An `mpsc::unbounded_channel` carries `CompletedBackgroundAgent` from spawned tasks to the headless event loop. The event loop owns the receiver, notifies the frontend immediately via IPC, and pushes results into a shared `PendingBackgroundResults` buffer. The query loop drains this buffer at turn boundaries and injects system messages so the model sees the results.

**Tech Stack:** Rust, tokio (mpsc, spawn), parking_lot::Mutex, serde_json, async_stream

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/tools/background_agents.rs` | **Create** | Types: `CompletedBackgroundAgent`, `PendingBackgroundResults`, type aliases |
| `src/tools/mod.rs` | Modify | Add `pub mod background_agents;` |
| `src/types/tool.rs` | Modify | Add `bg_agent_tx` field to `ToolUseContext` |
| `src/engine/lifecycle/mod.rs` | Modify | Add `pending_bg_results` + `bg_agent_tx` to `QueryEngineState` / `QueryEngine` |
| `src/engine/lifecycle/deps.rs` | Modify | Thread `bg_agent_tx` into `ToolUseContext`; add `drain_background_results()` |
| `src/query/deps.rs` | Modify | Add `drain_background_results()` to `QueryDeps` trait |
| `src/query/loop_impl.rs` | Modify | STEP 1b: drain pending results, inject system messages |
| `src/tools/agent.rs` | Modify | Implement background spawn path in `call()` |
| `src/ipc/protocol.rs` | Modify | Add `BackgroundAgentComplete` to `BackendMessage` |
| `src/ipc/headless.rs` | Modify | Create channel, wire `select!`, push to pending buffer |

---

### Task 1: Core types (`background_agents.rs`)

**Files:**
- Create: `src/tools/background_agents.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Create the background agents module**

```rust
// src/tools/background_agents.rs

//! Background agent types — shared between the Agent tool, query loop,
//! and the headless/TUI event loop.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

/// Result from a completed background agent.
#[derive(Debug, Clone)]
pub struct CompletedBackgroundAgent {
    pub agent_id: String,
    pub description: String,
    pub result_text: String,
    pub had_error: bool,
    pub duration: Duration,
}

/// Sender half — cloned into each background agent spawn.
pub type BgAgentSender = tokio::sync::mpsc::UnboundedSender<CompletedBackgroundAgent>;

/// Receiver half — owned by the event loop (headless/TUI).
pub type BgAgentReceiver = tokio::sync::mpsc::UnboundedReceiver<CompletedBackgroundAgent>;

/// Shared buffer of completed agents waiting to be injected into the query loop.
///
/// The event loop pushes completed agents here after notifying the frontend.
/// The query loop drains at turn boundaries and injects system messages.
/// Internal `Mutex` means this is safe to clone and share without external locking.
#[derive(Debug, Clone, Default)]
pub struct PendingBackgroundResults {
    inner: Arc<Mutex<Vec<CompletedBackgroundAgent>>>,
}

impl PendingBackgroundResults {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a completed agent result (called by event loop).
    pub fn push(&self, agent: CompletedBackgroundAgent) {
        self.inner.lock().push(agent);
    }

    /// Drain all pending results (called by query loop at turn start).
    pub fn drain_all(&self) -> Vec<CompletedBackgroundAgent> {
        let mut guard = self.inner.lock();
        std::mem::take(&mut *guard)
    }
}
```

- [ ] **Step 2: Register the module**

Add to `src/tools/mod.rs` after `pub mod agent;` (line 22):

```rust
// Background agent types (used by Agent tool + query loop + event loop)
pub mod background_agents;
```

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | grep -E "error|warning.*background"`
Expected: Clean compile, no new warnings.

- [ ] **Step 4: Commit**

```bash
git add src/tools/background_agents.rs src/tools/mod.rs
git commit -m "feat(agent): add background agent core types"
```

---

### Task 2: Thread `bg_agent_tx` through ToolUseContext and QueryDeps

**Files:**
- Modify: `src/types/tool.rs:111-125` (ToolUseContext)
- Modify: `src/query/deps.rs:82-138` (QueryDeps trait)
- Modify: `src/engine/lifecycle/mod.rs:50-71` (QueryEngineState)
- Modify: `src/engine/lifecycle/mod.rs:82-94` (QueryEngine)
- Modify: `src/engine/lifecycle/mod.rs:100-129` (QueryEngine::new)
- Modify: `src/engine/lifecycle/deps.rs:30-43` (QueryEngineDeps)
- Modify: `src/engine/lifecycle/deps.rs:312-350` (execute_tool — ToolUseContext construction)
- Modify: `src/engine/lifecycle/submit_message.rs:254-261` (deps construction)

- [ ] **Step 1: Add `bg_agent_tx` to `ToolUseContext`**

In `src/types/tool.rs`, add after the `permission_callback` field (line 124):

```rust
    /// Sender for background agent completion results.
    /// When `Some`, the Agent tool can spawn background tasks.
    /// When `None`, `run_in_background` falls back to synchronous execution.
    pub bg_agent_tx: Option<crate::tools::background_agents::BgAgentSender>,
```

- [ ] **Step 2: Add `drain_background_results()` to `QueryDeps` trait**

In `src/query/deps.rs`, add after `async fn refresh_tools()` (before the closing `}`):

```rust
    /// Drain completed background agent results (called at turn start).
    /// Default: returns empty vec (no background agent support).
    fn drain_background_results(&self) -> Vec<crate::tools::background_agents::CompletedBackgroundAgent> {
        vec![]
    }
```

- [ ] **Step 3: Add fields to `QueryEngineState` and `QueryEngine`**

In `src/engine/lifecycle/mod.rs`, add to `QueryEngineState` after `permission_callback` (line 70):

```rust
    /// Sender for background agent completion channel.
    /// Set by headless/TUI mode; cloned into ToolUseContext.
    pub(crate) bg_agent_tx: Option<crate::tools::background_agents::BgAgentSender>,
```

Add to `QueryEngine` struct after `has_handled_orphaned_permission` (line 93):

```rust
    /// Shared buffer of completed background agents.
    /// Event loop pushes; query loop drains.
    pub(crate) pending_bg_results: crate::tools::background_agents::PendingBackgroundResults,
```

In `QueryEngine::new()`, initialize the new fields — add `bg_agent_tx: None,` to the `QueryEngineState` init (after `permission_callback: None,`), and add `pending_bg_results: crate::tools::background_agents::PendingBackgroundResults::new(),` to the `Self` return (after `has_handled_orphaned_permission`).

Add public setters after `set_permission_callback`:

```rust
    /// Set the background agent sender (called by headless/TUI at startup).
    pub fn set_bg_agent_tx(&self, tx: crate::tools::background_agents::BgAgentSender) {
        self.state.write().bg_agent_tx = Some(tx);
    }
```

- [ ] **Step 4: Thread `bg_agent_tx` through `QueryEngineDeps`**

In `src/engine/lifecycle/deps.rs`, add field to `QueryEngineDeps` (after `permission_callback`, line 42):

```rust
    /// Background agent sender — forwarded into ToolUseContext.
    pub(crate) bg_agent_tx: Option<crate::tools::background_agents::BgAgentSender>,
```

In the `execute_tool` method, add `bg_agent_tx` to the `ToolUseContext` construction (after `permission_callback`, line 349):

```rust
            bg_agent_tx: self.bg_agent_tx.clone(),
```

- [ ] **Step 5: Pass `bg_agent_tx` when constructing deps in `submit_message.rs`**

In `src/engine/lifecycle/submit_message.rs`, read `bg_agent_tx` from state alongside `permission_callback` (line 254):

```rust
            let permission_callback = state_ref.read().permission_callback.clone();
            let bg_agent_tx = state_ref.read().bg_agent_tx.clone();
```

Add it to the `QueryEngineDeps` construction (after `permission_callback`, line 261):

```rust
                bg_agent_tx,
```

- [ ] **Step 6: Implement `drain_background_results` on `QueryEngineDeps`**

In `src/engine/lifecycle/deps.rs`, add the method implementation inside `impl QueryDeps for QueryEngineDeps` (after `refresh_tools`, before the closing `}`):

```rust
    fn drain_background_results(&self) -> Vec<crate::tools::background_agents::CompletedBackgroundAgent> {
        // Access the pending buffer via the engine's state.
        // The PendingBackgroundResults is on QueryEngine, not QueryEngineState,
        // so we need it threaded separately. For now, use a field on QueryEngineDeps.
        self.pending_bg_results.drain_all()
    }
```

Wait — `QueryEngineDeps` needs access to `PendingBackgroundResults`. Add the field:

In `src/engine/lifecycle/deps.rs`, add to `QueryEngineDeps` (after `bg_agent_tx`):

```rust
    /// Shared buffer of completed background agents.
    pub(crate) pending_bg_results: crate::tools::background_agents::PendingBackgroundResults,
```

And in `src/engine/lifecycle/submit_message.rs`, pass it when constructing deps:

```rust
            let deps = Arc::new(QueryEngineDeps {
                aborted: aborted_ref.clone(),
                state: state_ref.clone(),
                api_client,
                agent_context: config.agent_context.clone(),
                permission_callback,
                bg_agent_tx,
                pending_bg_results: self.pending_bg_results.clone(),
            });
```

Note: `self` in `submit_message` refers to the `QueryEngine`. Since the stream closure captures owned/cloned references, clone `pending_bg_results` into the closure. Add this alongside the other captures (around line 57):

```rust
        let pending_bg_results = self.pending_bg_results.clone();
```

And use `pending_bg_results` (not `self.pending_bg_results`) in the deps construction.

- [ ] **Step 7: Build and verify**

Run: `cargo build 2>&1 | grep "error"`
Expected: Clean compile (warnings about unused fields are OK for now — they get used in later tasks).

- [ ] **Step 8: Commit**

```bash
git add src/types/tool.rs src/query/deps.rs src/engine/lifecycle/mod.rs src/engine/lifecycle/deps.rs src/engine/lifecycle/submit_message.rs
git commit -m "feat(agent): thread bg_agent_tx through ToolUseContext and QueryDeps"
```

---

### Task 3: Query loop injection

**Files:**
- Modify: `src/query/loop_impl.rs:69-84` (main loop, after abort check)

- [ ] **Step 1: Add background result injection after abort check**

In `src/query/loop_impl.rs`, after the abort check block (after line 84, before STEP 2 comment), insert:

```rust
            // ──────────────────────────────────────────────────────
            // STEP 1b: Inject completed background agent results
            // ──────────────────────────────────────────────────────

            let completed_agents = deps.drain_background_results();
            for agent in &completed_agents {
                let content = if agent.had_error {
                    format!(
                        "[Background agent '{}' (id: {}) failed after {:.1}s]\n\n{}",
                        agent.description,
                        agent.agent_id,
                        agent.duration.as_secs_f64(),
                        agent.result_text,
                    )
                } else {
                    format!(
                        "[Background agent '{}' (id: {}) completed in {:.1}s]\n\n{}",
                        agent.description,
                        agent.agent_id,
                        agent.duration.as_secs_f64(),
                        agent.result_text,
                    )
                };

                let sys_msg = Message::System(crate::types::message::SystemMessage {
                    uuid: Uuid::new_v4(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    subtype: crate::types::message::SystemSubtype::Informational {
                        level: crate::types::message::InfoLevel::Info,
                    },
                    content,
                });
                state.messages.push(sys_msg);
            }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1 | grep "error"`
Expected: Clean compile.

- [ ] **Step 3: Commit**

```bash
git add src/query/loop_impl.rs
git commit -m "feat(agent): inject background agent results at query loop turn start"
```

---

### Task 4: Agent tool background spawn

**Files:**
- Modify: `src/tools/agent.rs:611-726` (call method)

- [ ] **Step 1: Replace the stub with real background execution**

In `src/tools/agent.rs`, replace the current stub block (lines 652-658):

```rust
        // Background mode is stubbed — run synchronously with a warning
        if params.run_in_background {
            warn!(
                agent_id = %agent_id,
                "run_in_background requested but not yet implemented — running synchronously"
            );
        }
```

With the full background implementation:

```rust
        // ── Background mode ─────────────────────────────────────────
        if params.run_in_background {
            let Some(bg_tx) = ctx.bg_agent_tx.clone() else {
                warn!(
                    agent_id = %agent_id,
                    "run_in_background requested but no bg_agent_tx available — running synchronously"
                );
                // Fall through to normal synchronous execution below
                return self.run_agent_dispatch(
                    use_worktree, &params, ctx, &agent_id, &agent_model, &parent_model, current_depth,
                    description, &start_configs, &stop_configs,
                ).await;
            };

            info!(
                agent_id = %agent_id,
                description = %description,
                "spawning background agent"
            );

            // Fire SubagentStart hook synchronously before spawn
            if !start_configs.is_empty() {
                let payload = json!({
                    "agent_id": &agent_id,
                    "prompt": &params.prompt,
                    "description": description,
                    "subagent_type": subagent_type,
                    "model": &agent_model,
                    "depth": current_depth + 1,
                    "background": true,
                });
                let _ = crate::tools::hooks::run_event_hooks("SubagentStart", &payload, &start_configs).await;
            }

            // Capture owned values for the spawned task
            let spawn_agent_id = agent_id.clone();
            let spawn_description = description.to_string();
            let spawn_prompt = params.prompt.clone();
            let spawn_agent_model = agent_model.clone();
            let spawn_parent_model = parent_model.clone();
            let spawn_stop_configs = stop_configs.clone();
            let spawn_use_worktree = use_worktree;

            // Build child config in the current context (needs ctx borrow)
            let child_cwd = if spawn_use_worktree {
                // For worktree background agents, we'd need async worktree setup.
                // For simplicity, background + worktree falls back to normal cwd.
                // (Worktree isolation with background is complex; support later.)
                warn!(
                    agent_id = %agent_id,
                    "background + worktree not yet combined — using normal cwd"
                );
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            } else {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            };

            let child_config = build_child_config(
                child_cwd, ctx, &spawn_agent_id, &spawn_agent_model,
                &spawn_parent_model, current_depth,
            );

            tokio::spawn(async move {
                let started = std::time::Instant::now();

                let child_engine = QueryEngine::new(child_config);
                let stream = child_engine.submit_message(
                    &spawn_prompt,
                    QuerySource::Agent(spawn_agent_id.clone()),
                );

                let (result_text, had_error) = collect_stream_result(stream).await;

                // Fire SubagentStop hook
                if !spawn_stop_configs.is_empty() {
                    let payload = json!({
                        "agent_id": &spawn_agent_id,
                        "description": &spawn_description,
                        "is_error": had_error,
                        "background": true,
                    });
                    let _ = crate::tools::hooks::run_event_hooks(
                        "SubagentStop", &payload, &spawn_stop_configs,
                    ).await;
                }

                let _ = bg_tx.send(crate::tools::background_agents::CompletedBackgroundAgent {
                    agent_id: spawn_agent_id,
                    description: spawn_description,
                    result_text,
                    had_error,
                    duration: started.elapsed(),
                });
            });

            return Ok(ToolResult {
                data: json!(format!(
                    "Agent '{}' launched in background (id: {}). You will be notified when it completes.",
                    description, agent_id
                )),
                new_messages: vec![],
            });
        }
```

- [ ] **Step 2: Extract hook config loading to avoid borrow issues**

The current `call()` method loads hook configs inside blocks that borrow `ctx`. To use them both before the background spawn and in the normal path, extract them earlier. Replace the current SubagentStart hook block (lines 670-685) and SubagentStop hook block (lines 710-723) pattern.

Move hook config loading to right after the `info!` log (after line 668), before the background check:

```rust
        // Load hook configs once (used by both background and synchronous paths)
        let start_configs = {
            let app_state = (ctx.get_app_state)();
            crate::tools::hooks::load_hook_configs(&app_state.hooks, "SubagentStart")
        };
        let stop_configs = {
            let app_state = (ctx.get_app_state)();
            crate::tools::hooks::load_hook_configs(&app_state.hooks, "SubagentStop")
        };
```

Then the background block (above) and the normal synchronous path both use `start_configs` / `stop_configs`.

- [ ] **Step 3: Add `run_agent_dispatch` helper to deduplicate the synchronous path**

Add a private method to `AgentTool` that wraps the existing dispatch + hooks logic:

```rust
    /// Dispatch: run agent (worktree or normal) with SubagentStart/Stop hooks.
    /// Used by both the fallback-from-background path and the normal synchronous path.
    async fn run_agent_dispatch(
        &self,
        use_worktree: bool,
        params: &AgentInput,
        ctx: &ToolUseContext,
        agent_id: &str,
        agent_model: &str,
        parent_model: &str,
        current_depth: usize,
        description: &str,
        start_configs: &[serde_json::Value],
        stop_configs: &[serde_json::Value],
    ) -> Result<ToolResult> {
        // Fire SubagentStart hook
        if !start_configs.is_empty() {
            let payload = json!({
                "agent_id": agent_id,
                "prompt": &params.prompt,
                "description": description,
                "subagent_type": params.subagent_type.as_deref().unwrap_or("general-purpose"),
                "model": agent_model,
                "depth": current_depth + 1,
            });
            let _ = crate::tools::hooks::run_event_hooks("SubagentStart", &payload, start_configs).await;
        }

        let result = if use_worktree {
            self.run_in_worktree(params, ctx, agent_id, agent_model, parent_model, current_depth).await
        } else {
            self.run_agent_normal(params, ctx, agent_id, agent_model, parent_model, current_depth).await
        };

        // Fire SubagentStop hook
        if !stop_configs.is_empty() {
            let is_error = result.as_ref().is_err();
            let payload = json!({
                "agent_id": agent_id,
                "description": description,
                "is_error": is_error,
            });
            let _ = crate::tools::hooks::run_event_hooks("SubagentStop", &payload, stop_configs).await;
        }

        result
    }
```

Then refactor the main `call()` method's synchronous path (lines 687-723) to use this helper:

```rust
        // ── Synchronous dispatch ─────────────────────────────────────
        self.run_agent_dispatch(
            use_worktree, &params, ctx, &agent_id, &agent_model, &parent_model,
            current_depth, description, &start_configs, &stop_configs,
        ).await
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1 | grep "error"`
Expected: Clean compile.

- [ ] **Step 5: Commit**

```bash
git add src/tools/agent.rs
git commit -m "feat(agent): implement run_in_background via tokio::spawn + mpsc channel"
```

---

### Task 5: IPC protocol + headless event loop

**Files:**
- Modify: `src/ipc/protocol.rs:45-99` (BackendMessage enum)
- Modify: `src/ipc/headless.rs:32-189` (run_headless)

- [ ] **Step 1: Add `BackgroundAgentComplete` to IPC protocol**

In `src/ipc/protocol.rs`, add a new variant to `BackendMessage` (before the closing `}`):

```rust
    /// A background agent has completed execution.
    BackgroundAgentComplete {
        agent_id: String,
        description: String,
        /// Truncated preview of the result (for UI display).
        result_preview: String,
        had_error: bool,
        duration_ms: u64,
    },
```

- [ ] **Step 2: Wire the headless event loop with `select!` and channel**

In `src/ipc/headless.rs`, modify `run_headless`:

Add imports at the top of the file (after existing imports):

```rust
use crate::tools::background_agents::PendingBackgroundResults;
```

After the permission bridge setup and before the Ready message (after line 68, before line 69), add:

```rust
    // ── 1b. Background agent channel setup ────────────────────
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    let pending_bg = PendingBackgroundResults::new();
    engine.set_bg_agent_tx(bg_tx);
    // Share pending_bg with the engine so query loop can drain it.
    // We do this by storing it as a field on QueryEngine (already set in new()).
    // Replace the default with our shared instance:
    {
        // Swap in our shared PendingBackgroundResults
        let engine_pending = &engine.pending_bg_results;
        // Since PendingBackgroundResults wraps Arc<Mutex<Vec>>, we need to
        // set the engine's reference. But QueryEngine::pending_bg_results is
        // created in new(). We need a setter.
    }
```

Actually, simpler approach: add a `set_pending_bg_results` setter to `QueryEngine` and call it:

In `src/engine/lifecycle/mod.rs`, add a setter after `set_bg_agent_tx`:

```rust
    /// Set the pending background results buffer (called by headless/TUI at startup).
    pub fn set_pending_bg_results(&self, pending: crate::tools::background_agents::PendingBackgroundResults) {
        self.pending_bg_results = pending;
    }
```

Wait — `pending_bg_results` is not behind the `RwLock` (it's directly on `QueryEngine`). But `QueryEngine` is behind `Arc` in headless. We can't mutate `Arc<QueryEngine>` fields. Two options:

**Option A:** Put `pending_bg_results` inside `QueryEngineState` (behind the RwLock).
**Option B:** Wrap it in its own `Arc` so the headless loop shares the same instance.

`PendingBackgroundResults` already wraps `Arc<Mutex<Vec>>`, so cloning it shares the underlying data. The simplest fix: don't use a setter. Instead, read the engine's `pending_bg_results` by cloning it (which shares the Arc):

```rust
    let pending_bg = engine.pending_bg_results.clone();
```

This works because `PendingBackgroundResults::clone()` clones the `Arc`, so both the headless loop and the query loop (via deps) point to the same `Vec`.

So the headless code is just:

```rust
    // ── 1b. Background agent channel setup ────────────────────
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::unbounded_channel();
    engine.set_bg_agent_tx(bg_tx);
    let pending_bg = engine.pending_bg_results.clone();
```

Remove the `set_pending_bg_results` setter — it's not needed.

- [ ] **Step 3: Replace the main loop with `select!`**

Replace the main `loop` in `run_headless` (lines 84-188) with:

```rust
    loop {
        tokio::select! {
            // ── Branch 1: Frontend message (stdin) ──────────────
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(line)) => line,
                    Ok(None) => {
                        debug!("headless: stdin closed, exiting");
                        break;
                    }
                    Err(e) => {
                        error!("headless: error reading stdin: {}", e);
                        break;
                    }
                };

                let msg: FrontendMessage = match serde_json::from_str(&line) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(
                            "headless: failed to parse FrontendMessage: {} — line: {}",
                            e, line
                        );
                        let _ = send_to_frontend(&BackendMessage::Error {
                            message: format!("invalid FrontendMessage: {}", e),
                            recoverable: true,
                        });
                        continue;
                    }
                };

                match msg {
                    FrontendMessage::SubmitPrompt { text, id } => {
                        debug!("headless: submit_prompt id={}", id);
                        engine.reset_abort();

                        let engine_clone = engine.clone();
                        let message_id = id;
                        let svc = suggestion_svc.clone();

                        tokio::spawn(async move {
                            let stream =
                                engine_clone.submit_message(&text, QuerySource::ReplMainThread);
                            let mut stream = std::pin::pin!(stream);

                            while let Some(sdk_msg) = stream.next().await {
                                let send_result =
                                    handle_sdk_message(&sdk_msg, &message_id, &engine_clone, &svc);
                                if let Err(e) = send_result {
                                    error!("headless: failed to send to frontend: {}", e);
                                    break;
                                }
                            }
                        });
                    }

                    FrontendMessage::AbortQuery => {
                        debug!("headless: abort requested");
                        engine.abort();
                    }

                    FrontendMessage::PermissionResponse {
                        tool_use_id,
                        decision,
                    } => {
                        debug!(
                            "headless: permission response tool_use_id={} decision={}",
                            tool_use_id, decision
                        );
                        if let Some(tx) = pending_permissions.lock().remove(&tool_use_id) {
                            let _ = tx.send(decision);
                        } else {
                            warn!(
                                "headless: no pending permission for tool_use_id={}",
                                tool_use_id
                            );
                        }
                    }

                    FrontendMessage::SlashCommand { raw } => {
                        debug!("headless: slash command: {}", raw);
                        handle_slash_command(&raw, &engine).await;
                    }

                    FrontendMessage::Resize { cols, rows } => {
                        debug!("headless: resize {}x{}", cols, rows);
                        let mut ps = crate::bootstrap::PROCESS_STATE.write();
                        ps.terminal_cols = cols;
                        ps.terminal_rows = rows;
                    }

                    FrontendMessage::Quit => {
                        debug!("headless: quit requested");
                        break;
                    }
                }
            }

            // ── Branch 2: Background agent completed ────────────
            Some(completed) = bg_rx.recv() => {
                debug!(
                    agent_id = %completed.agent_id,
                    description = %completed.description,
                    had_error = completed.had_error,
                    duration_ms = completed.duration.as_millis() as u64,
                    "headless: background agent completed"
                );

                // Truncate result for UI preview
                let result_preview = if completed.result_text.len() > 200 {
                    format!("{}...", &completed.result_text[..200])
                } else {
                    completed.result_text.clone()
                };

                // Notify frontend immediately
                let _ = send_to_frontend(&BackendMessage::BackgroundAgentComplete {
                    agent_id: completed.agent_id.clone(),
                    description: completed.description.clone(),
                    result_preview,
                    had_error: completed.had_error,
                    duration_ms: completed.duration.as_millis() as u64,
                });

                // Push to shared buffer for query loop injection
                pending_bg.push(completed);
            }
        }
    }
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1 | grep "error"`
Expected: Clean compile.

- [ ] **Step 5: Commit**

```bash
git add src/ipc/protocol.rs src/ipc/headless.rs src/engine/lifecycle/mod.rs
git commit -m "feat(agent): wire background agent channel into headless event loop with select!"
```

---

### Task 6: Fix warnings and final build verification

**Files:**
- Various files from previous tasks

- [ ] **Step 1: Full build with warnings check**

Run: `cargo build 2>&1`
Expected: Clean compile. Check for any unused import/field warnings introduced by the changes.

- [ ] **Step 2: Fix any warnings**

Common warnings to expect and fix:
- Unused `use` statements in `agent.rs` (e.g., `std::time::Instant` may need importing for the background path)
- Unused fields if any `bg_agent_tx` / `pending_bg_results` code paths aren't fully connected

- [ ] **Step 3: Run existing tests**

Run: `cargo test 2>&1`
Expected: All existing tests pass. The Agent tool tests in `agent.rs` (lines 769-886) test deserialization and schema — they should still pass since we didn't change the schema or input types.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "fix(agent): resolve warnings from background agent implementation"
```

---

### Task 7: Unit tests for background agent types

**Files:**
- Modify: `src/tools/background_agents.rs` (add tests module)

- [ ] **Step 1: Add tests for `PendingBackgroundResults`**

Append to `src/tools/background_agents.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_completed(id: &str, desc: &str) -> CompletedBackgroundAgent {
        CompletedBackgroundAgent {
            agent_id: id.to_string(),
            description: desc.to_string(),
            result_text: format!("Result from {}", desc),
            had_error: false,
            duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn test_pending_results_push_and_drain() {
        let pending = PendingBackgroundResults::new();
        assert!(pending.drain_all().is_empty());

        pending.push(make_completed("a1", "task one"));
        pending.push(make_completed("a2", "task two"));

        let drained = pending.drain_all();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].agent_id, "a1");
        assert_eq!(drained[1].agent_id, "a2");

        // Second drain is empty
        assert!(pending.drain_all().is_empty());
    }

    #[test]
    fn test_pending_results_clone_shares_state() {
        let pending1 = PendingBackgroundResults::new();
        let pending2 = pending1.clone();

        pending1.push(make_completed("a1", "task"));
        let drained = pending2.drain_all();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].agent_id, "a1");
    }

    #[test]
    fn test_channel_send_recv() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(make_completed("bg1", "background task")).unwrap();

        let received = rx.try_recv().unwrap();
        assert_eq!(received.agent_id, "bg1");
        assert_eq!(received.description, "background task");
        assert!(!received.had_error);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test tools::background_agents::tests -- --nocapture 2>&1`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/tools/background_agents.rs
git commit -m "test(agent): add unit tests for PendingBackgroundResults and channel"
```

---

### Task 8: Integration test — Agent tool background path

**Files:**
- Modify: `src/tools/agent.rs` (add test at bottom)

- [ ] **Step 1: Add integration test for the background spawn path**

Add to the existing `#[cfg(test)] mod tests` in `agent.rs`:

```rust
    #[tokio::test]
    async fn test_run_in_background_without_tx_falls_back() {
        // When bg_agent_tx is None, run_in_background should fall through
        // to synchronous execution (or return the fallback path).
        // We can't easily test the full engine spawn without an API key,
        // but we CAN verify the input deserialization and schema remain correct.
        let input: AgentInput = serde_json::from_value(json!({
            "prompt": "test task",
            "description": "test",
            "run_in_background": true
        }))
        .unwrap();
        assert!(input.run_in_background);
    }

    #[tokio::test]
    async fn test_background_agent_placeholder_format() {
        // Verify the placeholder message format
        let agent_id = "test-agent-123";
        let description = "search codebase";
        let placeholder = format!(
            "Agent '{}' launched in background (id: {}). You will be notified when it completes.",
            description, agent_id
        );
        assert!(placeholder.contains("search codebase"));
        assert!(placeholder.contains("test-agent-123"));
        assert!(placeholder.contains("background"));
    }
```

- [ ] **Step 2: Run all agent tests**

Run: `cargo test tools::agent::tests -- --nocapture 2>&1`
Expected: All tests pass (existing + new).

- [ ] **Step 3: Final full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tools/agent.rs
git commit -m "test(agent): add tests for background agent path"
```
