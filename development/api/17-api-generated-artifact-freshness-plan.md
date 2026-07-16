# API Generated Artifact Freshness Plan

> Status: Partial on 2026-07-16
> Audit date: 2026-07-16
> Audit HEAD: `6d426dc9` (frozen snapshot)

## Implementation Result (2026-07-16)

Backend freshness enforcement is implemented in `61583df9`, and the current
backend artifacts were regenerated and checked in `e5163791`:

- one six-artifact manifest drives shared write/check paths;
- the CLI supports explicit `backend-docs`, `frontend`, and `all` targets plus
  an explicit frontend directory;
- generation stages writes atomically, reports every stale/missing owned path,
  and embeds one protocol digest across backend/frontend output;
- backend CI executes the `backend-docs` check and verifies both tracked and
  untracked `docs/api` state; and
- stream metadata reaches schema/OpenAPI output, with streaming operations
  documented as `text/event-stream`.

The final backend protocol and `docs/api/routes.md` now both contain 273
operations, closing the backend drift observed below. This plan remains
Partial: the paired frontend artifacts/CI have not been updated in their owning
repository, and explicit invariance tests across timezone, locale, current
directory, and checkout path are still absent.

## Audit Snapshot Before Implementation

`crates/allthecodes-protocol/src/request.rs` currently declares 253 protocol
operations. The generated operation table in `docs/api/routes.md` contains 207
`/api/` rows, a drift of 46 operations.

This is a generated-artifact freshness defect. It is not evidence that those 46
product routes are missing from the runtime router. Route implementation and
registration must be audited separately through `HandlerRegistry` and route
coverage tests.

The current generator already has pure functions for TypeScript types/routes,
JSON Schema, route Markdown, and OpenAPI. However:

- `codegen --check` checks only the paired frontend generated directory;
- `route-doc`, `schema-export`, and `openapi-export` print to stdout and do not
  share one atomic write/check manifest;
- backend CI compiles the codegen binaries but does not execute a freshness
  diff-check for committed `docs/api/*` artifacts.

## Owned Artifacts

| Artifact | Canonical owner | Current path |
|---|---|---|
| Route documentation | Backend protocol crate | `docs/api/routes.md` |
| JSON Schema | Backend protocol crate | `docs/api/schema.json` |
| OpenAPI document | Backend protocol crate | `docs/api/openapi.json` |
| TypeScript DTOs | Paired frontend repository | `src/lib/generated/api-types.ts` |
| TypeScript route metadata | Paired frontend repository | `src/lib/generated/api-routes.ts` |
| Frontend API schema | Paired frontend repository | `src/lib/generated/api-schema.json` |

`allthecodes-protocol` owns generation logic and content. Repository owners own
the committed copies at the paths above. Generated files must not be edited by
hand.

## Goal

One protocol change must regenerate every owned artifact in the same change and
CI must fail on any byte-level drift. The generation and check paths must consume
the same in-memory artifact manifest so they cannot disagree about which files
exist or how content is rendered.

Because backend and frontend are separate Git repositories, “same commit” is
enforced per ownership boundary:

- the backend protocol commit includes all changed `docs/api/*` artifacts;
- the paired frontend commit includes all changed TypeScript artifacts;
- the two commits form one linked API change and carry the same protocol digest.

If literal single-Git-commit ownership is later required, the canonical
TypeScript artifacts must first move into this repository. This plan does not
create a second committed TypeScript mirror.

## Unified Artifact Manifest

Add a single deterministic manifest API in
`crates/allthecodes-protocol/src/codegen.rs`:

```rust
pub enum ArtifactOwner {
    Backend,
    Frontend,
}

pub struct GeneratedArtifact {
    pub owner: ArtifactOwner,
    pub relative_path: &'static str,
    pub contents: String,
}

pub fn generate_all_artifacts() -> Result<Vec<GeneratedArtifact>, CodegenError>;
```

The manifest includes all six artifacts. Write mode and check mode must iterate
this exact manifest. Existing pure renderers remain the only content source, and
the stdout binaries may remain as compatibility wrappers around them.

Do not infer the frontend checkout from whichever sibling directory happens to
exist in CI. Accept an explicit `--frontend-dir` or
`ALLTHECODES_FRONTEND_GENERATED_DIR`, and print the resolved path before writing
or checking.

## CLI Contract

Extend the existing `codegen` binary rather than add another independent writer:

```text
codegen --target backend-docs
codegen --target frontend --frontend-dir <path>
codegen --target all --frontend-dir <path>
codegen --check --target backend-docs
codegen --check --target frontend --frontend-dir <path>
codegen --check --target all --frontend-dir <path>
```

Rules:

- Write into sibling temporary files, flush, then rename; do not leave a mixed
  generation when one renderer fails.
- `--check` performs no writes and compares expected bytes with committed bytes.
- A stale or missing artifact fails with its owner, resolved path, and the exact
  regeneration command.
- Unknown targets, a missing explicit frontend directory, or duplicate output
  paths fail before generation starts.
- Keep `route-doc`, `schema-export`, and `openapi-export` stdout behavior for
  scripts, but document `codegen` as the canonical write/check entry point.

## Deterministic Output Contract

All generated output must be byte-for-byte stable across repeated runs and
machines:

- Route Markdown and TypeScript route arrays preserve the declared
  `API_METADATA` order.
- Schema/component maps use stable key ordering.
- JSON uses one pretty-print format, LF line endings, and one final newline.
- Markdown and TypeScript use LF line endings and one final newline.
- Output contains no timestamp, hostname, absolute checkout path, locale data,
  random identifier, or environment-dependent ordering.
- Generator version/protocol version fields are constants derived from source,
  not wall-clock state.
- Duplicate operation, method/path, schema name, or output path is a hard error.

`ANY` is a protocol transport pseudo-method for the three WebSocket upgrade
routes, not an OpenAPI HTTP verb. Define one canonical comparison mapping:

- route Markdown and TypeScript metadata retain source method `ANY` and mark the
  transport as WebSocket;
- OpenAPI represents the upgrade handshake under `get` and preserves
  `x-allthecodes-source-method: ANY` plus a WebSocket transport extension; and
- cross-artifact checks compare operation/path and the normalized effective
  method (`ANY` -> upgrade `GET`), while separately asserting the source-method
  extension. They must not require the raw method strings to be identical.

Add a protocol digest computed from canonical operation metadata and schema
content. Emit the same digest in backend and frontend generated headers so paired
repository changes can be verified without relying on commit timestamps.

## Same-Change Workflow

For any change to protocol operations, request/response DTOs, registered stream
event DTOs, errors, serialization metadata, or experimental flags:

1. Modify protocol source.
2. Run `codegen --target backend-docs`.
3. Run `codegen --target frontend --frontend-dir <paired-generated-dir>`.
4. Review generated diffs for the intended operation/schema changes only.
5. Commit source and backend artifacts together.
6. Commit paired frontend artifacts together and link the protocol digest.
7. Run both check modes before push.

Changes that affect no generated bytes should leave all artifact files untouched.
Formatting-only generated churn is a generator defect, not an acceptable commit.

## CI Diff-Check

Backend CI must execute generation, not merely compile the binaries:

```bash
cargo run --locked -p allthecodes-protocol --features codegen \
  --bin codegen -- --check --target backend-docs
```

The backend job must also run write mode in a clean temporary checkout and fail
if either of these detects changes:

```bash
git diff --exit-code -- docs/api
test -z "$(git status --porcelain -- docs/api)"
```

This catches both modified tracked files and newly generated untracked files.

The paired frontend workflow checks out the matching backend protocol revision,
sets an explicit frontend generated directory, and runs:

```bash
cargo run --locked -p allthecodes-protocol --features codegen \
  --bin codegen -- --check --target frontend \
  --frontend-dir <frontend>/src/lib/generated
```

It then performs the equivalent `git diff --exit-code` and porcelain check on
the three generated frontend files. CI must not silently skip the check when the
frontend checkout is absent; it should fail with a setup error or run the
backend-only target in a job that explicitly declares that narrower scope.

## Verification Tests

Add tests for:

- two consecutive generations produce identical bytes for all six artifacts;
- output is unchanged under different `TZ`, locale, current directory, and
  checkout path;
- generated route row count equals `API_METADATA.len()`;
- operation/path sets and normalized effective methods match across route
  Markdown, schema endpoints, OpenAPI paths, and TypeScript route metadata;
- all `ANY` WebSocket operations retain their source method/transport extension
  even though OpenAPI uses an upgrade `get` path item;
- every request/response type referenced by an operation exists in generated
  schema and TypeScript output;
- every stream event referenced by codegen metadata exists in generated schema,
  TypeScript, and OpenAPI documentation even when its HTTP response is `Value`;
- write mode and check mode consume the same manifest;
- stale, missing, and untracked artifacts fail with actionable paths;
- atomic generation leaves existing artifacts unchanged if any renderer fails;
- explicit frontend directory selection overrides sibling auto-detection;
- protocol digest is identical in backend and frontend artifacts.

Run:

```bash
cargo test -p allthecodes-protocol --features codegen
cargo run -p allthecodes-protocol --features codegen \
  --bin codegen -- --check --target backend-docs
```

## Acceptance

- Regenerating the current protocol produces 273 generated operation rows in
  `docs/api/routes.md`, closing the backend drift from the audit snapshot.
- A protocol or DTO change without regenerated backend artifacts fails backend
  CI with a precise file list and command.
- A paired frontend change with stale TypeScript artifacts fails frontend CI.
- All six artifacts are deterministic and carry the same protocol digest.
- Source plus generated outputs are committed in the same owned change; no
  generated file requires hand editing.
- The freshness gate makes no claim about runtime route presence. Runtime route
  coverage remains a separate handler-registry verification concern.
