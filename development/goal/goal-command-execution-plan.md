# Goal Command Execution Plan

Date: 2026-06-03

## Purpose

Upgrade allthecodes goal handling from a basic `/goal` command plus model tools into a robust long-running goal runtime, using the local reference implementations in:

- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/codex`
- `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/pi-codex-goal`

The target is not a literal port. Codex provides the canonical behavior model for thread goals, event-driven accounting, and TUI controls. pi-codex-goal provides a smaller modular design for state transitions, continuation scheduling, stale queued work protection, and recovery.

## Current Baseline

allthecodes already has:

- Slash command registration and handler: `crates/allthecodes-commands/src/goal.rs`
- Model tools: `crates/allthecodes-tools/src/goals/mod.rs`
- Goal feature gate: `crates/allthecodes-config/src/features.rs`
- Per-session JSON persistence: `allthecodes_config::paths::goal_file_path`
- Runtime accounting after assistant usage: `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- Simple active-goal continuation: `crates/allthecodes-engine/src/query/loop_impl.rs`
- SDK/headless/TUI `goal_updated` event plumbing: `crates/allthecodes-types/src/sdk.rs`, `crates/allthecodes-ipc-protocol/src/protocol/mod.rs`, `crates/allthecodes/src/ui/tui/engine_events.rs`
- Status bar summary: `crates/allthecodes/src/ui/app/status.rs`, `crates/allthecodes/src/ui/app/render.rs`

Important gaps:

- `GoalStatus` lacks `paused` and `usage_limited`.
- Goal records lack a stable `goal_id`, making stale continuation detection weak.
- `/goal` lacks `pause`, `resume`, `clear`, `edit`, and replace-confirm semantics.
- Runtime accounting is absolute usage based, not turn-delta/wall-clock state based.
- Continuation is a loop-level prompt injection without a per-goal queued marker, idle check, stale queued work guard, or recovery state.
- Budget-limited behavior stops continuation but does not inject an explicit budget steering prompt.
- State transition rules are spread across command/tool/runtime paths instead of centralized.
- Tests cover the basic lifecycle but not continuation races, stale goals, pause/resume, replacement, or recovery.

## Reference Map

Codex canonical behavior:

- `codex-rs/state/src/model/thread_goal.rs`
  - `ThreadGoalStatus`: `active`, `paused`, `blocked`, `usage_limited`, `budget_limited`, `complete`
  - `ThreadGoal`: thread id, goal id, objective, status, token budget, tokens/time, created/updated timestamps
- `codex-rs/state/src/runtime/goals.rs`
  - SQLite goal store, optimistic `expected_goal_id`, replace/insert/update/accounting helpers
- `codex-rs/core/src/goals.rs`
  - `GoalRuntimeEvent`
  - accounting locks
  - token deltas excluding cached input
  - wall-clock accounting
  - idle continuation
  - budget-limit steering
  - resume restoration
- `codex-rs/core/src/tools/handlers/goal.rs`
  - tool response shape with `remainingTokens` and `completionBudgetReport`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
  - `/goal clear`, `/goal edit`, `/goal pause`, `/goal resume`, and objective setting
- `codex-rs/tui/src/app/thread_goal_actions.rs`
  - replace confirmation, ephemeral-thread error, goal status updates, clear handling
- `codex-rs/tui/src/chatwidget/goal_menu.rs`
  - goal summary and command hints

pi-codex-goal modular behavior:

- `src/types.ts`
  - compact goal model with `goalId`, objective, status, token budget, usage, timestamps
- `src/state.ts`
  - objective/budget validation, creation/replacement, status update, usage application, session reconstruction
- `src/goal-transition.ts`
  - centralized transition planner and invariants
- `src/goal-transition-effects.ts`
  - transition side effects: clear continuation, clear accounting, reset recovery, clear budget warning
- `src/goal-accounting.ts`
  - active goal accounting state and one-shot budget steering
- `src/continuation-scheduler.ts`
  - idle-based continuation scheduling keyed by goal id
- `src/stale-queued-work-guard.ts` and related reducer files
  - guards against old queued work continuing after the goal changed or terminal cleanup occurred
- `src/recovery-machine.ts`, `src/recovery-runtime.ts`, `src/recovery.ts`
  - recovery phases and attention states
- `test/*.test.ts`
  - good coverage patterns for transitions, stale queued work, continuation, recovery, and command behavior

## Target Behavior

User-facing `/goal`:

- `/goal` or `/goal status`: show current goal summary.
- `/goal <objective>`: create a goal when none exists; confirm or explicitly replace when a non-complete goal exists.
- `/goal set <objective>` and `/goal create <objective>`: aliases for objective creation.
- `/goal pause [reason]`: pause active goals and stop continuation/accounting.
- `/goal resume [reason]`: resume paused/blocked/usage-limited goals, reset continuation state, and continue when idle.
- `/goal complete [reason]`: mark the goal complete and show final usage.
- `/goal block [reason]`: mark the goal blocked.
- `/goal clear`: remove the current goal and clear runtime state.
- `/goal edit`: if TUI can support it, open an objective editor; otherwise defer to a later UI phase.

Model tools:

- `GetGoal` / `get_goal`: return structured goal, remaining budget, and live usage if available.
- `CreateGoal` / `create_goal`: create one active goal only when no non-complete goal exists.
- `UpdateGoal` / `update_goal`: model-updatable statuses stay restricted to `complete` and `blocked`.
- Tool outputs should include a Codex-style `completion_budget_report` only when the model marks a goal complete.

Runtime:

- Account active goal token and wall-clock deltas once per turn/tool-completion boundary.
- Never let stale queued continuation for an old `goal_id` run after replace, clear, pause, budget limit, usage limit, block, or complete.
- Continue only when idle and no user/trigger-turn input is pending.
- Suppress continuation in plan mode and while another active turn/task exists.
- On budget limit, mark `budget_limited`, emit one event, clear active accounting, and optionally inject a budget-limit steering prompt once.
- On usage-limit/provider-limit stop, mark `usage_limited` where possible instead of overloading `budget_limited`.
- Resume restores active accounting only for active goals.

Persistence:

- Keep the existing JSON file backend for now; do not introduce SQLite in this phase.
- Add fields compatibly with defaults:
  - `goal_id: String`
  - `schema_version: u32`
  - `paused_at: Option<String>`
  - `blocked_at: Option<String>`
  - `usage_limited_at: Option<String>`
  - `budget_limited_at: Option<String>`
  - `last_status_reason: Option<String>` or keep `status_reason` as the canonical reason field
- Existing goal files without `goal_id` should load successfully and synthesize a durable id on next save.

## Execution Phases

### Phase 0 - Baseline Audit and Safety Rails

Deliverables:

- Add a short inventory doc or test notes confirming current behavior before changes.
- Identify all write paths to `GoalRecord`.
- Confirm which query loop paths can trigger continuation.

Files to inspect:

- `crates/allthecodes-commands/src/goal.rs`
- `crates/allthecodes-tools/src/goals/mod.rs`
- `crates/allthecodes-engine/src/query/loop_impl.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes-engine/src/query/token_budget.rs`
- `crates/allthecodes/src/ui/tui/command_availability.rs`

Acceptance:

- Existing tests still pass before feature work begins.
- No changes to unrelated web files.

### Phase 1 - Goal Model and Transition Core

Goal: centralize status transitions before touching command/runtime behavior.

Changes:

- Extend `GoalStatus` with `Paused` and `UsageLimited`.
- Add `goal_id` and `schema_version` to `GoalRecord`.
- Add helpers in `crates/allthecodes-tools/src/goals/mod.rs` or a new sibling module:
  - `validate_goal_objective`
  - `validate_token_budget`
  - `create_goal_record`
  - `replace_goal_record`
  - `update_goal_status`
  - `apply_goal_usage_delta`
  - `status_after_budget_limit`
  - `goal_is_open`, `goal_is_active`, `goal_is_terminal`
- Model transition invariants after `pi-codex-goal/src/goal-transition.ts`:
  - runtime accounting cannot change objective, budget, created_at, or goal_id.
  - usage and active seconds are non-decreasing.
  - only active goals can pause.
  - only paused/blocked/usage-limited goals can resume, subject to allthecodes policy.
  - complete is terminal for model tools; command clear/replace can start a new goal.
  - budget-limited goals cannot silently become active through runtime accounting.
- Preserve backward compatibility for old JSON goal files.

Tests:

- Unit tests for all status transitions.
- Unit tests for migration/default loading from old JSON.
- Unit tests for non-decreasing usage and expected-goal-id rejection.

Acceptance:

- All existing goal lifecycle tests pass.
- New transition tests cover active, paused, blocked, usage_limited, budget_limited, complete.

### Phase 2 - Command UX Expansion

Goal: make `/goal` match the useful Codex and pi-codex-goal controls.

Changes:

- Extend `parse_goal_command` in `crates/allthecodes-commands/src/goal.rs`:
  - `pause`, `resume`, `clear`, `status/show`, `complete/done`, `block/blocked`
  - keep `set/create` aliases
  - reserve `edit` for TUI-only or return a clear unsupported message outside TUI
- Update command formatting:
  - show command hints by status.
  - show `goal_id` only in debug/test output, not normal UX.
  - show final usage when completing.
- Update active-task availability in `crates/allthecodes/src/ui/tui/command_availability.rs`:
  - allow `/goal`, `/goal status`, `/goal show`, `/goal help`
  - consider allowing `/goal pause`, `/goal resume`, `/goal clear` during task only after runtime state clear semantics exist.
- Decide replacement behavior:
  - CLI/headless: reject replacing non-complete goals unless `/goal clear` was run first.
  - TUI later phase: add replace confirmation.

Tests:

- Parser tests for new commands.
- Command tests for pause/resume/clear.
- Duplicate/replacement behavior tests.
- Active-task availability tests.

Acceptance:

- `/goal pause` stops active continuation.
- `/goal resume` reactivates a paused goal.
- `/goal clear` removes the goal file or clears stored goal state.

### Phase 3 - Tool Contract Refinement

Goal: keep model tools narrow and safe while matching Codex response quality.

Changes:

- Keep `UpdateGoal` limited to `complete` and `blocked`.
- Add `remaining_tokens` to get/create/update responses.
- Make `completion_budget_report` a string instruction like Codex when status becomes complete, rather than a raw budget object only.
- Return structured errors consistently:
  - `goal_not_found`
  - `active_goal_exists`
  - `invalid_goal_transition`
  - `invalid_goal_objective`
  - `invalid_token_budget`
- Ensure aliases `get_goal`, `create_goal`, `update_goal` share exact semantics.

Tests:

- Semantic tool tests for blocked, complete, duplicate active, remaining budget, completion budget report.
- Validation tests for invalid status and invalid budget.

Acceptance:

- Model cannot pause/resume/clear through tools.
- Completion output instructs final usage reporting when there is useful usage/budget data.

### Phase 4 - Runtime Accounting Rewrite

Goal: move from absolute usage snapshots to guarded delta accounting.

Changes:

- Introduce a runtime-only state object inspired by Codex and pi-codex-goal:
  - active `goal_id`
  - last accounted usage snapshot
  - last accounted wall-clock instant
  - budget warning sent for goal id
  - queued continuation for goal id
- Account deltas:
  - token delta: prefer input + output minus cached input where usage supports it; otherwise use current allthecodes `UsageTracking` delta fields.
  - wall-clock delta: active elapsed seconds since last accounting.
- Require expected `goal_id` for runtime writes.
- Clear active accounting when goal becomes paused, blocked, complete, usage_limited, budget_limited, or cleared.
- Emit `SdkMessage::GoalUpdated` for runtime updates and terminal transitions.
- Ensure active goal updates are saved before event emission.

Files likely touched:

- `crates/allthecodes-tools/src/goals/mod.rs`
- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
- `crates/allthecodes-engine/src/query/loop_impl.rs`
- possibly a new `crates/allthecodes-engine/src/query/goal_runtime.rs`

Tests:

- Runtime accounting increments by deltas, not totals.
- Expected goal id mismatch does not mutate a replaced goal.
- Budget limit emits once.
- Clearing/pausing prevents further runtime accounting.

Acceptance:

- Repeated turns do not double-count old tokens.
- Goal replacement during runtime cannot receive stale usage updates.

### Phase 5 - Continuation Scheduler and Stale Work Guard

Goal: replace unconditional loop continuation with idle, per-goal continuation scheduling.

Changes:

- Extract continuation logic from `active_goal_continuation_message`.
- Add continuation state:
  - `continuation_queued_for: Option<String>`
  - `continuation_scheduled_for: Option<String>` if needed
- Before continuing:
  - goal exists
  - goal id matches
  - status is active
  - no active turn/task
  - no pending user or trigger-turn input
  - not in plan mode
  - stale queued work guard is not blocking
- Add prompt metadata or hidden context marker containing goal id.
- When processing a continuation input, verify the goal id still matches.
- Clear continuation state on set/replace/clear/pause/block/complete/budget_limit/usage_limit.

Tests:

- Active goal continues when idle.
- Paused/blocked/complete/budget-limited goals do not continue.
- Replaced goal does not execute old queued continuation.
- User message arriving before continuation suppresses automatic continuation.
- Plan mode suppresses continuation.

Acceptance:

- No stale continuation can target a superseded goal id.
- Goal continuation starts only from idle state.

### Phase 6 - Budget and Usage Limit Semantics

Goal: distinguish user token budget from provider/session usage limits.

Changes:

- `BudgetLimited`: token budget specified by the goal was reached.
- `UsageLimited`: provider, context, cost, rate, or system usage limit prevents continuation.
- On `BudgetLimited`, emit a single steering prompt if a turn is still active.
- On `UsageLimited`, stop continuation and show a clear status/reason.
- Resume behavior:
  - paused/blocked/usage_limited can resume.
  - budget_limited should require either new budget semantics or remain stopped; avoid silently resetting budget.

Tests:

- Goal budget crossing sets `BudgetLimited`.
- Cost/session budget stop sets `UsageLimited` where appropriate.
- Budget warning fires once per goal id.
- Resume from usage_limited works according to chosen policy.

Acceptance:

- UI/status events distinguish budget from external usage exhaustion.

### Phase 7 - TUI, Status, and IPC Polish

Goal: make goal state visible and controllable without relying only on command output.

Changes:

- Update `GoalStatusSnapshot` parsing for new statuses.
- Render paused, blocked, usage_limited, budget_limited distinctly.
- Add command hints similar to Codex `goal_menu.rs`.
- Consider adding a TUI goal editor after core behavior is stable.
- Extend normalized IPC payload only if existing `goal_updated` JSON is insufficient.
- Ensure Remote Control/web consumers receive consistent `goal_updated` payloads.

Tests:

- Status bar rendering for every status.
- `goal_updated` event mapping for new statuses.
- TUI command availability for goal controls.

Acceptance:

- Users can understand whether the goal is active, paused, blocked, usage-limited, budget-limited, or complete.

### Phase 8 - Recovery and Abort Handling

Goal: handle interrupted, aborted, and overflowing turns predictably.

Changes:

- On task abort:
  - account best-effort deltas.
  - pause active goal or leave active based on allthecodes policy.
  - clear queued continuation.
- On context overflow or repeated empty continuation:
  - stop automatic continuation.
  - set an attention/recovery reason if adding recovery state is warranted.
- On resume session:
  - reload goal.
  - restore accounting only for active goals.
  - prompt or hint for paused/blocked/usage-limited goals.

Tests:

- Abort pauses or stops continuation according to policy.
- Resume restores only active goals.
- Recovery does not loop forever when no progress is made.

Acceptance:

- Goal runtime cannot silently spin after abort, compaction, or overflow.

### Phase 9 - End-to-End Verification

Commands to run:

- `cargo test -p allthecodes-tools semantic_goal_lifecycle`
- `cargo test -p allthecodes-commands goal`
- `cargo test -p allthecodes-engine goal`
- targeted TUI/app tests for status rendering and command availability
- full workspace test only after targeted suites are clean

Manual smoke scenarios:

- `/goal ship release`
- verify automatic continuation starts.
- `/goal pause`, confirm continuation stops.
- `/goal resume`, confirm continuation resumes only when idle.
- `/goal clear`, confirm no stale continuation runs.
- create goal with a small token budget and confirm `budget_limited`.
- simulate usage/cost limit and confirm `usage_limited`.
- replace or clear while continuation is queued and verify old work is ignored.

## Suggested Implementation Order

1. Phase 1 model and transition helpers.
2. Phase 2 command UX.
3. Phase 3 tool response cleanup.
4. Phase 4 runtime accounting.
5. Phase 5 continuation guard.
6. Phase 6 budget/usage-limit split.
7. Phase 7 UI/IPC polish.
8. Phase 8 recovery.
9. Phase 9 verification.

This order keeps the data invariants in place before exposing more runtime behavior.

## Risks and Decisions

- Persistence backend: stay on JSON for now. SQLite parity with Codex is not required unless allthecodes later unifies thread/session state into a database.
- Replacement UX: command-line replacement should be conservative. TUI confirmation can come later.
- Resume from budget-limited: do not reset budget implicitly. A later command can add explicit budget editing.
- Recovery scope: pi-codex-goal has rich recovery logic. Port only stale continuation prevention first; add full recovery if real failure modes remain.
- Token accounting: allthecodes usage structures may not expose the same cached-input fields as Codex. Implement the best available delta accounting and document deviations in tests.

## Definition of Done

- Goal records have stable ids and explicit statuses.
- Commands cover status, set/create, pause, resume, complete, block, clear.
- Model tools remain safe and provide useful structured output.
- Runtime accounting is monotonic and expected-goal-id guarded.
- Automatic continuation is idle-only and stale-safe.
- Budget and usage limits are distinguishable.
- TUI/IPC surfaces show all statuses accurately.
- Targeted tests cover state transitions, command parsing, tool lifecycle, runtime accounting, stale continuation, and UI event/status rendering.
