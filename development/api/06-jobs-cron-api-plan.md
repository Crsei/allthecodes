# Jobs and Cron API Backend Plan

## Scope

Implement scheduled job and run history APIs:

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

Frontend contracts:

- `JobsListResponse`
- `JobCreateRequest`
- `JobUpdateRequest`
- `JobMutationResponse`
- `JobRunResponse`
- `CronHistoryResponse`

## Current Gap

`capabilities.rs` reports `jobs=false`, and no Jobs/Cron routes are registered.
The daemon has scheduler-related code, but the web API layer is missing.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/jobs.rs
```

Routes:

```rust
.route("/api/jobs", get(handlers::jobs_list_handler).post(handlers::jobs_create_handler))
.route(
    "/api/jobs/{id}",
    patch(handlers::jobs_update_handler).delete(handlers::jobs_delete_handler),
)
.route("/api/jobs/{id}/pause", post(handlers::jobs_pause_handler))
.route("/api/jobs/{id}/resume", post(handlers::jobs_resume_handler))
.route("/api/jobs/{id}/run", post(handlers::jobs_run_handler))
.route("/api/cron/history", get(handlers::cron_history_handler))
```

## Data Sources

Prefer existing scheduler/service crates if they already own job definitions.
If not available for web use, MVP store:

```text
ALLTHECODES_HOME/web/jobs.json
ALLTHECODES_HOME/web/job-runs.jsonl
```

## Behavior

- Support `manual`, `interval`, and `cron` schedule kinds.
- Use optimistic concurrency with `revision`.
- `pause`/`resume` toggle paused state and increment revision.
- `run` creates a `JobRunSummary`; MVP may queue a daemon command or mark run
  failed with a clear diagnostic when daemon is unavailable.
- `cron/history` returns recent runs, optionally filtered by `job_id`.

## Scheduler Integration

MVP should not silently execute arbitrary shell commands. For `payload.kind`:

- `prompt`: enqueue daemon submit only when daemon is running.
- `slash_command`: call command runtime only if safe and non-destructive.
- `command`: require an explicit allowlist before execution.

## Tests

Add tests for:

- Empty jobs list returns `[]`.
- Create/update/delete with revision checks.
- Pause/resume revision conflict behavior.
- Run creates run record or unavailable diagnostic.
- History filters by `job_id`.

Run:

```bash
cargo test -p allthecodes-web jobs
```

## Acceptance

- Jobs page can create and edit schedules.
- Manual run gives a real run record or a clear service-unavailable diagnostic.
- Capability can be changed to `jobs=true` after MVP.

