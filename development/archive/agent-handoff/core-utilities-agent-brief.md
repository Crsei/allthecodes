# Agent Handoff: Core Utilities Migration

Worktree:
`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/.claude/worktrees/core-utilities`

Primary plan:
`development/archive/plan/core-utilities-migration-plan.md`

## Mission

Turn the generic Bun `src/utils/` gap into an owner-aware Rust migration plan
and begin the first safe implementation batch. Do not migrate domain runtime
behavior into `cc-utils`.

## Read First

- `development/archive/plan/core-utilities-migration-plan.md`
- `docs/utils/core-utilities.md`
- `docs/utils/teams-swarm.md`
- `development/reference/CRATE_DEPENDENCY_TARGETS.md`
- `crates/cc-utils/src/lib.rs`
- `crates/cc-teams/src/lib.rs`

## Constraints

- Full Build mode applies. Do not skip Bun behavior because it was historically
  Lite unless the retained difference is documented as Intentional.
- Treat `development/reference/CRATE_DEPENDENCY_TARGETS.md` as the crate ownership
  boundary.
- Keep `cc-utils` pure and dependency-light.
- Put Team/Swarm behavior in `cc-teams`.
- Avoid wrappers around old root-owned code unless compatibility requires them.

## Suggested Batch 1

Write scope:
- `docs/utils/core-utilities.md`
- `crates/cc-utils/src/lib.rs`
- `crates/cc-utils/src/*` only if implementing pure helpers
- Any caller touched to remove duplicate helper code

Deliverables:
- Owner-aware utility matrix in docs.
- Explicit decision for `abort.rs`.
- Explicit decision for `file_state_cache.rs` vs `cc-tools::tool::FileStateCache`.
- Optional first pure helper module with caller adoption.

Do not touch:
- pane backend implementation
- broad Team/Swarm runtime rewrites
- git publication flow unless explicitly requested

## Suggested Batch 2

Write scope:
- `crates/cc-teams/src/mailbox.rs`
- `crates/cc-teams/src/protocol.rs`
- `crates/cc-teams/src/send_message.rs`
- `crates/cc-teams/src/runner.rs`
- `crates/cc-teams/src/types.rs`

Deliverables:
- Typed protocol helpers for structured team messages.
- Mailbox size/retention/compaction behavior.
- Mark-as-read by identity/predicate helpers.

## Suggested Batch 3

Write scope:
- `crates/cc-teams/src/helpers.rs`
- `crates/cc-teams/src/reconnection.rs`
- `crates/cc-teams/src/types.rs`
- new `crates/cc-teams/src/permission_sync.rs` if needed

Deliverables:
- Self-session persistence for teammates.
- Member removal/mode sync lifecycle helpers.
- Leader/worker permission and sandbox permission bridge.

## Expected Validation

When implementation begins, use targeted checks:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p cc-utils
cargo test -p cc-teams
cargo check -p claude-code-rs
```

Only run broader workspace builds after the targeted checks pass and the batch
is ready for integration.
