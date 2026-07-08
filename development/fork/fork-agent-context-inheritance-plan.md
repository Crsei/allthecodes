# Fork Agent Context Inheritance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make fork agents choose what parent information they inherit, instead of always receiving only a fixed content prompt.

**Architecture:** Add an explicit fork context selection surface to `AgentTool`, then route fork startup through a context builder that can create a full parent snapshot, a last-output-only prompt, or a read-only live parent context channel. TUI and prompt text must expose the selected inheritance mode so both the model and user can understand what the child agent received.

**Tech Stack:** Rust, `allthecodes-engine`, `allthecodes-types`, `allthecodes-tools`, Rust TUI under `crates/allthecodes/src/ui/`, existing task output/event infrastructure.

## Global Constraints

- Default fork inheritance mode is `full_snapshot`.
- Manual selection is through AgentTool parameters, not a required TUI prompt.
- `live_readonly` is a readable parent-output channel, not automatic injection of new parent messages into the child model context.
- Existing lightweight fork behavior for `/btw`, `/simplify`, and `SkillContext::Fork` remains unchanged unless explicitly migrated later.
- Upstream `FORK_SUBAGENT` parity behavior is preserved as documented TODOs only in this phase.

---

## Current State

- `crates/allthecodes-engine/src/agent/fork.rs` supports `parent_messages`, but this is a lightweight ephemeral fork primitive and is not the full AgentTool fork-subagent behavior.
- `crates/allthecodes-engine/src/agent/mod.rs::build_child_config` currently starts normal AgentTool children with `initial_messages: None`.
- `crates/allthecodes-engine/src/agent/tool_impl.rs` defaults missing `subagent_type` to `general-purpose`; it does not expose a fork inheritance strategy.
- TUI agent navigation consumes `AgentEvent::Spawned` and related events, but does not display parent context inheritance mode.

## Proposed Public Surface

AgentTool input gains:

```json
{
  "fork": true,
  "fork_context": "full_snapshot"
}
```

`fork_context` values:

- `full_snapshot`: inherit a snapshot of the parent conversation history at spawn time.
- `last_output`: inherit only the parent agent's latest assistant text/result plus the fork directive.
- `live_readonly`: provide a read-only live parent context channel that the child can inspect with tools when it needs the latest parent state.

Defaulting rules:

- If `fork == true` and `fork_context` is absent, use `full_snapshot`.
- If `fork_context` is present while `fork != true`, return a clear validation error.
- Missing `subagent_type` should not automatically mean fork. Fork must be explicit in this phase.

## Context Modes

### full_snapshot

Build child `initial_messages` from the parent context:

- Include parent conversation history available in `ToolUseContext.messages`.
- Include the current parent assistant message when needed to preserve active tool-use context.
- Use stable placeholder tool results for active parent tool uses so multiple fork children can share a cache-friendly prefix later.
- Append a fork directive message telling the child it is a fork worker and specifying the requested task.

### last_output

Build a small child context:

- Extract the latest parent assistant text/result from `ToolUseContext.messages` or the current assistant message.
- Do not include full parent history.
- Add a short note that only the parent's last visible output was inherited.
- Append the fork directive.

### live_readonly

Create a read-only parent context channel:

- Create a fork-specific context directory under the allthecodes session data area.
- Write `parent-context.md` with the latest rendered parent context snapshot.
- Append parent updates to `parent-updates.ndjson` with `seq`, timestamp, source, and text/summary fields.
- Optionally write `parent-context.diff` as a convenience view of new parent output since the previous snapshot.
- Inject the channel paths into the child prompt and tell the child to `Read` the files when it needs current parent output.
- Keep initial child context small; the live channel is available on demand.

This mode intentionally behaves like a readable status/diff file, not like real-time model-context mutation.

## TUI And Prompt Updates

- Add `fork_context` metadata to spawned agent events or agent node metadata.
- Show fork inheritance mode in agent navigation and task surfaces:
  - `fork · context: full snapshot`
  - `fork · context: last output`
  - `fork · context: live readonly`
- For `live_readonly`, include the short context file path in task details or the agent launch message.
- Update AgentTool prompt so the model knows:
  - Use `full_snapshot` for work that depends on full prior discussion.
  - Use `last_output` for quick verification or follow-up on the latest result.
  - Use `live_readonly` when parent output may keep changing and the child can inspect a file channel for updates.

## Upstream Parity TODOs

These upstream `fork-subagent.md` behaviors are intentionally not implemented in this phase. Leave TODO comments at the future extension points:

- TODO: `FEATURE_FORK_SUBAGENT` gate and automatic routing behavior.
- TODO: coordinator and non-interactive mutual exclusion.
- TODO: exact rendered system prompt inheritance for byte-identical cache prefix.
- TODO: exact parent tool pool inheritance and `useExactTools` equivalent.
- TODO: thinking config and content replacement state cloning.
- TODO: permission bubble behavior.
- TODO: recursive fork guard using query source and `<fork-boilerplate>` scan.
- TODO: forced async behavior and `/fork` slash command stub.
- TODO: worktree-specific inherited path translation notice.

## Upstream Gap Review

The first five tasks cover the selectable parent-information modes. A complete forkagent build also needs the following work from the upstream design:

- Fork launch metadata must survive every path: sync, background, worktree, hooks, IPC events, tree snapshots, and task output.
- Parent runtime state must be capturable at AgentTool call time. `ToolUseContext` currently has messages and tools, but not rendered system prompt, query source, thinking config, or content replacement state.
- Fork message construction must be centralized. The placeholder tool-result text, `<fork-boilerplate>` wrapper, and incomplete-tool-call filtering are part of the feature contract, even if exact prompt-cache parity remains TODO.
- `live_readonly` needs a real update source and lifecycle. The plan must define how parent output is appended, how children discover updates, and how channel files are retained or cleaned up.
- Background/resume behavior needs metadata. The existing task output path can carry fork results, but it must also carry fork context mode and live channel paths so TUI state can be reconstructed.
- Slash-command and gate behavior should stay TODO-only for this phase, but the code should have stable extension points so `/fork`, feature flags, non-interactive disabling, permission bubbling, and worktree notices are not rediscovered later.

## Task Breakdown

### Task 1: Define Fork Context Types

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Modify: `crates/allthecodes-types/src/agent_events.rs`

- [ ] Add `ForkContextMode` enum with `FullSnapshot`, `LastOutput`, and `LiveReadonly`.
- [ ] Parse AgentTool JSON fields `fork` and `fork_context`.
- [ ] Validate that `fork_context` only applies when `fork == true`.
- [ ] Add optional fork context metadata to spawned agent event/node data.
- [ ] Add unit tests for parsing, defaults, and validation errors.

### Task 2: Build Snapshot Contexts

**Files:**
- Create: `crates/allthecodes-engine/src/agent/fork_context.rs`
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`

- [ ] Implement a context builder that receives `ToolUseContext`, current assistant message, prompt directive, and `ForkContextMode`.
- [ ] Implement `full_snapshot` child `initial_messages`.
- [ ] Implement `last_output` child `initial_messages`.
- [ ] Thread the selected `initial_messages` into child `QueryEngineConfig`.
- [ ] Add unit tests proving `full_snapshot` carries parent history and `last_output` does not.

### Task 3: Add Live Read-Only Parent Context Channel

**Files:**
- Create: `crates/allthecodes-engine/src/agent/live_parent_context.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`

- [ ] Create per-fork context channel files in the session data area.
- [ ] Write `parent-context.md` before launching the child.
- [ ] Append parent updates to `parent-updates.ndjson` while the fork is active.
- [ ] Inject channel file paths into the child directive for `live_readonly`.
- [ ] Ensure files are read-only from the child's perspective by policy/prompt first; add stricter permission enforcement later if needed.
- [ ] Add tests for file creation, update append, and child prompt path injection.

### Task 4: Update Prompt And TUI

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Modify: `crates/allthecodes/src/ui/app.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/tasks.rs`

- [ ] Update AgentTool prompt with fork context selection guidance.
- [ ] Show fork context mode in agent navigation/task surfaces.
- [ ] Show live context channel path for `live_readonly`.
- [ ] Add TUI tests for spawned fork agents displaying inheritance mode.

### Task 5: Preserve Existing Lightweight Forks

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/fork.rs`
- Modify: `crates/allthecodes-engine/src/skill_tool.rs`
- Modify: `crates/allthecodes-commands/src/btw.rs`
- Modify: `crates/allthecodes-commands/src/simplify.rs`

- [ ] Update comments so lightweight fork is clearly documented as separate from AgentTool fork context inheritance.
- [ ] Confirm `/btw`, `/simplify`, and fork skills still use the existing lightweight fork path.
- [ ] Add regression tests or assertions that these commands do not require AgentTool `fork_context`.

### Task 6: Make Fork Metadata First-Class Across Runtime Paths

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`
- Modify: `crates/allthecodes-engine/src/agent/worktree.rs`
- Modify: `crates/allthecodes-types/src/agent_types.rs`
- Modify: `crates/allthecodes-types/src/agent_events.rs`

- [ ] Add a reusable fork launch metadata struct with `is_fork`, `fork_context`, and optional live-channel paths.
- [ ] Thread metadata through sync, background, and worktree spawn paths instead of only through normal `AgentTool` JSON parsing.
- [ ] Emit fork metadata in `AgentEvent::Spawned` and store it in `AgentNode` so tree snapshots reconstruct the same TUI state.
- [ ] Include fork metadata in `emit_subagent_event`, `SubagentStart`, and `SubagentStop` payloads.
- [ ] Ensure fork agents display `agent_type: fork` when no explicit specialized type is selected.

### Task 7: Capture Parent Runtime State At Tool Call Time

**Files:**
- Modify: `crates/allthecodes-tools/src/tool.rs`
- Modify: `crates/allthecodes-engine/src/lifecycle/*`
- Modify: `crates/allthecodes-engine/src/agent/fork_context.rs`
- Modify: `crates/allthecodes-engine/src/agent/mod.rs`

- [ ] Add optional parent runtime fields needed by fork context construction: rendered system prompt, query source, and thinking config.
- [ ] Use parent rendered prompt for fork children when available; leave exact byte-for-byte prompt cache parity as a TODO hook if the current config cannot preserve it yet.
- [ ] Preserve parent tool availability for fork context decisions; keep exact `useExactTools` parity as TODO unless implemented directly.
- [ ] Add a clear TODO extension point for ContentReplacementState cloning, since allthecodes currently has no equivalent field in `ToolUseContext`.
- [ ] Add tests that prove fork context construction receives the same parent runtime fields seen by the AgentTool call.

### Task 8: Centralize Fork Message And XML Contract

**Files:**
- Create or modify: `crates/allthecodes-engine/src/agent/fork_context.rs`
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Modify: `crates/allthecodes/src/ui/messages/user_text_message.rs`

- [ ] Define constants for `<fork-boilerplate>`, `<task-notification>`, and the stable placeholder text `Fork started — processing in background`.
- [ ] Implement incomplete-tool-call filtering before building `full_snapshot` messages.
- [ ] Clone the current parent assistant message, including text, thinking, and tool-use blocks that must remain visible to the child.
- [ ] Add placeholder tool results for active parent tool uses before the fork directive.
- [ ] Make `last_output` share the same directive builder but use only the latest parent visible output as inherited context.
- [ ] Keep TUI XML rendering aligned with the same constants instead of hard-coded tag strings.

### Task 9: Complete Live Read-Only Channel Semantics

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/live_parent_context.rs`
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/worktree.rs`
- Modify: `crates/allthecodes-tools/src/tasks/mod.rs`

- [ ] Define the per-fork channel directory layout under the allthecodes session data area.
- [ ] Append parent visible output updates with a monotonic `seq`, timestamp, source, and compact text field.
- [ ] Add an optional latest-diff view so child agents can inspect only new parent output.
- [ ] Inject the channel paths and latest sequence into the child directive.
- [ ] Provide a child-visible reminder mechanism for `live_readonly` before later model/tool turns, without automatically injecting full parent messages.
- [ ] Define retention and cleanup behavior for normal completion, abort, crash recovery, and resumed sessions.
- [ ] Add policy or permission TODOs for making the live files read-only from the child side beyond prompt guidance.

### Task 10: Align Background, Task Output, And Resume

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/supervisor.rs`
- Modify: `crates/allthecodes-tasks/src/*`
- Modify: `crates/allthecodes-ipc/src/agent_handlers.rs`
- Modify: `crates/allthecodes-ipc-protocol/src/protocol/mod.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/tasks.rs`

- [ ] Carry fork metadata into `TaskOutput` and `BackgroundAgentComplete` payloads.
- [ ] Make `QueryAgentOutput` return enough metadata for TUI to show fork context mode after a refresh or reconnect.
- [ ] Preserve live-channel path display for background fork agents.
- [ ] Add resume hooks or TODO stubs matching upstream `resumeAgent.ts`, including how resumed fork agents recover context mode and live-channel state.
- [ ] Decide whether explicit fork respects `run_in_background` in this phase; keep upstream forced-async behavior as a TODO-only policy.

### Task 11: Add Explicit TODO Hooks For Deferred Upstream Behaviors

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/tool_impl.rs`
- Modify: `crates/allthecodes-engine/src/agent/dispatch.rs`
- Modify: `crates/allthecodes-engine/src/agent/worktree.rs`
- Modify: `crates/allthecodes-commands/src/*`

- [ ] Add TODO hooks for `FEATURE_FORK_SUBAGENT`, coordinator-mode disabling, and non-interactive disabling without changing behavior.
- [ ] Add TODO hooks for automatic fork routing when `subagent_type` is absent; explicit `fork: true` remains the only active route.
- [ ] Add TODO hooks for nested fork guards using query source plus `<fork-boilerplate>` scan.
- [ ] Add TODO hooks for permission bubbling.
- [ ] Add TODO hooks for fork + worktree path translation notices.
- [ ] Add or document a `/fork` slash-command stub and `/branch` alias conflict handling without enabling upstream automatic behavior.

### Task 12: Add End-To-End Coverage For The Forkagent Contract

**Files:**
- Modify: `crates/allthecodes-engine/src/agent/tests.rs`
- Modify: `crates/allthecodes-engine/src/agent/*`
- Modify: `crates/allthecodes/src/ui/app/tests.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/tasks.rs`

- [ ] Test explicit `fork: true` defaults to `full_snapshot`.
- [ ] Test `fork_context` without `fork: true` fails validation.
- [ ] Test `full_snapshot` includes parent history and current assistant placeholder tool results.
- [ ] Test `last_output` excludes earlier parent history.
- [ ] Test `live_readonly` creates readable channel files and injects paths into the child directive.
- [ ] Test sync, background, and worktree fork spawns all emit fork metadata.
- [ ] Test TUI tree and task surfaces show the selected fork context mode after spawned events and tree snapshots.
- [ ] Test ordinary agents, teammates, `/btw`, `/simplify`, and fork skills do not silently switch to AgentTool fork mode.

## Verification Plan

- Run focused unit tests for agent parsing/context builder:
  - `cargo test -p allthecodes-engine agent::`
- Run command regressions:
  - `cargo test -p allthecodes-commands btw simplify branch`
- Run TUI focused tests for agent event display:
  - `cargo test -p allthecodes --lib agent`
- Run IPC/task output focused tests once fork metadata is threaded:
  - `cargo test -p allthecodes-ipc agent`
  - `cargo test -p allthecodes-tasks`
- Before commit, run the repository-required build:
  - `cargo build --workspace --release`

## Acceptance Criteria

- Fork AgentTool calls can explicitly select `full_snapshot`, `last_output`, or `live_readonly`.
- Default fork context is `full_snapshot`.
- TUI shows the selected inheritance mode.
- Fork metadata survives sync, background, worktree, task-output, and tree-snapshot paths.
- `live_readonly` gives the child a readable parent-output channel and does not mutate child model history after launch.
- Existing lightweight fork users keep working unchanged.
- Upstream parity behavior not implemented in this phase is represented as TODOs at extension points, not silently omitted.
