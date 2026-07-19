# TUI tool-render review fixes

> Date: 2026-07-19
> Task slug: `tui-tool-render-review-fixes`

## Goal

Close the three implementation defects found while reviewing
`development/worktree-workflow-artifacts/2026-07-19-tui-tool-render-style-upgrade.html`:

1. Read the same session/team-scoped task list used by `TaskCreate` and `TaskUpdate`, instead of the fixed global `tasklist` store.
2. Stop synchronously reloading the persistent task repository on every 16 ms UI tick.
3. Route task-view toggling through the existing configurable `app:toggleTodos` keybinding action instead of hard-coding physical `Ctrl+T` handling.

## Implementation

- Add a shared task-list scope resolver to `allthecodes-tools` and use the active TUI session/team identity when refreshing the compact task list.
- Cache the scoped task list in `RuntimeViewState`; refresh immediately when opening Tasks, on relevant backend events, and through a throttled fallback poll rather than every animation tick.
- Implement `app:toggleTodos` in `dispatch_bound_action` and remove the physical-key fallback so user keybinding overrides remain authoritative.
- Add regression tests for session-scoped task visibility, refresh throttling, and remapped/disabled toggle behavior.

## Verification

Follow the repository's layered SOP:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --exclude allthecodes --lib`
4. Focused `allthecodes` UI tests for the changed task-view and keybinding paths
5. `cargo build --workspace --release`
6. `git diff --check`

PTY coverage is not required unless the focused tests expose a terminal-only behavior gap; the fixes are store selection, refresh scheduling, and keybinding dispatch semantics.

## Workflow

Implement only in `.worktrees/tui-tool-render-review-fixes` on branch
`worktree/tui-tool-render-review-fixes`. Record the result in
`development/worktree-workflow-artifacts/2026-07-19-tui-tool-render-review-fixes.html`, fast-forward the completed branch into `allthecodes`, push, then remove the worktree and branch.
