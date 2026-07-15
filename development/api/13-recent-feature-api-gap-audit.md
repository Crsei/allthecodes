# Recent Feature API Gap Audit

> Audit date: 2026-07-16
> Committed HEAD (frozen audit snapshot): `6d426dc9ed25e9d3cd2175d8a3c4c356a595fe2d`
> Exclusive baseline: `d5e16fde2b0e84db5be13e7fa3f626af405a643f`
> Range: `d5e16fde..6d426dc9`

## Why This Is the Audit Boundary

`d5e16fde` (`Update gateway API and development docs`, 2026-06-18) is the
last and only commit that updated the existing `development/api/*.md` set.
Later commits changed the protocol, Web handlers, daemon API, IPC, and ACP, but
did not refresh this directory. Using the latest API-code commit as the
baseline would make the range empty and hide the documentation drift.

The full Git DAG contains 203 commits in the range: 193 non-merge commits and
10 merge commits. The audit grouped implementation commits by capability and
used this frozen committed source as the final-state check. Remaining
uncommitted API-plan work in the shared worktree is outside this audit. The
final seven commits add worktree/release/TUI plans and artifacts, prepare 0.1.14
package metadata, and update Rust TUI command/logo behavior; none adds a
backend/protocol capability and therefore none requires a product API.

## What Counts as API Coverage

For this Web API plan set, a capability is covered only when it has:

- a typed operation/DTO in `allthecodes-protocol`;
- a registered handler in `allthecodes-web` for REST or API RPC;
- explicit errors, ownership, and serialization behavior; and
- generated contract artifacts that can be kept current.

CLI commands, Rust TUI actions, tools callable only by the model, and daemon
private/control routes are useful integration surfaces, but do not by
themselves complete the Web API contract. ACP and headless IPC count as external
protocols; when they carry data that the Web transport drops, that difference
is recorded as a transport-parity gap.

## Required API Work

| Priority | Commits that introduced the capability | Current committed exposure | Missing API work | Plan |
|---|---|---|---|---|
| P0 | `e736cc37`, `077bf832`, `8b6a43f4` | Shared/normalized contracts can carry redacted security metadata; Web chat drops `operation`/`security`, Web `IpcRuntime::request_permission` omits `security`, and the chat permission-response route is not privileged | Preserve display-safe fields on both Web transports, require privileged response authorization, and bind responses to the existing one-shot exact-approval flow | [14](14-security-approval-api-parity-plan.md) |
| P0 | `eb3299d9`, `f94021b1` | `/api/memory` reads a separate `memory/entries.json`; runtime uses scoped memdir files and dream Markdown, while background review actively produces workflow warnings and reserves memory proposal consumers | Adapt list/update to memdir and add bounded dream/proposal list/detail/approve/reject contracts | [02](02-memory-api-plan.md) |
| P1 | `d7e0a4cc` | `FileWorkflow` tool supports lifecycle operations and `/workflows` lists definitions; no protocol domain exposes definitions or runs | Workflow definition list/detail plus run list/start/status/advance/cancel | [15](15-workflow-runtime-api-plan.md) |
| P1 | `f4b234c9`, `2514ceaa`, `ca4b95db` | Shared agent handlers implement active-list, cursor/limit-aware output, cancel, and fork metadata over process-global hosts; `/api/ipc/ws` only echoes `AgentCommand`/`TeamCommand` | Add authoritative session/workspace ownership and hard bounds, then dispatch the existing typed WebSocket commands through the shared handlers; do not add duplicate workflow/task routes | [18](18-web-ipc-agent-command-parity-plan.md) |
| P1 | `f94021b1` | `/learn` actively stages native skill proposals; `/skills pending/diff/approve/reject` also supports reserved background-review `SkillCreate`/`SkillPatch` records, but no built-in producer currently emits them; Web Skills routes expose none of the proposal lifecycle | Add typed, bounded proposal list/detail/diff/approve/reject operations using the existing owners | [03](03-skills-api-plan.md) |
| P1 | `f94021b1`, `fbd53a9d` | Jobs CRUD uses `web/jobs.json`; manual run persists a fixed failed diagnostic while the daemon's newer scheduled-task enqueue path uses a separate store and private command queue | Keep existing routes but move definitions/history onto the scheduler domain and dispatch through an explicit daemon boundary | [06](06-jobs-cron-api-plan.md) |
| P1 | `f4b234c9` (runtime; existing Group Chat routes rechecked at HEAD) | Group Chat's invite GET persists a code without privileged mutation classification, messages do not execute, and SSE is opaque snapshot/heartbeat only; Backend Services returns no-op/synthetic actions | Split invite read/create-rotate semantics, type the stream, and wire existing routes to delegated agents, sessions, tasks, and runtime events rather than adding duplicate paths | [07](07-group-chat-api-plan.md), [08](08-backend-services-api-plan.md) |
| P2 | `de425212` | Internal MCP/plugin search providers and scorer; mention autocomplete only searches session/file/skill prefixes | One bounded discovery endpoint that reuses existing providers and redaction rules | [16](16-discovery-search-api-plan.md) |

### P0 evidence: Web exact-approval metadata is dropped

The shared request type already contains:

```text
PermissionRequestPayload.operation
PermissionRequestPayload.security
SecurityDecisionDisplay.exact_approval
```

The normalized IPC payload supports those fields, and the standard IPC client
callback path forwards them. In contrast,
`crates/allthecodes-web/src/handlers/chat.rs::ChatPermissionRequestEvent`
contains only `session_id`, `tool_use_id`, `tool`, `command`, `input`, and
`options`. Its constructor does not copy `request.operation` or
`request.security`. In addition,
`allthecodes_ipc::runtime::IpcRuntime::request_permission`, used by
`/api/ipc/ws`, includes `operation` in its server-request params but omits
`security`. Web clients on either transport therefore lose part or all of the
exact-approval context. The engine already implements request-scoped, one-shot
exact approval; this gap is Web transport projection, not missing engine
enforcement. In addition, `requires_privileged_capability` does not currently
match `POST /api/chat/permissions/{tool_use_id}/response`, so the response
mutation needs an explicit privileged route rule before it consumes pending
state.

### P0/P1 evidence: newer runtimes are only partly exposed

- The Memory handler owns a second `MemoryEntry` shape and reads only
  `ALLTHECODES_HOME/memory/entries.json`. Runtime context injection uses
  `allthecodes_session::memdir` with global/project/team/auto scopes; dream
  output is stored separately under `memory/dream/`; the active background
  producer creates `WorkflowWarning`, while command consumers reserve
  `MemoryAdd`/`MemoryReplace`. None of that review queue has a typed Web API.
- `file_workflow.rs` has list/start/status/advance/cancel behavior and durable
  run records. Generic task summaries do not expose workflow definitions,
  steps, arguments, or workflow-specific mutations.
- `allthecodes_ipc::agent_handlers` implements `QueryActiveAgents`,
  cursor/limit-aware `QueryAgentOutput`, `AbortAgent`, team message injection,
  and team status, but current host/tree lookup is process-global and the Web
  output path lacks a hard maximum and can fall back to full retained output.
  Headless ingress calls those handlers, while the same `FrontendMessage`
  variants on `/api/ipc/ws` return only a debug `SystemInfo` echo.
- `allthecodes-skills` persists project/user `SkillProposal` records and owns
  stage/load/list/approve/reject; `/learn` is the active producer. The `/skills`
  command also adapts reserved background-review `SkillCreate` and `SkillPatch`
  records if present, although current background production emits only
  `WorkflowWarning`. Meanwhile,
  `crates/allthecodes-web/src/handlers/skills.rs` stops at list/detail/files and
  enabled/pinned patching. `WorkflowWarning` is not a skill mutation and must
  have one non-Skills owner rather than being consumable from two APIs.
- Jobs routes persist a Web-specific job model and deliberately record manual
  runs as failed. The daemon uses `allthecodes_services::scheduler::SchedulerStore`,
  while `allthecodes_tasks::scheduled` is a third persisted model; the API must
  adapt to one canonical runtime owner rather than creating another store.
- Group Chat invite GET creates and persists a code through a write store even
  though Group Chat is absent from privileged mutation classification. Message
  creation returns no execution handle, and its `Value`-typed SSE response sends
  a snapshot plus heartbeat rather than typed agent lifecycle events. Backend
  Services session sync is a successful no-op while compression and agent
  bridge retry return fixed not-found responses.
- `discovery_search.rs` already owns ranking, filtering, result DTOs, provider
  failure handling, and contribution redaction. No `ApiMethod` calls it.

## Recent Features Already Covered by API

These updates do not need another domain API. Their existing contracts may
still need generated-document refresh:

| Feature group | Representative commits | Existing coverage |
|---|---|---|
| Account auth and billing | `4c9c8d10`, `dca8946d`, `a5f62863`, `a23023ae` | Account-auth login/status/refresh/logout/billing routes |
| Project chat modes | `6e89fd8f`, `39bf9406` | Chat-mode protocol operations and handlers |
| MCP OAuth and scope isolation | `5a15c0bb`, `b65f9783`, `b08beb4b`, `294845f3`, `d10dcc6c` | MCP auth/probe/health/bindings handlers and protocol operations |
| Files, media, and Git metadata | `dc523ffe` | Typed file preview/media and Git metadata/worktree routes |
| Record/replay and session actions | `b068fc7c`, `ff707be3` | Session detail/actions, replay state, Web and IPC integration |
| Worktree session records | `657d2e3e` | List/current/by-session Web operations |
| Session search | `f94021b1` | `GET /api/sessions/search` plus gateway/runtime integration |
| Runtime dashboard | `fa2c27dc`, `a6fd9a09` | `GET /api/agent-runtime/dashboard` (read-only gap handled separately) |
| Structured channels | `a6893182` | Channel list/config/update/enable/disable/connect/test routes |
| Verification reports | `492286d6` | `GET /api/sessions/{id}/report`, API RPC, IPC summary, daemon and TUI events |
| Proactive/Kairos control plane | `ac936a6a` through `cd524720`, `77812d3b` | Kairos status includes automation snapshot; typed config/start/stop/restart Web operations cover the product control boundary |
| Fork context inheritance | `2514ceaa`, `ca4b95db` | Inheritance is automatic and no separate toggle should expose raw parent context; Web output/cancel command wiring is tracked separately in plan 18 |

## Updates That Do Not Need a New API

| Update class | Examples | Reason |
|---|---|---|
| TUI presentation/state refactors | operation renderer, overlay/domain-store work, color/context layer, welcome logo, slash-command first-Enter behavior | TUI-local interaction/presentation; existing command/backend events remain the data boundary |
| Engine/startup refactors | composition root, typed turn pipeline, domainized settings/state | Internal ownership changes with no new user operation |
| Build, CI, tests, and releases | cargo verification, PTY sharding, 0.1.6-0.1.14 | No runtime capability |
| Offline project KB builder | `2f84d5db` | Explicit maintainer-only script; generated KB is read-only at runtime |
| Hashline edit and policy guardrails | `13da354f`, `b26c49ac` | Internal tool execution behavior already reached through chat/tool transports |
| Team idle hooks and daemon I/O bounds | `e5ee7cd1`, `5b963813` | Lifecycle/reliability behavior, not a user-facing operation |
| Remote URL discovery gate | `c8236be2` | Feature gate marks the remote source as deferred; no enabled backend exists to expose |

## Generated Contract Drift

`crates/allthecodes-protocol/src/request.rs` currently declares 253 unique
operations. The committed `docs/api/routes.md`, last updated by `6e89fd8f` on
2026-06-23, contains 207 generated route rows. The 46 missing operations cover
Kairos, session search/report, tasks and runtime dashboard, worktree sessions,
MCP auth/health, channel config, file/media/Git additions, queue/messaging,
voice/image, and related newer handlers.

The generator binaries currently print route, schema, and OpenAPI output, but
CI has no repository-diff check for these three committed artifacts. This is
tracked by [Generated API Artifact Freshness](17-api-generated-artifact-freshness-plan.md).

## Recommended Order

1. Fix security approval transport parity before exposing more remote mutation
   APIs.
2. Add artifact freshness checks so every later protocol change updates the
   committed contract.
3. Move Memory and Jobs APIs onto their real runtime owners.
4. Add the workflow domain API by adapting the existing file-workflow owner;
   do not add a second run store.
5. Add the missing Skills proposal projection without duplicating proposal
   stores or allowing cross-workspace approval.
6. Add authoritative agent/team ownership and hard output bounds, then wire Web
   IPC Agent/Team commands to the shared runtime handlers.
7. Connect Group Chat and Backend Services to existing runtime services.
8. Add discovery search as a read-only, bounded endpoint.

## Verification for Future Implementations

Every plan in this audit must at minimum verify:

```bash
cargo test -p allthecodes-protocol
cargo test -p allthecodes-web
cargo fmt --all --check
git diff --check
```

Protocol changes must also regenerate and compare:

```text
docs/api/routes.md
docs/api/schema.json
docs/api/openapi.json
allthecodes-web/src/lib/generated/api-types.ts
allthecodes-web/src/lib/generated/api-routes.ts
allthecodes-web/src/lib/generated/api-schema.json
```
