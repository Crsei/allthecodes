# UI item allow cleanup - 2026-05-20

Scope:
- `crates/claude-code-rs/src/ui/components/**`
- `crates/claude-code-rs/src/ui/agents/**`
- `crates/claude-code-rs/src/ui/overlays/**`
- `crates/claude-code-rs/src/ui/notifications/**`
- `crates/claude-code-rs/src/ui/messages/**`
- `crates/claude-code-rs/src/ui/tasks/**`
- `crates/claude-code-rs/src/ui/mcp/**`
- `crates/claude-code-rs/src/ui/memory/**`
- `crates/claude-code-rs/src/ui/teams/**`
- `crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs`
- `crates/claude-code-rs/src/ui/permissions/file_write_permission_request/file_write_tool_diff.rs`
- `crates/claude-code-rs/src/ui/permissions/utils.rs`

## Deleted code

None.

## Reason

After removing the scoped `#[allow(dead_code)]` and `#[allow(unused_imports)]`
attributes, `cargo check -p claude-code-rs` completed without Rust
`dead_code` or `unused_imports` warnings. The scoped items are either already
reachable through production or test builds, or are public API surface that does
not need local lint suppression.

## Restore

No deleted objects need restoration. If a future change removes a reachable use
and reintroduces a lint, restore the specific production connection or delete
the now-isolated object in that change; do not restore broad allow attributes.
