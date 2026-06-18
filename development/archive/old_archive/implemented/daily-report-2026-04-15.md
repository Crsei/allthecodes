# Daily Report — 2026-04-15

**Commits:** 17 | **Files changed:** 64+ | **Lines added:** ~7,200+ | **Lines removed:** ~240

## Summary

Major push on three fronts: Agent IPC subsystem, plugin tool runtime, and two new integration features (Codex CLI credential fallback, ink-terminal UI experiment).

## Work Areas

### 1. Agent IPC Extensions (9 commits)

Built the full agent/team IPC pipeline from scratch:

- **Shared types** (`src/ipc/agent_types.rs`) — `AgentNode` (recursive tree), `AgentInfo`, `TeamMemberInfo` with serde tagged representation
- **Event/command enums** (`src/ipc/agent_events.rs`) — `AgentEvent`, `AgentCommand`, `TeamEvent`, `TeamCommand` for lifecycle, streaming, tool use, and team coordination
- **Agent channel** (`src/ipc/agent_channel.rs`) — `AgentIpcEvent` wrapper + typed mpsc sender/receiver
- **Agent tree manager** (`src/ipc/agent_tree.rs`) — Global agent hierarchy: register, update state, build nested snapshots, query active agents, prune completed
- **Protocol integration** (`src/ipc/protocol.rs`, `headless.rs`) — Wired agent/team variants into `BackendMessage`/`FrontendMessage`, added command handlers
- **Engine migration** — Replaced `BgAgentSender` with `AgentSender(AgentIpcEvent)` across engine lifecycle, tool.rs, and tool_impl.rs
- **Lifecycle events** — Agents now emit Spawned/Completed events; background agent stream events (text deltas, thinking, tool use) forwarded through IPC for frontend observability
- **SystemStatus extension** — Added agents and teams subsystems to the `/status` tool output

### 2. Plugin Tool Runtime (2 commits)

- Plugin manifests can declare executable stdio runtimes; startup registers plugin tools into the shared `Tool` pipeline
- Added `/plugin` command (list/status/enable/disable) and `PluginToolWrapper` for subprocess-based execution
- Refactored MCP discovery to merge configs from plugins -> global -> project with name-based override precedence
- Added `load_skill_from_file_path` for plugin skill integration
- E2E test coverage (`tests/e2e_plugin_tools.rs`)

### 3. Codex CLI Credential Fallback (2 commits)

- New module `src/auth/codex_cli.rs`: parses `~/.codex/auth.json`, decodes JWT expiry
- Extended auth priority chain: env -> credentials.json -> ~/.codex/auth.json
- Extracted `refresh_token_with_client_id()` for Codex CLI's client_id
- Added `/login 5` (codex-cli) command for manual verification
- Fixed `.env.example` — commented out `CC_BACKEND=native` default that broke `/login 4`

### 4. ink-terminal UI Experiment (2 commits)

- Scaffolded independent terminal UI at `rust/ink-ui/` using ink-terminal as rendering library
- Reuses same headless JSONL IPC protocol as the existing OpenTUI UI
- Includes IPC client/protocol/context, store (app-store, message-model), minimal REPL App.tsx
- Launch scripts for both bash and PowerShell

### 5. Misc Cleanup (2 commits)

- Reformatted single-field enum variants to inline style per rustfmt
- Added notification hook on query finish for frontend sound/alert feedback
- Removed dead code warnings across touched modules

## Key Design Decisions

| Decision | Chosen | Rejected |
|----------|--------|----------|
| Plugin tool execution | Reuse existing Tool pipeline | Separate plugin-only executor (would duplicate permissions/hooks) |
| Agent IPC channel | Unbounded mpsc with typed `AgentIpcEvent` | Reusing existing BackendMessage channel (insufficient granularity) |
| MCP config merging | Name-based override: plugin -> global -> project | Flat concatenation (no dedup) |
| Codex auth fallback | Auto-detect in resolve chain | Require manual `/login 5` only |

## Files of Note

| Path | What |
|------|------|
| `src/ipc/agent_*.rs` (5 files) | Entire agent IPC subsystem — new |
| `src/plugins/tools.rs` | Plugin tool wrapper and subprocess runtime — new |
| `src/auth/codex_cli.rs` | Codex CLI credential parser — new |
| `ink-ui/` | Entire ink-terminal UI scaffold — new |
| `tests/e2e_plugin_tools.rs` | Plugin tool E2E tests — new |
