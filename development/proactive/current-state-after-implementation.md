# Proactive Current State After Full Parity

Source-verified after implementing `development/proactive/2026-07-06-proactive-full-parity-plan.md`.

## Implemented

- `FEATURE_PROACTIVE=1` can run standalone.
- `FEATURE_KAIROS=1` implies proactive.
- `/proactive` toggles session proactive state and mirrors active/next tick state for daemon workers.
- `/sleep` and `SleepTool` use the shared sleep-state schema at `~/.allthecodes/daemon/sleep-state.json` or `ALLTHECODES_HOME`.
- TUI local tick driver submits `QuerySource::ProactiveTick` when proactive is active and idle.
- Daemon proactive worker queues `source=proactive_tick`, honors durable `/proactive` disabled state, and uses the shared tick payload builder.
- New user or remote work clears active sleep before queueing.
- `/api/status` and gateway run metadata expose automation state, including `next_tick_at`, `proactive_active`, terminal focus, query running, and pending input.
- Daemon SSE client lifecycle mirrors terminal focus for proactive tick payloads.
- Context blocking pauses proactive ticks during unsafe autonomous work windows and resumes after compact/context recovery.
- Focused no-provider proactive tests cover services, commands, daemon, engine, and TUI-facing registration paths.

## Verification

```bash
cargo test -p allthecodes-services proactive
cargo test -p allthecodes-commands proactive
cargo test -p allthecodes-engine proactive
cargo test -p allthecodes-daemon proactive
cargo test -p allthecodes proactive
```

Live provider soak is run only when credentials are available.
