# Web IPC Agent and Team Command Parity Plan

> Status: Implemented on 2026-07-16; final PTY integration gate remains pending
> Priority: P1
> Audit date: 2026-07-16
> Audit HEAD: `6d426dc9` (frozen snapshot)
> Related runtime updates: `f4b234c9`, `2514ceaa`, `ca4b95db`

## Implementation Result (2026-07-16)

Implemented in `f6e1b143` without adding duplicate REST task/agent/team
operations:

- `/api/ipc/ws` dispatches the existing Agent and Team commands through the
  shared authorized handler instead of returning debug `SystemInfo` text;
- the server-resolved session/workspace binds every command, and unknown or
  cross-scope targets fail closed;
- read results and failures use a requester-only direct lane that is not
  broadcast, persisted, replayed, or filtered by the hub replay high-watermark;
- successful mutations publish only after a real authorized state change; and
- agent output, tree/team projections, and team message input are bounded and
  display-redacted.

Shared dispatcher and Web delivery tests cover scope checks, one-shot mutation,
direct delivery, replay isolation, bounds, and redaction. A fresh final result
for the repository's PTY `commands` integration gate is still required before
claiming that specific end-to-end verification.

## Audit Snapshot Before Implementation

The delegated-agent updates already added a typed IPC command and event
contract plus working runtime handlers. The missing API work is confined to the
Web projection of that existing contract.

| Surface | Current behavior |
|---|---|
| Shared request DTOs | `FrontendMessage::AgentCommand` and `FrontendMessage::TeamCommand` carry the typed commands from `allthecodes-types` |
| Shared response DTOs | `BackendMessage::AgentEvent` and `BackendMessage::TeamEvent` carry tree, output, lifecycle, message, and team-status events |
| Headless ingress | `crates/allthecodes/src/app_runtime_adapters/ingress.rs` dispatches both command families to `allthecodes_ipc::agent_handlers` |
| Web IPC | `crates/allthecodes-web/src/ws/ipc.rs` accepts both command families, but responds with debug `SystemInfo` text and never invokes a runtime handler |
| WebSocket authentication | `/api/ipc/ws` already requires the control token, the privileged Web capability, and accepted host/origin checks before upgrade |

The current shared agent handler implements active-agent listing, incremental
output, and cancellation. It also implements team message injection and team
status. Therefore this is not a reason to add another REST task API. It is a
dispatch, authorization, delivery, and output-bounding defect in the existing
`/api/ipc/ws` API.

There are two adjacent correctness problems that must be fixed as part of the
Web wiring:

1. A supplied, unknown `session_id` currently falls back to the default engine
   while the WebSocket hub retains the supplied session ID. That can bind one
   connection to mismatched engine and hub scopes.
2. `QueryAgentOutput.limit_bytes` defaults to 64 KiB but has no hard maximum,
   and the legacy fallback can return the full retained output in a
   `SystemInfo` message. Neither path is safe for a remotely supplied WebSocket
   command.
3. The current targeted `send_control_to` helper stamps direct messages with
   `latest_seq()`, while the WebSocket writer discards sequence values at or
   below its replay high-watermark. A quiet connection's first direct response
   can therefore disappear unless direct delivery gets an independent lane.

## Goals

1. Make the existing Agent and Team commands work through `/api/ipc/ws` with
   the same runtime behavior as headless IPC.
2. Bind every command to the authenticated Web connection's actual session and
   workspace.
3. Authorize the target agent or team before reads and before any mutation.
4. Deliver query results only to the requesting connection.
5. Keep output bounded, incremental, resumable, and free of raw runtime
   metadata or host paths.
6. Return truthful errors instead of debug echoes or synthetic success events.
7. Preserve existing headless behavior and the serialized IPC DTO shapes.

## Non-Goals

- Do not add generic REST `TaskOutput`, `TaskStop`, agent, or team routes.
- Do not duplicate task or supervisor logic in `allthecodes-web`.
- Do not add Web-only `AgentCommand`, `TeamCommand`, `AgentEvent`, or
  `TeamEvent` variants.
- Do not expose arbitrary process output, task-store metadata, worktree paths,
  live-context paths, credentials, or environment values.
- Do not turn team message injection into an unrestricted cross-team mailbox
  API.
- Do not use a successful WebSocket upgrade as sufficient authorization for an
  arbitrary agent or team identifier.

## Existing Wire Contract

Keep these command variants unchanged:

| Command | Class | Result family |
|---|---|---|
| `AgentCommand::QueryActiveAgents` | Read | `AgentEvent::TreeSnapshot` |
| `AgentCommand::QueryAgentOutput` | Read | `AgentEvent::OutputBatch` |
| `AgentCommand::AbortAgent` | Mutation | `AgentEvent::Aborted` plus an updated tree snapshot |
| `TeamCommand::QueryTeamStatus` | Read | `TeamEvent::StatusSnapshot` |
| `TeamCommand::InjectMessage` | Mutation | `TeamEvent::MessageRouted` |

The outer messages remain `FrontendMessage::AgentCommand`,
`FrontendMessage::TeamCommand`, `BackendMessage::AgentEvent`, and
`BackendMessage::TeamEvent`. Invalid or rejected commands use the existing
`BackendMessage::Error { message, recoverable }` envelope. They must not be
reported as warning or success-flavored `SystemInfo` text.

Add serialization regression tests for every existing variant, but do not
regenerate REST/OpenAPI artifacts solely for this fix: the wire already exists
and is not a new protocol operation.

## Connection, Session, and Workspace Binding

Create an immutable command context when the WebSocket is upgraded. It contains
the server-resolved connection ID, actual session ID, canonical workspace
scope, verified privileged capability, and the session hub.

Binding rules:

1. If `session_id` is omitted, resolve it from the selected engine and use that
   exact value for both the engine and `IpcSessionHub`.
2. If `session_id` is supplied, require an exact authorized engine/session
   match. An unknown, empty, or inaccessible value must not fall back to the
   foreground engine.
3. Reject authentication and privileged-capability failures before upgrade
   with the existing HTTP `401`/`403` behavior. Reject an unknown explicit
   session before upgrade without revealing another workspace's session
   details.
4. After upgrade, report command validation, ownership, availability, and
   conflict failures to that connection as `BackendMessage::Error`.
5. Derive workspace identity server-side from the connection-bound engine and
   canonical project scope. Never accept a workspace or owner field from an
   Agent/Team command.

The current process-global agent host and supervisor indexes are not an
authorization boundary. Record or expose authoritative ownership alongside an
agent registration: parent session, canonical workspace scope, task-list/task
reference, and lifecycle state. Team lookup must likewise resolve the team's
canonical workspace owner and membership from server-side state. If ownership
cannot be proved, fail closed.

Cross-session and cross-workspace targets should use the same public
not-found/unauthorized result as an unknown target so the API does not become
an identifier oracle. Internal logs may retain a bounded reason and connection
ID, but must not log output, injected message bodies, credentials, or complete
task metadata.

## Command Authorization Policy

| Command | Required checks | Side effect and delivery |
|---|---|---|
| `QueryActiveAgents` | Authenticated connection scope; enumerate only agents owned by the bound session/workspace | Send a filtered, redacted snapshot only to the requester |
| `QueryAgentOutput` | Agent exists, belongs to the bound scope, and its task reference resolves to that same owner | Send one bounded batch only to the requester |
| `AbortAgent` | Privileged mutation capability, matching owner, active cancellable target, and no conflicting terminal transition | Cancel once; publish canonical lifecycle changes only after cancellation is accepted |
| `QueryTeamStatus` | Team belongs to the bound workspace and the connection is authorized to observe it | Send the bounded status snapshot only to the requester |
| `InjectMessage` | Privileged mutation capability, matching team scope, valid active recipient, bounded text, and an authorized sender identity | Write once; publish a bounded canonical routed-message event only after the write succeeds |

`QueryActiveAgents` must not return the process-global tree and filter it only
after serialization. The runtime lookup itself must be scoped so a missed
projection cannot expose another session's nodes.

For team injection, replace the hard-coded trust implied by
`from = "__frontend__"` with an audited server identity derived from the
authorized Web connection. The client must not be able to choose or impersonate
the sender. Apply a dedicated message-size limit before touching the mailbox.

## Shared Dispatch Boundary

Extract an owner-aware dispatcher around the existing logic in
`crates/allthecodes-ipc/src/agent_handlers.rs`. The dispatcher should accept a
trusted command context and return a typed outcome that separates direct query
responses from canonical mutation events, for example:

```text
dispatch_agent_command(context, command)
  -> Result<CommandDispatch, AgentCommandError>

dispatch_team_command(context, command)
  -> Result<CommandDispatch, TeamCommandError>

CommandDispatch
  direct: messages visible only to the requester
  publish: canonical events visible to the authorized session hub
```

The exact Rust type may differ, but the separation must be explicit. Do not
call a handler that returns an undifferentiated `Vec<BackendMessage>` and then
guess in `ws/ipc.rs` which messages are safe to broadcast.

The Web adapter supplies a scoped context and routes `direct` messages through
a new per-connection control lane that carries an unsequenced direct envelope.
Do not reuse the current `IpcSessionHub::send_control_to` implementation as-is:
it wraps the message with `latest_seq()`, while the writer drops any outbound
event at or below its replay high-watermark. On a quiet hub that can discard the
first direct response completely.

Model the writer input as distinct replayable and direct variants (or an
equivalent separate receiver). Replayable events retain durable session
sequence markers; direct control messages bypass replay filtering, do not emit
a session sequence marker, and never enter the event log, replay buffer, or
session-wide broadcast. This is especially important for agent output and team
status when two browser clients share one session hub.

Successful mutation lifecycle events may be published to the matching
session-scoped hub after the runtime mutation commits. Failures and validation
errors remain direct. The requester's connection receives a published mutation
through normal hub fanout; do not send a duplicate direct success event.

Headless ingress should use the same dispatcher with an explicit trusted-local
context and its existing sink. This preserves one runtime implementation while
allowing the Web transport to enforce stronger object scoping and delivery
rules.

## Output Bounds and Redaction

For Web `QueryAgentOutput`:

- Keep the existing default of 64 KiB and define 64 KiB as the hard Web maximum
  unless a lower shared response budget applies.
- Reject `limit_bytes = 0` and values above the maximum with a stable,
  recoverable error; do not silently accept an unbounded request.
- Pass the validated limit to `agent_output_batch` and enforce the same maximum
  again on the serialized projection.
- Preserve `after_seq`. Returned events must all follow the requested cursor,
  and `next_seq`, `first_available_seq`, and `truncated` must truthfully report
  retention loss and byte-limit truncation.
- Add pagination tests that consume several batches without gaps or duplicate
  chunks. Document that `after_seq` is the last consumed event sequence and how
  the response cursor is used for the next request.
- Ensure a single retained output event cannot exceed the response limit. Split
  oversized output into UTF-8-safe retained events before assigning sequence
  numbers, or reject a legacy oversized event without serializing it; never let
  the first event bypass the hard byte cap.
- Never use the legacy `agent_output()` fallback that formats the entire output
  into `SystemInfo` for a Web request. If incremental output is unavailable,
  return a bounded typed error.

Project `AgentEvent::OutputBatch` and `AgentEvent::TreeSnapshot` deliberately.
Keep the task ID, event cursor, stream, bounded chunk text, lifecycle state, and
display-safe fork context mode. Remove raw task metadata and
`ForkLaunchMetadata.live_channel`, because it can contain host paths. Apply the
same path redaction to nested tree nodes.

Bound every returned string and collection, including agent descriptions,
result previews, team members, and routed message text. If a valid scoped
snapshot exceeds its response budget, return a deterministic truncated
projection or a typed bounded error; never emit an oversized WebSocket frame.

## Truthful Errors and Mutation Ordering

Define stable public error classes in the existing `BackendMessage::Error`
message contract, including invalid command input, target unavailable,
unauthorized target, invalid output limit, output unavailable, target already
terminal, mutation conflict, and runtime unavailable. Error text must be
bounded and must not contain host paths or raw backend errors.

Cancellation ordering is mandatory:

1. Resolve and authorize the target.
2. Confirm it is active and cancellable.
3. Ask the supervisor/task store to transition it once.
4. Only after an accepted transition, update agent state and publish
   `AgentEvent::Aborted` plus the scoped tree change.

The current handler publishes `Aborted` even when there is no runtime host or
`cancel_agent` returns no task. Remove that synthetic success. Unknown,
completed, already-aborted, and unavailable targets return a direct error and
must not publish an abort event or mutate the tree.

Team injection follows the same rule: validate owner, sender, recipient, text
budget, and mailbox availability first; publish `MessageRouted` only after the
mailbox write succeeds. A failure must not echo the rejected message to other
connections.

## Implementation Tasks

### Task 1: Fix WebSocket scope selection

- Update `crates/allthecodes-web/src/ws/ipc.rs` so engine, actual session, and
  hub selection cannot diverge.
- Build the immutable connection command context from server-resolved state.
- Add upgrade tests for omitted, exact, unknown, empty, and inaccessible
  session IDs.

### Task 2: Add authoritative runtime ownership

- Extend agent registration/supervisor lookup under
  `crates/allthecodes-engine/src/agent/` with parent-session and workspace
  ownership needed for scoped read/cancel operations.
- Extend `AgentRuntimeHost` in
  `crates/allthecodes-ipc/src/agent_handlers.rs` and its adapter in
  `crates/allthecodes/src/app_runtime_adapters/mod.rs` with owner-aware lookup
  methods rather than returning process-global results.
- Add the equivalent workspace/team authorization lookup around
  `allthecodes-teams`; do not infer ownership from a client-supplied team name.

### Task 3: Extract the shared authorized dispatcher

- Refactor `crates/allthecodes-ipc/src/agent_handlers.rs` to validate, dispatch,
  and return explicit direct/publish outcomes.
- Preserve the existing Agent/Team command and event enums.
- Make failed cancellation and failed team injection return errors without
  lifecycle success events.
- Adapt headless ingress to the shared dispatcher and retain its local trusted
  behavior.

### Task 4: Wire Web commands and delivery

- Replace the debug branches in `crates/allthecodes-web/src/ws/ipc.rs` with the
  shared dispatcher.
- Route query responses and all errors through
  the new per-connection direct control lane.
- Keep direct envelopes separate from replay sequence/high-watermark handling;
  do not solve the drop by inventing non-persisted session sequence numbers.
- Publish only authorized, committed mutation events through the matching
  session hub.
- Keep query responses out of `crates/allthecodes-web/src/ipc_streams.rs`
  replay and broadcast state.

### Task 5: Enforce bounded projections

- Add the hard agent-output limit and eliminate the Web legacy full-output
  fallback.
- Make retained output event sizing and cursor semantics safe for repeated
  bounded reads.
- Redact fork live-channel paths and any raw task metadata from Web events.
- Bound team message input and all command response collections and strings.

## Required Tests

### Protocol and shared dispatcher

- Existing Agent/Team command JSON still deserializes unchanged.
- Existing Agent/Team event JSON still serializes unchanged.
- Read commands return direct outcomes; successful mutations return canonical
  publish outcomes.
- Missing runtime host, unknown target, failed cancellation, and completed
  target never produce `AgentEvent::Aborted`.
- Failed team writes never produce `TeamEvent::MessageRouted`.
- The headless command path retains active listing, incremental output,
  cancellation, team status, and team injection behavior.

### Web authorization and isolation

- An unknown supplied session ID cannot fall back to the default engine or
  create a mismatched hub binding.
- A connection can read only agents and teams owned by its bound
  session/workspace.
- Cross-session, cross-workspace, and cross-team reads and mutations fail
  closed without revealing whether the identifier exists.
- Authorized abort and team injection mutate exactly once.
- Unauthorized abort and team injection have no side effect and publish no
  lifecycle event.

### Delivery and replay

- With two concurrent Web connections on one hub, an agent-output response is
  received only by the requester.
- On an empty/quiet hub, the first direct query response is delivered even when
  replay high-watermark equals `latest_seq()`; it has no replay marker and is
  not available to a later connection.
- Active-tree and team-status query responses are not broadcast or replayed to
  a later connection.
- A successful scoped abort publishes one canonical abort/tree update to the
  authorized session hub.
- A failed mutation is visible only to its requester.

### Bounds and redaction

- Default, valid custom, zero, and over-maximum `limit_bytes` cases are
  deterministic.
- Multiple `after_seq` reads have no gaps or duplicate output events and report
  retention truncation truthfully.
- A single oversized retained chunk cannot bypass the hard output limit.
- No output response contains raw task metadata, worktree/live-context paths,
  secrets, or more than the configured byte budget.
- Tree snapshots and team status enforce collection and string bounds.
- Oversized team messages are rejected before mailbox mutation.

Run at least the narrow gates after implementation:

```bash
cargo test -p allthecodes-types agent_events
cargo test -p allthecodes-ipc agent_handlers
cargo test -p allthecodes-web ipc
cargo test -p allthecodes --test pty_tui_e2e commands
```

Use the repository-configured Cargo environment and add more targeted package
tests if ownership or output-retention code moves into another crate.

## Acceptance Criteria

- `/api/ipc/ws` executes every existing Agent and Team command instead of
  returning a debug `SystemInfo` echo.
- No duplicate REST task-output, task-stop, agent, or team API is added.
- Every command is authorized against the connection's actual session and
  workspace before runtime access.
- Agent output and read snapshots are bounded, redacted, requester-only, and
  absent from replay.
- Cancellation and team mutation publish success only after a real authorized
  state change.
- Two concurrent connections cannot observe each other's direct query results.
- IPC DTO serialization and headless command behavior remain compatible.
