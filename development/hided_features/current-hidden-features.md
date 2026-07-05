# Current Hidden, Gated, and Placeholder Features

This note records the current source-verified state of features, TUI slash commands,
and compatibility surfaces that are disabled, feature-gated, hidden, deferred, or
placeholder-only in the full-build branch.

## Runtime Feature Gates Disabled By Default

These gates default to off unless enabled through environment variables or a runtime
override such as `/experimental`.

- `FEATURE_KAIROS`: main Kairos / assistant daemon gate.
- `FEATURE_KAIROS_BRIEF`: enables `/brief` and `BriefTool`; requires `FEATURE_KAIROS`.
- `FEATURE_KAIROS_CHANNELS`: enables `/channels`; requires `FEATURE_KAIROS`.
- `FEATURE_KAIROS_PUSH_NOTIFICATION`: enables `/notify`; requires `FEATURE_KAIROS`.
- `FEATURE_KAIROS_GITHUB_WEBHOOKS`: GitHub webhook integration gate; requires `FEATURE_KAIROS`.
- `FEATURE_PROACTIVE`: enables proactive sleep/tick surfaces such as `/sleep` and `SleepTool`; also implied by `FEATURE_KAIROS`.
- `FEATURE_TEAMMEM`: enables team memory scope injection/recall.
- `FEATURE_SUBAGENT_DASHBOARD`: enables the subagent dashboard companion surface.
- `FEATURE_AGENT_TEAMS` / `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS`: enables experimental Agent Teams surfaces.
- `ALLTHECODES_COORDINATOR_MODE`: enables coordinator mode prompt/orchestration behavior.

Source reference: `crates/allthecodes-config/src/features.rs`.

KAIROS bridge session reuse is implemented, but its background worker is still
under the daemon/KAIROS runtime surface. Durable bridge state lives only under
`~/.allthecodes/daemon/bridge/`; the `allthecodes daemon bridge ...` and
`/daemon bridge ...` controls can inspect or prepare sessions, while actual
resident bridge processing requires the KAIROS daemon worker path to be active.

Source references:

- `crates/allthecodes-daemon/src/bridge_session.rs`
- `crates/allthecodes-daemon/src/bridge_worker.rs`
- `crates/allthecodes-commands/src/daemon_cmd.rs`

## Slash Commands That Execute As Gated By Default

These commands are registered in the TUI slash command list, but return a feature
gate message in the default environment:

- `/brief`: returns `Brief mode requires FEATURE_KAIROS_BRIEF=1`.
- `/dream`: returns `Dream mode requires FEATURE_KAIROS=1`.
- `/channels`: returns `Channels require FEATURE_KAIROS_CHANNELS=1`.
- `/notify`: requires `FEATURE_KAIROS_PUSH_NOTIFICATION=1`.
- `/sleep`: returns `Sleep command requires FEATURE_PROACTIVE=1`.

Source references:

- `crates/allthecodes-commands/src/brief.rs`
- `crates/allthecodes-commands/src/dream.rs`
- `crates/allthecodes-commands/src/channels.rs`
- `crates/allthecodes-commands/src/notify.rs`
- `crates/allthecodes-commands/src/sleep_cmd.rs`

## Registered But Placeholder Or Partially Unsupported

- `/voice`: registered for compatibility, but runtime voice support is explicitly
  unsupported in this build; push-to-talk is disabled.
- `/goal edit`: registered under `/goal`, but the command surface returns a clear
  unsupported message and asks users to use `/goal clear` then set a new goal.
- `/login` Microsoft Foundry path: exposed in auth/status text, but reports
  unsupported in this allthecodes build.
- Dynamic plugin and MCP commands: metadata can be registered, but execution
  currently returns routing/registration text rather than a concrete inline handler.

Source references:

- `crates/allthecodes-commands/src/voice_cmd.rs`
- `crates/allthecodes-commands/src/goal.rs`
- `crates/allthecodes-commands/src/login.rs`
- `crates/allthecodes-commands/src/lib.rs`

## Tool Gates Enabled By Default But Explicitly Disableable

These full-build tool gates default to enabled, but can be removed from model-visible
tool schemas by setting the env var to `0`, `false`, or `no`.

- `ALLTHECODES_WORKFLOW_SCRIPTS`: controls `Workflow`, `workflow`, and `DynamicWorkflow`.
- `ALLTHECODES_GOAL_TOOLS`: controls `GetGoal`, `CreateGoal`, `UpdateGoal`, and aliases.
- `ALLTHECODES_MULTI_AGENT_V2`: controls multi-agent v2 tools such as `ListAgents`,
  `FollowupTask`, `WaitAgent`, `CloseAgent`, `TeamSpawn`, and `SendMessage`.

Source reference: `crates/allthecodes-tools/src/registry.rs`.

## Deferred Tool Visibility

Some tools are not disabled or unimplemented, but are intentionally hidden from the
core visible schema and exposed through deferred discovery. The model should use
`SearchExtraTools` to discover them and `ExecuteExtraTool` to execute them.

Examples covered by tests include:

- `CronCreate`
- `WebBrowser`
- `Workflow`
- `LocalMemoryRecall`
- `VaultHttpFetch`
- `GetGoal`
- `ViewImage`
- `ListAgents`

Source reference: `crates/allthecodes-tools/src/deferred_tools.rs`.

## Compatibility Settings With No Runtime Effect

The settings schema accepts these legacy Electron or compatibility fields, but the
Rust TUI currently treats them as unsupported/no-op compatibility fields:

- `appIcon`
- `autoStart`
- `startMinimized`
- `minimizeToTray`
- `closeToTray`
- `quickChatHideOnBlur`
- `quickChatInjectScreen`
- `quickChatAmbient`
- `autoApproveTools`
- `analyticsEnabled`
- `teammateMode`

Source reference: `docs/schemas/settings.schema.json`.

## Notes

- Historical `deferred` entries in `docs/WORK_STATUS.md` should not be interpreted
  as permanently out of scope. The current branch policy says they must be
  re-evaluated when touched.
- TUI slash command completion uses `allthecodes_commands::get_all_commands()`, so
  command visibility and command executability are separate concerns.
