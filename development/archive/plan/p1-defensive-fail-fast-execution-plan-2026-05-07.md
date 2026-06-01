# P1 Defensive Fail-Fast Execution Plan

Created: 2026-05-07
Source: `docs/TECH_DEBT.md` section `## P1: 过度防御性容错 / 静默降级`
Mode: direct plan

## Requirements Summary

This plan converts the P1 defensive-code debt into a staged execution track. The goal is not to remove all defensive handling; the goal is to make infrastructure, policy, persistence, auth, and protocol failures visible at the first boundary where they occur.

Current confirmed risk areas:

- Task and mailbox persistence can continue after lock, read, or parse failures (`crates/claude-code-rs/src/tools/tasks/store.rs`, `crates/claude-code-rs/src/teams/mailbox.rs`).
- Hook execution and hook policy paths mostly warn and continue even when hooks may represent security, permission, or audit policy (`crates/claude-code-rs/src/tools/hooks/*`, `crates/claude-code-rs/src/engine/lifecycle/deps.rs`).
- A legacy tool orchestration path still exists beside the canonical `QueryDeps::execute_tool` path (`crates/claude-code-rs/src/tools/orchestration.rs`, `crates/claude-code-rs/src/tools/execution/mod.rs`).
- Existing config/auth/plugin/MCP/startup data can be treated as missing when it is actually invalid or unreadable (`crates/cc-mcp/src/discovery.rs`, `crates/cc-auth/src/lib.rs`, `crates/claude-code-rs/src/plugins/loader.rs`, `crates/claude-code-rs/src/startup/mod.rs`).
- Protocol and runtime result boundaries can hide corruption or panic state (`crates/claude-code-rs/src/query/loop_helpers.rs`, `crates/claude-code-rs/src/api/streaming.rs`, `crates/claude-code-rs/src/engine/output_style.rs`).

## Low-Reasoning Execution Contract

Use this table before making any local decision. Do not re-litigate the classification inside each phase unless a test proves the rule wrong.

| Boundary | Missing value | Present but invalid/unreadable | Required behavior |
| --- | --- | --- | --- |
| Optional config file | Default allowed | Error or diagnostic | Never overwrite or hide the invalid file |
| Required config value | Error | Error | Return actionable message |
| Environment variable | Default only when absent | Error or diagnostic | Present invalid values must be surfaced |
| Enum/string setting | Default only when field absent | Error or validation warning promoted to error before use | Unknown values must not become permissive defaults |
| Persistent state file | Default only on first creation | Error, backup, or explicit recovery path | Do not silently reset to empty |
| Lock acquisition | Not applicable | Operation error | No memory fallback for writes |
| Hook policy | Optional hook may warn | Critical hook fails current step/tool | Policy hooks must be able to fail closed |
| External protocol required field | Not applicable | Parse error | No `unwrap_or_default()` for required fields |
| Tool task panic / JoinError | Not applicable | Synthetic failed tool result | Preserve `tool_use_id` / `tool_name` cardinality |
| Cleanup/notification best effort | Ignore allowed | Warn optional | Keep out of scope unless it hides policy or persistence |

Per-step execution rule:

1. Add or identify a failing regression first.
2. Change only the files named by that phase.
3. Run that phase's targeted tests.
4. Run `cargo check -p claude-code-rs --message-format short` when the phase touches `claude-code-rs`.
5. Update `docs/TECH_DEBT.md` only after tests prove the phase is resolved.

## Acceptance Criteria

- TaskStore write operations do not proceed after task-list lock failure.
- Mailbox write operations do not reset corrupt or unreadable mailbox files to `[]`.
- Critical hooks can fail closed, and existing optional hook behavior remains available.
- Production tool execution has one canonical path; legacy orchestration is removed or made explicitly test-only.
- Concurrent tool execution always returns one tool result per tool use, including JoinError/panic cases.
- Existing but invalid MCP/plugin/config/auth/startup state produces a user-visible diagnostic or operation error, not an empty/default state.
- Required streaming fields fail parse when absent or malformed.
- Output style fallback is visible when a configured style cannot be resolved.
- Each completed phase has targeted tests and a recorded verification command.

## Phase 0 - Baseline And Test Harness

Goal: create a regression safety net without changing behavior.

Work:

- Inventory current tests near the affected modules:
  - `crates/claude-code-rs/src/tools/tasks/store.rs`
  - `crates/claude-code-rs/src/teams/mailbox.rs`
  - `crates/claude-code-rs/src/tools/hooks/pre_tool.rs`
  - `crates/claude-code-rs/src/tools/hooks/post_tool.rs`
  - `crates/claude-code-rs/src/tools/hooks/execution.rs`
  - `crates/claude-code-rs/src/query/loop_helpers.rs`
  - `crates/claude-code-rs/src/plugins/loader.rs`
  - `crates/cc-mcp/src/discovery.rs`
  - `crates/cc-auth/src/lib.rs`
- Add failing or ignored regression tests only where the next phase needs them.
- Record any package-level compile blockers before implementation.

Suggested verification:

- `cargo test -p claude-code-rs tools::hooks -- --nocapture`
- `cargo test -p claude-code-rs teams::mailbox -- --nocapture`
- `cargo test -p claude-code-rs plugins::loader -- --nocapture`
- `cargo test -p cc-mcp discovery -- --nocapture`
- `cargo test -p cc-auth --lib`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- Every later phase has at least one concrete regression target.
- Existing failures are documented before implementation begins.

## Phase 1 - Strict Persistence And Lock Boundaries

Goal: stop state writes when the repository, mailbox, or lock boundary is not trustworthy.

Work:

- In `crates/claude-code-rs/src/tools/tasks/store.rs`, replace write-path `acquire_task_list_lock(...).ok()` with explicit error propagation.
- Align create/update/delete/stop behavior with `claim_task()`'s explicit `LockUnavailable` behavior.
- In `crates/claude-code-rs/src/teams/mailbox.rs`, make write paths reuse strict read/parse behavior instead of `unwrap_or_default()` or `"[]"`.
- Preserve corrupt mailbox content by returning an error or writing a backup/recovery artifact; do not auto-reset to empty.
- Review lock stale recovery in mailbox code. Keep stale recovery only when the lock is provably stale; otherwise fail the operation.

Targeted tests:

- Task create/update/delete/stop returns a lock error when lock acquisition fails.
- Task write path does not mutate the store after lock failure.
- Mailbox write returns an error on corrupt JSON and preserves the original file.
- Mailbox read/write behavior is consistent for invalid mailbox content.

Suggested verification:

- `cargo test -p claude-code-rs tools::tasks -- --nocapture`
- `cargo test -p claude-code-rs teams::mailbox -- --nocapture`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- No persistent write path in TaskStore or mailbox silently falls back to memory/empty state after a failed lock/read/parse.

## Phase 2 - Hook Criticality And Fail-Closed Policy

Goal: keep optional hooks best-effort while allowing policy hooks to fail closed.

Work:

- Locate hook config types and parse points before changing execution behavior.
- Add a minimal critical/optional flag or equivalent policy field using existing config patterns.
- Preserve current behavior as optional/default only if that avoids breaking existing hook users.
- In `crates/claude-code-rs/src/tools/hooks/pre_tool.rs` and `crates/claude-code-rs/src/engine/lifecycle/deps.rs`, propagate critical pre-tool hook errors to the current tool step.
- In `crates/claude-code-rs/src/tools/hooks/post_tool.rs`, surface critical post/failure/stop hook failures according to the same policy.
- In `crates/claude-code-rs/src/tools/hooks/execution.rs`, record stdin/stdout/stderr/kill IO failures in the hook result or tracing path.

Targeted tests:

- Optional pre-tool hook error logs/warns and continues.
- Critical pre-tool hook error blocks tool execution.
- Critical post/failure/stop hook error is visible to the caller or session diagnostics.
- Hook IO failures are observable in test output/result data.

Suggested verification:

- `cargo test -p claude-code-rs tools::hooks -- --nocapture`
- `cargo test -p claude-code-rs engine::lifecycle -- --nocapture`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- Security/policy hook users have a documented fail-closed path.
- Existing best-effort hooks remain supported intentionally, not accidentally.

## Phase 3 - Single Canonical Tool Execution Path

Goal: remove or quarantine duplicate tool orchestration so hook/permission/result behavior cannot drift.

Work:

- Confirm no production caller depends on `crates/claude-code-rs/src/tools/orchestration.rs`.
- Remove the export from `crates/claude-code-rs/src/tools/mod.rs` if unused.
- Delete `tools/orchestration.rs`, or move the minimal needed parts into a test-only fixture.
- Keep `crates/claude-code-rs/src/tools/execution/mod.rs` and `QueryDeps::execute_tool` as the production execution path.
- Remove related `#![allow(unused)]` only after compile confirms the path is gone.

Targeted tests:

- Existing tool execution tests still cover validation, permission prompts, pre/post hooks, audit, and result truncation.
- Build fails if a production module tries to import the old orchestration path.

Suggested verification:

- `cargo test -p claude-code-rs tools::execution -- --nocapture`
- `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- There is exactly one production tool execution path.
- Hook and permission behavior no longer has a duplicate implementation to keep in sync.

## Phase 4 - Existing Config/Data Must Not Masquerade As Missing

Goal: distinguish absent optional inputs from invalid existing inputs.

Work:

- MCP:
  - Change `crates/cc-mcp/src/discovery.rs` so existing user/project settings read or parse failures return diagnostics.
  - Preserve per-server name/scope/error when one MCP server entry is invalid.
  - Update `claude-code-rs` call sites that currently use `unwrap_or_default()` to display or propagate diagnostics.
- Plugins:
  - Change `crates/claude-code-rs/src/plugins/loader.rs` to return structured diagnostics for `installed_plugins.json` and cache manifest failures.
  - Update `plugins/mod.rs` and `plugins/refresh.rs` so startup/status sees "plugin metadata invalid" rather than "no plugins".
- Config mutation:
  - Change `commands/memory.rs` and `commands/model_add.rs` load-or-default mutation paths to abort when existing files are unreadable or invalid.
- Startup/env/permissions/auth:
  - In `startup/mod.rs`, surface errors for existing `.env` files that cannot be loaded.
  - In `cc-types/src/permissions.rs`, stop mapping unknown permission mode strings to `Default` during configured-value parsing.
  - In `cc-auth/src/lib.rs`, distinguish invalid/present credentials and runtime refresh failures from no credentials.

Targeted tests:

- Missing MCP config returns empty discovery.
- Malformed existing MCP config returns diagnostics.
- One malformed MCP server entry preserves an error entry.
- Corrupt `installed_plugins.json` is reported and not treated as no plugins.
- Existing invalid `.env` or settings mutation path aborts without rewrite.
- Unknown permission mode in config is diagnostic/error; absent permission mode still defaults.
- Auth read/refresh infrastructure failure does not erase credentials or report plain no-auth.

Suggested verification:

- `cargo test -p cc-mcp discovery -- --nocapture`
- `cargo test -p claude-code-rs plugins -- --nocapture`
- `cargo test -p claude-code-rs commands::memory -- --nocapture`
- `cargo test -p claude-code-rs commands::model_add -- --nocapture`
- `cargo test -p cc-types permissions -- --nocapture`
- `cargo test -p cc-auth --lib`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- Existing invalid files/values produce diagnostics before any mutation or fallback.
- Truly absent optional files still retain default behavior.

## Phase 5 - Protocol And Runtime Result Cardinality

Goal: ensure corrupted protocol data and spawned tool panics cannot disappear into defaults.

Work:

- In `crates/claude-code-rs/src/query/loop_helpers.rs`, capture `tool_use_id` and `tool_name` before spawning concurrent tool tasks.
- For any JoinError, synthesize a failed `ToolExecResult` with the original id/name.
- Replace `unknown` id/name fallbacks in tool execution error branches with captured context.
- In `crates/claude-code-rs/src/api/streaming.rs`, classify SSE fields as required or optional.
- Replace defaulting of required fields with parse errors.
- In `crates/claude-code-rs/src/engine/output_style.rs`, return or record fallback diagnostics when a configured style cannot be resolved.

Targeted tests:

- A panicking spawned tool still emits a failed result for the same `tool_use_id`.
- A malformed streaming event missing a required index/content block returns parse error.
- Missing optional streaming fields preserve current behavior when the protocol allows absence.
- Unknown configured output style emits a visible fallback diagnostic.

Suggested verification:

- `cargo test -p claude-code-rs query::loop_helpers -- --nocapture`
- `cargo test -p claude-code-rs api::streaming -- --nocapture`
- `cargo test -p claude-code-rs engine::output_style -- --nocapture`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- Runtime cardinality and protocol parse failures are represented explicitly in results or errors.

## Phase 6 - Team Coordination And Worktree Isolation

Goal: keep team coordination failures visible instead of continuing under unsafe assumptions.

Work:

- In `crates/claude-code-rs/src/teams/runner.rs`, promote mailbox processing errors from warn-only to task-visible failure when they affect coordination state.
- Review `ShutdownRequest` auto-approval behavior and require explicit policy or documented non-interactive mode before auto-approving.
- In `crates/claude-code-rs/src/engine/agent/supervisor.rs`, make worktree isolation fallback visible and opt-in when background isolation is required.

Targeted tests:

- Runner marks coordination-affecting mailbox errors as task failure or structured diagnostic.
- Shutdown auto-approval is gated by explicit mode/policy.
- Worktree isolation failure cannot silently run in normal cwd when isolation is required.

Suggested verification:

- `cargo test -p claude-code-rs teams::runner -- --nocapture`
- `cargo test -p claude-code-rs engine::agent -- --nocapture`
- `cargo check -p claude-code-rs --message-format short`

Exit criteria:

- Team execution cannot silently proceed after coordination or isolation boundaries fail.

## Phase 7 - Documentation Closure And Allow Cleanup

Goal: record resolved debt and remove masking attributes only after behavior is protected.

Work:

- Update `docs/TECH_DEBT.md` after each completed phase with:
  - status;
  - changed files;
  - tests run;
  - remaining risks.
- Move completed entries to `development/archive/TECH_DEBT.md` only when all related tests pass.
- Remove broad `#![allow(unused)]`, `#[allow(dead_code)]`, or `#[allow(unused_imports)]` from files touched by completed phases.
- Keep unrelated lint cleanup out of this plan.

Suggested verification:

- `git diff --check`
- `cargo check -p claude-code-rs --message-format short`
- Targeted package tests from completed phases.

Exit criteria:

- `docs/TECH_DEBT.md` no longer lists completed fail-fast items as active.
- Remaining items have explicit blockers or follow-up plan references.

## Parallelization Map

Parallelize only after Phase 0 is complete.

| Lane | Can run in parallel with | Owns | Must not edit |
| --- | --- | --- | --- |
| Persistence lane | Hook lane, config lane | TaskStore, mailbox | Hook config, MCP/plugin/auth |
| Hook/tool lane | Persistence lane, config lane | hooks, `engine/lifecycle/deps.rs`, `tools/orchestration.rs`, `query/loop_helpers.rs` | MCP/plugin/auth |
| Config/data lane | Persistence lane, Hook lane after shared diagnostics agreed | MCP, plugin loader, config mutation, auth, permission parse, startup env | TaskStore/mailbox |
| Protocol/style lane | After Hook/tool lane captures result context | streaming parser, output style | Hook policy |
| Team coordination lane | After mailbox strictness is merged | runner shutdown, worktree isolation | mailbox parser |

Do not run two agents on the same file. If a lane needs shared diagnostic types, create them in a narrow preliminary patch before parallel work.

## Risks And Mitigations

| Risk | Mitigation |
| --- | --- |
| Fail-fast changes break users relying on silent fallback | Preserve defaults only for truly absent optional files; add clear diagnostics and migration notes |
| Hook criticality introduces config compatibility issues | Default existing hooks to optional unless the existing schema already implies blocking behavior |
| Diagnostics type grows into a large abstraction | Use local result structs first; extract shared diagnostics only after two call sites need the same shape |
| Tests require brittle filesystem races | Prefer deterministic lock injection or temp-dir corrupt-file tests over timing-dependent concurrency |
| Broad phase touches too many files | Split each phase by boundary; do not combine persistence, hooks, and config in one implementation commit |

## Definition Of Done

- All active P1 fail-fast entries in `docs/TECH_DEBT.md` are either fixed, downgraded with evidence, or split into smaller follow-up items.
- Every fixed item has a regression test that fails under the old behavior.
- Targeted tests and `cargo check -p claude-code-rs --message-format short` pass after the final code phase.
- No new dependency is introduced.
- Final report lists changed files, simplifications made, verification evidence, and remaining risks.
