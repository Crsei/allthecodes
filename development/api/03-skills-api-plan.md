# Skills API Backend Plan

## Scope

Implement Skills page and composer skill APIs:

```http
GET   /api/skills
GET   /api/skills/:id
GET   /api/skills/:id/files?path=...
PATCH /api/skills/:id
```

Frontend contracts:

- `SkillsListResponse`
- `SkillDetailResponse`
- `SkillFileResponse`
- `SkillUpdateRequest`
- `SkillUpdateResponse`

## Current Gap

`capabilities.rs` reports `skills=false`, and no `/api/skills` routes are
registered. The backend already has skill loading/runtime code; the web API
needs to expose it safely.

## Backend Module

Add:

```text
crates/allthecodes-web/src/handlers/skills.rs
```

Routes:

```rust
.route("/api/skills", get(handlers::skills_list_handler))
.route(
    "/api/skills/{id}",
    get(handlers::skills_detail_handler).patch(handlers::skills_update_handler),
)
.route("/api/skills/{id}/files", get(handlers::skills_file_handler))
```

Flip capability to `skills=true` after MVP route tests pass.

## Data Sources

Use existing skill infrastructure where possible:

- Bundled/system skills.
- User skills under configured allthecodes skills paths.
- Plugin-provided skills.
- Existing diagnostics from skill parsing/loading.

Do not duplicate skill discovery logic in the web crate. Prefer a service
function in the skills crate if the current API is not web-friendly.

## Behavior

`GET /api/skills`:

- Return summaries for every discovered skill.
- Include `categories` and usage stats when available.
- Include disabled/pinned flags from per-user state.

`GET /api/skills/:id`:

- Return summary, prompt body, files, and diagnostics.
- Return `404 skill_not_found` for unknown id.

`GET /api/skills/:id/files?path=...`:

- Resolve path within the skill root only.
- Return text content and media type.
- Cap max bytes and return `truncated=true` when needed.

`PATCH /api/skills/:id`:

- Persist only mutable UI flags: `enabled`, `pinned`.
- Return updated skill or refreshed list.

## Safety

- Reject file paths escaping the skill root.
- Do not expose hidden credential files from plugin directories.
- Do not allow arbitrary skill file writes in this API.

## Tests

Add tests for:

- Lists bundled/user skill summaries.
- Detail returns prompt body.
- File read rejects traversal.
- Patch persists enabled/pinned.
- Unknown skill returns `404`.

Run:

```bash
cargo test -p allthecodes-web skills
```

## Acceptance

- Skills route can be enabled in capabilities.
- Skills page and composer skill panels load without fallback 501.

