# Released Hidden Feature Decisions

This note records the 2026-07-07 release decision for items that were previously
tracked in `current-hidden-features.md`. "Released" here means the surface is no
longer tracked as hidden/unreleased. Some released beta surfaces still require
explicit opt-in because they start background work, expose orchestration tools,
or depend on local daemon setup.

## Released By Default

These full-build tool gates are released and default-enabled. The env vars stay
as explicit disable switches for operators and tests.

- `ALLTHECODES_WORKFLOW_SCRIPTS`: controls `Workflow`, `workflow`, and
  `DynamicWorkflow`.
- `ALLTHECODES_GOAL_TOOLS`: controls `GetGoal`, `CreateGoal`, `UpdateGoal`, and
  aliases.
- `ALLTHECODES_MULTI_AGENT_V2`: controls the multi-agent v2
  `ListAgents`/`FollowupTask`/`WaitAgent`/`CloseAgent` aliases. `TeamSpawn` and
  `SendMessage` are controlled by Agent Teams/Coordinator gates, not this gate.

Source references:

- `crates/allthecodes-config/src/features.rs`
- `crates/allthecodes-tools/src/registry.rs`
- `crates/allthecodes-startup/src/tool_registry.rs`

## Released Deferred Tool Discovery

Deferred discovery is released as the supported way to keep the core tool schema
small while still exposing extra tools. Models should use `SearchExtraTools` to
discover deferred tools and `ExecuteExtraTool` to execute them.

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

## Released As Opt-In / Beta

These surfaces are released from the hidden list, but remain explicit opt-in or
session-activated beta features.

- `FEATURE_PROACTIVE` and `/proactive`: released as an explicit toggle. The
  assistant continues work from periodic idle ticks only after the user enables
  proactive mode. `/sleep` is released as part of this surface and works when
  proactive mode is active or `FEATURE_PROACTIVE` is enabled.
- `FEATURE_KAIROS`: released as the opt-in local assistant daemon/KAIROS bundle
  for `/assistant`, `/dream`, and the resident daemon path. KAIROS implies
  proactive mode when enabled.
- `FEATURE_KAIROS_BRIEF`: released with the KAIROS bundle for `/brief` and
  `BriefTool`; still requires `FEATURE_KAIROS`.
- `FEATURE_KAIROS_CHANNELS`: released with the KAIROS bundle for `/channels`;
  still requires `FEATURE_KAIROS`. The released surface is gateway/outbound
  adapter status/control, not inbound channel sessions.
- `FEATURE_KAIROS_PUSH_NOTIFICATION`: released with the KAIROS bundle for
  `/notify`; still requires `FEATURE_KAIROS`.
- `FEATURE_AGENT_TEAMS` / `ALLTHECODES_EXPERIMENTAL_AGENT_TEAMS`: released as
  beta Agent Teams tooling for `/team`, `TeamSpawn`, `SendMessage`, and related
  team surfaces. The env var name remains for compatibility, but the release
  status is beta rather than hidden.
- `ALLTHECODES_COORDINATOR_MODE` and `/coordinator`: released as a beta
  coordinator surface. `/coordinator start` can enable coordinator mode for the
  current session and also enables Agent Teams in the runtime override.
- KAIROS bridge session reuse: released inside the opt-in KAIROS daemon surface.
  Durable bridge state remains under `~/.allthecodes/daemon/bridge/`; bridge
  controls can inspect or prepare sessions, while resident processing still
  requires the KAIROS daemon worker path.

Source references:

- `crates/allthecodes-config/src/features.rs`
- `crates/allthecodes-commands/src/proactive_cmd.rs`
- `crates/allthecodes-commands/src/sleep_cmd.rs`
- `crates/allthecodes-commands/src/brief.rs`
- `crates/allthecodes-commands/src/dream.rs`
- `crates/allthecodes-commands/src/channels.rs`
- `crates/allthecodes-commands/src/notify.rs`
- `crates/allthecodes-commands/src/coordinator.rs`
- `crates/allthecodes-daemon/src/bridge_session.rs`
- `crates/allthecodes-daemon/src/bridge_worker.rs`
- `crates/allthecodes-commands/src/daemon_cmd.rs`
- `crates/allthecodes-startup/src/tool_registry.rs`

## Release Boundaries

- Agent Teams beta intentionally does not include the upstream tmux/iTerm pane
  backend.
- Agent Teams still carries the known residual `TEAMS-001` around teammate
  self-session resume until `TeamMember.session_id` persistence is fully closed.
- KAIROS beta intentionally does not include GrowthBook/public bridge parity or
  the unreleased GitHub webhook gate.
- KAIROS `/channels` beta intentionally does not include Telegram/Lark inbound
  sessions or remote-triggered inbound sessions.
- Placeholder surfaces that still return unsupported/routing-only responses
  remain documented in `current-hidden-features.md`.
