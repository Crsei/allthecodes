# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is allthecodes?

allthecodes 是一个高性能、全平台的 AI 编程助手（coding agent harness），用 Rust 构建。它参考 Claude Code 的交互模型、工具系统和 agent 工作流，但不是官方版本——它在对照 Claude Code 行为的基础上，重新整理了工程结构、运行时边界、IPC、Rust TUI、权限模型和工具系统，并融合了作者自己对 coding agent 的理解。

仓库包含 Rust 后端（cargo workspace）和一个配套的 Next.js 前端。

## Repositories

| 仓库 | 路径 | 技术栈 |
|------|------|--------|
| **allthecodes** (本仓库) | `allthecodes/` | Rust workspace, ~40 crates |
| **allthecodes-web** (前端) | `../allthecodes-web/` | Next.js 15, React 19, TS strict |

## Build & Test

```bash
# Build everything (workspace)
cargo build --workspace                 # debug
cargo build --workspace --release       # release binary

# Run
cargo run --release                     # interactive TUI (default)
cargo run --release -- -p "<prompt>"    # non-interactive single query
cargo run --release -- --web --web-port 17322   # Web UI mode

# Lint & format
cargo clippy --workspace --lib --bins
cargo fmt --all --check

# Test
cargo test --workspace                  # all tests
cargo test -p allthecodes-tools         # single crate
cargo test -p allthecodes-query         # query loop tests

# Install Linux build dependencies
sudo apt-get install pkg-config libssl-dev libcap-dev
```

CI runs `cargo clippy --workspace --lib --bins` (on push/PR to main/master).  
Release builds use GitHub Actions (`v*.*.*` tag required).

## Workspace Architecture

Workspace root at `crates/*` — 40 member crates. All deps declared in root `Cargo.toml` workspace table; features stay crate-local.

### Layer 1 — Entrypoint & Modes

| Crate | Role |
|-------|------|
| `allthecodes` | CLI binary (`main.rs`), TUI, mode dispatch (TUI / headless / daemon / web). Modules: `cli.rs` (clap args), `full_init.rs` (Phase B init), `dashboard.rs`, `shutdown.rs` |
| `allthecodes-startup` | Startup orchestration, environment probing |

Phase structure in `main.rs`: **Phase A** (CLI parsing → fast path → exit), **Phase B** (full init → REPL), **Phase I** (shutdown/cleanup).

### Layer 2 — Engine & Query Loop

| Crate | Role |
|-------|------|
| `allthecodes-engine` | `QueryEngine`, agent runtime, lifecycle, prompt sections, system prompt, tool runtime, MCP tool adapter, input processing, effort, worktree hooks. `agent/`, `query/`, `tool_runtime/`, `lifecycle/` |
| `allthecodes-query` | Streaming query loop (`loop_impl.rs`), token budget (`token_budget.rs`), turn context (`turn_context.rs`), stop hooks |

The query loop (`allthecodes-query`) drives the main agent loop:
user message → model call → tool execution → model call → ... → reply.  
`allthecodes-engine` provides the runtime structures (`agent_runtime.rs`, `AppState`, `ToolUseContext`).

### Layer 3 — Tools

| Crate | Contents |
|-------|----------|
| `allthecodes-tools` | Tool specs, registry, and implementations: `fs/` (read/write/edit/glob/grep/apply_patch), `exec/` (bash/powershell/pty/sleep), `network/`, `media/`, `memory/`, `notifications/`, `plan_mode.rs`, `workflow/`, `goals/`, `tasks/`, `skills/`, `hooks/`, `interaction/`, `product/` |
| `allthecodes-mcp` | MCP client (Model Context Protocol) |
| `allthecodes-browser` | Browser automation (headless Chrome) |
| `allthecodes-computer-use` | Computer use capabilities |
| `allthecodes-shell-command` | Shell command execution with timeout/process group |
| `allthecodes-sandbox` | Sandboxing |
| `allthecodes-lsp-service` | LSP (Language Server Protocol) integration |

Tools are registered via `registry.rs` and dispatched through a `Tool` trait. Feature-gated behind `#[cfg(feature = "full")]`.

### Layer 4 — Protocol & IPC

| Crate | Role |
|-------|------|
| `allthecodes-protocol` | V1 protocol types: agents, capabilities, chat, chat_modes, files, gateways, hooks, kanban, models, people, plugins, profiles, prompts, providers, skills, workspaces |
| `allthecodes-types` | Pure leaf types: agent types, agent events, MCP types, message types, SDK types, permission events, state, status line, plan workflow |
| `allthecodes-ipc` | Headless IPC (JSONL over stdio): agent handlers, subsystem events |
| `allthecodes-ipc-protocol` | IPC envelope types, subsystem event definitions |
| `allthecodes-ipc-transport` | IPC transport layer (framing, JSONL) |
| `allthecodes-ipc-adapters` | IPC adapter layer (callbacks, event class, ingress, sdk mapping) |
| `allthecodes-ipc-client` | IPC client (query runner, request dispatch, transport) |

Headless/JSONL mode allows external frontends, automated tests, and integrations to drive the agent through stdio.

### Layer 5 — Server & Web

| Crate | Role |
|-------|------|
| `allthecodes-daemon` | Background daemon: gateway (HTTP API), server (axum), scheduler loop, webhook, supervisor, SSE, process state management, team memory proxy |
| `allthecodes-web` | Web UI server: handler registry, static files, workspace metadata, WebSocket handlers |
| `allthecodes-gateway` | Gateway |

### Layer 6 — Support

| Crate | Role |
|-------|------|
| `allthecodes-config` | Config resolution pipeline: `raw → source → effective`. Settings (schema/load/write/effective/first_run). Path management (`paths.rs`), CLAUDE.md detection (`claude_md.rs`), feature flags (`features.rs`) |
| `allthecodes-auth` | API keys, OAuth, credentials, keychain |
| `allthecodes-permissions` | Permission modes (Auto/ACD/Bypass), tool-level permission control |
| `allthecodes-session` | Session persistence & restore, context compression |
| `allthecodes-skills` | Skill loading, definition format, usage stats |
| `allthecodes-plugins` | Plugin discovery, installation, runtime loading |
| `allthecodes-commands` | Slash command system (including model resolution) |
| `allthecodes-observability` | OpenTelemetry tracing, Langfuse integration |
| `allthecodes-langfuse` | Langfuse-specific observability |
| `allthecodes-services` | Service layer |
| `allthecodes-bootstrap` | Bootstrap initialization |
| `allthecodes-worktree` | Git worktree management |
| `allthecodes-keybindings` | TUI keybindings |
| `allthecodes-voice` | Voice support |
| `allthecodes-teams` | Team collaboration |
| `allthecodes-tasks` | Task management system |
| `allthecodes-compact` | Compact/compressed representation |
| `allthecodes-safety` | Safety checks |
| `allthecodes-utils` | Shared utility functions |
| `allthecodes-models` | Model definitions |

## Toolchain

```
Rust: 1.91.1 (rust-toolchain.toml)
Components: rustfmt, clippy
Profile: minimal
```

Lint policy (`clippy.toml` + `[workspace.lints]`):
- `unwarp_used`, `expect_used`, `panic`, `panic_in_result_fn` → warn in production
- Test code exempted via clippy.toml

## Development Conventions

- CI: `.github/workflows/ci.yml` → clippy on push/PR
- Release: `.github/workflows/release.yml` — triggered by `v*.*.*` tag, cross-platform builds (linux x64/arm64, darwin x64/arm64, win x64/arm64), npm package distribution
- NPM packaging: `package.json` for optional native binaries; `scripts/stage_npm_packages.py` for staging
- `.cargo/config.toml`: MSVC lld linker config; Linux/macOS linker comments left as reference
- Path isolation: `~/.allthecodes/` for global data, `.allthecodes/settings.json` for project config (avoid conflict with Claude Code/Codex)
- DEBUG: `LIB` and `INCLUDE` env vars force-cleared in cargo config to avoid Windows build issues

## Frontend (allthecodes-web)

| Command | Purpose |
|---------|---------|
| `npm run dev` | Dev server (port 17321, proxies `/api/*` and `/ws/*` to :17322) |
| `npm run dev:restart` | Stop + restart both frontend and Rust backend |
| `npm run dev:check` | Diagnose dev server wiring |
| `npm run typecheck` | TypeScript checking |
| `npm run lint` | ESLint |
| `npm run build` | Next.js static export |
| `npm run test:unit` | Unit tests |
| `npm run test` | Playwright E2E tests |
| `npm run visual:smoke` | Visual smoke check |

Frontend dev server connects to Rust backend at `http://127.0.0.1:17322`.  
`npm run dev:restart` uses `../allthecodes/target/release/allthecodes`.
