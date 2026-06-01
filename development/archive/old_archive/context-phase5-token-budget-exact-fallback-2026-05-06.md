# Context Phase 5 Token Budget Exact Fallback

Date: 2026-05-06
Plan: `.omx/plans/context-followup-phase-plan-2026-05-06.md`

## Decision

Phase 5 wires provider exact token counts into auto-compact only when the
local heuristic estimate is close to the 80% context threshold. The pure
`cc-utils` and `cc-compact` layers remain provider-free; the engine layer owns
the optional exact-count preflight because it already has access to the
configured `ApiClient`.

The fallback is deliberately bounded. Clearly safe and clearly over-threshold
heuristic estimates keep the existing local decision. Near-threshold estimates
use provider exact counts when Anthropic/Azure/Gemini support is available; any
provider error keeps the heuristic decision so query dispatch is not blocked.

## Implemented

- `cc_compact::auto_compact::auto_compact_threshold_tokens()` exposes the
  existing 80% threshold calculation.
- `cc_compact::auto_compact::should_check_exact_for_auto_compact()` marks
  estimates within +/- 5% of the model context window as exact-count candidates.
- `PipelineResult` now exposes `auto_compact_triggered` so downstream code can
  distinguish this turn's threshold crossing from previously persisted tracking
  state.
- `QueryEngineDeps::autocompact()` asks `ApiClient::count_token_usage_exact()`
  for a provider report only near the threshold and only when the provider
  advertises exact-count support.
- Exact reports can suppress false-positive heuristic compaction or trigger
  false-negative heuristic misses near the threshold.
- Exact-count failures log a diagnostic and preserve the heuristic decision.

## Verification

- `cargo test -p cc-compact auto_compact`
- `cargo test -p claude-code-rs exact_auto_compact_trigger`

## Remaining Gaps

- OpenAI, Bedrock, and Vertex exact-count parity remains a Phase 6 decision.
- The exact preflight currently counts the message surface used by the
  compaction pipeline; full request-surface parity should be revisited only if
  Phase 6 introduces provider semantics that require it.
