# Tools Phase 3 - Tasks V2 Schema and Update Semantics

## Scope

Phase 3 fills the user-visible Tasks V2 data model gaps that remained after task-list scoping and locking. The Rust task tools now preserve the upstream `activeForm` and generic `metadata` fields, and `TaskUpdate` can mutate more than status/owner.

## Implementation

- Bumped the task schema version to 5.
- Added `active_form` / serialized `activeForm` to `TaskEntry`, `PersistedTaskRecord`, `TaskCreateOptions`, tool output, and persisted records.
- Added generic `metadata: Option<Value>` distinct from `remote_task_metadata`.
- `TaskCreate` now accepts `activeForm` and `metadata`.
- `TaskUpdate` now accepts `taskId` as a Bun-compatible alias for `task_id`.
- `TaskUpdate` now supports subject, description, `activeForm`, owner, metadata merge, metadata null-key deletion, `addBlocks`, `addBlockedBy`, and `status: "deleted"`.
- Dependency updates continue to use Rust's internal `depends_on` truth source while exposing Bun-compatible `blocked_by`, `blockedBy`, and computed `blocks`.

## Tests

- Converted `phase0_gap_task_v2_schema_requires_active_form_and_metadata` from ignored to active.
- Added `task_create_and_update_persist_active_form_and_metadata`.
- Added `task_update_adds_dependency_edges_and_deleted_cleans_them`.

Verification:

```text
cargo test -p claude-code-rs tools::tasks
57 passed / 1 ignored

rustfmt --edition 2021 --check crates/claude-code-rs/src/tools/tasks.rs
passed
```

## Remaining Work

- Phase 4 still needs teammate exit owner reset.
