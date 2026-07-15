# Web API Gap Plan Index

> Backend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes`
> Frontend repository: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes-web`
> Original basis: compare frontend `src/lib/api.ts` with backend
> `crates/allthecodes-web/src/mod.rs`.
>
> Latest committed-code audit: 2026-07-16 at frozen snapshot `6d426dc9`,
> covering the full commit range `d5e16fde..6d426dc9`. See
> [Recent Feature API Gap Audit](13-recent-feature-api-gap-audit.md).

## Remediation Result (2026-07-16)

The audited Security Approval, Memory, Workflow, Jobs/Cron, Skills proposal,
Web IPC Agent/Team, Group Chat, and Discovery gaps are implemented in the
`api-gap-remediation` worktree. Backend protocol metadata and the three owned
`docs/api` artifacts now agree on 273 operations, and the canonical freshness
check passes.

Two boundaries remain intentionally visible instead of being represented as
synthetic success:

- Backend Services is Partial: session/runtime/task projections are canonical,
  while compaction, durable retry, aggregate migration, and consistent backup
  action owners are still unavailable and return typed `409`/`503` responses.
- Generated-artifact freshness is Partial across repositories: backend
  generation and CI are enforced, but the paired frontend artifacts/CI and
  explicit environment-invariance tests remain outstanding.

Workflow mutation transport is complete only to its planned fail-closed
boundary. Production policy returns `Ask`, so start/advance/cancel return
`409 interactive_approval_required` without mutation until a challenge/resume
transport exists. Security Approval still lacks the plan-specific full Web
SSE-to-execution E2E, and Web IPC still awaits a fresh PTY integration result;
their existing protocol/runtime/handler tests are recorded in their plans.

## Audit Snapshot Before Remediation

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
| Done | [Memory API](02-memory-api-plan.md) | `memory=true` | Canonical memdir, bounded dream reads, and typed review proposals are exposed. |
| Done | [Files API](04-files-api-plan.md) | `files=true` | Implemented with workspace tree/stat/read/write/upload/download/mutation routes. |
| Done | [Skills API](03-skills-api-plan.md) | `skills=true` | Native and reserved background skill proposals share a typed, bounded lifecycle. |
| Partial | [Backend Services API](08-backend-services-api-plan.md) | `backend_services=true` | Canonical projections are live; action owners that do not exist fail closed. |
| Done | [Jobs and Cron API](06-jobs-cron-api-plan.md) | `jobs=true` | Definitions, history, manual/due dispatch, and task projection share the canonical scheduler. |
| Done | [Kanban API](05-kanban-api-plan.md) | `kanban=true` | Implemented with local board/task/comment persistence. |
| Done | [Group Chat API](07-group-chat-api-plan.md) | `group_chat=true` | Pure invite reads, privileged mutations, delegated execution, and typed replayable SSE are connected. |

## Audited Gap Outcomes

| Result | Plan | 2026-07-16 outcome | Remaining boundary |
|---|---|---|---|
| Implemented; Web E2E pending | [Security Approval API Parity](14-security-approval-api-parity-plan.md) | Both Web transports preserve display-safe exact-approval context and privileged one-shot response binding. | Add the plan-specific full SSE/bound-response/execution E2E. |
| Implemented | [Memory API](02-memory-api-plan.md) | Runtime memdir, dream reads, and Memory proposal decisions are exposed. | Active automatic producer remains `WorkflowWarning`; reserved consumer kinds are not advertised as active. |
| Implemented | [Jobs and Cron API](06-jobs-cron-api-plan.md) | Canonical scheduler definitions, durable history, and daemon dispatch replace Web-local simulation. | No known scoped code gap. |
| Fail-closed contract implemented | [Workflow Runtime API](15-workflow-runtime-api-plan.md) | Seven typed operations adapt the canonical FileWorkflow owner. | Production `Ask` cannot resume remotely and returns `409` without mutation. |
| Implemented; PTY gate pending | [Web IPC Agent Command Parity](18-web-ipc-agent-command-parity-plan.md) | Agent/Team commands use the shared authorized dispatcher and requester-only result lane. | Capture a fresh PTY command-integration result. |
| Implemented | [Skills API](03-skills-api-plan.md) | Native/reserved proposals have typed list/detail/diff/approve/reject operations. | Extended symlink/failure-injection evidence remains incomplete. |
| Implemented | [Group Chat API](07-group-chat-api-plan.md) | Delegated execution and typed replayable SSE replace store-only messages. | Compression remains an explicitly local room summary. |
| Partial | [Backend Services API](08-backend-services-api-plan.md) | Session, task, and runtime-history projections are canonical and unavailable actions fail closed. | Add real compaction, durable retry, aggregate migration, and consistent backup owners. |
| Implemented | [Discovery Search API](16-discovery-search-api-plan.md) | One bounded read-only endpoint reuses existing providers and scorer. | No known scoped code gap. |

## Cross-cutting Plans

| Priority | Plan | Capability status | Main user-visible risk |
|---|---|---|---|
| Draft | [API Architecture Upgrade Plan](09-api-architecture-upgrade-plan.md) | N/A — infra change | No immediate user risk; all phases are additive with backward compatibility. |
| Draft | [API File Consolidation Plan](11-api-file-consolidation-plan.md) | N/A — cleanup/refactor | Keeps upgraded API features grouped by domain after the architecture migration. |
| Partial | [Generated API Artifact Freshness](17-api-generated-artifact-freshness-plan.md) | N/A — docs/CI | Backend 273-operation artifacts and CI are current; paired frontend artifacts/CI remain. |
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
