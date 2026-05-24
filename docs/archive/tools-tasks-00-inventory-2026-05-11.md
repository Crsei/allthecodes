# tools-tasks-00 inventory checkpoint

Date: 2026-05-11

Scope: inventory only. No code was moved.

## Tool/task call sites

- Runtime registry: `crates/claude-code-rs/src/tools/registry.rs` builds the default tool pool. Task tools are registered as `TodoWrite`, `TaskCreate`, `TaskGet`, `TaskUpdate`, `TaskList`, `TaskStop`, and `TaskOutput` at lines 79-85.
- Runtime policy filtering: `registry.rs` exposes task tools differently by policy:
  - coordinator: `TaskList`, `TaskStop`
  - coordinator worker: `TodoWrite`, `TaskList`, `TaskUpdate`
  - in-process teammate: `TodoWrite`, `TaskList`, `TaskUpdate`, `TaskOutput`
- Query execution boundary: `crates/cc-query/src/deps.rs` defines `ToolExecRequest`, `ToolExecResult`, and `QueryDeps::execute_tool`.
- Main query loop: `crates/cc-query/src/loop_impl.rs` extracts assistant `tool_use` blocks through `stop_hooks::extract_tool_uses`, then calls `execute_tool_calls`; the streaming path uses `StreamingToolExecutor` before falling back to post-stream execution for remaining tool uses.
- Tool batching/result pairing: `crates/cc-query/src/loop_helpers.rs` owns `StreamingToolExecutor`, `execute_tool_calls`, join-error-to-tool-result synthesis, streamed/remaining result merge, observable input backfill, and tool-result message conversion.
- Canonical execution implementation: `crates/cc-engine/src/lifecycle/deps.rs` implements `QueryDeps::execute_tool`, including validation, hooks, permissions, progress callback decoration, result size enforcement, and audit context.
- Task command surface: `crates/claude-code-rs/src/commands/tasks_cmd.rs` aggregates tool tasks through `tools::tasks::global_store()` and team tasks for `/tasks` list/show/stop/delete.

## Task persistence shapes

- Public in-memory task shape: `crates/claude-code-rs/src/tools/tasks/store.rs` `TaskEntry` includes id, kind, subject, description, status, output metadata, parent/dependency fields, owner, `active_form`, `metadata`, tool/agent/supervisor/worktree metadata, remote metadata, poll/cancel/recovery fields, and timestamps.
- Status enum: `TaskStatus` serializes as snake_case and accepts legacy aliases such as `running` -> `in_progress` and `canceled` -> `cancelled`.
- Create/update input shapes:
  - `TaskCreateOptions` carries dependency, owner, active form, metadata, tool/agent/supervisor, isolation/worktree, remote task, and poll fields.
  - `TaskUpdateFields` supports subject, description, active form, owner, metadata patch, status, `addBlocks`, and `addBlockedBy`.
- Disk shape: `crates/claude-code-rs/src/tools/tasks/repository.rs` persists `PersistedTaskFile { schema_version, task }`; `PersistedTaskRecord` stores `output_file` plus output summary/bytes/truncation instead of requiring inline output.
- Legacy compatibility: repository loading accepts legacy unversioned records and dependency aliases `blocked_by` / `blockedBy`.
- Storage layout: task-list scoped repository under the cc-rust task root; `.lock` guards list writes, `.highwatermark` and `.highwatermark.lock` reserve monotonic task ids, and `output_file` preserves large output out of the JSON record.
- JSON output shape: `crates/claude-code-rs/src/tools/tasks/json.rs` emits `depends_on`, `blocked_by`, `blockedBy`, reverse `blocks`, `activeForm`, `metadata`, remote metadata, and `blocked_dependencies`.
- Runtime-only shape: `TaskRuntimeHandle` stores a cancellation token and is not durable; restart recovery is represented on task entries instead.

## Baseline tests

- Task unit/module baseline: `cargo test -p claude-code-rs tools::tasks -- --nocapture`
  - Covers task create/get/list/update/stop/output, storage migration, task-list isolation, id highwatermark, dependency aliases, activeForm/metadata persistence, remote metadata, runtime cancellation, output waiting, and schema exposure.
- Task command baseline: `cargo test -p claude-code-rs commands::tasks_cmd -- --nocapture`
  - Covers `/tasks` stop/show/delete integration with tool-backed tasks.
- Registry baseline: `cargo test -p claude-code-rs tools::registry -- --nocapture`
  - Covers tool uniqueness, schema presence, and coordinator/worker/in-process task tool visibility.
- Query tool execution baseline: `cargo test -p cc-query loop_helpers -- --nocapture`
  - Covers batching, result conversion, streamed result merge, observable input backfill, and synthesized failed results for spawned tool panics.
- Query loop baseline: `cargo test -p cc-query loop_tests -- --nocapture`
  - Covers loop-level tool-use and tool-result follow-up behavior.
- Engine execution baseline: `cargo test -p cc-engine lifecycle::deps -- --nocapture`
  - Covers canonical execution boundary behavior.
- E2E prompt/tool visibility baseline: `cargo test -p claude-code-rs --test e2e_tools -- --nocapture`
  - Covers system prompt exposure for task tools.
- Known broad-test risk: `docs/KNOWN_ISSUES.md` TEST-002 says package-wide `cargo test -p claude-code-rs` has unrelated/cross-test-state failures; use focused baselines for this checkpoint unless the package-wide issue is being fixed.

## Dependency graph

Tool/task execution graph:

```text
claude-code-rs
  -> tools/registry.rs
  -> tools/tasks.rs + tools/tasks/*
  -> commands/tasks_cmd.rs
  -> cc-query
      -> deps.rs: QueryDeps + ToolExecRequest/ToolExecResult
      -> loop_impl.rs: turn loop and streaming gate
      -> loop_helpers.rs: execution batching/result conversion
      -> stop_hooks.rs: tool_use extraction
  -> cc-engine
      -> types/tool.rs: Tool, ToolResult, ToolUseContext, ToolUseOptions
      -> lifecycle/deps.rs: QueryDeps::execute_tool implementation
  -> cc-types
      -> message.rs: ContentBlock::ToolUse/ToolResult
```

Crate-level direct dependencies from `cargo tree --depth 1`:

- `cc-query` depends directly on `cc-api`, `cc-engine`, `cc-observability`, `cc-services`, `cc-types`, and `cc-utils`.
- `cc-engine` depends directly on `cc-bootstrap`, `cc-compact`, `cc-config`, `cc-keybindings`, `cc-models`, and `cc-types`.
- `claude-code-rs` depends directly on the split crates used by the current monolith integration, including `cc-engine`, `cc-query`, `cc-types`, `cc-utils`, `cc-session`, `cc-config`, `cc-permissions`, `cc-sandbox`, `cc-services`, `cc-mcp`, `cc-browser`, and `gateway`.

## File-size hot spots

Repository-wide Rust hot spots from line counts:

| Lines | File |
| ---: | --- |
| 2219 | `crates/cc-engine/src/lifecycle/deps.rs` |
| 2188 | `crates/cc-query/src/loop_tests.rs` |
| 2103 | `crates/claude-code-rs/src/ipc/subsystem_handlers.rs` |
| 1768 | `crates/cc-permissions/src/dangerous.rs` |
| 1755 | `crates/cc-mcp/src/client.rs` |
| 1519 | `crates/cc-config/src/settings.rs` |
| 1434 | `crates/claude-code-rs/src/tools/tasks/tests.rs` |
| 1318 | `crates/claude-code-rs/src/tools/tool_search.rs` |
| 1283 | `crates/cc-engine/src/lifecycle/submit_message.rs` |
| 1204 | `crates/cc-query/src/loop_helpers.rs` |

Tool/task focused hot spots:

| Lines | File |
| ---: | --- |
| 2219 | `crates/cc-engine/src/lifecycle/deps.rs` |
| 2188 | `crates/cc-query/src/loop_tests.rs` |
| 1434 | `crates/claude-code-rs/src/tools/tasks/tests.rs` |
| 1204 | `crates/cc-query/src/loop_helpers.rs` |
| 751 | `crates/claude-code-rs/src/tools/tasks/store.rs` |
| 733 | `crates/claude-code-rs/src/tools/tasks/task_tools.rs` |
| 709 | `crates/cc-query/src/loop_impl.rs` |
| 531 | `crates/claude-code-rs/src/tools/tasks.rs` |

Follow-up constraint: future Rust edits in these areas should split along existing module boundaries instead of growing `deps.rs`, `loop_helpers.rs`, or task test/store/tool files further.

## Effects, defects, follow-ups

- Effect: added a checkpoint inventory artifact for tool/task ownership planning.
- Defect found: none from this inventory-only task.
- Follow-up: use the focused baseline tests above before and after any future task/tool code change; do not rely on package-wide `claude-code-rs` tests until TEST-002 is closed.
