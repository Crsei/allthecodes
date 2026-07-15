# Skills API Backend Status and Proposal Parity Plan

> Status: Implemented on 2026-07-16; extended failure-injection validation remains pending
> Current result: existing Skills operations and five typed proposal operations
> now share the canonical native/background-review proposal services.

## Implementation Result (2026-07-16)

Implemented in `6e3938cd` and included in the backend artifacts refreshed by
`f9dc6d76`:

- list/detail/diff/approve/reject are registered as typed REST and API-RPC
  operations;
- native user/project proposals and reserved background `SkillCreate`/
  `SkillPatch` records are adapted without creating a third proposal store;
- proposal IDs are namespace-qualified, mutations are workspace/scope-bound,
  digest-checked, idempotent, and single-consumption; and
- `WorkflowWarning` and Memory proposal kinds remain invisible to the Skills
  API.

Active production remains narrower than the consumer contract: `/learn`
currently stages native project-scoped `create` proposals. Native `patch` and
background skill records are supported when present but have no current
built-in producer. The narrow package tests pass; the replacement-failure and
replaced-symlink race cases listed later in this plan do not yet have separate
failure-injection evidence and should not be reported as independently
verified.

## Scope

Keep the implemented Skills page and composer routes:

```http
GET   /api/skills
GET   /api/skills/:id
GET   /api/skills/:id/files?path=...
PATCH /api/skills/:id
```

Add the missing proposal review surface:

```http
GET  /api/skills/proposals
GET  /api/skills/proposals/:proposal_id
GET  /api/skills/proposals/:proposal_id/diff
POST /api/skills/proposals/:proposal_id/approve
POST /api/skills/proposals/:proposal_id/reject
```

Active proposal creation remains owned by `/learn`. The background-review
schema and command adapters reserve `SkillCreate`/`SkillPatch` consumer paths,
but the current built-in review producer emits only `WorkflowWarning`. This
phase exposes review and disposition for native proposals plus those reserved
records when present; it does not add an unaudited endpoint that accepts
arbitrary skill Markdown.

## Audit Snapshot Before Implementation

The existing route set is implemented:

- Protocol operations: `SkillsList`, `SkillsDetail`, `SkillsPatch`, and
  `SkillsFiles` in `crates/allthecodes-protocol/src/request.rs`.
- Handler module: `crates/allthecodes-web/src/handlers/skills.rs`.
- Registration: `crates/allthecodes-web/src/handler_registry.rs`.
- Capability flag: `skills=true` in
  `crates/allthecodes-web/src/handlers/capabilities.rs`.
- Handler coverage: `crates/allthecodes-web/src/handlers/skills_tests.rs`.

The capability flag may remain `true` because the current page-level MVP works,
but the domain status is Partial until proposal review has typed coverage.

The implementation remains intentionally integrated as `allthecodes-web` plus
`allthecodes-skills`; do not create a standalone `skills-api` service or a new
proposal store.

## Evidence for the Missing API

Commit `f94021b1` added a native proposal source plus command-side adapters for
reserved background-review variants:

1. Native skill proposals are owned by `allthecodes-skills`:
   - `stage_skill_proposal` supports a `SkillProposal` with `create|patch`,
     `user|project`, source session, proposed target, Markdown, and timestamp.
     The active `/learn` producer currently stages project-scoped `create`;
     patch remains supported by the owner but has no current built-in producer.
   - `list_skill_proposals`, `load_skill_proposal`,
     `approve_skill_proposal`, and `reject_skill_proposal` provide the current
     storage operations.
   - User proposals live under the allthecodes data root; project proposals
     live under the active project's `.allthecodes/skill_proposals` directory.
2. Background review owns a separate canonical queue in
   `allthecodes-engine/src/services/background_review.rs`. Its enum and
   `/skills pending|diff|approve|reject` adapters can consume `SkillCreate` and
   `SkillPatch`, but current production code creates only `WorkflowWarning`.
   Treat the two skill kinds as reserved/forward-compatible records rather than
   a second active producer.

`/learn` stages a native project proposal rather than installing a skill
directly. `/skills` can list, inspect, approve, and reject native records and can
adapt reserved background skill records if one exists, but the Web handler
exposes only discovered skill list/detail/file reads and enabled/pinned PATCH.
There are no proposal operations in `allthecodes-protocol` or
`allthecodes-web`.

## Proposal Ownership

Keep both existing stores canonical:

| API source | Canonical owner | Included kinds |
|---|---|---|
| `native` | `allthecodes-skills` user/project proposal files | `create`, `patch` |
| `background_review` | `allthecodes-engine::services::background_review` | Reserved `SkillCreate`, `SkillPatch`; no current built-in producer |

Add a shared proposal facade at
`crates/allthecodes-engine/src/services/skill_proposals.rs`. This layer can
depend on `allthecodes-skills` and the existing background-review service;
`allthecodes-commands` and `allthecodes-web` already depend on the engine. Move
the command-private background proposal conversion and disposition semantics
into this facade so CLI and Web cannot drift.

The facade is an adapter, not a third owner. It must not copy background-review
records into a second pending queue or merge either source into Web-local JSON.
An internal temporary claim used during approval is transaction state, not a
new user-visible proposal.

### Workflow warning ownership

`WorkflowWarning` is currently visible through both `/memory` and `/skills`,
which makes two callers able to consume the same background-review record. The
Skills API must exclude it from list, detail, diff, approve, and reject.

`WorkflowWarning` remains owned by the Memory/background-review proposal plan in
[Memory API Backend Plan](02-memory-api-plan.md). The shared skill facade accepts
only `SkillCreate` and `SkillPatch`; every other background-review kind returns
`404 proposal_not_found` through this domain rather than exposing or consuming
it.

## Typed Protocol

Extend `crates/allthecodes-protocol/src/v1/skills.rs` and register all five
operations in `crates/allthecodes-protocol/src/request.rs`.

Use an opaque, namespace-qualified `proposal_id` so native and
background-review identifiers cannot collide. The decoder selects exactly one
owner. Do not reproduce the current command fallback of trying the second owner
after any error: an I/O or parse failure in one source must remain an error, not
silently target a same-named record in the other source.

Core DTOs:

```text
SkillProposalSource             # native | background_review
SkillProposalAction             # create | patch
SkillProposalScope              # user | project
SkillProposalSummary
SkillProposalDetailResponse
SkillProposalDiffResponse
SkillProposalMutationRequest
SkillProposalMutationResponse
SkillProposalListQuery
SkillProposalListResponse
```

Each summary includes:

- opaque `proposal_id`, source, action, scope, and skill name;
- optional source session identifier when the caller may inspect it;
- created timestamp and a display-safe relative target;
- proposal digest, validation state, and whether it is actionable;
- no absolute path, raw background payload, or host workspace identifier.

Detail adds bounded proposed Markdown, normalized frontmatter diagnostics, and
the current target digest when a patch target exists. Diff returns a bounded
server-generated unified diff, baseline/proposal digests, `truncated`, and the
untruncated byte count. It never accepts client-supplied baseline content.

Mutation requests include:

```text
request_id               # idempotency key scoped to caller/workspace/operation
expected_proposal_digest # rejects a changed proposal
expected_target_digest   # required for patch; create uses an explicit absent value
```

The response reports the final disposition, installed skill name/scope when
approved, resulting target digest, and whether the result was an idempotent
replay. It does not return an absolute installed path.

## Route Behavior

| Operation | Route | Behavior |
|---|---|---|
| `SkillProposalsList` | `GET /api/skills/proposals` | Cursor-paginated pending proposals from the two allowed sources |
| `SkillProposalDetail` | `GET /api/skills/proposals/{proposal_id}` | Validated, redacted proposal detail |
| `SkillProposalDiff` | `GET /api/skills/proposals/{proposal_id}/diff` | Diff proposed Markdown against empty content for create or the current canonical `SKILL.md` for patch |
| `SkillProposalApprove` | `POST /api/skills/proposals/{proposal_id}/approve` | Atomically claim, revalidate, install, consume, and audit one proposal |
| `SkillProposalReject` | `POST /api/skills/proposals/{proposal_id}/reject` | Atomically claim and consume one proposal without changing a skill |

List filters may include `source`, `scope`, and `action`; defaults include both
sources and both actions authorized for the active workspace. Use stable
newest-first ordering with `proposal_id` as the tie-breaker. Default to 50 rows
and reject limits above 100.

Static proposal routes must not be captured as the existing dynamic skill id
`/api/skills/{id}`. Register through protocol metadata and `HandlerRegistry`,
and add an explicit route-collision regression.

## Workspace and Scope Authorization

- Bind project proposals to the server-selected `WebState` workspace. Do not
  accept `cwd`, a project root, or a target path from the client.
- Resolve native project proposals only from that workspace's
  `.allthecodes/skill_proposals` directory.
- A background-review project proposal must have trusted project provenance
  that resolves to the active workspace. A legacy record without verifiable
  workspace provenance may be shown as non-actionable only to an authorized
  operator; Web approval must fail closed.
- Require project-write permission for project approval/rejection and a
  privileged global-skill capability for user scope.
- Recompute the target from validated scope plus skill name. Treat the stored
  `proposed_path` as evidence to validate, never as an instruction to follow.
- Reject scope changes between proposal creation and disposition.

## Path and Symlink Containment

The current native approval uses a lexical `starts_with` check. The shared
facade must harden the mutation boundary before exposing it remotely:

1. Validate the skill name with the canonical skill-name validator.
2. Derive the user or active-project skill root server-side.
3. Normalize components and reject absolute paths, `..`, alternate separators,
   and platform prefixes.
4. Inspect every existing component with `symlink_metadata`; reject symlinked
   proposal directories, skill directories, and target files.
5. Canonicalize the nearest existing ancestor and prove containment beneath the
   canonical allowed root.
6. Create the final directory without following a replaced symlink, write a
   same-directory temporary file, flush it, and rename atomically.
7. Recheck containment and target identity under the same approval lock just
   before the rename.

Detail and diff apply the same containment checks. An unsafe record is reported
as non-actionable with a stable diagnostic; its host path is never serialized.

## Bounds and Redaction

- Cap proposal Markdown at 256 KiB, generated diff text at 64 KiB, and every
  public string and diagnostics list independently.
- Parse and validate skill frontmatter before returning `actionable=true` and
  again under the mutation lock before approval.
- Never serialize absolute proposed/installed paths, background-review `cwd`,
  raw payloads, environment values, credentials, source transcript text, or
  unrelated proposal kinds.
- Escape control characters in display fields. Return Markdown and diff as data,
  never render or execute them server-side.
- Isolate corrupt proposal files. A list may report a bounded diagnostic count,
  but one corrupt record must not expose its bytes or suppress valid records.
- Apply response-byte limits after serialization and report truncation
  explicitly; do not cut UTF-8 or JSON at arbitrary byte offsets.

## Atomic Consumption, Concurrency, and Idempotency

Approval and rejection are single-consumption operations:

1. Acquire a cross-process per-proposal lock in addition to any in-process
   serialization.
2. Resolve the namespace-qualified owner and atomically move the pending record
   to a private claimed state. Only one caller may claim it.
3. Under the same lock, verify authorization, proposal digest, target digest,
   workspace provenance, frontmatter, and path containment.
4. For approval, atomically install the target and then finalize a bounded
   disposition receipt keyed by `request_id`. For rejection, finalize the
   receipt without touching the target.
5. A retry with the same caller, operation, and `request_id` returns the stored
   result. A different request for a claimed or consumed proposal returns
   `409 proposal_consumed`.
6. Failure before target replacement restores the pending record. If failure is
   ambiguous after replacement, reconcile target/proposal digests and finalize
   the original result; never install twice or resurrect an applied proposal.

Do not remove the background record before the target write succeeds. Do not
leave a staged native copy after a background proposal is approved. Disposition
receipts contain only identifiers, digests, outcome, and timestamps, not the
proposed Markdown or raw review payload.

## Errors

Use stable typed errors:

- `400 invalid_proposal` for invalid identifiers, filters, or malformed DTOs;
- `403 proposal_scope_forbidden` for unauthorized user/project scope;
- `404 proposal_not_found` for unknown IDs and excluded proposal kinds;
- `409 proposal_changed`, `target_changed`, or `proposal_consumed` for
  optimistic-concurrency and single-consumption failures;
- `413 proposal_too_large` or `diff_too_large` when configured bounds cannot be
  represented safely;
- `422 proposal_not_actionable` for invalid frontmatter, missing trusted
  workspace provenance, or an unsafe target;
- `500/503 proposal_store_unavailable` for owner, lock, or durable-write
  failures without leaking host paths.

## Existing Route Behavior to Preserve

`GET /api/skills` continues to return discovered summaries and registry
diagnostics. `GET /api/skills/:id` continues to return detail and prompt body.
`GET /api/skills/:id/files` remains a bounded, contained read.
`PATCH /api/skills/:id` remains limited to mutable UI flags `enabled` and
`pinned`.

Proposal approval is the only endpoint in this plan that writes skill content.
It must not broaden the existing PATCH route into arbitrary skill-file editing.

## Implementation Tasks

### Task 1: Add shared proposal semantics

- Add `crates/allthecodes-engine/src/services/skill_proposals.rs` and export it from
  `services/mod.rs`.
- Adapt native owner functions and filtered, reserved `SkillCreate`/`SkillPatch`
  background-review records without advertising an active producer.
- Move background proposal conversion out of `skills_cmd.rs`; make the command
  and Web handler call the same list/detail/diff/disposition service.
- Add locking, namespace-qualified IDs, digest checks, disposition receipts,
  safe target installation, and failure reconciliation.

### Task 2: Add protocol operations

- Add the typed DTOs to `allthecodes-protocol/src/v1/skills.rs`.
- Register all five `ApiMethod` entries in `request.rs` with concrete request and
  response types; do not use `serde_json::Value` for proposal contracts.
- Cover REST and JSON-RPC serialization, enums, optional fields, and errors.

### Task 3: Add Web adapters

- Add proposal processors/handlers under
  `crates/allthecodes-web/src/handlers/skills.rs` or a focused sibling module.
- Register them through `handlers()` and
  `handler_registry::all_api_handlers()`.
- Bind scope and authorization from Web state and map service errors without
  exposing host paths.

### Task 4: Update generated contracts

Regenerate route docs, JSON Schema, OpenAPI, and the paired frontend TypeScript
artifacts described by
[API Generated Artifact Freshness Plan](17-api-generated-artifact-freshness-plan.md).

## Tests

Preserve the existing list/detail/file/PATCH tests and add:

- native user and active-project proposal list/detail/diff;
- fixture-backed background `SkillCreate` and `SkillPatch` projection and
  approval, with capability text that does not claim automatic production;
- `MemoryAdd`, `MemoryReplace`, and `WorkflowWarning` are invisible and cannot
  be consumed through Skills operations;
- namespace collisions and an owner I/O error never fall through to the other
  proposal source;
- create diff uses an empty baseline; patch diff uses the current canonical
  target and reports deterministic truncation;
- no response contains an absolute path, raw background payload, workspace
  `cwd`, secret, or oversized content;
- cross-workspace project access and unauthorized user-scope mutation fail;
- traversal, symlinked ancestors/targets, replaced-symlink races, and malformed
  frontmatter fail closed;
- two concurrent approvals yield one durable install and one conflict;
- same-request retry returns the original outcome, while a different request
  cannot consume the record twice;
- target/proposal digest changes return `409` without mutation;
- failure before and after target replacement reconciles without proposal loss,
  duplicate install, or an orphaned second pending record;
- `/api/skills/proposals` resolves to the proposal list rather than skill detail;
- HandlerRegistry, capability, protocol metadata, and generated artifacts remain
  aligned.

Run the narrow gates:

```bash
cargo test -p allthecodes-skills skill_proposal
cargo test -p allthecodes-engine skill_proposal
cargo test -p allthecodes-commands skills
cargo test -p allthecodes-protocol skills
cargo test -p allthecodes-web skills
cargo fmt --all --check
git diff --check
```

## Acceptance

- Existing Skills page/composer endpoints remain compatible and
  `capabilities.skills=true` continues to mean that page-level MVP is usable.
- Domain status is Partial until all five proposal operations are registered,
  typed, authorized, generated, and tested.
- One API list covers active native proposals and reserved background skill
  records when present, without creating a third store or presenting one record
  twice.
- Detail/diff never expose host paths or raw review payloads.
- Approval/rejection are workspace-bound, symlink-safe, idempotent, and exactly
  single-consumption under concurrent callers.
- `WorkflowWarning` remains exclusively on the Memory/background-review review
  surface and cannot be consumed through Skills API operations.
