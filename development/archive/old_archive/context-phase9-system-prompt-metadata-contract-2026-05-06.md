# Context Phase 9 System Prompt Metadata Contract

Date: 2026-05-06
Plan: `.omx/plans/context-followup-phase-plan-2026-05-06.md`

## Decision

`user_context` and `system_context` are internal provenance metadata, not
provider-facing prompt layers.

`build_system_prompt()` returns them so the engine can carry structured context
about prompt construction. `build_messages_request()` intentionally serializes
only the model-facing request surface: system prompt blocks, messages, tools,
thinking, model, token limits, and advisor model.

## Evidence

- `user_context` currently records cwd, date, platform, and model.
- `system_context` is reserved and currently empty.
- `ModelCallParams` does not carry either map into request construction.
- `test_context_maps_are_metadata_not_prompt_sections` asserts the maps are
  returned as metadata and do not appear in prompt parts.

## Verification

- `cargo test -p claude-code-rs test_context_maps_are_metadata_not_prompt_sections`

## Remaining Scope

If upstream parity later requires a provider-facing context layer, add a new
request contract and tests instead of silently reusing these metadata maps.
