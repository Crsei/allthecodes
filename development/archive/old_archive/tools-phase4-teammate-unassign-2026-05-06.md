# Tools Phase 4 - Teammate Owner Release

Date: 2026-05-06

## Scope

Phase 4 closes the Tasks V2 teammate-exit gap from `architecture/tools-implementation-map.md`.
When an in-process teammate exits, is terminated, or is removed through team cleanup, non-terminal tasks owned by that teammate must become available again instead of staying pinned to an inactive owner.

## Changes

- Added `unassign_teammate_tasks(task_list_id, teammate_id, teammate_name, reason)`.
- Matched both teammate id and display name because different task paths can persist either value as `owner`.
- Reset only non-terminal tasks to `pending` and cleared `owner`.
- Preserved terminal task ownership for completed, failed, cancelled, interrupted, and stopped tasks.
- Returned a structured summary plus a leader-readable notification listing released task ids and subjects.
- Wired owner release into in-process runner shutdown, runner error, cancellation, `team kill`, and `team delete`.

## Verification

- `cargo test -p claude-code-rs tools::tasks` passed: 58 passed.
- `cargo test -p claude-code-rs team_cmd` passed: 7 passed.
- `cargo test -p claude-code-rs teams::in_process` passed: 8 passed.
- `rustfmt --edition 2021 --check crates\claude-code-rs\src\tools\tasks.rs crates\claude-code-rs\src\teams\runner.rs crates\claude-code-rs\src\commands\team_cmd.rs` passed.

## Notes

The implementation reuses the task-list scoped store and `.lock` mutation path from Phase 2, so teammate release observes the same disk-refresh boundary as claim/update/delete.
The release operation is intentionally idempotent; repeated shutdown/termination hooks return an empty release list after the first successful reset.
