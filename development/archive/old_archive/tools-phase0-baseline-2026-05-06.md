# Tools Phase 0 Baseline

Date: 2026-05-06
Plan: `.omx/plans/tools-implementation-phase-plan.md`
Source map: `architecture/tools-implementation-map.md`

## Decision

Phase 0 treats the implemented tool families as the baseline and narrows follow-up work to Tasks V2 parity. The map records no core tool family as unimplemented; task management is the only partially implemented tools area.

## Baseline Evidence

- `architecture/tools-implementation-map.md` marks tool system, file operations, search/navigation, shell execution, and web tools as implemented.
- Existing `tasks.rs` tests cover TodoWrite replacement/clear semantics, incremental task IDs and high-water marks, dependency aliases, reverse `blocks` output, owner claim checks, `checkAgentBusy`, TaskList, TaskStop, and TaskOutput wait/timeout behavior.
- Phase 0 adds ignored gap tests for the remaining Tasks V2 work:
  - cross-store claim must fail after another store has claimed the same task;
  - TaskCreate/TaskUpdate must expose Bun V2 `activeForm`, `metadata`, and update fields;
  - task-list-id resolution and teammate exit unassign still require runtime integration tests.

## Follow-Up For Phase 1

Implement task-list-id resolution and storage isolation before changing cross-process locking. The next phase should add concrete non-ignored tests for environment/team/session task-list selection and ensure all paths stay under `$CC_RUST_HOME/tasks/<task-list-id>/`.
