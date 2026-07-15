# Jobs and Cron API Backend Plan

> Status reviewed: 2026-07-16
> Current result: all advertised routes exist, but they do not control the
> scheduler used by the daemon.

## Scope

Keep the existing Jobs/Cron HTTP surface and replace its private persistence
and simulated execution with the canonical scheduler lifecycle:

```http
GET    /api/jobs
POST   /api/jobs
PATCH  /api/jobs/:id
DELETE /api/jobs/:id
POST   /api/jobs/:id/pause
POST   /api/jobs/:id/resume
POST   /api/jobs/:id/run
GET    /api/cron/history
```

This is a service-wiring and contract-hardening task, not a request for another
parallel set of scheduling routes.

## Current Implementation

The routes are registered and `capabilities.jobs` is `true`. CRUD,
pause/resume, optimistic revision checks, and a run-history shape are present
in `crates/allthecodes-web/src/handlers/jobs.rs`. They are not connected to the
runtime scheduler:

- the Web handler owns `{data_root}/web/jobs.json` and
  `{data_root}/web/job-runs.jsonl`;
- `POST /api/jobs/:id/run` always appends a synthetic `failed` run with
  `Job execution backend is not wired`;
- the daemon polls `allthecodes_services::scheduler::SchedulerStore::open_default()`
  and dispatches its due tasks;
- `/api/tasks` also reads `allthecodes_tasks::load_scheduled_tasks()` from
  `{data_root}/scheduled_tasks/tasks.json`, even though production code does
  not populate that store.

There are therefore three incompatible scheduled-task stores:

| Store | Current consumer | Runtime authority |
|---|---|---:|
| `{data_root}/scheduled_tasks.json` / scheduler SQLite | daemon `SchedulerStore` | Yes |
| `{data_root}/web/jobs.json` plus `job-runs.jsonl` | Jobs/Cron Web API | No |
| `{data_root}/scheduled_tasks/tasks.json` | `/api/tasks` projection/tests | No |

A job created through the Web API is not seen by the daemon, and a task fired
by the daemon is not represented by Web history.

## Canonical Ownership

`allthecodes_services::scheduler::SchedulerStore` must own definitions and
schedule state. It is a passive persistence/schedule-calculation store, not an
execution queue. The Web and daemon layers should depend on a shared scheduler
domain service that provides atomic operations for:

- list/get/add/update/remove;
- pause/resume;
- due-task claiming and dispatch intent creation;
- manual trigger intent creation;
- run creation and lifecycle transitions;
- bounded run-history queries.

Do not make the Web handler mutate the scheduler JSON or SQLite tables
directly. Preserve the store's cross-process locking and SQLite fallback
behavior.

The obsolete `allthecodes_tasks::scheduled` store must be migrated or removed.
If `/api/tasks` continues to include scheduled definitions, build that
projection from `SchedulerStore`; do not load a third file.

## Protocol and Data Model

Replace the current `Value` operation metadata with typed DTOs in
`allthecodes-protocol`. The canonical model and Web contract need an explicit
compatibility decision for fields that currently differ:

| Web field/behavior | Scheduler status | Required decision |
|---|---|---|
| `manual`, `interval`, `cron` | interval and cron definitions | Represent manual-only jobs explicitly or model manual trigger separately. |
| `prompt`, `slash_command`, `command` | prompt and slash-command payloads | Reject raw command payloads unless a separately approved allowlisted executor exists. |
| `revision` | not in `ScheduledTask` | Persist a canonical revision/version and check it atomically. |
| `profile_id`, `session_id`, artifacts | not fully represented | Add typed optional metadata or remove/deprecate it; never silently drop it. |
| timezone | stored, cron currently evaluated by scheduler rules | Report actual evaluation semantics; do not claim local-time support until implemented. |

Creation and updates must use the scheduler's interval/cron parser and compute
the same `next_run_at` the daemon will use.

## Store Migration

Provide an idempotent migration before switching the handlers:

1. Read and validate legacy `web/jobs.json` under its existing lock.
2. Map supported jobs into `SchedulerStore` with deterministic legacy ids or a
   durable id mapping.
3. Quarantine unsupported raw-command jobs and report them; do not enable them
   implicitly.
4. Resolve duplicate ids/schedules without overwriting newer canonical tasks.
5. Import useful historical records into the canonical run store, marked as
   legacy/synthetic where appropriate.
6. Mark migration complete and stop dual reads/writes.
7. Remove the `allthecodes_tasks::scheduled` projection after confirming it has
   no unique records, or migrate those records by the same rules.

Rollback may retain backups of the legacy files, but after cutover only
`SchedulerStore` may own definitions and only the canonical scheduler run
repository may own history.

## Dispatch Ownership and Availability

The current due-task path is daemon-private: `scheduler_loop.rs` loads due rows
from `SchedulerStore`, then calls the daemon crate's private
`protocol_store().enqueue_command(...)` and only afterwards calls
`record_fired`. Opening `SchedulerStore` from a Web handler therefore cannot
submit work and must not be presented as a manual-run implementation.

Introduce one explicit command-dispatch boundary owned by the daemon. Either:

- expose an authenticated local daemon RPC/IPC operation for scheduled submit;
  or
- inject a `SchedulerCommandDispatcher`-style interface from the daemon
  composition root when Web and daemon run in one process.

Both due and manual triggers call that boundary with the canonical job snapshot,
run ID, trigger source, workspace/profile context, and idempotency key. The
typed dispatch receipt includes at least `run_id`, `command_id`, `accepted_at`,
and the idempotency key. Do not make
`protocol_store()` public as a general file API, let the Web handler write daemon
command files directly, or duplicate the assistant submit payload builder in
`jobs.rs`.

Ownership stays explicit:

- `SchedulerStore` owns definitions, pause state, and schedule calculation;
- the scheduler domain's canonical run repository owns durable trigger intents
  and lifecycle records;
- the daemon command queue owns enqueue/claim/handled/failed command state;
- the assistant worker/session/task owners provide execution and terminal
  evidence;
- scheduler history is a bounded projection joining those stable identifiers,
  not a synthetic Web log.

If no daemon dispatcher is registered, the daemon is unhealthy, or enqueue
cannot be durably acknowledged, manual run returns `503 dispatcher_unavailable`
or `503 enqueue_failed`. If a canonical run was created first, transition that
same run to a typed `rejected`/`failed_to_enqueue` state; otherwise create no
history row. Never append the current fixed diagnostic as a normal failed run.
Only advance a recurring schedule after the due enqueue is durably accepted;
manual triggers do not advance it.

Persist an `enqueueing` run before dispatch, transition it to `queued` only
after the daemon durably accepts the command, and return HTTP 202 only with that
acceptance receipt. Daemon command ack/start/completion/failure events advance
the same run through `running` and a terminal state. One idempotency key must
cover the run-store/command-queue crash window; reconciliation after restart
must find the accepted command or finalize the original enqueue failure, never
enqueue a second command.

## Real Run Lifecycle

`POST /api/jobs/:id/run` must create a real run and submit work through the
same injected daemon command-dispatch boundary used for due scheduled tasks. It
should return an accepted run/task handle, not wait for the model and not
manufacture a failed record.

At minimum the canonical run record needs:

- run id and job id;
- `enqueueing`, `queued`, `running`, `rejected`/`failed_to_enqueue`, and
  execution-terminal status;
- scheduled/manual trigger source and idempotency key;
- enqueue/start/completion timestamps;
- daemon command id, task/session id, and artifact references when available;
- bounded output summary and a typed failure reason.

The dispatcher must update this record as enqueue, execution, cancellation,
and failure occur. `GET /api/cron/history` reads these records and supports
bounded pagination plus job/status/time filters. `last_run_at` and
`next_run_at` must come from the same committed transition, so the definition
and history cannot disagree after a crash.

Manual triggering must not accidentally advance a recurring schedule unless
that behavior is explicitly part of the scheduler service contract.

## Error Semantics

- `400` for invalid schedule, timezone, payload, or unsupported execution kind.
- `404` for an unknown canonical job/run.
- `409` for revision conflict, duplicate idempotency key, or invalid lifecycle
  transition.
- `503` when the daemon/worker cannot accept a run; persist a real failed or
  rejected transition only if a run was created.
- Never return a normal `200` response whose only behavior is the fixed
  "backend is not wired" diagnostic.

## Safety

- Keep scheduled execution limited to prompt and slash-command semantics
  already enforced by the daemon.
- Apply command permission, taint, workspace, and profile rules at execution
  time, not only when a job is created.
- Use idempotency keys for due and manual dispatch to prevent duplicate work
  after retry/restart.
- Bound history, logs, and artifact metadata returned by the API.
- Serialize mutations per job and use the scheduler store's cross-process
  locking for daemon/Web concurrency.

## Tests

Add coverage for:

- A Web-created job is returned by `SchedulerStore` and can become due.
- A daemon-created scheduler task appears in `GET /api/jobs` and `/api/tasks`.
- Create/update/delete/pause/resume preserve canonical revision checks.
- Interval and cron `next_run_at` match daemon evaluation.
- Manual run enqueues a real command and returns a stable run/task id.
- Due and manual runs traverse the same daemon dispatcher and persist the
  daemon command id; Web code never writes the private command store directly.
- Missing/unhealthy dispatch returns 503 and either creates no run or records
  one truthful `failed_to_enqueue` transition, never a synthetic execution.
- An accepted dispatch returns 202 only after durable daemon enqueue; retry and
  restart reconciliation reuse the original run/command instead of dispatching
  twice.
- Enqueue failure and terminal worker failure produce truthful lifecycle rows.
- History filters and pagination read the canonical run store.
- Repeated idempotency keys do not dispatch twice.
- Legacy Web and `allthecodes-tasks` migrations are idempotent and report
  unsupported rows.
- Protocol metadata, handlers, schema, OpenAPI, and generated clients agree.

Run targeted verification:

```bash
cargo test -p allthecodes-services scheduler
cargo test -p allthecodes-daemon scheduler
cargo test -p allthecodes-web jobs
cargo test -p allthecodes-web tasks
```

## Acceptance

- Existing Jobs/Cron definition routes operate on the scheduler domain backed by
  `SchedulerStore`, while trigger routes use the daemon dispatcher; neither
  legacy Web/`allthecodes-tasks` scheduled-task store remains active.
- A created or updated job is visible to the daemon without copying state.
- `POST /api/jobs/:id/run` submits real work and returns a trackable run/task
  handle through the daemon-owned dispatch boundary.
- `/api/cron/history` reflects actual enqueueing/queued/running/rejected and
  execution-terminal transitions.
- Unsupported execution kinds fail closed instead of being stored as runnable
  jobs.
