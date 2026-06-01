# Tools Phase 1 - Task List ID and Storage Boundary

## Scope

Phase 1 narrows Tasks V2 persistence from a single process-global task store to a resolved task-list store. The intent is to let team leaders and in-process teammates share a team task list while standalone sessions stay isolated.

## Implementation

- Added task-list id resolution in `crates/claude-code-rs/src/tools/tasks.rs`.
- Resolution order is `CC_RUST_TASK_LIST_ID`, `CLAUDE_CODE_TASK_LIST_ID`, in-process teammate team name, `AppState.team_context.team_name`, `CLAUDE_CODE_TEAM_NAME`, session id, then `tasklist`.
- Added sanitized task-list directories under `$CC_RUST_HOME/tasks/<task-list-id>/` or `~/.cc-rust/tasks/<task-list-id>/`.
- Switched Tasks V2 tools to use the store resolved from `ToolUseContext`.
- Kept `TaskStore::with_dir()` for direct unit tests and explicit repository use.
- Added a non-destructive migration path for old flat default tasks: when the default `tasklist` store opens and the new directory has no JSON tasks, legacy `*.json`, `*.output.log`, and `.highwatermark` files are copied from `$CC_RUST_HOME/tasks/` into `$CC_RUST_HOME/tasks/tasklist/`.

## Tests

- `task_list_id_prefers_explicit_env_over_team_context`
- `task_list_id_uses_team_context_before_session_and_team_env`
- `task_list_id_uses_in_process_teammate_team_name`
- `task_tools_use_task_list_scoped_store`
- `default_task_list_store_copies_legacy_flat_tasks`

Verification:

```text
cargo test -p claude-code-rs tools::tasks
49 passed / 3 ignored
```

## Remaining Work

- Phase 2 still needs task-list-level cross-process locking and atomic claim.
- Phase 3 still needs Tasks V2 `activeForm` / generic `metadata` and broader update semantics.
- Phase 4 still needs teammate exit owner reset.
