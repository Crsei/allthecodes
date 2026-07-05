# Hermes TUI Switch Plan

## Goal

Add a Hermes runtime switch that is enabled from the Rust TUI with `/hermes`.
The switch controls Hermes autonomous behavior while keeping manual management
commands available.

## User Decisions

- Scope: support user-level and project-level startup settings.
- Default command: bare `/hermes` enables Hermes when it is off, and shows
  status when it is already on.
- Default write target: project settings at `.allthecodes/settings.json`.
- Merge rule: On wins. If any user/project layer sets Hermes to `true`, the
  effective runtime is enabled; a `false` value in another layer cannot override
  that `true`.
- Gated surface: autonomy only. Model-visible autonomous tools, background
  review, and scheduled autonomous dispatch are controlled by Hermes. Manual
  slash commands remain available.

## Command Shape

```text
/hermes
/hermes status
/hermes on [--project|--user]
/hermes off [--project|--user]
```

`/hermes` without arguments writes `hermesEnabled=true` to project settings when
effective Hermes is off. When effective Hermes is already on, it performs a
status read only.

## Implementation Notes

- Add a typed `hermesEnabled` boolean setting across raw, effective, runtime,
  and schema projections.
- Implement the On-wins merge rule in the settings merge layer so it is shared
  by TUI, Web, engine, and daemon code.
- Register `/hermes` in the existing slash command registry used by the Rust TUI.
- After `/hermes` writes settings, refresh the command context's runtime
  settings so the next turn observes the new gate immediately.
- Filter Hermes autonomous model tools when effective Hermes is off.
- Skip automatic background review staging when effective Hermes is off.
- Skip automatic daemon scheduler dispatch when effective Hermes is off. The
  daemon still requires its existing startup path and feature gates; `/hermes`
  does not spawn or stop daemon processes.

## Verification

- Config tests cover `hermesEnabled` parse, schema, and On-wins merge.
- Command tests cover project default write, user write, status, and an `off`
  command that remains effectively on because another layer is true.
- Tool registry tests cover Hermes tools hidden by default and visible when
  Hermes is enabled.
- Engine/daemon tests cover background review and scheduled dispatch disabled
  while Hermes is off.
