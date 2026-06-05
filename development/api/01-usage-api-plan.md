# Usage API Backend Plan

## Scope

Implement the API used by the Usage page:

```http
GET /api/usage?period=24h|7d|30d|90d|all&profile_id=...
```

Frontend contract: `UsageDashboardResponse` in
`allthecodes-web/src/lib/types.ts`.

## Current Gap

`capabilities.rs` currently reports `usage=true`, but no `/api/usage` route is
registered in `crates/allthecodes-web/src/mod.rs`. The page can be shown and
then hit the API fallback.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/usage.rs
```

Export from `handlers/mod.rs`, then register:

```rust
.route("/api/usage", get(handlers::usage_handler))
```

## Data Sources

Use a layered source strategy:

1. Runtime app state usage totals from `state.engine().app_state()`.
2. Session metadata from the session store where available.
3. Durable run/session usage records if present under `ALLTHECODES_HOME`.
4. Empty dashboard fallback when no usage source exists.

## Response Rules

Return:

- `period`: requested period, default `7d`.
- `generated_at`: milliseconds timestamp.
- `totals`: token and cost totals, zeroed if empty.
- `buckets`: time buckets for the selected period.
- `by_model`: grouped totals by model id.
- `by_provider`: grouped totals by provider id.
- `partial=true` and `warnings` when only runtime totals are available.

For `period=all`, aggregate all known records and use coarse monthly buckets.

## Validation

- Reject unknown `period` with `400 invalid_period`.
- Accept missing `profile_id`; echo explicit profile id when provided.
- Never expose API keys, auth headers, or raw prompts.

## Tests

Add tests for:

- Empty store returns zero totals and `200`.
- Runtime usage produces non-zero totals.
- Invalid period returns `400 invalid_period`.
- `profile_id` is echoed.
- Capability stays `usage=true` only when route is registered.

Run:

```bash
cargo test -p allthecodes-web usage
```

## Acceptance

- `curl http://127.0.0.1:17322/api/usage?period=7d` returns `200`.
- Usage page no longer displays fallback `API endpoint not implemented`.
- Response matches the frontend `UsageDashboardResponse` shape.

