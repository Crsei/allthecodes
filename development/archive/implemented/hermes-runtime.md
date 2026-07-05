# Hermes-like Runtime Implementation Note

> Updated: 2026-07-04

This slice adds a Hermes-like persistent runtime layer around the existing
`QueryEngine`. It does not replace the main agent loop, tool router, or session
format. The shared storage and command/tool/API surfaces are the source of
truth.

## Implemented Scope

- Session warm memory: `allthecodes-session` now exposes bounded
  `session_search` APIs over SQLite with JSON fallback. Search indexes a
  plain-text projection of persisted messages and caps result snippets.
- Human search surfaces: `/session search`, `/session search all`,
  `/session find`, and `/session find all` use the shared storage API.
- Web/API search: `GET /api/sessions/search` and protocol DTOs expose the same
  search results to Web clients.
- Model-visible recall: `SessionSearch` is registered as a normal tool with
  workspace/all scopes and a bounded result limit.
- Curated memory: memory writes use allthecodes paths and approval-oriented
  pending/approve/reject command surfaces. Prompt memory uses a frozen snapshot
  for the session prompt section.
- Learnable skills: `/learn` stages skill proposals instead of activating them
  directly, and `/skills pending|diff|approve|reject` manages proposal approval.
- Background review: after-turn review can emit durable memory/skill proposals
  under the allthecodes data root without mutating memory or skills before
  approval.
- Scheduled runtime entry: scheduled agent tasks are persisted under
  `~/.allthecodes/scheduled_tasks/tasks.json`, surfaced through gateway/source
  metadata, and dispatched by the daemon through the existing engine path.
- Worktree delegation envelope: `DelegateTask` is model-visible, creates a
  child session with parent lineage, records a local-agent task, stores optional
  worktree metadata, and makes the child bootstrap transcript searchable.
- Web task visibility: `/api/tasks` now returns scheduled tasks and default task
  store entries, including local-agent session/worktree metadata where present.

## Path Isolation

All new persistent files use allthecodes locations:

- `~/.allthecodes/sessions/` and the allthecodes SQLite state database for
  session history and search projections.
- `~/.allthecodes/review_proposals/` for background review proposals.
- `~/.allthecodes/skill_proposals/` or project `.allthecodes/skill_proposals/`
  for staged skill proposals.
- `~/.allthecodes/scheduled_tasks/tasks.json` for scheduled agent tasks.
- `~/.allthecodes/worktrees/agent-worktree-*` for delegated worktree metadata.

No new code writes to `.Codex`, `~/.Codex`, or project `.Codex` paths.

## Verification

Targeted verification run in `.worktrees/hermes-runtime-session-search`:

```bash
cargo test -p allthecodes-session search_sessions -- --nocapture
cargo test -p allthecodes-session storage::tests::test_search_workspace_sessions_filters_by_workspace -- --nocapture
cargo test -p allthecodes-commands session_search -- --nocapture
cargo test -p allthecodes-protocol session_search -- --nocapture
cargo test -p allthecodes-web session_search -- --nocapture
cargo test -p allthecodes-web migration_tracker_marks_dispatched_operations -- --nocapture
cargo test -p allthecodes-tools session_search_tool -- --nocapture
cargo test -p allthecodes-tools allthecodes_tools_registry_has_builtin_tools -- --nocapture
cargo test -p allthecodes-session curated -- --nocapture
cargo test -p allthecodes-engine frozen_section -- --nocapture
cargo test -p allthecodes-commands approval_commands -- --nocapture
cargo test -p allthecodes-session memdir -- --nocapture
cargo test -p allthecodes-commands memory -- --nocapture
cargo test -p allthecodes-skills skill_proposal -- --nocapture
cargo test -p allthecodes-commands learn_conversation -- --nocapture
cargo test -p allthecodes-commands test_skills_proposal_commands_approve_and_reject -- --nocapture
cargo test -p allthecodes-engine background_review -- --nocapture
cargo test -p allthecodes-session record_replay::types -- --nocapture
cargo test -p allthecodes-commands skills -- --nocapture
cargo test -p allthecodes-tasks scheduled -- --nocapture
cargo test -p allthecodes-gateway scheduled -- --nocapture
cargo test -p allthecodes-daemon scheduled_task_system_prompt_includes_registry_metadata -- --nocapture
cargo test -p allthecodes-web scheduled_task_maps_to_task_item -- --nocapture
cargo test -p allthecodes-protocol tasks -- --nocapture
cargo test -p allthecodes-tools delegate -- --nocapture
cargo test -p allthecodes-worktree delegate -- --nocapture
cargo test -p allthecodes-web task -- --nocapture
cargo check -p allthecodes-tools
cargo check -p allthecodes-daemon -p allthecodes-web -p allthecodes-protocol
```

## Remaining Boundaries

- Scheduled task `cwd` is currently metadata only. The daemon dispatch path uses
  the existing engine submit API, which has no per-submit cwd override yet.
- `DelegateTask` is a durable delegation envelope. It creates searchable child
  session state and task/worktree metadata, but it does not yet directly spawn a
  child `QueryEngine` runtime with the returned `child_session_id`.
- Web task listing covers scheduled tasks and the default task store. Session
  scoped task stores still require task-list/session-scoped API parameters.
- Additional Hermes-style platform adapters such as Slack, Discord, and Matrix
  are intentionally deferred until there is a product/runtime contract for
  identity, authorization, audit, retry, and session ownership.
