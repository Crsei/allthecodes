# Query Loop Boundary Decision

> Date: 2026-07-03
> Scope: `crates/allthecodes-engine/src/query/`
> Status: Active decision for the Full Build phase

## Decision

Keep the query loop inside `allthecodes-engine` for now.

`crates/allthecodes-engine/src/query/` is the only production query loop. The previous
independent `allthecodes-query` crate was removed because it duplicated behavior.

Do not recreate `allthecodes-query` as a parallel implementation for tests,
experiments, or staged migration. If the query loop is extracted again, extraction must
happen exactly once into the canonical production crate, and the engine-internal
implementation must be removed as part of the same boundary change.

## Current State

The query loop still coordinates engine-owned lifecycle state, recovery behavior, token
budgeting, stop hooks, tool execution flow, and submit-side effects. Keeping it under
`allthecodes-engine` makes the active ownership explicit while Tasks 1-8 reduce runtime
coupling around services, tool metadata, shared state, tool execution, submit handling,
and runtime settings.

This is not a Lite boundary. Full Build work must continue to preserve and complete
upstream-equivalent behavior in the canonical engine query loop.

## Extraction Criteria

Query loop extraction is allowed only when all of these are true:

- `QueryDeps` is split into stable capability traits.
- The query loop no longer reaches engine-internal state directly.
- Submit side effects are represented by typed events or `SubmitTransaction`.
- There is one production implementation after extraction.

These criteria are required to prevent a second implementation from becoming a drift
source. Tests may use fakes for stable traits, but they must exercise the same
production query loop implementation.

## Non-Goals

- Do not add new process-wide `LazyLock`, `RwLock`, or `OnceLock` callback registries as
  the extraction path.
- Do not move permission, shell-risk, or tool JSON parsing decisions into the UI layer.
- Do not reintroduce `allthecodes-query` as an active crate while
  `crates/allthecodes-engine/src/query/` remains production code.
