# Web API Gap Plan Index

> Backend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`
> Frontend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`
> Original basis: compare frontend `src/lib/api.ts` with backend
> `crates/allthecodes-web/src/mod.rs`.
>
> Latest committed-code audit: 2026-07-16 at frozen snapshot `6d426dc9`,
> covering the full commit range `d5e16fde..6d426dc9`. See
> [Recent Feature API Gap Audit](13-recent-feature-api-gap-audit.md).

## Current Result

The original page-level routes below are implemented, but route availability is
not the same as real runtime integration. The 2026-07-16 audit found newer
runtime/API contract gaps introduced after this directory was last updated:

1. Web chat permission events drop `operation` and redacted security context;
   the Web IPC runtime also omits `security` even though the normalized wire
   contract can carry it. The Web chat permission-response mutation is also not
   classified as privileged.
2. `/api/memory` still reads a Web-only `memory/entries.json` store instead of
   the runtime `memdir`; dream memories and approval-gated proposals are absent.
3. File-backed workflows have lifecycle support through `WorkflowTool` and a
   definition-list-only `/workflows` command, but no domain API for definition
   list/detail or run list/start/status/advance/cancel.
4. Jobs routes use a separate Web store and manual runs always fail instead of
   using the real scheduled-task/scheduler runtime.
5. Group Chat and Backend Services expose MVP shapes, but still return
   store-only, no-op, synthetic, or fixed-error results for runtime actions.
   Group Chat's invite GET also creates persistent state without privileged
   mutation authorization and its stream schema remains opaque.
6. Unified MCP/plugin discovery search, including summarized skill/tool
   contributions from those providers, is internal-only; existing mention
   autocomplete is a different, narrower contract.
7. Native skill proposals created by `/learn` can be listed, inspected,
   approved, or rejected through `/skills`; the command also has reserved
   consumers for background-review skill variants, but the Web Skills API has
   no proposal contract. The current background producer emits only workflow
   warnings.
8. `/api/ipc/ws` accepts typed Agent/Team commands but currently echoes them as
   `SystemInfo`; delegated-agent output, cancel, and team control never reach
   the shared headless handlers.

The protocol source currently declares 253 operations while the committed
protocol-generated section of `docs/api/routes.md` contains 207 route rows.
This 46-operation drift is tracked separately because it is a documentation/CI
gap, not a missing runtime endpoint.

Gateway and Logs/Diagnostics remain outside this page-gap index because their
dedicated handlers already existed at the original baseline:

- `crates/allthecodes-web/src/handlers/gateways.rs`
- `crates/allthecodes-web/src/handlers/logs.rs`
- registrations in `crates/allthecodes-web/src/handler_registry.rs`

Usage, Memory, Files, Skills, Kanban, Jobs/Cron, Group Chat, and Backend
Services all have route shapes in the committed tree:

- `crates/allthecodes-web/src/handlers/usage.rs`
- `crates/allthecodes-web/src/handlers/memory.rs`
- `crates/allthecodes-web/src/handlers/files.rs`
- `crates/allthecodes-web/src/handlers/skills.rs`
- `crates/allthecodes-web/src/handlers/kanban.rs`
- `crates/allthecodes-web/src/handlers/jobs.rs`
- `crates/allthecodes-web/src/handlers/group_chat.rs`
- `crates/allthecodes-web/src/handlers/backend_services.rs`
- route registrations in `crates/allthecodes-web/src/handler_registry.rs`
- capability flags enabled in `crates/allthecodes-web/src/handlers/capabilities.rs`
- tests colocated in handler modules and adjacent `*_tests.rs` files

Route presence alone is not completion: Memory and Jobs must stop treating
their Web-only stores as canonical, while Group Chat and Backend Services still
need runtime wiring. Skills remains intentionally integrated as a Web handler
backed by `allthecodes-skills`, but its newer proposal lifecycle is not exposed;
do not split either side into a standalone `skills-api` service or crate.

The frontend-called APIs are grouped below. Completed rows are kept for
traceability; incomplete rows still need backend routes or service wiring.

## Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| Done | [Usage API](01-usage-api-plan.md) | `usage=true` | Implemented as a zeroed/partial dashboard until runtime usage accumulation exists. |
| Partial | [Memory API](02-memory-api-plan.md) | `memory=true` | Routes exist, but they use a disconnected Web-only store and omit memdir, dream memory, and review proposals. |
| Done | [Files API](04-files-api-plan.md) | `files=true` | Implemented with workspace tree/stat/read/write/upload/download/mutation routes. |
| Partial | [Skills API](03-skills-api-plan.md) | `skills=true` | List/detail/files and enabled/pinned updates work; active native proposals and reserved background skill records have no typed Web lifecycle. |
| Partial | [Backend Services API](08-backend-services-api-plan.md) | `backend_services=true` | Route shapes exist; session sync, compression, and agent-event retry still use no-op/synthetic sources. |
| Partial | [Jobs and Cron API](06-jobs-cron-api-plan.md) | `jobs=true` | CRUD is Web-local and manual execution remains fixed-failure instead of using the scheduler. |
| Done | [Kanban API](05-kanban-api-plan.md) | `kanban=true` | Implemented with local board/task/comment persistence. |
| Partial | [Group Chat API](07-group-chat-api-plan.md) | `group_chat=true` | Durable CRUD/SSE snapshot exists; invite GET mutates state, SSE metadata is opaque, and message execution is not connected to the delegated-agent runtime. |

## Gaps Found After the Original Index

| Priority | Plan | Current exposure | Required outcome |
|---|---|---|---|
| P0 | [Security Approval API Parity](14-security-approval-api-parity-plan.md) | Standard IPC callbacks preserve `security`; Web chat drops it, Web IPC runtime omits it, and the chat response route lacks privileged classification | Preserve redacted exact-approval metadata on both Web transports, require privileged response authorization, and enforce one-shot response semantics. |
| P0 | [Memory API](02-memory-api-plan.md) | Web-only entries store; runtime memdir/dream/proposals elsewhere | Make the API a projection of runtime memory and add bounded dream/proposal contracts. |
| P1 | [Jobs and Cron API](06-jobs-cron-api-plan.md) | Web-local CRUD; fixed-failure run | Adapt the existing routes to the scheduled-task/scheduler owner and real run history. |
| P1 | [Workflow Runtime API](15-workflow-runtime-api-plan.md) | Tool lifecycle + definition-list-only command | Expose workflow definitions plus discoverable persisted runs and their lifecycle through the protocol registry. |
| P1 | [Web IPC Agent Command Parity](18-web-ipc-agent-command-parity-plan.md) | Typed Agent/Team commands are accepted but only echoed | Wire output/status/cancel/team commands to the shared runtime handlers with Web authorization. |
| P1 | [Skills API](03-skills-api-plan.md) | `/skills pending/diff/approve/reject` only | Add bounded proposal list/detail/diff/approve/reject operations for active native proposals and reserved background skill records. |
| P1 | [Group Chat API](07-group-chat-api-plan.md) | Mutating invite GET, store-only messages, opaque snapshot SSE | Split pure invite read from privileged/idempotent create-rotate, reuse the delegated-agent supervisor, and publish typed lifecycle events. |
| P1 | [Backend Services API](08-backend-services-api-plan.md) | MVP/synthetic service state | Replace no-op and fixed-error actions with real session/task/runtime sources. |
| P2 | [Discovery Search API](16-discovery-search-api-plan.md) | Internal MCP/plugin search tools | Expose one bounded, typed search endpoint that reuses the existing scorer/providers. |

## Cross-cutting Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| Draft | [API Architecture Upgrade Plan](09-api-architecture-upgrade-plan.md) | N/A — infra change | No immediate user risk; all phases are additive with backward compatibility. |
| Draft | [API File Consolidation Plan](11-api-file-consolidation-plan.md) | N/A — cleanup/refactor | Keeps upgraded API features grouped by domain after the architecture migration. |
| P0 | [Generated API Artifact Freshness](17-api-generated-artifact-freshness-plan.md) | N/A — docs/CI | Make route, schema, OpenAPI, and TypeScript drift fail verification. |
| Audit | [Recent Feature API Gap Audit](13-recent-feature-api-gap-audit.md) | N/A — evidence | Records the commit boundary, covered features, necessary gaps, and exclusions. |

## Shared Implementation Rules

- Define the typed operation and DTOs in `allthecodes-protocol` before adding a
  Web transport.
- Add a dedicated handler module per domain under
  `crates/allthecodes-web/src/handlers/`, export its `handlers()` entries, and
  include them in `handler_registry::all_api_handlers()`.
- Keep capability flags `false` until the corresponding page has working MVP
  behavior, not merely registered routes; then flip to `true` in
  `handlers/capabilities.rs`.
- For profile-aware APIs, accept `profile_id` and echo it in responses even when
  the MVP uses process-global or workspace-global data.
- Prefer `200` with an empty-but-valid response for empty state. Use `404` for
  missing entities, `409` for revision conflicts, and `503` for unavailable
  local services.
- Add route registration tests to prevent capabilities and router state from
  drifting again.
- Treat `crates/allthecodes-protocol/src/request.rs` and its DTO modules as the
  route contract source of truth. A handler-only or daemon-only route does not
  count as complete Web API coverage.
- Update generated API artifacts in the same commit as protocol metadata, and
  fail CI when regeneration produces a diff.
