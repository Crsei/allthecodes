# Context Phase 3 Partial Compact

Date: 2026-05-06
Plan: `.omx/plans/context-implementation-phase-plan-2026-05-06.md`

## Decision

Phase 3 implements Partial Compact in the core `cc-compact` crate first. The
command/UI selection surface is intentionally left as a follow-up because there
is not yet an existing anchor-selection command surface to attach without
inventing UI behavior.

## Implemented

- `cc_compact::partial_compact` adds `PartialCompactDirection::{UpTo, From}`.
- `up_to(anchor)` summarizes messages before the adjusted anchor boundary and
  preserves the anchor plus later context.
- `from(anchor)` preserves early context through the adjusted anchor boundary
  and summarizes later messages.
- Both directions preserve API invariants:
  - retained `tool_result` blocks pull in their matching assistant `tool_use`;
  - retained assistant `tool_use` blocks pull in their matching `tool_result`
    when that result exists later in the conversation.
- Partial Compact emits a normal `CompactBoundary` with `preserved_segment`
  metadata, including the summary message UUID and preserved message UUIDs.
- `cc_compact::gates::CompactionFeatureGates` makes auto, reactive,
  session-memory, and partial compact gates explicit. Defaults keep existing
  behavior; falsey env values disable individual lanes.

## Verification

- `cargo test -p cc-compact partial_compact`
- `cargo test -p cc-compact gates::tests`
- `cargo test -p cc-compact`

## Remaining Gaps

- No slash-command or UI anchor picker is wired yet.
- No session export/resume round-trip specific to Partial Compact has been
  added yet.
- PTL retry still uses existing collapse/reactive paths; Partial Compact is not
  currently selected automatically during PTL recovery.
