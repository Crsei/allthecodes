# TUI TaskList default display

> Date: 2026-07-19
> Task slug: `tui-task-list-default-and-stream-recovery`

## Goal

Make the task checklist the default user-facing representation of task mutations:

1. Suppress `TaskCreate` and `TaskUpdate` tool-use/result rows from the normal and verbose message stream.
2. Automatically open the Spinner-adjacent `Tasks` view when either tool starts.
3. Refresh the session/team-scoped checklist after the tool result so the user sees the canonical `✔/◼/◻` TaskList without pressing `Ctrl+T` first.
4. Preserve `Ctrl+T` as an explicit way to hide, reopen, or cycle the expanded view.

## Implementation

- Filter `TaskCreate` and `TaskUpdate` operations, plus their paired tool results, in the renderable-message grouping layer only; persisted transcript data and tool execution remain unchanged.
- Detect task mutation `BackendMessage::ToolUse` events in `App`, switch `expanded_view` to `Tasks`, and request an immediate scoped refresh.
- Reuse the existing forced refresh on `BackendMessage::ToolResult` and the five-second fallback poll.
- Add render-pipeline and app-event regression tests proving raw task tool rows are absent and the checklist opens automatically.

## Verification

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --exclude allthecodes --lib`
4. Focused TUI app and message-render tests
5. PTY regression suite after focused tests because this changes a visible TUI surface
6. `cargo build --workspace --release`
7. `git diff --check`

## Workflow

Commit this plan on `allthecodes`, then implement only in
`.worktrees/tui-task-list-default-and-stream-recovery` on branch
`worktree/tui-task-list-default-and-stream-recovery`. Record the result in the matching
HTML worktree artifact, fast-forward into `allthecodes`, push, and remove the worktree.
