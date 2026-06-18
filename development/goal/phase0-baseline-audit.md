# Phase 0 Baseline Audit

Date: 2026-06-03

## GoalRecord Write Paths

- `crates/allthecodes-commands/src/goal.rs`
  - `/goal <objective>` creates and saves a session goal.
  - `/goal complete`, `/goal block`, `/goal pause`, and `/goal resume` update status.
  - `/goal clear` removes the stored goal file.
- `crates/allthecodes-tools/src/goals/mod.rs`
  - `CreateGoal` creates and saves a goal when no unfinished goal exists.
  - `UpdateGoal` is model-facing and remains restricted to `complete` and `blocked`.
  - `account_goal_runtime_for_session` updates active-goal token/time accounting.
  - `mark_goal_budget_limited_for_session` marks active or already budget-limited goals as budget limited.

## Continuation Trigger Paths

- `crates/allthecodes-engine/src/query/loop_impl.rs`
  - After an assistant turn with no tool calls, the loop runs stop hooks and token-budget checks.
  - If the current persisted goal is still `active`, `active_goal_continuation` injects a synthetic user message and continues the query loop.
  - Non-active statuses (`paused`, `blocked`, `usage_limited`, `budget_limited`, `complete`) do not trigger this continuation.

## Runtime Accounting Path

- `crates/allthecodes-engine/src/lifecycle/submit_message/stream_handler.rs`
  - After assistant usage is accumulated into engine state, `account_goal_runtime_message` calls `account_goal_runtime_for_session`.
  - It emits `goal_updated` with `runtime_updated` or `budget_limited`.

## Baseline Test Note

- Initial targeted test command attempted:
  - `cargo test -p allthecodes-commands goal -p allthecodes-tools semantic_goal_lifecycle --no-fail-fast`
- Result in the default shell:
  - `cargo: command not found`
- Follow-up verification should use the repository's local Rust toolchain path.
