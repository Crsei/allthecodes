# Tools Phase 5 - Web Provider Difference Closure

Date: 2026-05-06

## Scope

Phase 5 documents the WebSearch / WebFetch implementation-path difference called out by `architecture/tools-implementation-map.md`.
No runtime code changed in this phase.

## Changes

- Added `architecture/web-tools-provider-diff.md`.
- Recorded that Bun WebSearch uses Anthropic `web_search_20250305`, while cc-rust WebSearch uses local Tavily / Brave providers.
- Recorded that cc-rust WebFetch keeps the WebFetch tool surface while enforcing local URL normalization, sandbox network policy, same-origin redirect limits, response caching, and binary response rejection.
- Updated `architecture/tools-implementation-map.md` to link the provider-difference note and remove the old follow-up action.

## Verification

- `git diff --check -- architecture/tools-implementation-map.md architecture/web-tools-provider-diff.md docs/archive/tools-phase5-web-provider-diff-2026-05-06.md` passed.

## Notes

The documented difference is provider parity, not tool-family implementation parity.
If the product later requires Anthropic server-tool WebSearch specifically, track it as a separate provider decision.
