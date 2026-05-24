# Context Phase 8 Model-Assisted Memory Recall Gate

Date: 2026-05-06
Plan: `.omx/plans/context-followup-phase-plan-2026-05-06.md`

## Decision

Model-assisted memory recall is implemented as an explicit opt-in side query.
The default path remains deterministic recall so normal turns stay offline
friendly, low latency, and testable without credentials.

Gate:

- `CC_RUST_MODEL_ASSISTED_MEMORY_RECALL=1`
- Legacy alias: `CC_RUST_MODEL_MEMORY_RECALL=1`
- Timeout override: `CC_RUST_MODEL_ASSISTED_MEMORY_RECALL_TIMEOUT_MS`

## Implementation

- `crates/cc-session/src/memdir.rs`
  - Exposes model-assisted recall constants.
  - Builds a bounded ranking prompt from deterministic memory candidates.
  - Parses JSON identity selections and maps them back to known candidates.
  - Reuses the existing `<memory-context>` formatter for selected memories.
- `crates/claude-code-rs/src/engine/lifecycle/submit_message.rs`
  - Checks the explicit gate while building the system prompt.
  - Skips session-insights and side recall when the user asks to ignore memory.
  - Sends at most 20 candidate snippets to the side query and accepts at most 5.
  - Reuses `surfaced_memory_keys` for both deterministic and model-ranked recall.
  - Falls back to deterministic recall if the provider is unavailable, times out,
    errors, or returns no recognizable memory identities.

## Verification

- `cargo test -p cc-session model_assisted_recall`
- `cargo test -p cc-session memory`
- `cargo test -p claude-code-rs submit_message`

## Remaining Scope

The Bun Sonnet-specific side-recall idea is mapped to the currently configured
cc-rust backend/model behind a gate. Making it default-on or forcing a dedicated
ranking model would change latency and provider-failure behavior, so that needs
a separate product decision.
