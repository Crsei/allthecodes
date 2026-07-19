# OpenAI stream timeout recovery

> Date: 2026-07-19
> Task slug: `tui-task-list-default-and-stream-recovery`

## Evidence

The live Codex-backend request for session `99b73658-f3fc-49d1-9571-cf01e591e8c3`
failed after 120014 ms with `error reading OpenAI response chunk`. The shared reqwest
client applies a 120-second total request timeout, so a valid long-lived streaming request
can be terminated even when the query layer already owns idle/stall watchdog behavior.
The stream parser also adds a provider-specific top-level context while the query loop only
recognizes the generic `error reading response chunk` text for partial-response recovery.

## Goal

1. Prevent the HTTP client's 120-second total timeout from terminating an otherwise active OpenAI streaming response.
2. Preserve the underlying reqwest cause in query-loop diagnostics.
3. Accept usable partial text for generic and provider-qualified response-chunk read errors when no tool call is present.
4. Retry the same model once when a retryable stream interruption occurs before any assistant content or tool call, avoiding duplicate tool execution.

## Implementation

- Override the OpenAI streaming request timeout with a long-lived bound; the query-layer idle and stall watchdogs remain the authoritative progress limits.
- Format stream errors with their anyhow cause chain before classification and persistence.
- Centralize response-chunk/idle/stall interruption recognition in query recovery helpers.
- Add one same-model retry only for an empty accumulator. Keep partial tool-use failures terminal and keep the existing fallback-model path unchanged.
- Add API and query recovery regression tests covering the provider-qualified OpenAI error and the empty-stream retry boundary.

## Verification

1. Focused `allthecodes-api` and `allthecodes-engine` tests
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace --exclude allthecodes --lib`
5. `cargo build --workspace --release`
6. `git diff --check`

## Workflow

Implement in the same per-session worktree as the paired TaskList correction, but commit
runtime changes separately. Include the live 120014 ms failure evidence and all validation
results in `development/worktree-workflow-artifacts/2026-07-19-tui-task-list-default-and-stream-recovery.html`.

## 2026-07-20 stall-progress regression follow-up

> Follow-up task slug: `openai-stream-stall-progress-recovery`

### Live evidence

Session `ddc64e0a-e103-43ec-8a37-dbd5f784a65d` received no assistant progress for
76121 ms and then delivered a progress event. The query loop checked the elapsed stall
duration only after identifying that event as real progress, rejected the newly arrived
progress, and retried the same model. The retry established an HTTP 200 SSE stream but did
not complete before the session ended. This exposes an inverted watchdog boundary:
non-progress events never trigger the stall check, while the first useful event after a
long reasoning pause does.

### Correction

- Always accept a progress event and reset `last_progress_at`, even when it follows a pause
  longer than the stall threshold.
- Apply the stall threshold when a non-progress event arrives without intervening assistant
  progress. This preserves protection against heartbeat-only streams.
- Keep the independent idle timeout for streams that produce no events at all.
- Add deterministic query-loop tests for delayed useful progress and heartbeat-only stall
  recovery, while retaining the single empty-stream retry and no-duplicate-tool boundaries.

### Follow-up workflow and verification

Implement in `.worktrees/openai-stream-stall-progress-recovery` on branch
`worktree/openai-stream-stall-progress-recovery`. Record the change and live evidence in
`development/worktree-workflow-artifacts/2026-07-20-openai-stream-stall-progress-recovery.html`.
Run focused engine recovery tests, formatting, workspace clippy, non-PTY workspace library
tests, a workspace release build, and `git diff --check`. This engine-only correction does
not require the PTY TUI suite.
