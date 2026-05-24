# MVP-010 Skill System Package Management Optimization Plan

## Goal

Evolve skills from loaded markdown/context snippets into versioned, validated, reloadable packages with dependency handling and MCP skill builder parity.

## Current Rust Surface

- `crates/cc-skills/src/`
- `crates/claude-code-rs/src/tools/skill.rs`
- `crates/claude-code-rs/src/commands/skills_cmd.rs`

Current status: implemented locally on 2026-04-29. Skill loading now treats candidates as validated packages, resolves dependency graphs, tracks versions and registry revisions, exposes reload diagnostics, and can ingest MCP `skill://` resources.

## Bun Reference Surface

- `F:\AIclassmanager\cc\claude-code-bun\src\skills\loadSkillsDir.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\skills\bundledSkills.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\skills\mcpSkillBuilders.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\skills\mcpSkills.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\skillSearch\remoteSkillLoader.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\services\skillSearch\remoteSkillState.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\commands\reload-plugins\reload-plugins.ts`
- `F:\AIclassmanager\cc\claude-code-bun\src\plugins\builtinPlugins.ts`

The Bun project has bundled skills, MCP skill builders, remote skill state, skill search, and plugin reload surfaces.

## Architecture Delta

| Concern | Rust target | Bun reference | Gap |
| --- | --- | --- | --- |
| Loading | `cc-skills` loader | `loadSkillsDir.ts` | Implemented: frontmatter diagnostics, package metadata, dependency/version validation, package path checks. |
| Bundled skills | packaged registry | `bundledSkills.ts` | Implemented: bundled skills now expose versioned package metadata before registry insertion. |
| MCP skills | builder bridge | `mcpSkillBuilders.ts`, `mcpSkills.ts` | Implemented: connected MCP `skill://` resources are read and converted through the same Rust skill loader. |
| Remote skills | optional loader | `remoteSkillLoader.ts` | Intentional crop for this pass: no remote skill search/state service until a product-level remote marketplace requirement exists. |
| Reload | command/watch | `reload-plugins.ts` | Implemented: startup, IPC skill reload, `/skills reload`, and `/reload-plugins` rebuild the registry with a revision and diagnostics. File watchers remain cropped. |

## Plan

1. Done: define a skill package schema with name, version, description, triggers, dependencies, compatible app version, assets, and entry docs.
2. Done: validate frontmatter and package layout with actionable diagnostics.
3. Done: add dependency graph resolution with cycle/conflict errors.
4. Done: add reload/invalidation for bundled, user, project, plugin, and MCP skill candidates.
5. Done: implement MCP skill builders for `skill://` resources.
6. Already covered by MVP-007: ToolSearch indexes current model-invocable skills from the runtime registry.

## Implemented Rust Surface

- `crates/cc-skills/src/lib.rs`
  - Adds package metadata fields, dependency declarations, app-version compatibility, validation diagnostics, registry revisions, and dependency ordering.
- `crates/cc-skills/src/loader.rs`
  - Parses normalized frontmatter keys, reports malformed/unknown frontmatter, supports dependencies/assets/entry docs, and exposes diagnostic directory loads.
- `crates/cc-skills/src/bundled.rs`
  - Returns bundled skills as versioned package candidates before registration.
- `crates/claude-code-rs/src/main.rs`
  - Loads bundled/user/project/plugin skills through one resolver and folds MCP skill resources into the registry after MCP connection.
- `crates/claude-code-rs/src/mcp/tools.rs`
  - Converts MCP `skill://` resources into `SkillDefinition` values using the same Rust loader.
- `crates/claude-code-rs/src/commands/skills_cmd.rs`
  - Adds `/skills reload`, `/skills diagnostics`, version display, dependencies, path filters, assets, entry docs, and base-dir detail output.
- `crates/claude-code-rs/src/commands/reload_plugins_cmd.rs`
  - Reloads skill packages after plugin reload so plugin-distributed skills are invalidated with plugin state.
- `crates/claude-code-rs/src/ipc/subsystem_handlers.rs`
  - Uses the same package reload path for frontend skill reload commands.

## Intentional Crops

- Remote skill search/state loading from `remoteSkillLoader.ts` and `remoteSkillState.ts` is cropped for this pass. cc-rust does not yet have a remote skill marketplace/product contract, and adding one would couple MVP-010 to remote/service scope from MVP-014.
- Continuous filesystem watchers for skill directories are cropped. Reload is explicit through startup, `/skills reload`, `/reload-plugins`, and IPC `SkillCommand::Reload`; this preserves deterministic registry rebuilds without adding watcher lifecycle complexity.
- Conditional path activation is parsed and surfaced, but automatic activation based on touched files remains a future UX/runtime integration.

## Verification

- Completed: `cargo test -p cc-skills -- --nocapture`
- Completed: `cargo test -p claude-code-rs commands::skills_cmd -- --nocapture`
- Completed: `cargo test -p claude-code-rs commands::reload_plugins_cmd -- --nocapture`
- Completed: `cargo test -p claude-code-rs test_normalize_mcp_skill_component -- --nocapture`
- Completed: `cargo check -p claude-code-rs --all-targets`
- Completed: `cargo clippy -p claude-code-rs --all-targets -- -D warnings`
- Pending before remote-marketplace claims: remote skill service fixtures and product-scope requirements.

## Close Criteria

MVP-010 is closed for local/package/MCP scope as of 2026-04-29. Reopen only if cc-rust commits to remote skill marketplace state, continuous filesystem watching, or automatic conditional path activation.
