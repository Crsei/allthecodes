# Memory API Backend Plan

## Scope

Implement the Memory page data APIs:

```http
GET   /api/memory?profile_id=...
PATCH /api/memory/:id
```

Existing routes only cover memory configuration:

```http
GET   /api/memory/config
PATCH /api/memory/config
```

Frontend contracts:

- `MemoryListResponse`
- `MemoryUpdateRequest`
- `MemoryUpdateResponse`

## Current Gap

`capabilities.rs` reports `memory=true`, but the actual list/update memory
entry routes are missing. Users can reach the Memory page and hit fallback 501.

## Backend Module

Either extend `settings_phase1.rs` only for config and create a new domain
module for data:

```text
crates/allthecodes-web/src/handlers/memory.rs
```

Routes:

```rust
.route("/api/memory", get(handlers::memory_list_handler))
.route("/api/memory/{id}", patch(handlers::memory_update_handler))
```

## Data Model

MVP `MemoryEntry` fields:

- `id`
- `timestamp`
- `session_id`
- `workspace`
- `content`
- `tags`
- `pinned`
- `updated_at`

Persist under an allthecodes-owned path, for example:

```text
ALLTHECODES_HOME/memory/entries.json
```

If a deeper memory service already exists, wrap it instead of creating a second
store. The web handler should remain a thin adapter.

## Behavior

`GET /api/memory`:

- Return entries sorted newest first.
- Filter by `profile_id` if the backing store supports it.
- Return an empty list when memory has no records.

`PATCH /api/memory/:id`:

- Update `content`, `tags`, and `pinned`.
- Preserve existing `timestamp`, `session_id`, and `workspace`.
- Return `404 memory_not_found` for unknown id.

## Safety

- Enforce max content length.
- Normalize tags: trim, dedupe, cap count.
- Do not allow path traversal through ids; ids are logical keys only.

## Tests

Add tests for:

- Empty list returns `200`.
- Updating an entry persists content/tags/pinned.
- Unknown id returns `404`.
- Invalid content/tags return `400`.
- Capability/route registration alignment.

Run:

```bash
cargo test -p allthecodes-web memory
```

## Acceptance

- `GET /api/memory` returns `MemoryListResponse`.
- `PATCH /api/memory/:id` returns updated `MemoryEntry`.
- Memory page no longer falls through to the API fallback.

