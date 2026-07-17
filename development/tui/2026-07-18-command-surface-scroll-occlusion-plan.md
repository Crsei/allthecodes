# Command Surface Scroll Occlusion Fix Plan

Date: 2026-07-18

## Problem

When a Rust TUI command surface is open over a session containing text, mouse-wheel input still scrolls the underlying conversation. The command-surface renderer also does not clear its overlay rectangle before drawing, which lets session text remain visible beneath unpainted panel cells.

## Scope

1. Make prompt-adjacent command surfaces fully occlude the buffer region they occupy.
2. Treat the active command surface as modal for mouse-wheel input so the underlying session scroll offset cannot change.
3. Add focused regression tests for overlay clearing and scroll isolation.
4. Record the user-visible Rust TUI issue and resolution in the known-issues document.
5. Produce the required per-session worktree HTML artifact.

## Verification

Run the repository's layered checks from an isolated worktree target directory:

1. `cargo fmt --all --check`
2. Focused Rust TUI overlay and app input/render tests
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace --exclude allthecodes --lib`
5. The targeted command-surface PTY test slice if the focused in-process tests do not fully cover the visible behavior
6. `cargo build -p allthecodes --release`

The full PTY suite is reserved for the final validation layer and will only be run when the targeted evidence requires it.
