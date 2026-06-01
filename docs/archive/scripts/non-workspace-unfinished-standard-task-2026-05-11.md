# Non-Workspace Script Backlog Standard Task

Date: 2026-05-11

This package converts the unfinished script lanes outside
`scripts/workspace_crate_extraction_omx_py/` into one standard-task-list item.
Run it through `scripts/standard_task_list_omx_py/` rather than the older
lane-specific wrappers.

## Standard Runner

Task list:

```text
docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt
```

Dry run:

```powershell
python .\scripts\standard_task_list_omx.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --dry-run
```

Supervisor status:

```powershell
python .\scripts\standard_task_list_omx_supervisor.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --observed-output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --supervisor-output-root target/codex-runs/non-workspace-unfinished-standard-task-supervisor `
  --plan-only
```

Execution:

```powershell
python .\scripts\standard_task_list_omx_supervisor.py `
  --tasks-file docs/scripts/non-workspace-unfinished-standard-task-2026-05-11.txt `
  --observed-output-root target/codex-runs/non-workspace-unfinished-standard-task `
  --supervisor-output-root target/codex-runs/non-workspace-unfinished-standard-task-supervisor `
  -- --skip-final-validation
```

Remove `--skip-final-validation` for the release gate once the package-level
repair and scoped tests are green.

## Excluded Scope

Do not execute, rewrite, or close out the workspace crate extraction lane from
this package:

- `scripts/workspace_crate_extraction_omx_py/**`
- `scripts/workspace-crate-extraction-omx/**`
- `scripts/run-workspace-crate-extraction-omx.ps1`
- `scripts/workspace_crate_extraction_omx.py`
- `scripts/workspace_crate_extraction_omx_supervisor.py`
- `docs/scripts/workspace-crate-extraction-*`
- `target/codex-runs/workspace-crate-extraction-*`

## Included Evidence

Treat these older lanes as evidence that must be read before deciding whether a
follow-up is still required. Do not blindly rerun completed work.

Completed or superseded lanes:

- P1 defensive after phase 1:
  `docs/scripts/achieve/p1-defensive-fail-fast-after-phase1-tasks.txt`,
  `target/codex-runs/p1-defensive-after-phase1/**`, and
  `target/codex-runs/session-07-final-verification/task-01.last-message.txt`.
  The later final verification reports targeted checks passed.
- P1 review-B residual:
  `scripts/achieve/run-p1-review-b-residual-sessions.ps1` and
  `target/codex-runs/p1-review-b-residual-*/**`. Its final verification was
  blocked by a missing `crates/gateway/Cargo.toml`, which is now present; do
  not reopen implementation unless current checks still fail.
- Remote-control gateway:
  `docs/scripts/achieve/remote-control-gateway-omx-tasks-2026-05-08.txt`,
  `scripts/achieve/run-remote-control-gateway-omx-sessions.ps1`, and
  `target/codex-runs/remote-control-gateway/session-17-final-verification/task-01.last-message.txt`.
  Session 17 reports the remote-control targeted checks passed with remaining
  test-coverage risk, not an unfinished script task.

Open lanes to complete:

- Generic selectable command surface:
  `scripts/tmp/run-generic-selectable-command-surface-sessions.ps1`,
  `.omx/plans/generic-selectable-command-surface-medium-session-plan-2026-05-08.md`,
  and `docs/plan/generic-selectable-command-surface-plan-2026-05-08.md`.
  Current code has no `SelectableListState`, no `selectable_list` module, and
  no `CommandSurface::Plugin` / plugin command-surface module. Complete the
  first-version DoD or, if concurrent work has landed before execution, close
  it with evidence.
- Ratatui UI parity repair and closeout:
  `target/codex-runs/ratatui-ui-parity-omx/repair-followup.tasks.txt`,
  `target/codex-runs/ratatui-ui-parity-repair/batch-01/task-01.last-message.txt`,
  and the ratatui parity output roots. The repair attempt reports
  `ERROR: TEST-002 is not fully repaired yet`; the verification gate and
  packaging plan have not run.

## Single Task Contract

The standard task must:

1. Read this file first, then read only the included evidence needed for the
   current substep.
2. Preserve unrelated dirty worktree changes. Do not revert or stage files
   outside the substep's ownership.
3. Finish the generic selectable command-surface first-version DoD:
   add or verify a thin generic selectable-list state, keep it
   business-agnostic, wire the `/plugin` empty-args command surface only when
   safe, preserve existing `/plugin <args>` text behavior, and add focused
   unit/snapshot coverage.
4. Finish the ratatui parity repair backlog:
   reproduce the remaining package-wide failures from the repair last-message,
   fix concrete shared-state/test-state leaks in scope, rerun focused filters,
   run the verification gate from `repair-followup.tasks.txt`, and write the
   packaging summary requested there.
5. Reconcile the completed/superseded lanes. If current evidence proves an old
   final verification blocker is already fixed, document that in the package
   summary instead of redoing old implementation sessions.
6. Update only the docs needed to record current effects, defects,
   verification, and remaining risks. Use `docs/KNOWN_ISSUES.md` only for
   current user-visible issues.
7. Run `git diff --check`, focused Rust tests for changed code, and the
   standard-task-list focused Python tests if orchestration scripts or this
   package are edited.
8. Return `PASS`, `WARNING`, `BLOCKER`, or `ERROR` explicitly, with changed
   files, verification evidence, and remaining risks.

## Completion Evidence

The package is complete when the standard runner has a PASS/WARNING summary for
the one task and the final message identifies:

- generic selectable implementation or evidence-backed closure;
- ratatui repair result and verification gate result;
- any completed lane that was intentionally not rerun;
- changed files and commits, if the runner was allowed to commit;
- remaining risks that are tracked in docs rather than hidden in runner logs.
