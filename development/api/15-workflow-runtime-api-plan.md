# File Workflow Runtime API Plan

> Status: Implemented to the planned fail-closed boundary on 2026-07-16
> Current result: typed reads are available; production mutations remain
> unavailable because the shared workflow policy returns `Ask`, which maps to
> `409 interactive_approval_required` without mutation.
> Priority: P1
> Scope: project-local file workflow definitions and runs

## Implementation Result (2026-07-16)

Implemented in `6603aa89` and hardened in `7f6a017a`; the seven operations are
included in the backend artifacts refreshed by `e5163791`:

- definition list/detail and run list/status read the canonical project-local
  FileWorkflow owner through typed REST/API-RPC adapters;
- strict identifiers, workspace containment, symlink rejection, bounded
  pagination, revision checks, cross-process locking, atomic writes, and
  request idempotency protect the shared service;
- status reads are pure and task state remains a derived projection; and
- start/advance/cancel are typed and privileged, but production authorization
  returns `Ask`; without a challenge/resume channel they return
  `409 interactive_approval_required` and perform no write.

Successful mutation paths are exercised only with an explicit test `Allow`
policy; that fixture is not production readiness. Workflow tests pass 52/52.
The atomic-write/task-projection failure-injection cases listed later are not
separately evidenced. Generic Agent output and abort remain on the existing IPC
contract, whose Web dispatch is now implemented by plan 18.

## Audit Snapshot Before Implementation

The workflow lifecycle is already implemented in
`crates/allthecodes-tools/src/workflow/file_workflow.rs` and wired through
`crates/allthecodes-tools/src/workflow/mod.rs::WorkflowTool`.

Implemented FileWorkflow behavior:

| Action | Current owner |
|---|---|
| `list` | Enumerates valid Markdown/YAML definitions with `list_workflow_scripts` |
| `start` | Parses one definition, creates a UUID run, starts its first step, persists it, and projects it into the task store |
| `status` | Loads the canonical run record, but currently also refreshes the task projection as a read-side effect |
| `advance` | Applies `completed`, `failed`, or `cancelled` to the active step and performs the next state transition |
| `cancel` | Cancels all pending/running steps and finalizes the run |

Definitions and runs are stored under project-local paths:

```text
<project>/.allthecodes/workflows/*.{md,yaml,yml}
<project>/.allthecodes/workflow-runs/<run_id>.json
```

That storage location is not yet a complete isolation guarantee. Definition
discovery and parsing use `Path::is_file` and `fs::read_to_string`, which follow
symlinks, and run lookup relies on a sanitized joined file name. The Web API
must add canonical containment, strict identifiers, and symlink rejection
before treating these paths as an authorization boundary.

`allthecodes_tasks::global_store()` receives a
`TASK_KIND_LOCAL_WORKFLOW` projection for task UI and agent coordination. It is
not the workflow definition or run source of truth.

The gap is that `allthecodes-protocol` and `allthecodes-web` do not expose this
domain as typed operations. The implementation must adapt the existing owner,
not create a Web-only workflow model or store.

## Goals

1. Add typed definition list/detail and run list/start/status/advance/cancel
   contracts, with truthful mutation availability.
2. Keep one canonical definition directory and one canonical run record.
3. Bind every request to the active Web workspace; never accept arbitrary cwd.
4. Apply the same permission and taint policy as `WorkflowTool`.
5. Make run mutations atomic, concurrency-safe, and retry-safe.
6. Keep generic delegated-agent output/cancel on the existing IPC contract and
   complete its WebSocket dispatch separately in [plan 18](18-web-ipc-agent-command-parity-plan.md).

## Non-Goals

- Do not add CRUD for workflow definition files in this phase.
- Do not expose the legacy static `WorkflowRecord` mode as a second API.
- Do not create `web/workflows.json`, a workflow SQLite table, or another task
  store.
- Do not execute a step's `run` command from an HTTP handler. FileWorkflow
  currently records and presents steps; command execution must continue through
  the normal tool pipeline and security checks.
- Do not add generic `TaskOutput` or `TaskStop` operations. The existing
  `AgentCommand::QueryAgentOutput` and `AgentCommand::AbortAgent` DTOs remain
  the intended contract. At the audit snapshot, headless ingress dispatched
  them while the WebSocket branch emitted a debug `SystemInfo` response; that
  separate Web gap is now implemented by
  [plan 18](18-web-ipc-agent-command-parity-plan.md).

## Domain Ownership

Expose a small FileWorkflow service facade from
`crates/allthecodes-tools/src/workflow/` around the existing parser and store
functions. Both `WorkflowTool` and Web handlers must call this facade.

Canonical ownership rules:

- Definition source: `workflow_scripts_dir(cwd)` and
  `parse_workflow_script`.
- Run source: `FileWorkflowRunRecord` files under `workflow_runs_dir(cwd)`.
- Task store: derived projection only. API reads must not reconstruct workflow
  state from generic task output or metadata.
- Status reads: split the current `load_file_workflow_run` into a pure canonical
  load and an explicit idempotent task-projection reconciliation operation.
  `GET` handlers call only the pure load.
- Protocol DTOs: transport projections only; they do not own persistence.
- Web handler: authorization, workspace binding, DTO mapping, and error mapping
  only.

If task projection synchronization fails after a run is durably written, the
run file remains authoritative. Return an explicit projection warning or error
and support idempotent reconciliation; never roll forward a second copy of run
state in the task store.

## Typed Protocol

Add `crates/allthecodes-protocol/src/v1/workflows.rs`, export it from
`v1/mod.rs`, and register operations in
`crates/allthecodes-protocol/src/request.rs`.

Core DTOs:

```text
WorkflowDefinitionSummary
WorkflowDefinitionDetail
WorkflowStepDefinition
WorkflowRunSummary
WorkflowRun
WorkflowRunStep
WorkflowRunStatus
WorkflowRunListQuery
WorkflowRunPage
WorkflowStartRequest
WorkflowAdvanceRequest
WorkflowCancelRequest
WorkflowMutationMetadata
```

Transport DTOs should use stable identifiers and project-relative file names.
Do not return absolute `workflow_path` or `run_path`. Do not return raw stored
arguments by default; expose `has_args` and an explicitly redacted argument
view if the UI needs one.

Every full run response includes:

```text
run_id
workflow
workflow_file
status
current_step_index
steps
completed_steps
total_steps
revision
created_at
updated_at
```

`WorkflowRunSummary` omits step prompts, run suggestions, and arguments while
retaining run/workflow identity, closed typed status, progress counts, revision,
and timestamps. `WorkflowRunPage` contains `runs`, `next_cursor`, `truncated`,
and a bounded corrupt-entry diagnostic count.

Mutation requests include:

```text
request_id        # idempotency key scoped to workspace and operation
expected_revision # optimistic concurrency check for existing runs
```

Unknown request fields remain forward compatible where repository convention
allows it, but unknown actions/status values are rejected.

## Web Domain API

Register a dedicated handler module at
`crates/allthecodes-web/src/handlers/workflows.rs` through the normal
`HandlerRegistry` and `ApiMethod` dispatcher.

| Operation | Route | Semantics |
|---|---|---|
| `WorkflowDefinitionsList` | `GET /api/workflows` | List valid project definitions and bounded parse errors |
| `WorkflowDefinitionDetail` | `GET /api/workflows/{workflow}` | Return one parsed definition; no arbitrary path input |
| `WorkflowRunsList` | `GET /api/workflow-runs` | List canonical runs for the active workspace, with status filtering and opaque cursor pagination |
| `WorkflowRunStart` | `POST /api/workflows/{workflow}/runs` | Start a run with optional args and a required idempotency key |
| `WorkflowRunStatus` | `GET /api/workflow-runs/{run_id}` | Purely read the canonical run record; do not update task state |
| `WorkflowRunAdvance` | `POST /api/workflow-runs/{run_id}/advance` | Apply completed/failed/cancelled to the active step |
| `WorkflowRunCancel` | `POST /api/workflow-runs/{run_id}/cancel` | Cancel a non-final run |

Definition `list` is the existing FileWorkflow `list` action. Run listing is a
separate required read operation: after a process restart, a client otherwise
cannot discover runs created by `WorkflowTool` or loaded from the canonical run
directory unless it already knows every run ID.

`GET /api/workflow-runs` is implicitly scoped to `WebState::engine().cwd()` and
accepts only a closed-enum `status`, `cursor`, and a bounded `limit` (default 50,
hard maximum 100). Return newest-updated first with `run_id` as the deterministic
tie-breaker. The cursor is opaque and bound to the workspace, filter, and
ordering; a cursor from another workspace or filter returns
`400 invalid_cursor`. Corrupt or non-regular run entries are isolated and
reported as a bounded diagnostic count rather than leaking paths or making every
valid run undiscoverable.

Use `spawn_blocking` for filesystem work. Map errors consistently:

- 400: invalid identifier, action, status, args, or malformed definition;
- 403: workspace/path rejection or an explicit permission/taint denial;
- 404: definition or run not found;
- 409: revision mismatch, conflicting idempotency key, terminal transition,
  concurrent mutation, or `interactive_approval_required` when policy returns
  `Ask` but this REST operation has no challenge/resume channel;
- 413: definition/args/response exceeds configured bounds;
- 500/503: durable store, lock, or task-projection failure.

## Path Isolation and Input Bounds

All handlers derive the workspace from `WebState::engine().cwd()`. A request
must not supply `cwd`, an absolute path, or a parent-relative path.

Required hardening around the existing store:

1. Canonicalize the project `.allthecodes/workflows` directory and candidate
   definition; reject symlinks or canonical paths outside that directory.
2. Accept only an exact workflow stem or file name with an allowed extension.
   Preserve the existing ambiguity error.
3. Require API run IDs to match the server-generated
   `workflow-run-<uuid>` form. Do not rely only on `sanitize_segment`, because
   different hostile strings can sanitize to the same file name.
4. Persist a stable workspace identity in new run records. After loading, verify
   the stored `record.run_id` and workspace identity match the request; migrate
   legacy records only after canonical containment proves they came from the
   active workspace's run directory.
5. For run listing, reject symlinks and non-regular files, validate every stored
   run ID/workspace identity, and bound scanned records, page size, diagnostics,
   and serialized response bytes.
6. Bound definition bytes, step count, prompt/run length, argument bytes, and
   response size before parsing or serialization.
7. Keep all persistent paths under project `.allthecodes/`; never read
   `.claude/workflows`, global `~/.allthecodes` definitions, or a sibling
   project's state.

## Permission and Taint Policy

The planned definition list/detail and run list/status operations must be
read-only, which requires removing the current task-store write from the status
load path.
Mutations must not bypass the policy currently expressed by `WorkflowTool`:

- `start`, `advance`, and `cancel` require the Web privileged capability in
  `crates/allthecodes-web/src/mod.rs::requires_privileged_capability`.
- Extract one shared workflow-action authorization/classification function so
  the tool and Web adapter cannot drift.
- Preserve the current `WorkflowTool::check_permissions` distinction: reads
  allow; mutations ask; unsupported actions deny.
- A privileged HTTP token authenticates the caller but is not a reusable exact
  approval. The first REST version has no live chat callback, pending sender,
  challenge event, or resume token with which to satisfy `Ask`; it must
  therefore fail closed with a stable `interactive_approval_required` response
  and perform no mutation. `Deny` also fails closed. Supporting `Ask` later
  requires a separately specified asynchronous challenge/resume contract;
  [plan 14](14-security-approval-api-parity-plan.md)'s active chat-SSE pending
  map cannot be reused implicitly.
- Treat Web-supplied `args` as untrusted provenance. Persist only bounded data
  and never interpolate it into a shell command in the handler.
- A definition's optional `run` value is descriptive/suggested input at this
  layer. Any later execution must re-enter the normal engine tool pipeline,
  setup/supply-chain scanners, taint policy, and exact one-shot approval.
- Audit mutation decisions with workspace, run ID, action, revision, and
  redacted rule IDs/digests; never log args, prompts, commands, or tokens.

`WorkflowTool` currently returns `Ask` for every `start`, `advance`, and
`cancel`, so the initial production REST adapter cannot truthfully advertise
those mutations as ready. Keep the typed routes/contracts so clients receive a
stable `interactive_approval_required` result, but report mutation capability as
unavailable until one of these is separately implemented and reviewed:

- a complete asynchronous challenge, bound response, expiry, and resume flow;
  or
- a shared policy decision that can return `Allow` for a specific authenticated
  non-interactive request without treating the privileged token itself as an
  approval.

Handler success tests may inject an explicit `Allow` policy fixture to verify
the adapter and state machine. Production acceptance must not use that fixture
or bypass the current `Ask` decision.

## State Machine

Use one validated transition function in the FileWorkflow service:

```text
start:
  definition -> running(first step)

advance(completed):
  running(step N) -> running(step N+1)
  running(last step) -> completed

advance(failed):
  running -> failed

advance(cancelled) / cancel:
  running -> cancelled

terminal states:
  completed | failed | cancelled
```

Invariants:

- exactly one step is active while a run is `running`;
- a terminal run has no current step;
- terminal runs cannot advance;
- cancelling an already-cancelled run with the same request ID is idempotent;
- cancelling completed/failed runs is a conflict;
- timestamps and revision change exactly once per accepted mutation;
- a task projection never gets ahead of the canonical run record.

The terminal cancel rules above are deliberate hardening. The current
`cancel_file_workflow` function rewrites even completed or failed runs to
`cancelled`; implementations must not describe the new conflict behavior as
retaining current semantics.

## Idempotency, Locking, and Atomic Writes

The current direct `fs::write` path is insufficient once tool, CLI, and Web
callers can mutate the same run concurrently.

Implementation requirements:

1. Add a workspace/run-scoped cross-process lock under the project workflow
   runtime directory. An in-process mutex alone is not sufficient.
2. Persist `revision: u64`, `last_request_id`, and a canonical request digest in
   `FileWorkflowRunRecord` with migration defaults for existing records.
3. Under the lock, reload the record, verify `expected_revision`, validate the
   transition, write a temporary file, fsync as appropriate, and atomically
   rename it.
4. Repeating the same `request_id` and request digest returns the prior result
   without applying the transition again.
5. Reusing a request ID with different input returns 409.
6. Two advances at one revision permit exactly one transition; the loser gets
   a revision conflict and the next step is not skipped.
7. Start idempotency is scoped to workspace plus definition. Store its request
   ID/digest in the canonical run record; do not create a separate Web
   idempotency database.

## Implementation Tasks

### Task 1: Stabilize the FileWorkflow service

- Expose typed definition/run methods from
  `crates/allthecodes-tools/src/workflow/`.
- Add workspace-scoped, status-filtered, cursor-paginated canonical run
  enumeration so persisted/tool-created runs remain discoverable after restart.
- Add strict identifiers, path containment, bounds, revisions, locks, atomic
  writes, and idempotency.
- Split pure run loading from explicit task-projection reconciliation.
- Reject terminal cancel transitions instead of rewriting completed/failed
  history.
- Keep the existing tool JSON adapter as a caller of the service.

### Task 2: Add protocol operations

- Add the v1 workflow DTO module and seven `ApiMethod` entries.
- Define serialization keys: workspace/definition for start and run ID for run
  mutations.
- Add schema and Serde round-trip coverage.

### Task 3: Add Web handlers

- Add the handler module, registry wiring, API RPC dispatch, and capability
  reporting if a new capability flag is needed.
- Bind to the active workspace and use privileged authorization for mutations.
- Report read support separately from mutation readiness; while policy returns
  `Ask` and no challenge/resume transport exists, do not advertise mutation
  support merely because the routes are registered.
- Map domain errors without leaking absolute paths or stored content.

### Task 4: Preserve task and agent integrations

- Continue updating `TASK_KIND_LOCAL_WORKFLOW` as a projection.
- Do not add generic task-output/stop routes.
- Keep WebSocket Agent/Team dispatch work and its integration tests in
  [plan 18](18-web-ipc-agent-command-parity-plan.md); workflow tests should only
  prove that this plan does not introduce duplicate task routes.

### Task 5: Regenerate contracts

Regenerate and compare:

```text
docs/api/routes.md
docs/api/schema.json
docs/api/openapi.json
allthecodes-web/src/lib/generated/api-types.ts
allthecodes-web/src/lib/generated/api-routes.ts
allthecodes-web/src/lib/generated/api-schema.json
```

## Required Tests

### FileWorkflow service

- Markdown/YAML list/detail and bounded parse errors.
- Run listing is workspace-scoped, status-filtered, cursor-paginated, stable
  across restart, and isolates corrupt/symlink entries.
- Runs created through `WorkflowTool` remain discoverable from a fresh Web
  state, while workspace A records, forged internal workspace identities, and
  stale/foreign cursors never appear in workspace B.
- Start/advance preserve valid current transitions; status becomes a pure load,
  and terminal cancel follows the hardened conflict semantics above.
- Definition traversal, symlink escape, invalid run ID, and sanitize collision
  are rejected.
- Existing run records migrate with a deterministic initial revision.
- Atomic-write failure leaves the previous record readable.
- Task projection matches the canonical record or reports reconciliation need.

### State and concurrency

- Repeated start with the same request ID returns one run.
- Reused request ID with changed args conflicts.
- Two parallel advances at one revision produce one success and one conflict.
- A retry after a lost response does not skip a step.
- Terminal-state and cancel idempotency rules hold.
- Tool and Web callers contend on the same cross-process lock and store.

### Security

- Read routes require the normal Web control token.
- Mutation routes additionally require the privileged token.
- Permission `Ask` returns `interactive_approval_required` without mutation;
  `Deny` is also fail-closed.
- An injected test policy that returns `Allow` can exercise successful REST
  mutations, but the production adapter does not replace `Ask` with `Allow`.
- Tainted args cannot become command input without the engine security
  pipeline and exact approval.
- Logs and responses omit absolute paths, raw args, tokens, and taint payloads.

### Protocol and Web

- DTO round trips and generated schemas cover every status and transition.
- Registry validation proves all seven operations have handlers.
- Route tests cover 400/403/404/409/413 and successful lifecycle behavior.
- API RPC and REST use the same DTO and service implementation.
- Route inventory proves no duplicate generic task output/abort operation was
  added; Web IPC behavior is verified by plan 18.

## Acceptance Criteria

- A Web client can list/detail definitions, discover persisted runs in the
  active workspace, and read run status through typed contracts.
  Start/advance/cancel are typed and fail closed with
  `interactive_approval_required` until a separately reviewed approval path can
  authorize them; only then may capability discovery mark remote mutations
  ready.
- The tool, Web, CLI/task projection, and future callers share one definition
  owner and one canonical run record.
- No request escapes project `.allthecodes` or introduces a Web-only store.
- Mutations are permission-aware, taint-safe, atomic, idempotent, and safe
  under concurrent callers.
- Generic agent output and abort remain on the existing IPC contract, are not
  duplicated as workflow/task endpoints, and their Web dispatch is provided by
  plan 18.
