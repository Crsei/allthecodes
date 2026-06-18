# Context Phase 0 Decisions

Date: 2026-05-06
Plan: `.omx/plans/context-implementation-phase-plan-2026-05-06.md`
Source map: `architecture/context-implementation-map.md`

## Decision

Phase 0 treats the existing context implementation as the behavior baseline and records two implementation boundaries before feature work begins.

1. `cc-utils` remains the synchronous, offline token-estimation layer. Provider-backed exact token counting must live above it in the async API/client or engine layer so the pure estimator remains usable for offline compaction, tests, and fallback paths.
2. `build_system_prompt()`'s `user_context` and `system_context` return values are internal provenance metadata for now. The serialized model request path continues to send `system_prompt`, messages, tools, thinking config, and model options. If a later provider path needs a separate context input layer, that must be introduced deliberately with request-builder tests.

## Baseline Evidence

- Token usage already has a heuristic report test covering threshold, method, and `exact_count_available=false` in `crates/cc-utils/src/tokens.rs`.
- Memory directory behavior already has tests for empty context, populated `MEMORY.md` index context, closed memory taxonomy, and all memory scope paths in `crates/cc-session/src/memdir.rs`.
- Session insight context already has tests for formatting, workspace filtering, current-session exclusion, age filtering, and tag include/exclude filtering in `crates/cc-services/src/session_memory.rs`.
- System prompt memory injection already has tests for auto-memory gating and session-insights injection in `crates/claude-code-rs/src/engine/system_prompt.rs`.
- Compaction baseline already has tests for pipeline no-op behavior, context-collapse ordering, local-token savings before auto-compact threshold, microcompact metadata, and preserved-segment creation in `crates/cc-compact/src/**`.

## New Phase 0 Lock

`test_context_maps_are_metadata_not_prompt_sections` locks the current `user_context` / `system_context` contract:

- `user_context` carries `cwd`, `date`, `platform`, and `model`;
- `system_context` remains empty reserved metadata;
- neither map is emitted as a literal `user_context` or `system_context` prompt section.

## Follow-Up For Phase 1

The exact-count implementation should add a provider matrix and official-doc check before enabling any network count path. Unsupported providers must fall back to the current heuristic report rather than blocking the query loop.
