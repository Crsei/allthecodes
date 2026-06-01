# Context Phase 7 Partial Compact Command And Round Trips

Date: 2026-05-06
Plan: `.omx/plans/context-followup-phase-plan-2026-05-06.md`

## Decision

Partial Compact is now exposed through the existing `/compact` command instead
of adding a new command family.

Supported syntax:

- `/compact up-to <message-index-or-uuid-prefix> [summary]`
- `/compact from <message-index-or-uuid-prefix> [summary]`

Message indexes are 1-based over visible user/assistant messages. UUID anchors
use deterministic prefix matching and reject ambiguous prefixes. If a caller
does not provide a summary, the command creates a bounded deterministic local
summary from the selected segment.

## Implementation

- `crates/claude-code-rs/src/commands/compact.rs`
  - Parses `up-to`, `up_to`, `upto`, and `from` partial compact modes.
  - Resolves anchors by visible message index or UUID prefix.
  - Calls `cc_compact::partial_compact()`.
  - Returns explicit disabled/no-op/error messages.
- `crates/cc-session/src/storage.rs`
  - Persists and resumes `SystemSubtype::CompactBoundary` metadata.
  - Preserves `compact_metadata.preserved_segment` across save/load.
- `crates/cc-session/src/session_export/`
  - Adds `compact_boundaries[].preserved_segment` to structured exports.

## Verification

- `cargo test -p claude-code-rs partial_compact`
- `cargo test -p claude-code-rs compact::tests`
- `cargo test -p cc-session compact`
- `cargo test -p cc-compact partial_compact`

## Remaining Scope

The command-level behavioral contract is implemented. A richer TUI anchor
picker remains optional polish; it should call the same command/primitive path
instead of inventing a separate compaction behavior.
