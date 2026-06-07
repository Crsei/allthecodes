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

Usage, Memory, Files, Skills, Kanban, Jobs/Cron, Group Chat, and Backend
Services APIs are no longer gaps in the current worktree:

- `crates/allthecodes-web/src/handlers/usage.rs`
- `crates/allthecodes-web/src/handlers/memory.rs`
- `crates/allthecodes-web/src/handlers/files.rs`
- `crates/allthecodes-web/src/handlers/skills.rs`
- `crates/allthecodes-web/src/handlers/kanban.rs`
- `crates/allthecodes-web/src/handlers/jobs.rs`
- `crates/allthecodes-web/src/handlers/group_chat.rs`
- `crates/allthecodes-web/src/handlers/backend_services.rs`
- route registrations in `crates/allthecodes-web/src/mod.rs`
- capability flags enabled in `crates/allthecodes-web/src/handlers/capabilities.rs`
- tests in `crates/allthecodes-web/src/handlers/mod.rs`

Skills remains intentionally integrated as a web handler backed by
`allthecodes-skills`; do not split it into a standalone `skills-api` service or
crate. Kanban, Jobs/Cron, Group Chat, and Backend Services use local MVP stores
under `ALLTHECODES_HOME/web`.

The frontend-called APIs are grouped below. Completed rows are kept for
traceability; incomplete rows still need backend routes or service wiring.

## Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| Done | [Usage API](01-usage-api-plan.md) | `usage=true` | Implemented as a zeroed/partial dashboard until runtime usage accumulation exists. |
| Done | [Memory API](02-memory-api-plan.md) | `memory=true` | Implemented with list/update/config routes. |
| Done | [Files API](04-files-api-plan.md) | `files=true` | Implemented with workspace tree/stat/read/write/upload/download/mutation routes. |
| Done | [Skills API](03-skills-api-plan.md) | `skills=true` | Implemented in `allthecodes-web` and backed by `allthecodes-skills`; no separate `skills-api`. |
| Done | [Backend Services API](08-backend-services-api-plan.md) | `backend_services=true` | Implemented as a complete MVP dashboard/action shape with local state and backups. |
| Done | [Jobs and Cron API](06-jobs-cron-api-plan.md) | `jobs=true` | Implemented with local job/run persistence; execution backend remains diagnostic-only. |
| Done | [Kanban API](05-kanban-api-plan.md) | `kanban=true` | Implemented with local board/task/comment persistence. |
| Done | [Group Chat API](07-group-chat-api-plan.md) | `group_chat=true` | Implemented durable MVP with SSE snapshot; real agent runner is a follow-up. |

## Cross-cutting Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| Draft | [API Architecture Upgrade Plan](09-api-architecture-upgrade-plan.md) | N/A — infra change | No immediate user risk; all phases are additive with backward compatibility. |
| Draft | [API File Consolidation Plan](11-api-file-consolidation-plan.md) | N/A — cleanup/refactor | Keeps upgraded API features grouped by domain after the architecture migration. |

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
