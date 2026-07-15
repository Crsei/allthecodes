# 2026-07-16 API Gap Remediation Worktree Plan

> Status: Approved for implementation
> Audit source commit: `0832368d` (`docs(api): audit recent API coverage gaps`)
> Base branch: `allthecodes`
> Worktree branch: `worktree/api-gap-remediation`
> Worktree path: `.worktrees/api-gap-remediation`
> Artifact: `development/worktree-workflow-artifacts/2026-07-16-api-gap-remediation.html`

## 1. Goal

Implement the necessary API work identified by the frozen
`d5e16fde..6d426dc9` audit. The result must expose existing runtime owners
through typed protocol operations instead of adding disconnected Web stores or
synthetic success paths. Security-sensitive mutations remain fail-closed when
the current transport cannot complete an approval challenge.

This plan is the main-branch prerequisite required by
`2026-07-16-per-session-worktree-workflow-plan.md`. It is not modified inside
the task worktree.

## 2. Source Plans

- `development/api/02-memory-api-plan.md`
- `development/api/03-skills-api-plan.md`
- `development/api/06-jobs-cron-api-plan.md`
- `development/api/07-group-chat-api-plan.md`
- `development/api/08-backend-services-api-plan.md`
- `development/api/14-security-approval-api-parity-plan.md`
- `development/api/15-workflow-runtime-api-plan.md`
- `development/api/16-discovery-search-api-plan.md`
- `development/api/17-api-generated-artifact-freshness-plan.md`
- `development/api/18-web-ipc-agent-command-parity-plan.md`

The detailed DTOs, authorization rules, error semantics, bounds, and test
matrices in those files are normative. This orchestration plan only fixes the
order, ownership boundaries, commit structure, and repository-wide gates.

## 3. Scope and Order

### Phase P0-A: Security approval parity

1. Add a typed, backward-compatible permission stream projection that reuses
   `SecurityDecisionDisplay`.
2. Preserve normalized operation and display-safe security metadata in Web chat
   SSE and Web IPC permission requests.
3. Bind pending responses to the exact request, require the privileged Web
   credential, and consume each response at most once.
4. Keep exact approvals one-shot; reject reusable `always_allow` for exact
   requests and never serialize raw taint, output, secrets, or binding hashes.

Primary paths:

- `crates/allthecodes-protocol/src/v1/chat.rs`
- `crates/allthecodes-web/src/handlers/chat.rs`
- `crates/allthecodes-web/src/state.rs`
- `crates/allthecodes-web/src/handler_registry.rs`
- `crates/allthecodes-ipc/src/runtime.rs`

### Phase P0-B: Runtime-owned Memory API

1. Replace `memory/entries.json` as the canonical source with the runtime
   memdir projection used by prompt and recall behavior.
2. Preserve a bounded, explicit migration path for legacy Web entries.
3. Add bounded dream list/detail operations.
4. Add background-review proposal list/detail/approve/reject operations for the
   proposal kinds owned by Memory; `WorkflowWarning` stays on this surface and
   must not leak into Skills proposals.

Primary paths:

- runtime memory and background-review services under
  `crates/allthecodes-engine/src/services/`
- `crates/allthecodes-protocol/src/v1/memory.rs`
- `crates/allthecodes-web/src/handlers/memory.rs`

### Phase P0-C: Generated artifact freshness

1. Add one deterministic generated-artifact manifest and one canonical
   `codegen` write/check command.
2. Keep compatibility stdout binaries, but make all backend and explicit
   frontend generation use the same renderers and manifest.
3. Normalize protocol `ANY` WebSocket metadata to OpenAPI `GET` with source
   method and transport extensions.
4. Add a canonical protocol digest, deterministic-output tests, and a backend
   CI diff check.
5. Regenerate backend artifacts in this repository. Frontend generation must
   require an explicit directory; it must never guess a sibling checkout.

Primary paths:

- `crates/allthecodes-protocol/src/codegen.rs`
- `crates/allthecodes-protocol/src/bin/`
- `crates/allthecodes-protocol/tests/`
- `docs/api/`
- the existing backend CI workflow that owns protocol freshness

### Phase P1-A: Workflow domain API

1. Stabilize `FileWorkflow` as the shared typed owner for definition and run
   operations.
2. Add definition list/detail plus run list/start/status/advance/cancel (seven
   protocol operations total).
3. Make persisted runs workspace-scoped, discoverable after restart,
   revisioned, atomic, locked, cursor-paginated, and idempotent.
4. Register REST and API-RPC handlers. Reads use ordinary Web authentication;
   mutations require privileged authorization and remain
   `interactive_approval_required` when production policy returns `Ask`.

Primary paths:

- `crates/allthecodes-tools/src/workflow/`
- `crates/allthecodes-protocol/src/v1/workflows.rs`
- `crates/allthecodes-web/src/handlers/workflows.rs`

### Phase P1-B: Scheduler-owned Jobs API

1. Adapt existing Jobs routes to scheduled-task and scheduler ownership instead
   of the Web-only jobs store.
2. Migrate legacy records without creating duplicate schedules.
3. Make manual run enqueue real work through an injected dispatcher and expose
   truthful run/task lifecycle data.
4. Keep unavailable dispatch explicit; never append a synthetic failed run as
   a substitute for submission.

Primary paths:

- scheduled-task, scheduler, and daemon protocol-store modules
- `crates/allthecodes-protocol/src/v1/jobs.rs`
- `crates/allthecodes-web/src/handlers/jobs.rs`

### Phase P1-C: Skills proposal API

1. Extract one shared proposal service for native proposals and reserved
   background `SkillCreate`/`SkillPatch` records.
2. Add typed list/detail/diff/approve/reject operations with namespace-qualified
   identifiers, digest checks, idempotency, locking, and single consumption.
3. Bind user/project scope to server-resolved workspace context and make target
   installation path/symlink safe.
4. Keep `MemoryAdd`, `MemoryReplace`, and `WorkflowWarning` invisible to Skills
   operations.

Primary paths:

- `crates/allthecodes-engine/src/services/skill_proposals.rs`
- `crates/allthecodes-commands/src/commands/skills_cmd.rs`
- `crates/allthecodes-protocol/src/v1/skills.rs`
- `crates/allthecodes-web/src/handlers/skills.rs`

### Phase P1-D: Web IPC Agent/Team command parity

1. Bind the WebSocket once to the server-resolved session, engine, workspace,
   and matching hub.
2. Extract an owner-aware shared dispatcher from the existing headless Agent
   and Team handlers.
3. Execute all existing typed commands; return read/error results through a
   requester-only direct lane independent of replay sequence state.
4. Publish only authorized committed mutation events, and bound/redact output,
   collections, team messages, and live-context paths.

Primary paths:

- `crates/allthecodes-engine/src/agent/`
- `crates/allthecodes-ipc/src/agent_handlers.rs`
- `crates/allthecodes/src/app_runtime_adapters/mod.rs`
- `crates/allthecodes-web/src/ws/ipc.rs`
- `crates/allthecodes-web/src/ipc_streams.rs`

### Phase P1-E: Group Chat runtime integration

1. Make invite `GET` a pure read and add a privileged idempotent create/rotate
   mutation.
2. Dispatch messages through the existing delegated-agent supervisor with
   workspace/session ownership and exactly-once semantics.
3. Replace opaque snapshots with bounded typed SSE lifecycle events while
   retaining reconnect behavior.
4. Keep compression and stored history on their existing canonical owners.

Primary paths:

- `crates/allthecodes-protocol/src/v1/group_chat.rs`
- `crates/allthecodes-web/src/handlers/group_chat.rs`
- delegated-agent/session runtime adapters

### Phase P1-F: Backend Services runtime integration

1. Replace no-op session sync, synthetic compression state, and fixed agent
   retry errors with real session, compact, task, and runtime adapters.
2. Expose truthful status and explicit unavailable/partial states.
3. Preserve migration/backup safety, authorization, and concurrency behavior.

Primary paths:

- `crates/allthecodes-protocol/src/v1/backend_services.rs`
- `crates/allthecodes-web/src/handlers/backend_services.rs`
- existing session, compact, task, and agent services

### Phase P2: Unified discovery search

1. Add one typed, read-only discovery operation for MCP and plugin providers;
   do not replace mention autocomplete.
2. Reuse the existing scorer, match reasons, provider adapters, and safe next
   actions.
3. Resolve workspace exclusively from `WebState::engine().cwd()` and never from
   process cwd or request input.
4. Bound candidates, results, summaries, response bytes, and provider time;
   redact schemas, credentials, URLs, config, and executable paths.
5. Return partial provider failures as typed states without hiding successful
   results.

Primary paths:

- `crates/allthecodes-protocol/src/v1/discovery.rs`
- `crates/allthecodes-tools/src/discovery_search.rs`
- `crates/allthecodes-web/src/handlers/discovery.rs`

## 4. Shared Integration Rules

- Add or change DTOs in `allthecodes-protocol` before registering handlers.
- Use `handler_registry::all_api_handlers()` for REST and API-RPC parity; do not
  add an unregistered handler-only route.
- Treat capability flags as truthful runtime readiness, not route-presence
  flags.
- Use server-derived workspace/session identity. Request-provided identifiers
  may narrow a scope but may not create or broaden it.
- Reuse runtime stores and services. No new Web-only canonical store is allowed.
- All mutations need explicit ordinary/privileged authorization classification,
  permission policy handling, atomicity, idempotency, and stable conflicts.
- Generated files are never hand-edited.
- Shared registry/codegen files are integrated serially after domain-local work
  to prevent parallel edits from dropping operations.

## 5. Commit Structure

The worktree uses explicit-path staging and one bounded commit per completed
domain. The expected sequence is:

1. `fix(api): preserve exact approval semantics on Web transports`
2. `feat(api): project runtime memory and review proposals`
3. `build(api): enforce generated artifact freshness`
4. `feat(api): expose file workflow runtime operations`
5. `feat(api): connect jobs API to scheduler runtime`
6. `feat(api): expose skill proposal lifecycle`
7. `feat(ipc): dispatch Web agent and team commands`
8. `feat(api): connect group chat to delegated agents`
9. `feat(api): connect backend services to runtime owners`
10. `feat(api): add bounded discovery search`
11. `docs(api): regenerate contracts and record API remediation`

If implementation evidence shows two planned domains share an inseparable
atomic change, combine only those commits and record the reason in the artifact.
Do not mix unrelated pre-existing worktree changes into any commit.

## 6. Verification

Run domain tests listed in every source plan after its commit. Before merge,
run at minimum with the repository Cargo environment:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p allthecodes-protocol --features codegen
cargo test -p allthecodes-web
cargo test -p allthecodes-ipc agent_handlers
cargo test -p allthecodes-engine
cargo test -p allthecodes-tools
cargo test -p allthecodes-commands skills
cargo test --workspace
cargo run --locked -p allthecodes-protocol --features codegen \
  --bin codegen -- --check --target backend-docs
cargo build --workspace --release
git diff --check
```

A green command that ran zero relevant tests is non-evidence. Record exact
command, exit code, test count where available, and any honest unrelated
workspace blocker in the artifact. Resolve all warnings introduced by this
task.

## 7. Completion and Merge

1. Create the standalone HTML artifact at the fixed path in this plan. It must
   include task goal, workflow steps, changed paths by domain, this plan path,
   actual commit list, and exact verification evidence.
2. Ensure the worktree is clean and all task commits are on
   `worktree/api-gap-remediation`.
3. If `allthecodes` advanced, rebase the worktree branch onto the latest
   `allthecodes`; never create a merge commit.
4. Fast-forward merge with
   `git merge --ff-only worktree/api-gap-remediation`.
5. Push `allthecodes` to `origin`, then remove
   `.worktrees/api-gap-remediation` and delete the merged worktree branch.

Completion means every audited domain is either implemented and verified or is
reported as a concrete, evidence-backed blocker. A registered route backed by a
synthetic/no-op implementation does not count as complete.
