---
doc_type: mvp-compromise-memory
project: cc-rust
last_updated: 2026-04-29
owner: Codex
status: active
---

# MVP Architecture Compromise Memory

## 1. Scope

This document records design shortcuts from the historical `rust-lite` phase that now need production-grade follow-up in the Full Build phase. It is not a replacement for `development/archive/IMPLEMENTATION_GAPS.md`; it classifies those gaps by production risk and records the production design that should replace each shortcut.

Evidence used in this pass:

- `development/archive/IMPLEMENTATION_GAPS.md`
- `docs/WORK_STATUS.md`
- `docs/TECH_DEBT.md`
- `development/archive/KNOWN_ISSUES.md`
- `development/archive/COMPLETED_SIMPLIFIED.md`
- Targeted code checks under `crates/claude-code-rs/src/` and `ui/team-memory-server/`
- MVP-007 implementation pass on `crates/claude-code-rs/src/tools/tool_search.rs`, `crates/claude-code-rs/src/tools/registry.rs`, and `crates/claude-code-rs/src/main.rs`

## 2. Current Context

| Field | Value |
| --- | --- |
| MVP shape | Historical `rust-lite` Rust port, now in Full Build parity mode |
| Out of scope for this pass | Implementing the missing features |
| Target production bar | Align with upstream TypeScript behavior unless an intentional crop is recorded |
| Primary risk | Old "accepted simplification" entries being mistaken for stable product boundaries |

## 3. Compromise Index

| ID | Area | Type | Summary | Severity | Status |
| --- | --- | --- | --- | --- | --- |
| MVP-001 | API providers | temporary-api, happy-path | Bedrock and Vertex variants remain incomplete; selecting them can panic or fail at runtime. | P1 required before launch | open |
| MVP-002 | File writing | happy-path, unvalidated | Write path lacks atomic temp-rename, backup/restore, size limits, permission preservation, and binary checks. | P0 production blocker | open |
| MVP-003 | Task tools | resolved | Task records now persist under the cc-rust data root with restart recovery, bounded output retention, dependencies, and runtime cancellation handles. | P1 required before launch | resolved |
| MVP-004 | Background agents | resolved | Background agents now run under a unified supervisor with worktree setup, permission callback propagation, cancellation, shutdown cleanup, and retained output lookup. | P1 required before launch | resolved |
| MVP-005 | Agent Teams backend | temporary-api, unscaled | Teams use in-process mailbox execution only; tmux/iTerm2 backend remains an interface placeholder. | P2 hardening | open |
| MVP-006 | Plan mode | partial, deterministic-classifier, manual-linking | Plan mode now has a durable workflow record, approval state, IPC/daemon sync, classifier entry, and trace events; remaining gaps are full auto-mode classifier parity, unified team approval flow, and automatic implementation evidence tracking. | P1 required before launch | partial |
| MVP-007 | Tool search | resolved former unscaled | ToolSearch now uses a deterministic retrieval index with query normalization, stable ranking, lazy schema hydration, and skill/plugin/MCP source coverage. | P2 hardening | resolved |
| MVP-008 | LSP | partial, transport-reader | LSP now has live document sync, cached diagnostics, completion, and plugin/settings config discovery; diagnostics are still drained opportunistically instead of by a dedicated background reader. | P2 hardening | partial |
| MVP-009 | WebFetch | partial, browser-observability | WebFetch now has explicit HTTP/browser modes, redirect limits, content-type dispatch, private-network policy, cookie/proxy gates, and headless browser rendering; browser redirect/JS navigation observability still needs hardening. | P1 required before launch | partial |
| MVP-010 | Skill system | resolved local package scope, intentional remote crop | Skill loading now has package metadata, dependency/version validation, reload invalidation, diagnostics, and MCP `skill://` resource ingestion; remote skill marketplace/state loading and continuous file watching are intentional crops. | P2 hardening | resolved |
| MVP-011 | OpenTUI frontend | UX, happy-path | Resize and narrow-terminal behavior still has open layout and repaint issues. | P2 hardening | open |
| MVP-012 | IPC protocol | temporary-api | IPC uses unversioned JSON payload surfaces, making frontend/backend skew hard to detect. | P1 required before launch | open |
| MVP-013 | Configuration/model metadata | hardcoded | Model aliases and metadata are duplicated across backend and UI surfaces instead of one canonical registry. | P2 hardening | open |
| MVP-014 | Remote/service/analytics surface | deferred, temporary-api | Remote control, server transports, MDM, analytics, desktop/mobile, and internal Ant-only tools need Full Build re-evaluation. | P2 hardening | open |
| MVP-015 | Team Memory sync docs | resolved, doc-stale | Docs still say client sync is pending, but code now contains TS sync/watcher and Rust spawn wiring. | P3 polish | doc reconciliation needed |

## 4. Detailed Entries

### MVP-001 API providers still incomplete

| Field | Value |
| --- | --- |
| Category | API, configuration |
| Type | temporary-api, happy-path |
| Severity | P1 required before launch |
| Evidence | `docs/WORK_STATUS.md` §1.1, `development/archive/IMPLEMENTATION_GAPS.md` §1 |
| Code paths | `crates/claude-code-rs/src/api/`, `crates/claude-code-rs/src/config/` |

Current MVP behavior:

- Anthropic, OpenAI-compatible, Google Gemini, and Azure paths are available.
- Bedrock and Vertex are still listed as not implemented in status docs.

Production design:

- Implement provider-specific request signing, config validation, auth resolution, streaming conversion, retries, and error mapping.
- Add provider-specific integration tests and config-schema coverage.

Revisit trigger:

- Any user-facing claim of provider parity, enterprise deployment, or use of AWS/GCP-managed model endpoints.

Verification before production:

- Provider smoke tests against mocked and real endpoints.
- Error-path tests for auth, region/project mismatch, throttling, and malformed provider responses.

### MVP-002 FileWrite lacks durable safe-write semantics

| Field | Value |
| --- | --- |
| Category | Data and persistence |
| Type | happy-path, unvalidated |
| Severity | P0 production blocker |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §1.6 |
| Code path | `crates/claude-code-rs/src/tools/file_write.rs` |

Current MVP behavior:

- The tool performs path validation, creates parent directories, and writes content.
- The documented missing behavior includes atomic temp-file rename, backup/restore, file-size limits, permission preservation, and binary content checks.

Production design:

- Write to a temp sibling, fsync where appropriate, then rename atomically.
- Preserve mode/ownership where supported, reject unsafe binary writes unless explicitly allowed, and keep a rollback copy for destructive overwrites.

Revisit trigger:

- Any workflow where FileWrite edits user projects, credentials, generated artifacts, or files outside disposable test fixtures.

Verification before production:

- Crash/interruption tests around temp writes.
- Regression tests for permissions, binary detection, oversized files, and rollback.

### MVP-003 TaskTools persistence resolved

| Field | Value |
| --- | --- |
| Category | Data and persistence |
| Type | resolved former in-memory |
| Severity | P1 required before launch |
| Status | Resolved 2026-04-27 |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.2 |
| Code paths | `crates/claude-code-rs/src/tools/tasks.rs`, `crates/claude-code-rs/src/commands/tasks_cmd.rs`, `crates/claude-code-rs/src/engine/agent/tool_impl.rs` |

Previous MVP behavior:

- `TaskStore` is explicitly documented in code as a shared in-memory store backed by `Arc<Mutex<HashMap>>`.
- Process restart loses task state; there is no dependency graph, durable task log, timeout controller, or independent task UI.

Implemented design:

- Task records persist under `{data_root}/tasks/` using a versioned JSON envelope.
- Retained output is stored in a bounded sidecar log, with summary metadata in the task record.
- `pending`, `in_progress`, and `recoverable` tasks reload as `interrupted` when no live process supervisor exists.
- Task records include kind, parent, dependency, cancellation, recovery, and previous-status fields.
- Background Agent tasks now create `local_agent` task records, register runtime cancellation tokens, append retained output, and clear handles on completion.

Revisit trigger:

- Long-running tasks, remote/background task kinds beyond local agents, or UI flows that need richer replay than retained task output.

Verification before production:

- Completed 2026-04-27: repository create/update/list/get/delete, schema migration, restart recovery, bounded output, dependency, runtime cancellation, concurrent output append, `/tasks` command, Agent integration, `cargo check --all-targets`, `cargo clippy --all-targets -D warnings`, and release build.
- Still useful before UI release: OpenTUI component-level replay test against persisted task records.

### MVP-004 Background agents lifecycle resolved

| Field | Value |
| --- | --- |
| Category | Concurrency, permissions |
| Type | resolved former happy-path |
| Severity | P1 required before launch |
| Status | Resolved 2026-04-28 |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §3, `development/archive/resolved-known-issues-2026-05-07.md` #6-#8 |
| Code paths | `crates/claude-code-rs/src/engine/agent/supervisor.rs`, `crates/claude-code-rs/src/engine/agent/tool_impl.rs`, `crates/claude-code-rs/src/tools/tasks.rs`, `crates/claude-code-rs/src/ipc/agent_handlers.rs`, `crates/claude-code-rs/src/ipc/runtime.rs`, `crates/claude-code-rs/src/shutdown.rs` |

Previous MVP behavior:

- `run_in_background` uses `tokio::spawn`.
- If `run_in_background` and `isolation: "worktree"` are both requested, code logs a warning and runs in the normal cwd.
- The spawned child engine is built without the same frontend permission callback surface as the main IPC runtime.
- The background spawn path does not retain a central `JoinHandle` for user abort or shutdown cancellation.

Implemented design:

- Background `Agent` runs are launched through `engine::agent::supervisor`, which owns task registration, cancellation token, join handle, lifecycle completion, and shutdown cleanup.
- Worktree isolation is prepared before spawn; clean worktrees are removed on completion, changed worktrees are retained and reported in task output.
- Child `QueryEngine` instances inherit permission and AskUserQuestion callbacks, hook runner, command dispatcher, and background event channel.
- `TaskStore` persists `agent_id`, `supervisor_id`, isolation mode, worktree path, and branch so `/tasks`, `TaskOutput`, IPC abort, and output lookup can correlate the job.
- IPC `AbortAgent` requests now cancel the supervised runtime instead of only updating UI state; `QueryAgentOutput` reads retained task output.

Revisit trigger:

- Detached-process or tmux-backed background engines, restart/resume of still-running external jobs, or richer UI replay than retained task output.

Verification before production:

- Completed 2026-04-28: supervisor unit tests, task metadata persistence tests, `cargo check -p claude-code-rs --all-targets`, targeted task/supervisor tests, and `cargo clippy -p claude-code-rs --all-targets -- -D warnings`.
- Still useful before external-backend launch: end-to-end IPC test for permission Ask from a background child and worktree cleanup under forced process termination.

### MVP-005 Agent Teams support only the in-process backend

| Field | Value |
| --- | --- |
| Category | Architecture and boundaries |
| Type | temporary-api, unscaled |
| Severity | P2 hardening |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §1.1, `docs/WORK_STATUS.md` §1.3 |
| Code path | `crates/claude-code-rs/src/teams/mod.rs` |

Current MVP behavior:

- Teams run as same-process tokio tasks with mailbox IPC.
- `PaneBackend` remains as a placeholder interface for the full edition; tmux/iTerm2 backends are not implemented.

Production design:

- Decide whether cc-rust will provide terminal-pane backends or explicitly crop them in `IMPLEMENTATION_GAPS.md` §7.
- If supported, add backend abstraction tests for lifecycle, pane identity, output routing, and cleanup.

Revisit trigger:

- User-facing promise of upstream Agent Teams parity or requests for visible terminal panes.

Verification before production:

- Backend conformance tests, crash cleanup tests, and cross-platform terminal integration tests.

### MVP-006 Plan mode workflow partially implemented

| Field | Value |
| --- | --- |
| Category | Architecture and workflow |
| Type | partial, deterministic-classifier, manual-linking |
| Severity | P1 required before launch |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.4, `docs/mvp-optimization-plans/MVP-006-plan-mode-workflow-plan.md` |
| Status | Partial implementation 2026-04-28 |
| Code paths | `crates/cc-types/src/plan_workflow.rs`, `crates/claude-code-rs/src/plan_workflow.rs`, `crates/claude-code-rs/src/commands/plan.rs`, `crates/claude-code-rs/src/tools/plan_mode.rs`, `crates/claude-code-rs/src/ipc/`, `crates/claude-code-rs/src/daemon/routes.rs`, `ui/src/ipc/protocol.ts`, `ui/src/store/` |

Previous MVP behavior:

- Plan mode can enter/restore permission mode and read or open a plan file.
- Code contains a TODO noting daemon ingress does not fully sync all mode transitions.
- Missing documented parity includes auto-mode/classifier gate, team approval, plan persistence semantics, and implementation tracking.

Implemented behavior:

- Plan state is represented by a versioned `PlanWorkflowRecord` with status, approval state, owner, plan text, linked task ids, and trace events.
- `/plan` now supports enter/show, status, trace, approve, reject, link, and classifier-preview commands.
- `EnterPlanMode` and `ExitPlanMode` tools persist workflow state; `ExitPlanMode` records pending approval before asking and records approval before restoring the prior permission mode.
- Headless IPC emits `Ready.permission_mode`, `Ready.plan_workflow`, and `plan_workflow_event`; slash-command and tool-result paths sync workflow state back to `QueryEngine`.
- Daemon submit and command routes can enter/sync plan workflow state and broadcast `plan_workflow_event`.
- OpenTUI protocol/store/status-line surfaces can receive and display the active plan workflow.

Remaining MVP compromises:

- Classifier entry is deterministic and explicit-phrase based; it is not the full upstream auto-mode or LLM classifier.
- Implementation tracking is manual via `/plan link <task-id> [summary]`; commits, tests, and verification evidence are not linked automatically.
- Team plan approval messages are not unified with the durable plan workflow yet.
- Daemon `/api/command` syncs slash-command state but still does not run `CommandResult::Query` as a full query turn.

Production design:

- Treat plan state as a durable workflow object, not only a mode flag and markdown file.
- Sync plan-mode transitions through TUI, headless IPC, and daemon paths.
- Add links between approved plan items, implementation commits, and verification evidence.

Revisit trigger:

- Multi-agent planning, headless operation, or any workflow that depends on plan approval gates.

Verification before production:

- Completed 2026-04-28: workflow record unit tests, `/plan` command tests, Enter/ExitPlanMode tool tests, IPC/e2e plan command coverage, `cargo check -p claude-code-rs --all-targets`, and OpenTUI `bun.cmd run build`.
- Still needed before closing: full auto-mode classifier parity tests, team approval integration tests, daemon query-command execution coverage, and automatic implementation evidence trace tests.

### MVP-007 Tool search ranking resolved

| Field | Value |
| --- | --- |
| Category | Search and performance |
| Type | resolved former unscaled |
| Severity | P2 hardening |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.3 |
| Code path | `crates/claude-code-rs/src/tools/tool_search.rs` |
| Status | Resolved 2026-04-29 |

Previous MVP behavior:

- Exact `select:` and keyword fuzzy search existed only as the intended lightweight surface.
- TF-IDF ranking, full-text indexing, and deferred schema loading remained missing.

Implemented design:

- `ToolSearchTool` is registered as a real read-only tool and searches built-in tools, plugin tools, MCP tools, and model-invocable skills.
- The search index normalizes casing, punctuation, camel-case names, action aliases, and plural forms before ranking.
- Ranking combines exact/prefix/alias matches with weighted TF-IDF-style field scoring over names, aliases, categories, tags, and descriptions.
- Result ordering is deterministic by score, source priority, display name, and registration ordinal.
- Input schemas are loaded lazily only for `select:` lookups, `include_schema=true`, or schema-shaped queries where schema terms are needed for ranking.
- Startup installs the fully merged runtime catalog after plugins, skills, MCP, and Computer Use tools are discovered; search also merges the current registry snapshot on each call.

Revisit trigger:

- UI or API behavior changes to use Anthropic `tool_reference` blocks instead of text/JSON search results.
- Large plugin/tool catalogs, MCP-heavy sessions, or degraded tool selection quality.

Verification before production:

- Completed 2026-04-29: ranking regression fixtures for exact/name/text search, disabled tools, plugin and MCP source filters, model-invocable skill results, `select:` schema hydration, schema-lazy ranking, and a 600-entry large-catalog lazy-schema regression.
- Completed 2026-04-29: `cargo test -p claude-code-rs tools::tool_search -- --nocapture`.

### MVP-008 LSP live-editor path partially implemented

| Field | Value |
| --- | --- |
| Category | Developer tooling |
| Type | partial, transport-reader |
| Severity | P2 hardening |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.5 |
| Status | Partial implementation 2026-04-29 |
| Code paths | `crates/claude-code-rs/src/tools/lsp.rs`, `crates/claude-code-rs/src/lsp_service/`, `crates/claude-code-rs/src/ipc/`, `ui/src/ipc/protocol.ts`, `ui/src/store/` |

Previous MVP behavior:

- Core JSON-RPC transport and nine LSP operations exist.
- Incremental sync, passive diagnostics, completion suggestions, and plugin-side LSP config integration are still missing.

Implemented behavior:

- LSP clients now track open documents with versions and send `didOpen`, `didChange`, `didSave`, and `didClose`.
- Incremental range edits are applied to cached text using UTF-16 LSP positions before being sent to the server.
- `publishDiagnostics` notifications are parsed, cached per URI, emitted over IPC, and exposed through the LSP tool.
- `textDocument/completion` responses are parsed and exposed through both tool calls and IPC completion-result events.
- LSP server resolution now reads plugin `.lsp.json`, plugin manifest `lspServers`, settings `lspServers`, then built-in defaults.
- OpenTUI protocol/store paths can send document sync/completion commands and cache diagnostics/completion results.

Remaining MVP compromises:

- Passive diagnostics are drained opportunistically after sync and request operations; a production transport should split reader/writer tasks and continuously route notifications, responses, and server requests.
- Verification uses parser/protocol/config/reducer tests and build checks, not real language-server fixture tests across Rust, TypeScript, Python, Go, C/C++, and Java.

Production design:

- Maintain open document state and send `didChange` increments.
- Subscribe to server diagnostics and expose them to UI/tool callers.
- Centralize language server config across plugins and settings.
- Move JSON-RPC reads to a background router so diagnostics do not depend on the next editor command or tool request.

Revisit trigger:

- Editing workflows that depend on live diagnostics, completions, or plugin-provided LSP configuration.

Verification before production:

- Language-server fixture tests for Rust, TS, Python, Go, C/C++, and Java.
- Incremental edit tests and diagnostic push tests.

### MVP-009 WebFetch browser-grade behavior partially implemented

| Field | Value |
| --- | --- |
| Category | Network and external integration |
| Type | partial, browser-observability |
| Severity | P1 required before launch |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.6 |
| Code path | `crates/claude-code-rs/src/tools/web_fetch.rs` |
| Status | Partial implementation 2026-04-29 |

Current MVP behavior:

- `mode: "http"` uses a dedicated HTTP adapter with manual redirect budget, sandbox/domain checks on each hop, private-network rejection by default, max-byte enforcement, safe-cache gating, content-type dispatch, JSON pretty text, HTML extraction, text handling, and binary rejection.
- Cookie handling is opt-in through `allowCookies`; the HTTP adapter keeps a scoped in-process jar and disables cache for cookie-enabled fetches.
- Proxy handling is opt-in through explicit `proxy` input or `sandbox.network.httpProxyPort` / `socksProxyPort`; ambient proxy environment variables are ignored for WebFetch.
- `mode: "browser"` is explicit and uses a local Chromium-family binary in headless `--dump-dom` mode with an isolated temporary profile by default, or a cc-rust profile when cookies are explicitly enabled.
- Risk-sensitive options (`mode: "browser"`, cookies, proxy, private-network access, and cleartext HTTP) force an additional WebFetch permission prompt.

Production design:

- Keep HTTP and browser adapters separate so rendered output is never silently substituted by simple HTTP output.
- Harden browser mode with DevTools-level navigation telemetry or browser-MCP integration so server redirects, JS client-side navigations, final URL, status code, console/network errors, and blocked private-network attempts are observable.
- Add a real browser-rendering smoke fixture in CI where a Chromium binary is available.

Remaining MVP compromises:

- Browser mode relies on Chromium `--dump-dom`; it can render JavaScript but does not expose HTTP status, final URL after redirects, or JS/client-side navigation chain.
- Browser mode preflights the initial URL against sandbox/private-network policy, but it cannot yet intercept every subresource request or script-initiated navigation.
- SOCKS proxy support is passed to reqwest/Chromium when configured, but it does not yet have a dedicated fixture in this pass.

Revisit trigger:

- Fetching modern JS-heavy apps, authenticated pages, proxied enterprise networks, or untrusted URLs.

Verification before production:

- Completed 2026-04-29: unit/local TCP fixtures for redirects, max redirects, HTML extraction, JSON, binary rejection, HTTP cookie jar, private-network rejection, cache behavior, browser argument construction, `cargo test -p claude-code-rs tools::web_fetch -- --nocapture`, `cargo check -p claude-code-rs --all-targets`, `cargo clippy -p claude-code-rs --all-targets -- -D warnings`, related system-prompt e2e, and release build.
- Still needed before closing: browser-rendering smoke fixture with real Chromium, proxy fixture coverage, and DevTools/browser-MCP telemetry coverage for browser redirects, status, final URL, and JS navigation.

### MVP-010 Skill system package management resolved locally

| Field | Value |
| --- | --- |
| Category | Extensibility |
| Type | resolved local package scope, intentional remote crop |
| Severity | P2 hardening |
| Status | Resolved 2026-04-29 |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §2, `development/archive/COMPLETED_SIMPLIFIED.md` §2.1 |
| Code paths | `crates/cc-skills/src/`, `crates/claude-code-rs/src/tools/skill.rs`, `crates/claude-code-rs/src/commands/skills_cmd.rs`, `crates/claude-code-rs/src/mcp/tools.rs`, `crates/claude-code-rs/src/ipc/subsystem_handlers.rs` |

Previous MVP behavior:

- Skill lookup, argument replacement, inline context injection, and fallback context behavior exist.
- Missing behavior includes dependency resolution, hot reload, version management, full frontmatter validation, and MCP skill builder parity.

Implemented behavior:

- `cc-skills` now models skills as packages with version, dependency, compatible app version, assets, entry docs, path filters, and source metadata.
- Loader diagnostics report malformed frontmatter, unknown keys, invalid package paths, missing package assets/docs, incompatible app versions, missing dependencies, dependency version mismatches, duplicate skills, version conflicts, and dependency cycles.
- Startup loads bundled, user, project, and plugin skill candidates through one resolver with deterministic dependency ordering and a registry revision.
- `/skills reload`, IPC `SkillCommand::Reload`, and `/reload-plugins` rebuild the skill registry and expose warning/error counts.
- Bundled skills carry version metadata before insertion.
- MCP `skill://` resources from connected MCP servers are read and converted through the same skill loader.
- `/skills` detail output now surfaces canonical name, version, dependencies, compatibility, path filters, assets, entry docs, and base directory.

Intentional crops:

- Remote skill search/state loading from the Bun `remoteSkillLoader.ts` and `remoteSkillState.ts` surfaces is not implemented in MVP-010. This is intentionally deferred to MVP-014 remote/service product scope.
- Continuous filesystem watchers are not implemented. Explicit reload remains the supported invalidation mechanism for this pass.
- `paths` frontmatter is parsed and visible, but automatic conditional activation based on touched files is not yet wired into tool/file-operation flows.

Production design:

- Treat skills as versioned packages with dependency metadata, compatibility checks, and reload invalidation.
- Validate frontmatter with a schema and produce actionable diagnostics.

Revisit trigger:

- Product requirements for remote skill marketplaces, live file-watch reload, or automatic conditional path activation.

Verification before production:

- Completed 2026-04-29: `cargo test -p cc-skills -- --nocapture`, `cargo test -p claude-code-rs commands::skills_cmd -- --nocapture`, `cargo test -p claude-code-rs commands::reload_plugins_cmd -- --nocapture`, `cargo test -p claude-code-rs test_normalize_mcp_skill_component -- --nocapture`, `cargo check -p claude-code-rs --all-targets`, and `cargo clippy -p claude-code-rs --all-targets -- -D warnings`.
- Still useful before remote-skill launch: remote service fixtures, marketplace state sync tests, file-watch lifecycle tests, and conditional path activation tests.

### MVP-011 OpenTUI frontend still has layout/repaint gaps

| Field | Value |
| --- | --- |
| Category | UX and product completeness |
| Type | happy-path |
| Severity | P2 hardening |
| Evidence | `development/archive/KNOWN_ISSUES.md` `UI-001`, `development/archive/resolved-known-issues-2026-05-07.md` welcome-screen resolved entries |
| Code paths | `ui/src/components/App.tsx`, `ui/src/components/resize-sync.ts` |

Current MVP behavior:

- Rust TUI resize is marked fixed, but TS/OpenTUI resize/reflow remains open.
- Narrow welcome layout and maximize/restore artifacts still have open issues.

Production design:

- Centralize terminal dimension state, clear/repaint alternate-screen transitions, and test narrow-width layout rules.

Revisit trigger:

- Defaulting users to OpenTUI on Windows or shipping a public terminal UI release.

Verification before production:

- PTY screenshot tests across narrow, normal, resize, maximize, and first-prompt transitions.

### MVP-012 IPC protocol lacks explicit version strategy

| Field | Value |
| --- | --- |
| Category | API and protocol boundaries |
| Type | temporary-api |
| Severity | P1 required before launch |
| Evidence | `docs/TECH_DEBT.md` §18, `development/archive/IMPLEMENTATION_GAPS.md` §6 |
| Code paths | `crates/claude-code-rs/src/ipc/protocol/`, `ui/src/ipc/protocol.ts` |

Current MVP behavior:

- Multiple IPC surfaces use free-form `serde_json::Value`.
- No explicit protocol version field is documented as the compatibility gate.

Production design:

- Add protocol version negotiation or at least a versioned envelope.
- Keep typed compatibility tests between Rust protocol structs and TS protocol types.

Revisit trigger:

- Any independently updated frontend/backend release or plugin-driven IPC extension.

Verification before production:

- Schema compatibility tests, downgrade/upgrade tests, and unknown-field behavior tests.

### MVP-013 Model/config metadata is duplicated and hardcoded

| Field | Value |
| --- | --- |
| Category | Configuration and deployment |
| Type | hardcoded |
| Severity | P2 hardening |
| Evidence | `docs/TECH_DEBT.md` §13 |
| Code paths | `crates/claude-code-rs/src/engine/agent/mod.rs`, `crates/claude-code-rs/src/commands/model.rs`, `crates/cc-config/src/constants.rs`, `ui/src/components/agent-settings/constants.ts` |

Current MVP behavior:

- Model aliases and model metadata are repeated in several backend and frontend files.

Production design:

- Create a single model registry with canonical id, aliases, marketing name, availability, capabilities, and knowledge cutoff.
- Generate or share the UI view from the same source.

Revisit trigger:

- Adding or renaming any model, provider, or model capability flag.

Verification before production:

- Registry snapshot tests and UI/backend consistency tests.

### MVP-014 Remote, service, analytics, and multi-end surfaces need re-evaluation

| Field | Value |
| --- | --- |
| Category | Product architecture |
| Type | deferred, temporary-api |
| Severity | P2 hardening |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §5, `docs/WORK_STATUS.md` §3 |
| Code/doc paths | `bridge/`, `server/`, `remote/`, `services/remoteManagedSettings/`, `services/analytics/` references |

Current MVP behavior:

- Historical Deferred lists include remote control, server transports, MCP server mode, MDM, analytics, desktop/mobile integration, and Ant-only tools.
- Full Build rules say these are no longer automatically "won't implement"; each must be either implemented or moved to Intentional Crop with rationale.

Production design:

- Split the list into product-required, upstream-parity-required, and intentionally-cropped categories.
- Record every intentional crop in `development/archive/IMPLEMENTATION_GAPS.md` §7 with decision owner, date, and revisit trigger.

Revisit trigger:

- Roadmap work involving remote operation, enterprise management, analytics, desktop/mobile surfaces, or upstream parity claims.

Verification before production:

- Product decision record plus implementation tests for every retained surface.

### MVP-015 Team Memory sync appears implemented but docs are stale

| Field | Value |
| --- | --- |
| Category | Documentation and architecture memory |
| Type | resolved, doc-stale |
| Severity | P3 polish |
| Evidence | `development/archive/IMPLEMENTATION_GAPS.md` §1, `docs/superpowers/plans/2026-04-11-team-memory-sync.md` |
| Code paths | `ui/team-memory-server/sync.ts`, `ui/team-memory-server/watcher.ts`, `ui/team-memory-server/index.ts`, `crates/claude-code-rs/src/daemon/team_memory_proxy.rs` |

Current observed behavior:

- Documentation says Team Memory client sync is pending.
- Code now includes pull/push delta logic, fs watcher, `--repo`, `--team-mem-path`, and Rust subprocess spawn wiring.

Production design:

- Reconcile docs with code: either mark Team Memory sync complete with verification evidence, or document the exact remaining gaps.

Revisit trigger:

- Any Team Memory work, storage migration, or release notes touching memory sync.

Verification before closing:

- Run the Team Memory server build, Rust daemon spawn path test, API PUT/GET sync test, local-file watcher push test, and restart pull test.

## 5. Production Blockers

| Severity | Needed design | Compromise IDs | Trigger |
| --- | --- | --- | --- |
| P0 | Atomic and recoverable file writes | MVP-002 | Before real project files are modified by FileWrite in production workflows |
| P1 | Stable provider and protocol compatibility | MVP-001, MVP-012 | Before provider parity or separate frontend/backend releases |
| P1 | Browser-grade fetch behavior for modern web targets | MVP-009 | Before web fetch is advertised beyond simple unauthenticated HTTP pages |

## 6. Accepted For Now

| Risk | Why acceptable now | Revisit condition |
| --- | --- | --- |
| In-process Agent Teams backend only | Current code provides a usable closed-loop team model without terminal-pane complexity. | Users need visible tmux/iTerm panes or upstream parity becomes required. |
| OpenTUI layout gaps | Known issues are documented and not data-loss risks. | OpenTUI becomes the default production UI on all platforms. |

## 7. Resolved Or Possibly Resolved Shortcuts

| ID | Resolution | Date observed | Verification still needed |
| --- | --- | --- | --- |
| MVP-003 | Task tool state now uses a versioned disk repository with bounded output sidecars, restart interruption recovery, dependency fields, `/tasks` delete/cancel/detail support, and runtime cancellation token registration for background local agents. | 2026-04-27 | OpenTUI component replay test before UI release. |
| MVP-004 | Background local agents now run under a supervisor with callback parity, worktree pre-spawn setup, cancellation handles, shutdown cleanup, task metadata correlation, and retained output lookup. | 2026-04-28 | End-to-end IPC permission Ask and forced-process-termination cleanup tests before external backend launch. |
| MVP-007 | ToolSearch now has a deterministic retrieval index with query normalization, stable ranking, lazy schema hydration, source filters, and skill result invocation metadata. | 2026-04-29 | UI smoke coverage for result labels and explanations before relying on ToolSearch from the OpenTUI selector. |
| MVP-010 | Skill packages now have schema metadata, validation diagnostics, dependency/version resolution, explicit reload invalidation, bundled version metadata, plugin reload integration, and MCP `skill://` ingestion. | 2026-04-29 | Remote marketplace state sync, continuous file-watch reload, and conditional path activation only if those product scopes are accepted. |
| MVP-015 | Team Memory sync code exists despite stale docs. | 2026-04-26 | Build and end-to-end sync test before removing from gap docs. |

## 8. Open Design Questions

| Question | Impact | Owner or milestone |
| --- | --- | --- |
| Which historical Deferred remote/server/analytics items are product requirements versus intentional crops? | Prevents Full Build parity scope from staying ambiguous. | Full Build roadmap review |
| Should cc-rust support tmux/iTerm2 team panes or formally crop them? | Determines Agent Teams backend architecture. | Agent Teams parity review |
| Should model metadata be generated from config or maintained as a Rust-first registry exported to UI? | Determines how to avoid stale hardcoded aliases. | Config/model registry refactor |
| What compatibility promise should IPC make between Rust backend, OpenTUI frontend, Web UI, and SDK clients? | Determines protocol versioning and schema tests. | IPC refactor milestone |
