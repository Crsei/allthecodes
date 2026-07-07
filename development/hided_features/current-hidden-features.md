# Current Hidden, Unreleased, and Placeholder Features

This note records features and compatibility surfaces that are intentionally
kept hidden or unreleased after the 2026-07-07 release pass. Features released
from the previous hidden list are recorded in
[`released-features.md`](released-features.md).

## Runtime Feature Gates Kept Hidden

These gates remain disabled by default and should not be advertised as released
user-facing features.

- `FEATURE_KAIROS_GITHUB_WEBHOOKS`: GitHub webhook integration still needs
  release evidence for remote-control safety, operator-facing administration,
  and public exposure boundaries. It still requires `FEATURE_KAIROS` when used
  in local testing.
- `FEATURE_TEAMMEM`: team memory scope injection/recall remains an active gap.
  Do not release until sync, disconnect, conflict handling, e2e coverage, and
  docs closeout are complete.
- `FEATURE_SUBAGENT_DASHBOARD`: the companion dashboard surface depends on the
  Bun dashboard server path and extra runtime environment. Keep hidden/default
  off. The Rust TUI `/subagents` local surface is separate.
- `FEATURE_MCP_SKILLS`: MCP `skill://` resource ingestion remains experimental.
- `FEATURE_EXPERIMENTAL_SKILL_SEARCH`: local skill search prefetch and turn-zero
  discovery scaffolding remains experimental.

Source references:

- `crates/allthecodes-config/src/features.rs`
- `crates/allthecodes-daemon/src/team_memory_proxy.rs`
- `crates/allthecodes/src/dashboard.rs`
- `crates/allthecodes/src/startup/mode_router.rs`

## Channel And Remote Boundaries Not Released

KAIROS and `/channels` are released only as opt-in/beta local surfaces. These
remote channel behaviors remain unreleased:

- Telegram/Lark inbound channel sessions.
- Remote-triggered inbound sessions.
- Teleport-style channel handoff semantics beyond the currently wired gateway
  and outbound adapter status/control surface.

Source references:

- `development/archive/IMPLEMENTATION_GAPS.md`
- `crates/allthecodes-commands/src/channels.rs`
- `crates/allthecodes-commands/src/remote_cmd.rs`

## Registered But Placeholder Or Unsupported

- `/voice`: registered for compatibility, but runtime voice support is
  explicitly unsupported in this build; push-to-talk is disabled.
- `/goal edit`: registered under `/goal`, but the command surface returns a
  clear unsupported message and asks users to use `/goal clear` then set a new
  goal.
- `/login` Microsoft Foundry path: exposed in auth/status text, but reports
  unsupported in this allthecodes build.
- Dynamic plugin and MCP slash commands: metadata can be registered, but
  execution currently returns routing/registration text rather than a concrete
  inline handler.

Source references:

- `crates/allthecodes-commands/src/voice_cmd.rs`
- `crates/allthecodes-commands/src/goal.rs`
- `crates/allthecodes-commands/src/login.rs`
- `crates/allthecodes-commands/src/lib.rs`

## Compatibility Settings With No Runtime Effect

The settings schema accepts these legacy Electron or compatibility fields, but
the Rust TUI currently treats them as unsupported/no-op compatibility fields:

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

- Historical `deferred` entries in `docs/WORK_STATUS.md` should not be
  interpreted as permanently out of scope. The current branch policy says they
  must be re-evaluated when touched.
- TUI slash command completion uses `allthecodes_commands::get_all_commands()`,
  so command visibility and command executability are separate concerns.
- Released opt-in/beta features may still require an explicit env var or slash
  command activation. That is a release mode, not a hidden/unreleased state.
