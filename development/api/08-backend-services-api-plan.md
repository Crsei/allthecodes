# Backend Services API Plan

> Status: Partial on 2026-07-16
> Current result: session reconciliation and agent/task projections use
> canonical owners; compression, durable retry, aggregate migrations, and
> consistent backups still lack canonical action services and fail closed.

## Implementation Result (2026-07-16)

Commit `af35d541` replaced synthetic success with truthful typed projections and
failure semantics; its contracts are included in `e5163791`:

- dashboard/session sync queries canonical persisted sessions plus the current
  Web runtime inventory, supports dry-run, and reports partial inventory
  coverage explicitly;
- agent and event rows project `agent_runtime_history` and canonical
  `TaskStore` state;
- a valid compression target returns `503 compaction_service_unavailable`
  while an unknown target remains `404`;
- a known agent event without a durable replay descriptor returns `409`, while
  an unknown event remains `404`; and
- migrations and backups return typed `503` responses because no aggregate
  registry or consistent snapshot service exists, and no Web-local marker or
  backup envelope is created.

This is intentionally Partial. The dashboard can report canonical session,
task, and runtime-history evidence, but it does not yet provide real compaction
lifecycle, replayable agent requests, aggregate database migration coverage, or
consistent backups. The Backend Services focused suite passes 6/6; those
unavailable owners remain the unmet acceptance boundary below.

## Scope

Keep the current dashboard/action routes and replace placeholder behavior with
truthful projections and real service operations:

```http
GET  /api/backend-services
POST /api/backend-services/sessions/sync
POST /api/backend-services/context-compression/:id/run
POST /api/backend-services/agent-bridge/events/:id/retry
POST /api/backend-services/migrations/run
POST /api/backend-services/backups
```

No additional backend-services namespace is required. Each section must adapt
an existing canonical owner or remain explicitly unavailable until such an
owner exists.

## Audit Snapshot Before Implementation

The routes are registered and `capabilities.backend_services` is `true`.
`crates/allthecodes-web/src/handlers/backend_services.rs` returns the expected
top-level shape, but its behavior is not a real service dashboard:

| Surface | Current behavior | Gap |
|---|---|---|
| `GET /api/backend-services` | Session sync, compression, and agent bridge are always `unknown` with empty arrays. | Existing sessions, tasks, and runtime events are not queried. |
| `POST .../sessions/sync` | Returns `ok=true` with a message saying no runtime inventory is available. | Successful no-op; no comparison or mutation occurs. |
| `POST .../context-compression/:id/run` | Returns `404` for every id. | No job lookup or compaction invocation. |
| `POST .../agent-bridge/events/:id/retry` | Returns `404` for every id. | No event lookup, retryability check, or retry. |
| `POST .../migrations/run` | Updates a Web-local `schema-version.json`. | Does not describe migrations of the real service databases. |
| `POST .../backups` | Writes a JSON envelope under `web/backups`. | Does not snapshot actual service state. |
| Database status | Mirrors the Web-local schema marker and returns no tables. | Can report `ready` without checking real databases. |

The placeholder shape was useful to remove route fallback, but it must not be
treated as completed backend integration.

## Canonical Data Sources

Build each projection from the subsystem that already owns the state:

| Dashboard section | Required source |
|---|---|
| Database | `allthecodes-db` health/migration results plus the concrete stores used by sessions, scheduler, and runtime history. |
| Session sync | `allthecodes_session::storage::list_sessions` and the Web engine/runtime session inventory. |
| Context compression | A shared engine/compaction service with canonical task records and persisted compact boundaries. |
| Agent bridge | `allthecodes_services::agent_runtime_history` plus canonical `TaskStore` lifecycle/output state. |
| Local migrations | The migration registries of the actual stores being upgraded. |
| Backups | A consistent snapshot service over the real files/databases in scope. |

The handler must not scrape private JSON/SQLite formats when a service API is
available. Add small query/action interfaces to the owning crates and inject
them into Web state.

Define typed request/response DTOs in `allthecodes-protocol`; the current
`Value` route metadata cannot enforce compatibility or generate useful client
types.

## Truthful Status Semantics

Every subsection should distinguish:

- `ready`: its canonical source was queried successfully and is healthy;
- `degraded`: data is available but reconciliation, migration, or failures are
  pending;
- `unavailable`: the owner is disabled or not connected;
- `error`: the owner was contacted and the query failed.

Do not use `unknown` plus an empty array when known state exists. Include a
bounded warning/error code and `observed_at`. A dashboard request may return
`200` with degraded subsections, but it must not replace a source error with a
healthy empty result.

`ServiceActionResponse.ok` means the requested mutation was performed or was
accepted as a real asynchronous task. A no-op caused by missing integration
must return a typed `409`/`503`, not `ok=true`.

## Session Sync

`POST /sessions/sync` should run an idempotent reconciliation:

1. Snapshot persisted session metadata and the current runtime inventory.
2. Match by canonical session id and workspace/profile ownership.
3. Create or refresh only defined metadata projections; never rewrite
   transcripts from dashboard state.
4. Detect runtime-only, persisted-only, stale, and conflicting records.
5. Return examined/created/updated/conflict/skipped counts plus bounded item
   results and a reconciliation id.

Support dry-run before mutation. If a complete runtime inventory is not
available, reconcile the current engine session and report the coverage as
partial; do not claim a full sync.

## Context Compression Action

`POST /context-compression/:id/run` must first resolve `id` to a real
compression target or existing job. It should then:

- validate session/room ownership and token budget;
- enqueue the canonical compaction operation as a task;
- return `202` with task/job id and initial status;
- persist input/output token counts, compact-boundary or summary reference,
  timestamps, and terminal error;
- surface later state through the dashboard projection.

Do not simulate compression, return a successful local excerpt, or mutate a
live session concurrently with an active turn. If no shared compaction service
exists, return `503 unavailable` for a valid target and keep the endpoint
explicitly incomplete.

## Agent Bridge Retry

Populate agents/events from runtime history and task lifecycle rather than an
empty synthetic list. An event is retryable only when a durable retry descriptor
exists and policy allows replay.

`POST /agent-bridge/events/:id/retry` should:

1. Load the real event by id.
2. Reject unknown ids with `404` and known non-retryable/terminal events with
   `409`.
3. Revalidate permission, taint, workspace, model, and idempotency constraints.
4. Enqueue through the canonical agent/task runtime.
5. Link the new attempt to the original event and return its task/run id.

Never reconstruct an executable request from a display summary, and never
retry solely because the client sends `retryable=true`.

## Migrations and Backups

Preserve the existing routes, but make their scope explicit:

- Migration status/action must enumerate and invoke real registered migrations
  for the selected local stores. A Web-only schema marker is not global backend
  migration evidence.
- `dry_run` returns the exact pending steps without mutation.
- Backup must snapshot the selected real stores consistently. For SQLite, use a
  safe online-backup/checkpoint strategy; for files, use atomic copies with a
  manifest and checksums.
- The backup response identifies included/excluded stores, consistency status,
  byte sizes, and restore compatibility. A metadata envelope alone is not a
  backend backup.

Until those semantics exist, label current migration/backup rows as
`web_local_state` and do not present them as coverage of sessions, scheduler,
or agent history.

## Safety and Concurrency

- Require privileged mutation capability for sync, compression, retry,
  migration, and backup actions.
- Serialize mutations by subsystem/target and attach idempotency keys to
  asynchronous actions.
- Redact credentials, raw prompts, tool inputs, absolute paths, and unbounded
  output from dashboard items.
- Bound list size and support cursor pagination for sessions, jobs, agents,
  events, migrations, and backups.
- Never hold a Web handler lock while waiting for model, daemon, or backup I/O;
  return a task handle for long-running work.

## Tests

Add coverage for:

- A non-empty session fixture produces real sync rows and mutation counts.
- Dry-run session sync reports changes without writing them.
- Missing runtime coverage is `unavailable`/partial, not successful full sync.
- A real compression target creates a trackable job and reaches terminal state.
- Unknown and non-retryable agent events return distinct `404`/`409` errors.
- Retrying a stored retryable event creates one linked attempt and respects
  idempotency.
- Dashboard agent/event rows match runtime-history fixtures.
- A failing canonical source produces a degraded/error subsection, not an empty
  `ready` result.
- Migration dry run and apply use real registries.
- Backup manifest contains actual selected stores and passes checksum/restore
  validation.
- Protocol metadata, handlers, schema, OpenAPI, and generated clients agree.

Run targeted verification:

```bash
cargo test -p allthecodes-session
cargo test -p allthecodes-services agent_runtime_history
cargo test -p allthecodes-web backend_services
```

## Acceptance

- The dashboard is populated from canonical session, task, runtime-history,
  database, migration, and backup services.
- Session sync never returns `ok=true` without performing or truthfully
  accepting reconciliation work.
- Compression and retry actions resolve real targets and return trackable
  lifecycle ids; they no longer return fixed `404` for every request.
- `ready` and successful action responses are backed by current evidence.
- Migration and backup claims describe the actual stores covered, not only
  Web-local metadata files.
