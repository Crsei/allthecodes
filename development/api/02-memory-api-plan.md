# Memory API Backend Plan

> Status reviewed: 2026-07-16
> Current result: routes exist, but the entry API is connected to a Web-only
> store instead of the runtime memory service.

## Scope

Keep the existing Memory page routes, migrate them to the canonical memory
service, and expose the newer dream-memory and approval-review projections.

Existing routes:

```http
GET   /api/memory?profile_id=...
PATCH /api/memory/:id
GET   /api/memory/config
PATCH /api/memory/config
```

Migrate the list contract to typed `scope=global|project|team|auto` filtering;
retain `profile_id` only for the compatibility window described below.

Required additions:

```http
GET  /api/memory/dream
GET  /api/memory/dream/:date
GET  /api/memory/proposals
GET  /api/memory/proposals/:id
POST /api/memory/proposals/:id/approve
POST /api/memory/proposals/:id/reject
```

Dream routes are read-only. Running dream distillation remains a daemon or
command operation; this plan does not create a second scheduler endpoint.

## Current Implementation

The base routes are registered and `capabilities.memory` is `true`, but their
data source is obsolete:

- `crates/allthecodes-web/src/handlers/memory.rs` defines a private
  `MemoryEntry` and persists one JSON array at
  `{data_root}/memory/entries.json`.
- The real runtime reads and writes `allthecodes_session::memdir`. It stores
  typed entries per key and scope, maintains `MEMORY.md`, and carries memory
  type, source-session, and approval provenance.
- The background-review schema and `/memory` command support curated
  `MemoryAdd`/`MemoryReplace` consumption through
  `memdir::write_curated_memory`, but the current built-in producer emits only
  `WorkflowWarning`. Those memory-mutation kinds are reserved consumer paths,
  not an active proposal source.
- `/memory pending|approve|reject` operates on the supported background-review
  kinds, including the actively produced workflow warnings, but no structured
  Web operation exposes that review queue.
- Kairos dream distillation writes Markdown to
  `{global_memory_dir}/dream/YYYY-MM-DD.md`; the current list endpoint never
  reads that directory.

Consequently, a successful `GET /api/memory` does not prove that the client is
seeing the memory that affects prompts and recall.

## Canonical Ownership

`allthecodes_session::memdir` must be the only owner of ordinary and curated
memory content. The Web handler should be a thin adapter over these operations:

- `list_memories`
- `read_memory`
- `write_memory` or a typed update operation added beside it
- `delete_memory` when deletion is exposed later
- `write_curated_memory` for approved proposals

Scope resolution must follow `MemoryScope`:

| API scope | Canonical location |
|---|---|
| `global` | `memory_dir_global()` |
| `project` | `{cwd}/.allthecodes/memory/` |
| `team` | `team_memory_dir(cwd)` |
| `auto` | `auto_memory_dir()` |

`profile_id` is not a memory scope and must not create another storage
partition. If it remains temporarily for frontend compatibility, document it
as deprecated and do not use it to select a different store.

## Entry Contract

Define typed request and response DTOs in `allthecodes-protocol`; the current
`Value` metadata is not sufficient for code generation or compatibility
checks. A canonical entry projection should include:

- stable opaque `id` encoding scope plus key;
- `scope`, `key`, `value`, `category`, and closed memory `type`;
- optional `description` and `search_terms`;
- optional `source_session_id` and `approval_id`;
- `created_at` and `updated_at`.

Do not synthesize legacy `tags` or `pinned` state in a Web sidecar. If those
fields remain required by the frontend, add them to the canonical memdir
schema and its index/migration logic first. `PATCH /api/memory/:id` must
preserve provenance and fields omitted by the request.

The list operation should support bounded filtering by scope, type, category,
and query. Project/team scope must use the server-selected workspace; clients
must not submit an arbitrary filesystem root.

## Legacy Store Migration

Add a one-time, idempotent migration from
`{data_root}/memory/entries.json` into memdir:

1. Parse and validate every legacy row before writing anything.
2. Select a deterministic key and scope for each row; record collisions rather
   than silently overwriting a newer canonical entry.
3. Preserve timestamps and source session where representable.
4. Refresh the affected memory indexes after the batch succeeds.
5. Keep a backup or migration marker, then stop reading and writing the legacy
   file.

During a bounded compatibility window the API may merge legacy rows as
read-only data, but every mutation must go through memdir. It must never
dual-write the two stores.

## Dream Memory API

`GET /api/memory/dream` returns a newest-first, paginated index of available
dates with bounded metadata such as byte size and a short preview.

`GET /api/memory/dream/:date` returns the selected Markdown plus normalized
metadata. Requirements:

- parse `date` strictly as `YYYY-MM-DD` and derive the path with
  `memory_path_for_date`; never join a raw route segment;
- reject invalid dates and directory traversal before filesystem access;
- cap response bytes and report truncation explicitly;
- do not expose an absolute source-log path or other host-only path in the
  public projection;
- return `404` for a valid date with no dream artifact.

The read API must not trigger distillation as a side effect.

## Background-Review Proposal API

Reuse `background_review::list_background_review_proposals` and
`load_background_review_proposal`. The Memory projection includes only
`MemoryAdd`, `MemoryReplace`, and `WorkflowWarning`, matching the `/memory`
command's supported consumer behavior. At this audit HEAD, background review
actively creates only `WorkflowWarning`; `MemoryAdd` and `MemoryReplace` remain
reserved variants with existing consumers and fixture/forward-compatibility
coverage. Do not report them as currently generated memory suggestions.

`SkillCreate` and `SkillPatch` belong exclusively to the
[Skills proposal plan](03-skills-api-plan.md). A proposal ID must resolve through
one domain only so two APIs cannot race to consume the same background-review
record.

List and detail responses should expose a redacted, bounded proposal DTO:

- `id`, `kind`, `summary`, `source_session_id`, and `created_at`;
- the proposed target/key/value for memory proposals;
- whether approval writes memory or only acknowledges a workflow warning;
- no raw host path, unbounded tool error, or unrelated proposal payload.

Approval must use the same semantics as `/memory approve`:

- `MemoryAdd` and `MemoryReplace` call `write_curated_memory` with the proposal
  id as `approval_id`;
- `WorkflowWarning` is acknowledged without writing memory;
- reject removes the pending proposal without changing memory.

Serialize mutations per proposal and atomically claim the pending record so
two clients cannot approve it twice. Repeated approve/reject calls return a
typed conflict or not-found result, never a second successful write.

## Safety

- Require the privileged mutation capability for update, approve, and reject.
- Enforce content, search-term, list-page, and response-size limits.
- Validate logical keys independently of their sanitized filenames.
- Keep project/global/team path isolation in `memdir`; do not accept raw paths.
- Redact proposal evidence before serialization and preserve approval
  provenance on every curated write.

## Tests

Add coverage for:

- Web list/update observes entries written directly through memdir.
- All four scopes resolve to the canonical directories.
- Legacy migration is idempotent and reports key conflicts.
- Update preserves type, timestamps, source session, and approval provenance.
- Dream list sorts dates and detail rejects invalid/traversal dates.
- Dream detail enforces size limits and hides host-only source paths.
- Proposal list excludes skill-only proposals.
- Approve writes one curated entry with `approval_id`; reject writes none.
- Active `WorkflowWarning` records are reviewable without writing memory;
  reserved `MemoryAdd`/`MemoryReplace` fixtures exercise the supported consumer
  path without implying a production generator exists.
- Concurrent or repeated decisions cannot apply the same proposal twice.
- Protocol metadata, handler registration, generated schema, and OpenAPI stay
  aligned.

Run targeted verification:

```bash
cargo test -p allthecodes-session memdir
cargo test -p allthecodes-services dream
cargo test -p allthecodes-commands memory
cargo test -p allthecodes-web memory
```

## Acceptance

- `GET /api/memory` reflects the same entries used by runtime prompt and recall
  code; `{data_root}/memory/entries.json` is no longer an active store.
- Entry mutations preserve canonical memdir indexes and provenance.
- Dream artifacts are available through bounded list/detail operations.
- Supported memory/background-review records have typed
  list/detail/approve/reject operations, and each proposal decision is
  single-use; capability text states that only workflow warnings are currently
  produced automatically.
- No API action creates a second memory, dream, or proposal state owner.
