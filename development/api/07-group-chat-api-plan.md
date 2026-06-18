# Group Chat API Backend Plan

## Scope

Implement group chat room, agent, message, compression, and stream APIs:

```http
GET    /api/group-chat/rooms
GET    /api/group-chat/rooms/:id
POST   /api/group-chat/rooms
POST   /api/group-chat/rooms/:id/clone
DELETE /api/group-chat/rooms/:id
GET    /api/group-chat/rooms/:id/invite
POST   /api/group-chat/rooms/:id/agents
PATCH  /api/group-chat/rooms/:roomId/agents/:agentId
DELETE /api/group-chat/rooms/:roomId/agents/:agentId
POST   /api/group-chat/rooms/:id/messages
POST   /api/group-chat/rooms/:id/context-compression
GET    /api/group-chat/rooms/:id/stream
```

Frontend contracts:

- `GroupChatRoomsResponse`
- `GroupChatRoomDetailResponse`
- `GroupChatRoomMutationResponse`
- `GroupChatAgentMutationResponse`
- `GroupChatMessageResponse`
- `GroupChatCompressionResponse`
- `GroupChatStreamEvent`

## Current Gap

`capabilities.rs` reports `group_chat=false`, and no group-chat routes are
registered. This is a large feature surface and should be phased.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/group_chat.rs
```

Register explicit routes before the API fallback.

## Phase 1: Durable Room State

Implement non-streaming CRUD first:

- List rooms.
- Get room detail.
- Create room.
- Clone room.
- Delete/archive room.
- Invite generation.
- Add/update/remove room agents.

Use a local store initially:

```text
ALLTHECODES_HOME/web/group-chat.json
```

Longer term, migrate to the backend services database.

## Phase 2: Messages

`POST /api/group-chat/rooms/:id/messages` should:

- Persist a user message.
- Validate target agents.
- Return a `GroupChatMessageResponse`.
- Optionally enqueue agent work if a group-chat runner exists.

Do not fake agent output as if it came from a model. If the runner is not wired,
return the user message and update agent statuses to `idle` with a warning.

## Phase 3: Stream

`GET /api/group-chat/rooms/:id/stream` should use SSE:

- Send initial `room_snapshot`.
- Send message/status/compression events.
- Keep heartbeat comments.
- Close cleanly on missing room.

## Phase 4: Context Compression

Implement actions:

- `enable`
- `disable`
- `compress`
- `reset`

MVP `compress` can create a local summary from existing messages. Model-backed
compression should be a follow-up with budget and cancellation handling.

## Tests

Add tests for:

- Room create/list/detail.
- Clone with and without history.
- Agent add/update/remove.
- Send message validates room and targets.
- Stream emits `room_snapshot`.
- Compression action changes state.

Run:

```bash
cargo test -p allthecodes-web group_chat
```

## Acceptance

- Group Chat page can create rooms, manage agents, and send a stored message.
- Stream endpoint returns valid SSE.
- Capability can be changed to `group_chat=true` only after Phase 1 and Phase 2
  are usable.
