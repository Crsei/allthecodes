# Skill/MCP/Plugin Search Discovery and Kairos Tips Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` or `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split exact callable-tool lookup from domain discovery so allthecodes can proactively find relevant skills, MCP capabilities, and plugins, then surface those discoveries as Kairos tips while preserving upstream MCP skill and experimental skill-search semantics.

**Architecture:** Keep `ToolSearch` as the exact runtime tool catalog search that can return schema and invocation hints. Add domain search surfaces for `SkillSearch`, `McpSearch`, and `PluginSearch` using shared local ranking/result types plus runtime-injected MCP/plugin providers to avoid dependency cycles. Align MCP `skill://` resource ingestion and experimental skill-search prefetching with the upstream Bun feature docs, then feed high-confidence local discovery results into the existing suggestions/tips channels when Kairos or Proactive mode is active.

**Tech Stack:** Rust workspace, `allthecodes-tools` tool contracts, `allthecodes-commands` slash commands, `allthecodes-services` local suggestion heuristics, `allthecodes-daemon` proactive tick loop, Rust TUI command surfaces.

## Global Constraints

- This is Full Build work. Do not keep Lite-style omissions when touching these modules.
- `ToolSearch` remains for exact callable tools: tool names, input schema hydration, and invocation examples.
- `SkillSearch`, `McpSearch`, and `PluginSearch` are domain discovery surfaces: they return capability summaries, status summaries, and next-action hints, not executable tool schemas.
- Search may show lightweight status summaries, but status commands remain the source of full diagnostics: `/mcp status`, `/plugin status`, `/skills diagnostics`, and `SystemStatus`.
- All search/tip behavior is local-only in this implementation. Remote URL / remote registry discovery is an explicit deferred TODO and must not perform network requests.
- Add upstream-parity feature gates in `allthecodes-config`: `FEATURE_MCP_SKILLS` controls MCP `skill://` resource ingestion; `FEATURE_EXPERIMENTAL_SKILL_SEARCH` controls skill-search prefetch, turn-zero discovery, remote-state scaffolding, and extra Kairos tips. Baseline local `SkillSearch` remains available because allthecodes already exposes local `DiscoverSkills`.
- MCP skill discovery only reads `skill://` resources from connected MCP servers that advertise resources support. It must not treat normal MCP resources, tools, or prompts as skills.
- Experimental skill search has local search first. Remote skill marketplace/registry loading remains an inert TODO in this plan.
- Kairos tips are advisory. They must not install plugins, enable plugins, connect MCP servers, reload state, or execute skills automatically.
- Preserve path isolation: all persisted allthecodes data uses `~/.allthecodes/` and `.allthecodes/`, not original Codex paths.

## 2026-07-07 Status Update

- `FEATURE_MCP_SKILLS` and `FEATURE_EXPERIMENTAL_SKILL_SEARCH` are implemented
  as hidden/default-off gates.
- Remote skill/plugin/MCP URL discovery is now split into its own hidden gate:
  `FEATURE_REMOTE_URL_DISCOVERY` / `Feature::RemoteUrlDiscovery`.
- With `FEATURE_REMOTE_URL_DISCOVERY` disabled, local discovery still works but
  remote state is reported as `feature_disabled`.
- With `FEATURE_REMOTE_URL_DISCOVERY` enabled, the current implementation still
  reports `deferred` and performs no remote URL fetch, registry refresh, install,
  enable, reload, or remote trust action.
- Search tips now support persistent dismiss/remind-later using the same JSON
  shape as LSP recommendation dismissal records: `plugin_id`, `dismissed_at`,
  `remind_after_secs`, stored under
  `~/.allthecodes/search-tip-dismissals.json` or the `ALLTHECODES_HOME`
  equivalent.

## Current State

- `ToolSearch` lives in `crates/allthecodes-tools/src/runtime/tool_search.rs` and indexes runtime tools plus model-invocable skills. It can filter `source=plugin|mcp|skill` and hydrate schemas.
- `DiscoverSkills` lives in `crates/allthecodes-tools/src/skills/mod.rs` and already searches bundled, user, project, plugin, and MCP skills.
- Upstream `docs/features/mcp-skills.md` defines `FEATURE_MCP_SKILLS=1`: MCP servers expose skills through `skill://` resources, fetched only when resources are supported, converted into prompt/skill commands, refreshed on prompt/resource list changes, and cleared on disconnect.
- Rust has MCP `skill://` ingestion in `crates/allthecodes-engine/src/mcp_tool_adapter.rs` through `discover_mcp_skill_resources_for_context(...)`, producing `SkillSource::Mcp(server)`, gated by `FEATURE_MCP_SKILLS`.
- Upstream `docs/features/experimental-skill-search.md` defines `FEATURE_EXPERIMENTAL_SKILL_SEARCH=1`: `DiscoverSkills`, local search, prefetch, turn-zero discovery, signals, telemetry, remote loader, and remote state are wired but stubbed. In this Rust plan, local search is real, while remote URL/registry loading is separately gated by `FEATURE_REMOTE_URL_DISCOVERY` and still deferred.
- `/plugin marketplace search <q>` exists in `crates/allthecodes-commands/src/plugin_cmd.rs`, but top-level plugin discovery and model-facing `PluginSearch` do not exist.
- `/mcp` has list/status/config/runtime subcommands in `crates/allthecodes-commands/src/mcp/`, but no `McpSearch` command/tool exists.
- Rust TUI command surfaces already exist for skills, MCP, and plugins under `crates/allthecodes/src/ui/command_surface/surfaces/`.
- Suggestions already use `BackendMessage::Suggestions`, `PromptSuggestionService`, and TUI bottom-pane rendering. Kairos/proactive runtime uses `FEATURE_KAIROS`, `FEATURE_PROACTIVE`, and `crates/allthecodes-daemon/src/tick.rs`.

## Public Interfaces

### Model Tools

- `ToolSearch`
  - Keep existing input shape: `query`, `limit`, `source`, `include_schema`.
  - Keep `select:<tool-name>` exact lookup semantics.
  - Keep returning `input_schema` and `invocation` only from this surface.

- `SkillSearch`
  - Input:
    - `query: string`
    - `source?: "all" | "bundled" | "user" | "project" | "plugin" | "mcp"`
    - `max_results?: integer`
  - Output:
    - `query`, `source`, `count`
    - `results[]` with `kind="skill"`, `name`, `display_name`, `source`, `description`, `when_to_use`, `user_invocable`, `model_invocable`, `status_summary`, `match_reasons`, `next_action`, `discovery_signal`, `prefetch_source`, `remote_url_todo`
  - `DiscoverSkills` remains available and calls the same search helper while preserving its current `skills[]` output compatibility.
  - `FEATURE_EXPERIMENTAL_SKILL_SEARCH` enables prefetch/turn-zero enrichment and remote-state placeholders; it does not disable the baseline local search tool.

- `McpSearch`
  - Input:
    - `query: string`
    - `scope?: "all" | "user" | "project" | "runtime"`
    - `max_results?: integer`
    - `include_tool_summaries?: boolean`
  - Output:
    - `query`, `scope`, `count`
    - `results[]` with `kind="mcp_server" | "mcp_resource" | "mcp_capability" | "mcp_skill"`, `name`, `server_name`, `description`, `status_summary`, `capabilities`, `tool_summaries`, `skill_summaries`, `match_reasons`, `next_action`, `remote_url_todo`
  - `tool_summaries` may list MCP tool names and descriptions, but must not include full input schemas or invocation JSON. Use `ToolSearch(source=mcp, query="select:mcp__server__tool", include_schema=true)` for exact callable tool details.
  - `skill_summaries` may list MCP-provided skills derived from `skill://` resources when `FEATURE_MCP_SKILLS` is enabled.

- `PluginSearch`
  - Input:
    - `query: string`
    - `source?: "all" | "installed" | "active" | "marketplace_cache"`
    - `max_results?: integer`
    - `include_contributions?: boolean`
  - Output:
    - `query`, `source`, `count`
    - `results[]` with `kind="plugin"`, `id`, `name`, `version`, `description`, `status_summary`, `skills`, `tools`, `mcp_servers`, `marketplace`, `match_reasons`, `next_action`, `remote_url_todo`
  - Plugin-contributed tool names are summaries only. Use `ToolSearch(source=plugin, ...)` for exact callable tool schema and invocation details.

### Commands

- Add `/skills search <query>` as a discovery command while keeping `/skills`, `/skills <name>`, `/skills reload`, and `/skills diagnostics`.
- Add `/mcp search <query>` as a discovery command while keeping `/mcp list` and `/mcp status` unchanged.
- Add top-level `/plugin search <query>` for local plugin discovery. Keep `/plugin marketplace search <q>` as marketplace-cache search.
- Search command output should include a short line when full diagnostics are elsewhere, for example: `For health details, run /mcp status`.

### Tips

- Use existing `BackendMessage::Suggestions { items }` for UI/headless tips; do not add a new IPC message for v1.
- Add a local search-tip service that emits plain prompt suggestion strings such as:
  - `Tip: matching skill found: debug. Use /skills debug for details.`
  - `Tip: MCP server github has matching tools. Use /mcp search github or ToolSearch source=mcp for tool schema.`
  - `Tip: plugin rust-analyzer may help this workspace. Use /plugin info rust-analyzer before installing or enabling.`
- Tips must be feature-gated by `FEATURE_KAIROS` or `FEATURE_PROACTIVE`, rate-limited, deduplicated, and dismissible/remind-later compatible with the existing recommendation dismissal pattern.
- When `FEATURE_EXPERIMENTAL_SKILL_SEARCH` is enabled, tips may use prefetch/turn-zero skill discovery results. When disabled, tips may still use local search results that are already available from explicit search tools.

## File Plan

- Modify `crates/allthecodes-config/src/features.rs`
  - Add `Feature::McpSkills` with env var `FEATURE_MCP_SKILLS`.
  - Add `Feature::ExperimentalSkillSearch` with env var `FEATURE_EXPERIMENTAL_SKILL_SEARCH`.
  - Add `Feature::RemoteUrlDiscovery` with env var `FEATURE_REMOTE_URL_DISCOVERY`.
  - Keep all three disabled by default and included in `/experimental list` / `FeatureFlags::all_enabled()`.

- Create `crates/allthecodes-tools/src/discovery_search.rs`
  - Shared result structs: `DiscoverySearchInput`, `DiscoverySearchResult`, `DiscoveryResultKind`, `DiscoveryStatusSummary`, `DiscoveryNextAction`.
  - Shared signal structs: `DiscoverySignal`, `SkillPrefetchResult`, and remote-state placeholder fields.
  - Shared ranking helpers for local domain discovery.
  - Runtime provider registration using `OnceLock`/`RwLock` so MCP/plugin catalogs are injected by the root runtime instead of imported directly.
  - Tool implementations: `SkillSearchTool`, `McpSearchTool`, `PluginSearchTool`.

- Modify `crates/allthecodes-tools/src/lib.rs` and `crates/allthecodes-tools/src/registry.rs`
  - Export `discovery_search`.
  - Register the three new tools in `allthecodes_tools_base_tools()`.
  - Keep `ToolSearchTool` registration unchanged.

- Modify `crates/allthecodes-tools/src/skills/mod.rs`
  - Extract the current `DiscoverSkills` scoring into a reusable helper.
  - Implement `SkillSearch` output from the helper.
  - Preserve `DiscoverSkills` input aliases `description` and `limit`, and preserve its current `skills[]` output for compatibility.

- Modify runtime wiring in `crates/allthecodes/src/command_runtime_bridge.rs` or the adjacent runtime adapter used by `full_init.rs`
  - Install the MCP discovery provider from existing MCP discovery/runtime snapshots.
  - Install the plugin discovery provider from installed plugins, active plugins, and marketplace cache helpers.
  - The providers return plain `DiscoverySearchResult`-like data; no search tool should directly depend on MCP/plugin implementation crates.

- Modify MCP skill lifecycle wiring in `crates/allthecodes-engine/src/mcp_tool_adapter.rs` and `crates/allthecodes/src/full_init.rs`
  - Gate `discover_mcp_skill_resources_for_context(...)` registration with `Feature::McpSkills`.
  - Track MCP skill provenance by server name and `skill://` URI so reconnect/disconnect/resource-refresh can replace or clear stale skills.
  - Keep the builder boundary: MCP lists/reads resources; `allthecodes-skills` parses frontmatter and validates packages.

- Modify slash command surfaces:
  - `crates/allthecodes-commands/src/skills_cmd.rs`: parse `search <query>` and render `SkillSearch`-equivalent local results.
  - `crates/allthecodes-commands/src/mcp/mod.rs`: route `search`; create `crates/allthecodes-commands/src/mcp/search.rs`.
  - `crates/allthecodes-commands/src/mcp/help.rs`: document `/mcp search <query>`.
  - `crates/allthecodes-commands/src/plugin_cmd.rs`: add top-level `search <query>` and keep `marketplace search <q>`.

- Modify TUI command surfaces:
  - `crates/allthecodes/src/ui/command_surface/surfaces/skills.rs`: when filtered text is present, Enter should produce `/skills search <filter>` unless a selected exact skill is chosen.
  - `crates/allthecodes/src/ui/command_surface/surfaces/mcp.rs`: expose search action without replacing status/list actions.
  - `crates/allthecodes/src/ui/command_surface/surfaces/plugin.rs`: expose search action and keep enable/disable/uninstall/info behavior.
  - `crates/allthecodes/src/ui/command_surface/tests.rs`: cover command output strings for the new actions.

- Create `crates/allthecodes-services/src/search_tips.rs`
  - Define `SearchTipCandidate`, `SearchTipService`, suppression reasons, cooldown/dedupe keys, and formatting.
  - Reuse the style of `PromptSuggestionService`: local heuristics, no network, rate-limited output.
  - Support dismiss/remind-later data shape compatible with `RecommendationDismissal` semantics from `allthecodes-lsp-service`.

- Create `crates/allthecodes-services/src/skill_search_prefetch.rs`
  - Provide Rust equivalents of upstream `startSkillDiscoveryPrefetch()`, `collectSkillDiscoveryPrefetch()`, and `getTurnZeroSkillDiscovery()`.
  - Use local skill indexes only in this plan.
  - Return remote skill state placeholders without fetching remote URLs.

- Modify `crates/allthecodes-services/src/lib.rs` and `crates/allthecodes-services/src/prompt_suggestion.rs`
  - Export `search_tips`.
  - Export `skill_search_prefetch`.
  - Merge search tips and prefetch-derived skill tips with prompt suggestions by confidence, with a small cap so tips do not crowd out normal suggestions.

- Modify Kairos/proactive integration:
  - `crates/allthecodes-daemon/src/tick.rs`: during proactive ticks, gather local search-tip candidates and include at most three advisory tips in the tick prompt or emitted suggestions.
  - `crates/allthecodes/src/app_runtime_adapters/sdk_mapper.rs`: include search-tip suggestions when sending `BackendMessage::Suggestions`.
  - `crates/allthecodes/src/ui/tui/engine_events.rs`: include search-tip suggestions in TUI generation path when the feature gate is enabled.

## Tasks

### Task 0: Feature Gates and Upstream Parity Contracts

**Files:**
- Modify: `crates/allthecodes-config/src/features.rs`
- Modify: `crates/allthecodes-commands/src/experimental.rs`
- Test: `crates/allthecodes-config/src/features.rs`
- Test: `crates/allthecodes-commands/src/experimental.rs`

**Interfaces:**
- Produces `Feature::McpSkills` / `FeatureFlags::mcp_skills`.
- Produces `Feature::ExperimentalSkillSearch` / `FeatureFlags::experimental_skill_search`.
- Produces `Feature::RemoteUrlDiscovery` / `FeatureFlags::remote_url_discovery`.

- [ ] Add descriptors for `FEATURE_MCP_SKILLS`, `FEATURE_EXPERIMENTAL_SKILL_SEARCH`, and `FEATURE_REMOTE_URL_DISCOVERY`.
- [ ] Keep all three flags disabled by default and enabled by `FeatureFlags::all_enabled()`.
- [ ] Update feature descriptor count tests and `/experimental list` expectations.
- [ ] Add tests that `FEATURE_KAIROS` does not imply either search flag.

### Task 1: Shared Discovery Search Types and Provider Boundary

**Files:**
- Create: `crates/allthecodes-tools/src/discovery_search.rs`
- Modify: `crates/allthecodes-tools/src/lib.rs`
- Modify: `crates/allthecodes-tools/src/registry.rs`

**Interfaces:**
- Produces `DiscoverySearchRuntime` with provider functions for `mcp_items` and `plugin_items`.
- Produces `SkillSearchTool`, `McpSearchTool`, and `PluginSearchTool`.
- Produces a shared `search_items(query, max_results, source_filter)` helper used by tools and tests.
- Produces `DiscoverySignal` values for explicit search, prefetch search, MCP resource discovery, and plugin marketplace-cache discovery.

- [ ] Write tests with fake provider data for ranking, max-result clamping, and empty-query validation.
- [ ] Implement shared structs and provider installation.
- [ ] Register the new tools without changing `ToolSearchTool`.
- [ ] Verify invalid provider state returns an empty local result set with an explanatory preview instead of panicking.
- [ ] Verify remote-state fields are present but inert: no URL fetcher, no registry refresh, no network dependency.

### Task 2: SkillSearch and DiscoverSkills Compatibility

**Files:**
- Modify: `crates/allthecodes-tools/src/skills/mod.rs`
- Test: existing module tests or new tests in the same file

**Interfaces:**
- `SkillSearch` returns normalized `results[]`.
- `DiscoverSkills` keeps current `skills[]` output and existing aliases.

- [ ] Extract current skill scoring into a pure helper that returns scored skill rows.
- [ ] Add `SkillSearchTool` using that helper.
- [ ] Keep `DiscoverSkillsTool` behavior compatible.
- [ ] Add tests for source filters `plugin` and `mcp`, `description` alias, and deterministic ordering.
- [ ] Add tests that `SkillSource::Mcp(server)` results include server provenance and can be surfaced as `mcp_skill` summaries in `McpSearch`.
- [ ] Add tests that experimental prefetch metadata is absent when `FEATURE_EXPERIMENTAL_SKILL_SEARCH` is disabled.

### Task 3: MCP Skill Lifecycle and Discovery Providers

**Files:**
- Modify: `crates/allthecodes/src/command_runtime_bridge.rs`
- Modify: `crates/allthecodes/src/full_init.rs` only if provider installation belongs beside existing runtime catalog installation
- Modify: `crates/allthecodes-engine/src/mcp_tool_adapter.rs`
- Modify: `crates/allthecodes-mcp/src/manager.rs` only if stale resource-cache invalidation needs a manager-owned hook
- Test: add focused tests near the adapter if existing patterns allow it

**Interfaces:**
- MCP provider returns configured servers, runtime server state, resources, capabilities, tool summaries, and lightweight status summary.
- MCP skill provider returns `skill://` resource summaries only when `Feature::McpSkills` is enabled.
- Plugin provider returns disk-installed plugins, active plugins, marketplace-cache entries, contribution summaries, and lightweight status summary.

- [ ] Build MCP rows from existing scoped discovery and runtime status builders.
- [ ] Include connected MCP `skill://` resources as `mcp_skill` rows with `SkillSource::Mcp(server)` provenance.
- [ ] Gate MCP skill registration with `Feature::McpSkills`.
- [ ] Clear or replace server-owned MCP skills when a server disconnects, reconnects, or emits resource-list refresh events.
- [ ] Skip MCP skill discovery when a server does not advertise resources support.
- [ ] Build plugin rows from installed plugins, active plugins, and marketplace-cache helpers.
- [ ] Include `remote_url_todo: true` / `remote_source: "todo"` fields where appropriate.
- [ ] Ensure providers never call reconnect, refresh, install, enable, reload, or network operations.
- [ ] Add tests for feature disabled, resources unsupported, `skill://` resource read success, resource read failure diagnostics, and stale server skill cleanup.

### Task 4: Slash Commands and TUI Surfaces

**Files:**
- Modify: `crates/allthecodes-commands/src/skills_cmd.rs`
- Modify: `crates/allthecodes-commands/src/mcp/mod.rs`
- Create: `crates/allthecodes-commands/src/mcp/search.rs`
- Modify: `crates/allthecodes-commands/src/mcp/help.rs`
- Modify: `crates/allthecodes-commands/src/plugin_cmd.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/skills.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/mcp.rs`
- Modify: `crates/allthecodes/src/ui/command_surface/surfaces/plugin.rs`
- Test: `crates/allthecodes-commands/src/mcp/tests.rs`, `crates/allthecodes/src/ui/command_surface/tests.rs`

**Interfaces:**
- `/skills search <query>`
- `/mcp search <query>`
- `/plugin search <query>`

- [ ] Add command parsing and usage text.
- [ ] Render concise result tables with match reasons and next-action hints.
- [ ] Include status boundary hints, for example `/mcp status` for health and `ToolSearch` for exact tool schemas.
- [ ] Update command surface key handling so filtered searches can be submitted without replacing exact item actions.
- [ ] Add tests for command strings and unknown/empty query behavior.

### Task 5: Kairos Search Tips

**Files:**
- Create: `crates/allthecodes-services/src/search_tips.rs`
- Create: `crates/allthecodes-services/src/skill_search_prefetch.rs`
- Modify: `crates/allthecodes-services/src/lib.rs`
- Modify: `crates/allthecodes-services/src/prompt_suggestion.rs`
- Modify: `crates/allthecodes-daemon/src/tick.rs`
- Modify: `crates/allthecodes/src/app_runtime_adapters/sdk_mapper.rs`
- Modify: `crates/allthecodes/src/ui/tui/engine_events.rs`

**Interfaces:**
- `SearchTipService::try_generate(context, candidates) -> Option<Vec<PromptSuggestion>>`
- `start_skill_discovery_prefetch(context) -> SkillPrefetchHandle`
- `collect_skill_discovery_prefetch(handle) -> SkillPrefetchResult`
- `get_turn_zero_skill_discovery(session_id) -> Option<SkillPrefetchResult>`
- Tips flow through existing `PromptSuggestion` and `BackendMessage::Suggestions`.

- [ ] Add local tip candidate model with kind `skill`, `mcp`, `plugin`.
- [ ] Gate generation on `Feature::Kairos` or `Feature::Proactive`.
- [ ] Gate prefetch/turn-zero skill discovery on `Feature::ExperimentalSkillSearch`.
- [ ] Implement prefetch using local skill metadata only: names, descriptions, `when_to_use`, source, argument hints, paths, assets, dependencies, and prompt body.
- [ ] Store remote skill state as `not_configured` / `feature_disabled` / `deferred` and never fetch remote URLs.
- [ ] Apply dedupe and cooldown so the same domain item is not repeated in short intervals.
- [ ] Format tips as advisory prompts, never as automatic actions.
- [ ] Add tests for feature gate disabled, no candidates, one high-confidence candidate, prefetch collection, turn-zero retrieval, dedupe, cooldown, and remote URL exclusion.

### Task 6: Verification and Documentation

**Files:**
- Modify: `development/runtime/tool-discovery-current-state.md` after implementation lands
- Optional: add a short index entry if `development/README.md` is being updated for nearby work

**Commands:**

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes-tools discovery_search skills:: -- --nocapture
cargo test -p allthecodes-commands plugin_cmd:: mcp:: skills_cmd:: -- --nocapture
cargo test -p allthecodes-config features -- --nocapture
cargo test -p allthecodes-engine mcp_tool_adapter -- --nocapture
cargo test -p allthecodes-services search_tips skill_search_prefetch prompt_suggestion -- --nocapture
cargo test -p allthecodes --lib command_surface search_tips -- --nocapture
cargo build --workspace --release
```

- [ ] Confirm no new warnings after build.
- [ ] Confirm `ToolSearch` exact schema lookup still works for builtin, MCP, plugin, and skill sources.
- [ ] Confirm `McpSearch` and `PluginSearch` do not return full callable schemas.
- [ ] Confirm `FEATURE_MCP_SKILLS=0` prevents MCP `skill://` resource registration while preserving normal MCP tools/resources.
- [ ] Confirm `FEATURE_EXPERIMENTAL_SKILL_SEARCH=0` suppresses prefetch/turn-zero enrichment while preserving explicit local `SkillSearch`.
- [ ] Confirm Kairos/proactive tips are absent when feature gates are disabled.
- [ ] Confirm remote URL discovery remains inert: `feature_disabled` while the gate is off, `deferred` while the gate is on.

## Deferred TODO: Remote URL Discovery

Remote URL search is intentionally not implemented in this plan. The implementation should reserve result fields such as `remote_url`, `remote_source`, or `remote_url_todo`, but it must not fetch URLs, refresh remote registries, install packages, or trust remote metadata.

Future work should define:

- Remote skill loader and remote skill state behavior corresponding to upstream `remoteSkillLoader.ts` and `remoteSkillState.ts`.
- Allowed URL schemes and trust policy.
- Cache location under `~/.allthecodes/`.
- User approval flow before first network access.
- Marketplace/plugin/MCP/skill metadata validation.
- Rate limits, timeout policy, checksum/signature handling, and offline behavior.

## Acceptance Criteria

- A model can use `SkillSearch`, `McpSearch`, and `PluginSearch` to discover relevant domain capabilities without needing exact callable tool names.
- A model uses `ToolSearch` when it needs exact tool names, input schemas, or invocation details.
- Search and status no longer compete: search returns row-level summaries; status remains diagnostic authority.
- MCP `skill://` resources appear as searchable MCP-provided skills only when `FEATURE_MCP_SKILLS` is enabled, and stale server-owned skills are cleared on disconnect/refresh.
- `FEATURE_EXPERIMENTAL_SKILL_SEARCH` adds local prefetch and turn-zero discovery metadata without adding network behavior.
- Kairos/proactive mode can emit local, rate-limited tips for relevant skills, MCP servers, and plugins.
- Remote URL discovery is visible as a tracked deferred item and cannot accidentally perform network access in this implementation.
