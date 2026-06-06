# Skills API Backend Status

## Scope

Skills page and composer skill APIs:

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

Current Rust request/response names use `SkillPatchRequest` and
`SkillPatchResponse` for the PATCH endpoint.

## Current Status

Implemented in the current backend worktree.

- Handler module: `crates/allthecodes-web/src/handlers/skills.rs`
- Router registration: `crates/allthecodes-web/src/mod.rs`
- Capability flag: `skills=true` in
  `crates/allthecodes-web/src/handlers/capabilities.rs`
- Tests: skills handler tests in `crates/allthecodes-web/src/handlers/mod.rs`

The implementation is intentionally not split into a standalone `skills-api`
crate/service. `allthecodes-web` exposes the REST endpoints, while skill
discovery/runtime state stays in `allthecodes-skills`.

## Backend Module

Implemented:

```text
crates/allthecodes-web/src/handlers/skills.rs
```

Routes:

```rust
.route("/api/skills", get(handlers::skills_list_handler))
.route(
    "/api/skills/{id}",
    get(handlers::skills_detail_handler).patch(handlers::skills_patch_handler),
)
.route("/api/skills/{id}/files", get(handlers::skills_files_handler))
```

## Data Sources

Uses existing skill infrastructure:

- Bundled/system skills.
- User skills under configured allthecodes skills paths.
- Plugin-provided skills.
- Existing diagnostics from skill parsing/loading.

The web crate calls `allthecodes-skills` registry functions such as
`get_all_skills`, `find_skill`, `get_skill_diagnostics`, and
`registry_revision`; it does not duplicate skill discovery logic.

## Behavior

`GET /api/skills`:

- Return summaries for every discovered skill.
- Include source, display name, description, invocation flags, context, version,
  and disabled/pinned UI flags.
- Include registry diagnostics, registry revision, and echoed `profile_id` when
  supplied.

`GET /api/skills/:id`:

- Return source, base directory, frontmatter, prompt body, and disabled/pinned
  UI flags.
- Return `404` with `code=not_found` for unknown id.

`GET /api/skills/:id/files?path=...`:

- Resolve path within the skill root only.
- Return text content and media type.
- Cap max bytes and return `truncated=true` when needed.

`PATCH /api/skills/:id`:

- Persist only mutable UI flags: `enabled`, `pinned`.
- Return updated `name`, `enabled`, and `pinned`.

## Safety

- Reject file paths escaping the skill root.
- Do not expose hidden credential files from plugin directories.
- Do not allow arbitrary skill file writes in this API.

## Tests

Covered by tests for:

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

- Skills route is enabled in capabilities.
- Skills page and composer skill panels should load without fallback 501.
- Keep this integrated as `allthecodes-web` + `allthecodes-skills`; do not
  create a separate `skills-api`.
