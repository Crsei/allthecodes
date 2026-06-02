# Workspace Split — Build Time Measurements

Baseline and per-phase build-time measurements for the workspace-split refactor
(design doc: [2026-04-20-workspace-split-design.md](superpowers/specs/2026-04-20-workspace-split-design.md)).

## Protocol

All timings recorded on the same workstation, same target-dir
(`F:/cargo-target/cc-rust`), with the existing `.cargo/config.toml` settings
(rust-lld linker on Windows MSVC). Each incremental row is taken in "steady
state" — immediately after a no-op build, then `touch` the file and rebuild.

```bash
# Incremental
touch <file> && cargo build --offline 2>&1 | tail -2

# Cold release
cargo clean && cargo build --offline --release 2>&1 | tail -1
```

Times come from cargo's own `Finished ... in Xs` line (wall-clock inside cargo).

## Baseline — pre-split (single crate)

Recorded 2026-04-20 on branch `workspace-split` at commit `8f048c4` (same tree
as `rust-lite`). Host: Windows 11, rustc 1.91.1, cargo 1.91.1.

| Scenario | Time |
|---|---|
| Incremental after `touch src/main.rs` | **6.74s** |
| Incremental after `touch src/tools/file_read.rs` | **0.42s** |
| Cold `cargo build --release` | *(pending — see note)* |

### Observations

- The incremental gap between `main.rs` (6.74s) and `tools/file_read.rs` (0.42s)
  is large (~16×). Cargo sub-divides the single crate into rustc codegen units;
  `main.rs` sits in a codegen unit with many sibling modules (wide module tree),
  while `tools/file_read.rs` sits in a smaller unit. The workspace split is
  expected to reduce the `main.rs` case the most, since splitting hub modules
  into crates eliminates the cross-codegen-unit re-link each time.
- Link time dominates both numbers; `rust-lld` is already in use (see
  `.cargo/config.toml`).

### Note on cold release

Cold `cargo clean && cargo build --release` was not run in the baseline commit:
the shared target-dir (`F:/cargo-target/cc-rust`) is used by other
worktrees/branches in-flight, and `cargo clean` would invalidate caches
unrelated to this measurement. We will capture the cold release figure on a
dedicated measurement commit later in the phase sequence (Phase 8), where a
full rebuild is required anyway.

## Per-phase timings

Appended as each phase lands on `rust-lite`.

| Phase | Date | Commit | `touch main.rs` | `touch file_read.rs` | Cold release |
|---|---|---|---|---|---|
| Baseline (P0 pre-move) | 2026-04-20 | `8f048c4` | 6.74s | 0.42s | — |
