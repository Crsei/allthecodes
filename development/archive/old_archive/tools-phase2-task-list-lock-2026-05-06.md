# Tools Phase 2 - Task List Lock and Atomic Claim

## Scope

Phase 2 upgrades Tasks V2 claim from process-local map state to a task-list-level file lock. The lock protects the read/check/write path so separate `TaskStore` instances and separate processes do not claim from stale task state.

## Implementation

- Added a task-list `.lock` file guarded by `create_new` and bounded retry/backoff.
- Added `lock_unavailable` as a structured claim failure reason.
- `claim_task()` now acquires the task-list lock, performs a live disk refresh, then checks task existence, terminal status, owner, unresolved blockers, and `checkAgentBusy` before persisting the owner/status update.
- `create`, `update_status`, `stop`, and `delete` now perform write-path mutations under the same task-list lock when available.
- `list` and `get` live-refresh disk state so stale stores observe tasks created or claimed by other stores.
- `delete` now removes references to the deleted task from other tasks' `depends_on` lists and persists the cleaned records.
- Startup recovery remains separate from live refresh: active tasks are only converted to interrupted/recoverable during `TaskStore::with_dir()` startup load, not during lock-protected live refresh.

## Tests

- Converted `phase0_gap_cross_store_claim_requires_task_list_lock` from ignored to active.
- Added `task_list_lock_makes_agent_busy_check_cross_store_atomic`.
- Added `task_claim_reports_lock_unavailable_when_list_lock_is_held`.
- Added `task_delete_removes_dependency_references_under_list_lock`.
- Added `task_list_refreshes_tasks_created_by_other_store`.

Verification:

```text
cargo test -p claude-code-rs tools::tasks
54 passed / 2 ignored

rustfmt --edition 2021 --check crates/claude-code-rs/src/tools/tasks.rs
passed
```

## Remaining Work

- Phase 3 still needs Tasks V2 `activeForm` / generic `metadata` and broader update semantics.
- Phase 4 still needs teammate exit owner reset.
