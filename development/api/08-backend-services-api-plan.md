# Backend Services API Plan

## Scope

Implement backend services dashboard and action APIs:

```http
GET  /api/backend-services
POST /api/backend-services/sessions/sync
POST /api/backend-services/context-compression/:id/run
POST /api/backend-services/agent-bridge/events/:id/retry
POST /api/backend-services/migrations/run
POST /api/backend-services/backups
```

Frontend contracts:

- `BackendServicesResponse`
- `ServiceActionRequest`
- `ServiceActionResponse`

## Current Gap

`capabilities.rs` reports `backend_services=false`, and no backend-services
routes are registered. The frontend dashboard is already wired and expects
database, session sync, context compression, agent bridge, migration, and backup
status.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/backend_services.rs
```

Routes:

```rust
.route("/api/backend-services", get(handlers::backend_services_handler))
.route(
    "/api/backend-services/sessions/sync",
    post(handlers::backend_services_sessions_sync_handler),
)
.route(
    "/api/backend-services/context-compression/{id}/run",
    post(handlers::backend_services_context_compression_run_handler),
)
.route(
    "/api/backend-services/agent-bridge/events/{id}/retry",
    post(handlers::backend_services_agent_bridge_retry_handler),
)
.route(
    "/api/backend-services/migrations/run",
    post(handlers::backend_services_migrations_run_handler),
)
.route(
    "/api/backend-services/backups",
    post(handlers::backend_services_backup_handler),
)
```

## Status Sources

The dashboard should aggregate:

- Local database/schema status.
- Session runtime vs persisted session inventory.
- Context compression jobs.
- Agent bridge agents/events.
- Local state migrations and backups.

When a subsystem does not exist yet, return `status="unknown"` or
`status="blocked"` with a warning instead of route fallback.

## Actions

`sessions/sync`:

- Compare runtime sessions with persisted sessions.
- Create/update missing metadata records.

`context-compression/:id/run`:

- Start or simulate a compression job for session/room id.
- Return a task handle when async.

`agent-bridge/events/:id/retry`:

- Retry only events marked retryable.

`migrations/run`:

- Support `dry_run`.
- Apply known local schema migrations only.

`backups`:

- Create a timestamped backup of local service state.
- Return backup path in refreshed services response.

## MVP Behavior

Return a complete `BackendServicesResponse` even if all subsystems are empty:

- `database.status = "unknown"` or `"ready"` depending on store availability.
- Empty `tables`, `sessions`, `jobs`, `agents`, `events`, `migrations`,
  `backups` arrays.
- `warnings` explaining unavailable subsystems.

## Tests

Add tests for:

- Empty status response matches frontend shape.
- Session sync returns `ok=true`.
- Migrations dry run does not mutate state.
- Backup creates file under allthecodes-owned directory.
- Unknown compression/event id returns `404`.

Run:

```bash
cargo test -p allthecodes-web backend_services
```

## Acceptance

- Services page loads without API fallback.
- All action buttons receive structured success/error responses.
- Capability can be changed to `backend_services=true` after MVP.
