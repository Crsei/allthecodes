# UI Test Target Dead Code Subagent Plan

Date: 2026-05-20

Scope:

- `cargo check -p claude-code-rs --all-targets --message-format short`
- Rust TUI warnings emitted by `bin "claude-code-rs" test`
- Current baseline: 180 Rust source warnings, all under `crates/claude-code-rs/src/ui/**`

Non-goals:

- Do not count the local missing-`npm` web-ui build-script warning as Rust source warning.
- Do not delete Full Build parity surfaces just because they are not wired yet.
- Do not add broad `allow(dead_code)` or file-level `allow(unused)` to pass the gate.

## Subagent Batches

Each batch owns a disjoint file set. Workers must not revert unrelated work and must not edit another batch's files.

| Batch | Owner paths | Delete log |
| --- | --- | --- |
| Agents/theme/helpers | `ui/agents/**`, `ui/theme/**`, `ui/helpers/skills_helpers.rs` | `docs/delete/ui-warning-cleanup-agents-theme.md` |
| App/runtime/platform | `ui/app.rs`, `ui/app/**`, `ui/runtime/**`, `ui/platform/**`, `ui/notifications/**`, `ui/overlays/**` | `docs/delete/ui-warning-cleanup-app-runtime.md` |
| Components/input | `ui/components/**`, `ui/input/**`, `ui/prompt_input.rs`, `ui/command_palette/**` | `docs/delete/ui-warning-cleanup-components-input.md` |
| Rendering/surfaces | `ui/rendering/**`, `ui/messages/**`, `ui/permissions/**`, `ui/mcp/**`, `ui/tasks/**`, `ui/diff.rs`, `ui/diff/**`, `ui/memory/**`, `ui/teams/**` | `docs/delete/ui-warning-cleanup-rendering-surfaces.md` |

## Per-Warning Decision Rule

For every warning, choose exactly one outcome:

1. **Wire**: connect the item to an existing production or test flow.
2. **Test**: add focused coverage that uses the retained API.
3. **Move**: move test-only helpers into the test module that uses them.
4. **Feature/platform cfg**: gate real optional surfaces behind the relevant feature or platform cfg.
5. **Delete**: remove redundant code and record the deleted item in the batch delete log.
6. **Intentional**: keep only with a narrow item-level explanation, owner, and removal condition.

## Verification

Batch-level verification:

```bash
env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH cargo check -p claude-code-rs --all-targets --message-format short
```

Final verification:

```bash
env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH cargo check --workspace --all-targets --message-format short
env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH cargo test -p claude-code-rs ui:: -- --nocapture
env CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup PATH=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo/bin:$PATH cargo build --workspace --release
```

Exit criteria:

- `claude-code-rs --all-targets` has zero Rust source warnings under `crates/claude-code-rs/src/ui/**`.
- Workspace all-targets has zero Rust source warnings, excluding documented environment build-script output.
- Every deleted UI item is recorded under `docs/delete/`.
- No new broad dead-code or unused-code suppression is introduced.
