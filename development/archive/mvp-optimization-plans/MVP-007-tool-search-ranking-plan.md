# MVP-007 Tool Search Ranking Optimization Plan

Status: implemented 2026-04-29.

Implementation evidence: `crates/claude-code-rs/src/tools/tool_search.rs`,
`crates/claude-code-rs/src/tools/registry.rs`, `crates/claude-code-rs/src/main.rs`,
and `cargo test -p claude-code-rs tools::tool_search -- --nocapture`.

## Goal

Upgrade lightweight tool search into deterministic, scalable retrieval over built-in tools, plugin tools, MCP tools, and skill-provided tools.

## Current Rust Surface

- `crates/claude-code-rs/src/tools/tool_search.rs`
- `crates/claude-code-rs/src/tools/`
- `crates/cc-skills/src/`

Current risk: simple keyword/fuzzy matching is acceptable for small catalogs but will degrade as tool and skill catalogs grow.

## Bun Reference Surface

- `F:\AIclassmanager\cc\claude-code-bun\src\utils\toolSearch.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\api\src\utils\toolSearch.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\skillSearch\localSearch.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\skillSearch\intentNormalize.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\skillSearch\__tests__\localSearch.test.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\components\agents\ToolSelector.tsx`

The Bun project has separate tool search and skill search utilities with tests around local search and intent normalization.

## Architecture Delta

| Concern | Rust target | Bun reference | Gap |
| --- | --- | --- | --- |
| Normalization | query parser | `intentNormalize.ts` | Rust needs reusable query normalization. |
| Ranking | search index | `toolSearch.ts`, `localSearch.ts` | Need deterministic scoring beyond fuzzy contains. |
| Schema loading | tool registry | API tool search | Need lazy schema hydration for large tools. |
| Tests | fixture corpus | `localSearch.test.ts` | Need ranking fixtures and regression snapshots. |
| UI | tool selector | `ToolSelector.tsx` | Need explainable results for user-facing search. |

## Plan

1. Build a `ToolSearchIndex` from tool name, description, category, tags, input schema summary, and examples.
2. Normalize query text with casing, punctuation, aliases, and action verbs.
3. Add deterministic scoring: exact name, prefix, alias, semantic keywords, schema terms, and recency/enablement boosts.
4. Defer expensive schema loading until the result is selected or ranking requires schema summary.
5. Add a stable fixture corpus with expected result ordering.

## Verification

- Ranking snapshot tests for common queries.
- Large-catalog performance test.
- Regression tests for disabled tools, plugin tools, MCP tools, and skill tools.
- UI smoke test for result labels and explanations.

## Close Criteria

MVP-007 can close when search quality is fixture-protected and catalog growth does not require loading every full schema on every query.
