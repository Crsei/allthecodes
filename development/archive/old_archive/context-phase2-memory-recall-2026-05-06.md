# Context Phase 2 Memory Recall

Date: 2026-05-06
Plan: `.omx/plans/context-implementation-phase-plan-2026-05-06.md`

## Decision

Phase 2 implements the local relevant-memory recall path first. The recall path
is deterministic and offline so it can run before the main request without
adding provider latency, credentials requirements, or extra failure modes.

Model-assisted side-query recall remains an advanced parity gap. It should be
added only behind an explicit gate because it would add another provider call
before normal query dispatch.

## Implemented

- `MemoryEntry` accepts optional `description` and `search_terms` metadata while
  preserving legacy entries that only have `key`, `value`, `category`, and
  timestamps.
- `cc_session::memdir::recall_relevant_memories()` scans Project, Global, Team
  when enabled, and Auto when requested.
- Recall scores query terms across key, category, closed memory type, value,
  description, and search terms.
- Results are capped by caller, currently 5 in the main submit flow.
- Generic development/reference memories for recently used tools are skipped, while
  warnings, pitfalls, known issues, security notes, and failure notes are kept.
- Surfaced identities use `<scope>:<key>` and are tracked in `AppState` so the
  same memory is not repeatedly recalled within a session.
- Explicit "ignore memory" requests skip relevant memory and session-insights
  injection for that turn.
- `build_system_prompt_with_memory_contexts()` lets the submit path pass a
  prebuilt relevant memory context while preserving the old full-memory wrapper
  for dump/tests/legacy call sites.

## Verification

- `cargo test -p cc-session memdir::tests`
- `cargo test -p claude-code-rs test_prebuilt_memory_context_overrides_full_memory_scan`
- `cargo test -p claude-code-rs test_session_memory_context_injection`

## Remaining Gaps

- No model-assisted memory side-query is enabled yet.
- Relevant recall is deterministic keyword scoring, not semantic embedding or
  provider-ranked recall.
- Existing full-memory wrapper remains available for non-query prompt surfaces.
