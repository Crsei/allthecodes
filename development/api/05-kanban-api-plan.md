# Kanban API Backend Plan

## Scope

Implement Kanban board/task APIs:

```http
GET   /api/kanban/boards
GET   /api/kanban/boards/:id
POST  /api/kanban/tasks
PATCH /api/kanban/tasks/:id
POST  /api/kanban/tasks/:id/comments
```

Frontend contracts:

- `KanbanBoardsResponse`
- `KanbanBoardDetailResponse`
- `KanbanTaskCreateRequest`
- `KanbanTaskUpdateRequest`
- `KanbanTaskMutationResponse`

## Current Gap

`capabilities.rs` reports `kanban=false`, and no Kanban routes are registered.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/kanban.rs
```

Routes:

```rust
.route("/api/kanban/boards", get(handlers::kanban_boards_handler))
.route("/api/kanban/boards/{id}", get(handlers::kanban_board_detail_handler))
.route("/api/kanban/tasks", post(handlers::kanban_task_create_handler))
.route("/api/kanban/tasks/{id}", patch(handlers::kanban_task_update_handler))
.route(
    "/api/kanban/tasks/{id}/comments",
    post(handlers::kanban_task_comment_handler),
)
```

## Persistence

MVP can use a local JSON store under:

```text
ALLTHECODES_HOME/web/kanban.json
```

Longer term, move to the backend services database schema tracked by the
Backend Services API.

## Behavior

- Create a default board when none exists.
- Use default columns: backlog, todo, in_progress, review, blocked, done.
- Use optimistic concurrency with `revision`.
- Mutations return the changed task and board summary.
- Comments increment task revision.

## Tests

Add tests for:

- Empty store creates/returns default board.
- Create task returns revision 1.
- Update with stale revision returns `409 revision_conflict`.
- Add comment appends comment and increments revision.
- Profile id is echoed and can partition data later.

Run:

```bash
cargo test -p allthecodes-web kanban
```

## Acceptance

- Kanban page can list a board, create/update tasks, and add comments.
- Capability can be changed to `kanban=true` after MVP.
