# Web API Gap Plan Index

> Backend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`
> Frontend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`
> Basis: compare frontend `src/lib/api.ts` with backend
> `crates/allthecodes-web/src/mod.rs`.

## Current Result

Gateway and Logs/Diagnostics are excluded from this index because the current
backend worktree already contains uncommitted route/handler work for them:

- `crates/allthecodes-web/src/handlers/gateways.rs`
- `crates/allthecodes-web/src/handlers/logs.rs`
- route registrations in `crates/allthecodes-web/src/mod.rs`

The remaining frontend-called APIs that still need backend routes are grouped
below.

## Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| P0 | [Usage API](01-usage-api-plan.md) | `usage=true` | Usage route is visible but `GET /api/usage` falls through. |
| P0 | [Memory API](02-memory-api-plan.md) | `memory=true` | Memory route is visible but list/update APIs are missing. |
| P0 | [Files API](04-files-api-plan.md) | `files=false` | File upload, read, preview, and file workspace page need backend support. |
| P1 | [Skills API](03-skills-api-plan.md) | `skills=false` | Skills page and composer skill management need listing/detail/file APIs. |
| P1 | [Backend Services API](08-backend-services-api-plan.md) | `backend_services=false` | Services dashboard is fully frontend-wired but has no backend. |
| P2 | [Jobs and Cron API](06-jobs-cron-api-plan.md) | `jobs=false` | Scheduled jobs page and cron history need persistence and runner wiring. |
| P2 | [Kanban API](05-kanban-api-plan.md) | `kanban=false` | Kanban page needs persistent boards/tasks/comments. |
| P3 | [Group Chat API](07-group-chat-api-plan.md) | `group_chat=false` | Highest scope: room state, agents, messages, stream, and compression. |

## Shared Implementation Rules

- Add a dedicated handler module per domain under
  `crates/allthecodes-web/src/handlers/`.
- Export each module from `crates/allthecodes-web/src/handlers/mod.rs`.
- Register explicit routes in `crates/allthecodes-web/src/mod.rs` before
  `/api/{*path}`.
- Keep capability flags `false` until the corresponding page has working MVP
  routes, then flip to `true` in `handlers/capabilities.rs`.
- For profile-aware APIs, accept `profile_id` and echo it in responses even when
  the MVP uses process-global or workspace-global data.
- Prefer `200` with an empty-but-valid response for empty state. Use `404` for
  missing entities, `409` for revision conflicts, and `503` for unavailable
  local services.
- Add route registration tests to prevent capabilities and router state from
  drifting again.

