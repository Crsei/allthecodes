# Group Chat API Backend Plan

> Status: Implemented on 2026-07-16
> Current result: invites have a safe read/write split, messages launch
> canonical delegated work, and typed long-lived SSE exposes bounded lifecycle
> events with replay/reset support.

## Implementation Result (2026-07-16)

Implemented in `0df0a8d7`, with SSE OpenAPI output included in `e5163791`:

- invite GET is pure; privileged POST owns revisioned, request-idempotent
  create/rotation;
- message dispatch validates the complete target set and launches one canonical
  delegated task/child session per enabled target, preserving partial success;
- task, child-session, output, completion, failure, and cancellation state are
  observed from the delegated-agent runtime rather than editable room fields;
- the per-room stream remains open, sends heartbeats, maintains bounded durable
  replay, and emits a reset/snapshot for stale cursors; and
- protocol/OpenAPI metadata uses the tagged `GroupChatStreamEnvelope` with
  `text/event-stream`, not an opaque JSON response.

The focused Group Chat Web suite passes 7/7. Context compression intentionally
remains the existing room-local summary and is not presented as canonical model
compaction. External invite join/expiry and a second cancellation endpoint were
not added; cancellation continues through the shared IPC agent command path.

## Scope

Keep the current Group Chat surface, correct the invite read/write boundary,
and connect it to the canonical delegated agent runtime:

```http
GET    /api/group-chat/rooms
GET    /api/group-chat/rooms/:id
POST   /api/group-chat/rooms
POST   /api/group-chat/rooms/:id/clone
DELETE /api/group-chat/rooms/:id
GET    /api/group-chat/rooms/:id/invite
POST   /api/group-chat/rooms/:id/invite
POST   /api/group-chat/rooms/:id/agents
PATCH  /api/group-chat/rooms/:roomId/agents/:agentId
DELETE /api/group-chat/rooms/:roomId/agents/:agentId
POST   /api/group-chat/rooms/:id/messages
POST   /api/group-chat/rooms/:id/context-compression
GET    /api/group-chat/rooms/:id/stream
```

Do not add a second message or agent-control namespace. Extend the existing
message response and SSE contract with task/run lifecycle identifiers and
events.

## Audit Snapshot Before Implementation

The routes are registered and `capabilities.group_chat` is `true`.
`crates/allthecodes-web/src/handlers/group_chat.rs` currently provides:

- durable room/member/agent/message state in
  `{data_root}/web/group-chat.json`;
- room list/detail/create/clone/delete operations;
- an invite GET that currently creates and persists a code when one is absent;
- agent add/update/delete operations;
- target-id validation when a user message is stored;
- local context-compression state and a deterministic text summary;
- an SSE response containing one `room_snapshot` event and one heartbeat.

The implementation does not run agents:

- a submitted message is persisted with `status="sent"` and `run_id=None`;
- agent status fields are editable JSON values rather than observed runtime
  state;
- no task, child session, supervisor handle, output, failure, or completion is
  linked to the room;
- the SSE source is a finite `stream::iter`, so it closes after the initial
  snapshot/heartbeat and cannot deliver later changes.

The API therefore supports durable room administration, not a functioning
multi-agent chat.

## Invite Contract

The current `GET /api/group-chat/rooms/:id/invite` is not a read: it acquires
the write store, creates an invite code when absent, updates the room, and
persists the file. Group Chat routes are not in the Web privileged-capability
classifier, so that state change currently requires only the normal control
token. Do not preserve this unsafe GET side effect as compatibility behavior.

Split the contract as follows:

- `GET /api/group-chat/rooms/:id/invite` performs a pure read of an existing
  invite and returns `404 invite_not_found` when none exists.
- privileged `POST /api/group-chat/rooms/:id/invite` creates the first invite
  or rotates it when explicitly requested. Its body includes `request_id`,
  `expected_revision`, and `rotate`; it returns the committed room revision and
  bounded invite projection.
- repeating the same request ID and body returns the same result, reusing a
  request ID with different input conflicts, and a stale room revision returns
  `409 revision_conflict`.
- a create request with `rotate=false` may return the already-existing invite
  without changing its code or revision; rotation is never implicit.

Ship the frontend caller update with the server change. During a documented
compatibility window, the GET URL and successful response shape remain stable
for existing invites, but an absent invite returns a typed
`invite_creation_required`/404 result directing capable clients to POST. Never
retain dual read-and-create semantics or silently rotate a code.

## Runtime Ownership

Reuse the lifecycle already implemented for `DelegateTask`:

- a persisted `TaskEntry` with bounded output events;
- a child session and parent/child lineage;
- the canonical Agent runtime executor and background supervisor;
- runtime cancellation and terminal status propagation;
- agent runtime history/audit records.

Extract or inject a shared delegation service into `WebState`; do not invoke
`DelegateTask` by posting synthetic chat text and do not implement another
process supervisor inside `group_chat.rs`.

The group-chat JSON file remains the owner of room configuration and displayed
messages. `TaskStore`, the Agent supervisor, and runtime history remain the
owners of execution state. Room records persist only stable links such as
`task_id`, `run_id`, `child_session_id`, and `agent_id`.

Observed fields (`running`, `typing`, terminal status, current task) must be
derived from those owners. Agent PATCH may change desired configuration such
as name, role, model, or enabled state, but must not forge observed runtime
status.

## Message Dispatch Contract

Extend `GroupChatMessageResponse` with a typed dispatch projection. Because one
message may target multiple agents, use an array rather than a single ambiguous
task id:

```text
dispatches[] = {
  agent_id,
  task_id,
  run_id,
  child_session_id,
  status,
  error?
}
```

The message-level `run_id` may remain as a correlation id for compatibility,
but it must not be used as the identity of every child task.

`POST /messages` should:

1. Validate the room, revision/profile scope, non-empty text, target agents,
   enabled state, role/model, and concurrency limits.
2. Persist the user message with a `dispatching` status and stable correlation
   id before launching work.
3. Build a bounded prompt from the room message context and the target agent's
   declared role. Do not place hidden credentials or unrelated room data in the
   child prompt.
4. Create one canonical delegated task per target, using a durable room
   coordinator/parent session id and the Agent runtime executor.
5. Persist returned task, run, child-session, and supervisor identities.
6. Return accepted dispatches immediately; agent execution remains
   asynchronous.
7. Preserve the user message if one target fails to launch and report partial
   success per dispatch instead of rolling back successful launches.

The first implementation may use one child session per target/message. Reusing
a long-lived child session for conversational continuity requires an explicit
resume/ownership design and must not be inferred from a display-only agent id.

Cancellation continues through the canonical agent cancellation path:
`AgentCommand::AbortAgent` dispatched by the shared IPC agent handler. There is
no generic `TaskStop` protocol operation to duplicate. The Web IPC wiring is
now implemented by [plan 18](18-web-ipc-agent-command-parity-plan.md); Group
Chat observes and streams that canonical transition rather than adding a
second cancellation owner.

## Live SSE Contract

Keep `GET /rooms/:id/stream`, but replace the finite iterator with a per-room
event broker plus a durable replay projection. On connection:

1. Validate access and send a complete `room_snapshot`.
2. Subscribe before or atomically with the snapshot so no event is lost in the
   handoff.
3. Forward room mutations and canonical task/runtime transitions.
4. Send heartbeat comments while the subscription remains open.
5. Stop promptly on disconnect and release subscriptions.

Required event kinds:

- `room_snapshot`
- `message_created`
- `agent_started`
- `agent_output`
- `agent_completed`
- `agent_failed`
- `agent_cancelled`
- `compression_updated`

Model these as a public, tagged `GroupChatStreamEvent` union in
`allthecodes-protocol`, with typed payload DTOs for the snapshot, message,
agent lifecycle, output chunk, compression update, and replay reset. Wrap data
events in an envelope containing the monotonic event id, room id/revision, and
typed event. Heartbeats remain SSE comments rather than fake domain events.

The stream operation metadata must declare `text/event-stream` and reference
the event/envelope schema. Every payload derives the repository's schema traits,
and route/schema/OpenAPI/TypeScript generation must traverse the stream event
type instead of retaining the current opaque `serde_json::Value` response.

Give data events monotonic ids and support `Last-Event-ID` or an explicit
cursor. If a cursor is older than retained events, send a fresh snapshot and a
reset marker. Output events must reuse the task store's bounded, sequenced
chunks; do not copy unbounded logs into the room JSON file.

## Context Compression Boundary

The current `compress` action estimates tokens from character count and creates
a local excerpt. Keep it labelled as a local summary until a real compaction
service is connected. It must not be reported as model-backed context
compression or mutate delegated child-session transcripts.

When real compression is added, represent it as a trackable task and emit its
actual lifecycle over the same room stream.

## Protocol and Safety

- Replace `Value` metadata for Group Chat operations, including invite and SSE
  payloads, with typed protocol DTOs and stream-event metadata.
- Require the privileged mutation capability for invite creation/rotation,
  message dispatch, and agent configuration changes. A pure invite GET performs
  no write under a read token.
- Apply the same permission, taint, workspace, model, and tool policy as a
  normal delegated Agent launch.
- Never accept an arbitrary child-session id, supervisor id, worktree path, or
  host cwd from a room client.
- Enforce per-room and per-profile concurrency/cost limits.
- Redact tool input, secrets, host paths, and oversized output from SSE events.
- Use idempotency via `client_message_id` plus target agent so retries do not
  spawn duplicate tasks.

## Tests

Add coverage for:

- Existing room CRUD, clone, and agent configuration paths remain compatible.
- Invite GET never creates or rotates state; privileged POST creation/rotation
  enforces revision checks and request idempotency.
- Message dispatch creates one canonical task/child session per enabled target
  and returns their identities.
- Unknown, disabled, or over-limit targets fail before launch.
- Duplicate `client_message_id` retries do not launch duplicate agents.
- Partial launch failure preserves successful dispatches and records the
  failed target truthfully.
- Agent output and completed/failed/cancelled states update room projections.
- SSE remains open, emits ordered lifecycle events, and sends heartbeats.
- SSE reconnect resumes after a cursor or explicitly resets to a snapshot.
- Generated protocol metadata exposes the tagged SSE event union and
  `text/event-stream`, with no opaque `Value` fallback.
- Slow/disconnected clients cannot block the supervisor or grow memory without
  bounds.
- Permission denial and taint policy failure are visible as failed dispatches
  without leaking sensitive evidence.

Run targeted verification:

```bash
cargo test -p allthecodes-tasks
cargo test -p allthecodes-engine agent
cargo test -p allthecodes-tools delegate_task
cargo test -p allthecodes-web group_chat
```

## Acceptance

- The existing Group Chat CRUD and SSE URLs remain stable; invite GET becomes
  pure and the explicit privileged POST owns create/rotate mutations.
- Sending a message to an enabled agent launches canonical delegated work and
  returns trackable task/run/child-session identities.
- Agent output and all terminal states reach clients over a long-lived,
  reconnectable SSE stream.
- Cancellation and failure are sourced from the canonical task/supervisor
  lifecycle, not manually edited room status.
- Room persistence contains links and messages, not a duplicate task runtime.
