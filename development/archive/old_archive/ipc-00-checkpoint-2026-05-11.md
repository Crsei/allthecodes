# ipc-00 Checkpoint: IPC Inventory And Baseline

Date: 2026-05-11
Scope: record-only checkpoint. No Rust code was moved or changed. TypeScript IPC/UI files were not inspected.

## Wire Inventory

Transport contract: headless mode uses newline-delimited JSON over stdio. The canonical discriminators are serde `type` tags with `snake_case` names in `crates/cc-ipc-protocol/src/protocol/mod.rs`.

Frontend to backend:

- Core input: `submit_prompt`, `abort_query`, `permission_response`, `slash_command`, `resize`, `question_response`, `quit`.
- Subsystem input: `lsp_command`, `mcp_command`, `plugin_command`, `skill_command`, `ide_command`, `agent_settings_command`, `query_subsystem_status`.
- Agent/team input: `agent_command`, `team_command`.
- Search input: `search_files`.

Backend to frontend:

- Core lifecycle/conversation: `ready`, `stream_start`, `stream_delta`, `thinking_delta`, `stream_end`, `tombstone`, `assistant_message`, `conversation_replaced`, `usage_update`, `error`.
- Tool/permission: `tool_use`, `tool_result`, `tool_progress`, `permission_request`, `question_request`.
- Workflow/status: `plan_workflow_event`, `system_info`, `status_line_update`, `suggestions`, `background_agent_complete`, `brief_message`, `autonomous_start`, `notification_sent`.
- Subsystem output: `lsp_event`, `mcp_event`, `plugin_event`, `skill_event`, `ide_event`, `agent_settings_event`, `subsystem_status`.
- Agent/team output: `agent_event`, `team_event`.
- Search output: `file_search_result`.

Supporting wire payload types:

- `ToolResultContentInfo`, `ConversationMessage`, and `FileSearchMatch` live under `crates/cc-ipc-protocol/src/protocol/`.
- LSP helper DTOs live in `crates/cc-ipc-protocol/src/lsp.rs`.
- Subsystem payloads live in `crates/cc-ipc-protocol/src/subsystem_events.rs` and `crates/cc-ipc-protocol/src/subsystem_types.rs`.
- Agent/team payloads live in `cc-types::{agent_events, agent_types, agent_channel}` and are consumed by the protocol crate.
- `crates/claude-code-rs/src/ipc/protocol/mod.rs`, `subsystem_events.rs`, and `subsystem_types.rs` are compatibility re-exports over `cc-ipc-protocol`, with `subsystem_events.rs` also retaining the in-process `SubsystemEventBus`.
- `cc-ipc` is still a scaffold/re-export crate for the agent IPC spine, not the full protocol owner.

## Line-Size Baseline

Canonical IPC protocol files:

| File | Lines | Bytes |
| --- | ---: | ---: |
| `crates/cc-ipc-protocol/src/subsystem_events.rs` | 1063 | 40837 |
| `crates/cc-ipc-protocol/src/subsystem_types.rs` | 885 | 37798 |
| `crates/cc-ipc-protocol/src/protocol/mod.rs` | 319 | 13070 |
| `crates/cc-ipc-protocol/src/protocol/base.rs` | 135 | 5509 |
| `crates/cc-ipc-protocol/src/protocol/subsystem.rs` | 128 | 4938 |
| `crates/cc-ipc-protocol/src/lsp.rs` | 34 | 1230 |
| `crates/cc-ipc-protocol/src/protocol/agent.rs` | 33 | 1261 |
| `crates/cc-ipc-protocol/src/protocol/team.rs` | 29 | 1105 |
| `crates/cc-ipc-protocol/src/lib.rs` | 9 | 299 |

Headless/runtime IPC files in `claude-code-rs`:

| File | Lines | Bytes |
| --- | ---: | ---: |
| `crates/claude-code-rs/src/ipc/subsystem_handlers.rs` | 2103 | 83128 |
| `crates/claude-code-rs/src/ipc/agent_settings.rs` | 959 | 37344 |
| `crates/claude-code-rs/src/ipc/sdk_mapper.rs` | 640 | 26197 |
| `crates/claude-code-rs/src/ipc/ingress.rs` | 529 | 21998 |
| `crates/claude-code-rs/src/ipc/agent_settings_generate.rs` | 314 | 15316 |
| `crates/claude-code-rs/src/ipc/runtime.rs` | 305 | 15474 |
| `crates/claude-code-rs/src/ipc/file_search.rs` | 274 | 9416 |
| `crates/claude-code-rs/src/ipc/builtin_agents.rs` | 241 | 13432 |
| `crates/claude-code-rs/src/ipc/agent_handlers.rs` | 208 | 7927 |
| `crates/claude-code-rs/src/ipc/agent_tree.rs` | 165 | 5826 |
| `crates/claude-code-rs/src/ipc/callbacks.rs` | 152 | 6785 |
| `crates/claude-code-rs/src/ipc/query_runner.rs` | 43 | 1792 |
| `crates/claude-code-rs/src/ipc/sink.rs` | 34 | 1353 |
| `crates/claude-code-rs/src/ipc/subsystem_events.rs` | 24 | 709 |
| `crates/claude-code-rs/src/ipc/headless.rs` | 23 | 1109 |
| `crates/claude-code-rs/src/ipc/mod.rs` | 20 | 456 |
| `crates/claude-code-rs/src/ipc/protocol/mod.rs` | 15 | 499 |
| `crates/claude-code-rs/src/ipc/agent_channel.rs` | 7 | 368 |
| `crates/claude-code-rs/src/ipc/agent_types.rs` | 6 | 347 |
| `crates/claude-code-rs/src/ipc/agent_events.rs` | 6 | 330 |
| `crates/claude-code-rs/src/ipc/subsystem_types.rs` | 2 | 98 |

`cc-types` agent IPC support baseline:

| File | Lines | Bytes |
| --- | ---: | ---: |
| `crates/cc-types/src/agent_events.rs` | 128 | 3935 |
| `crates/cc-types/src/agent_types.rs` | 56 | 2125 |
| `crates/cc-types/src/agent_channel.rs` | 21 | 932 |

Guard threshold note: `subsystem_handlers.rs`, `subsystem_events.rs`, `agent_settings.rs`, and `subsystem_types.rs` are already large enough that future IPC work should split before adding substantial behavior there.

## Metadata Graph

Current ownership graph:

```text
stdio JSONL
  -> cc_ipc_protocol::protocol::{FrontendMessage, BackendMessage}
     -> core payloads: protocol::base::{ConversationMessage, ToolResultContentInfo}
     -> search payloads: protocol::FileSearchMatch
     -> plan payloads: cc_types::plan_workflow::PlanWorkflowRecord
     -> agent/team payloads:
        cc_types::{agent_events, agent_types, agent_channel}
     -> subsystem payloads:
        cc_ipc_protocol::subsystem_events::{LspEvent, McpEvent, PluginEvent, IdeEvent, SkillEvent, AgentSettingsEvent}
        cc_ipc_protocol::subsystem_events::{LspCommand, McpCommand, PluginCommand, IdeCommand, SkillCommand, AgentSettingsCommand}
        cc_ipc_protocol::subsystem_types::{SubsystemStatusSnapshot, LspServerInfo, McpServerStatusInfo, PluginInfo, IdeInfo, SkillInfo, AgentDefinitionEntry, AgentToolInfo}
     -> compatibility imports:
        claude_code_rs::ipc::{protocol, subsystem_events, subsystem_types}
```

Runtime graph:

```text
headless::run_headless
  -> runtime::HeadlessRuntime
     -> callbacks installs permission / ask-user / tool-progress callbacks
     -> ingress handles FrontendMessage dispatch and question compatibility
     -> query_runner starts query turns
     -> sdk_mapper maps SDK stream messages to BackendMessage
     -> sink serializes BackendMessage as JSONL
     -> subsystem_handlers and agent_handlers produce subsystem/agent BackendMessage effects
```

## Roundtrip Test Inventory

Current Rust IPC roundtrip/serde tests:

- `cc-ipc-protocol`: 84 unit tests for backend/frontend serde coverage across core, subsystem, agent, team, LSP helper, subsystem event, and subsystem status payloads.
- `claude-code-rs ipc::`: 81 focused IPC tests covering compatibility re-export serde, ingress question routing, file-search parsing/truncation, SDK stream mapping, agent handlers/tree/settings/generation, builtin agents, and subsystem handlers.
- `cc-ipc`: 0 unit/doc tests; current value is compilation of the re-export crate.

Tool baseline tests:

- `--test e2e_tools`: 29 tests covering system-prompt tool inventory and print-mode smoke tests.
- `tools::`: 413 package tests under an isolated `CC_RUST_HOME`; covers registry, execution/security, filesystem tools, hooks, plan mode, task tools, team spawn, web fetch/search, LSP tool, system status, and related MCP/plugin tool adapters.

## Verification

Passed:

- `cargo test -p cc-ipc-protocol` -> 84 passed.
- `cargo test -p claude-code-rs ipc::` -> 81 passed.
- `cargo test -p cc-ipc` -> 0 tests, package and doctest harness passed.
- `cargo test -p claude-code-rs --test e2e_tools` -> 29 passed.
- `CC_RUST_HOME=.tmp/ipc-00-cc-rust-home` with web-search provider env vars removed, `cargo test -p claude-code-rs tools::` -> 413 passed.

Warnings:

- `cargo test -p claude-code-rs tools::` initially failed 2 web-search provider-detection tests when inherited provider API-key environment variables were present. Rerunning with `TAVILY_API_KEY`, `BRAVE_API_KEY`, and `BRAVE_SEARCH_API_KEY` removed passed all 413 tests. This is environment contamination in the broad baseline, not an IPC code failure.

## Effects, Defects, Follow-Ups

Effects:

- Updated this checkpoint inventory only. No Rust source or protocol shape changed.

Defects:

- Broad `tools::` tests are sensitive to inherited web-search provider API-key environment variables.
- `subsystem_handlers.rs` remains above 2000 lines; avoid adding more behavior there in follow-up IPC work.

Follow-ups:

- Add CI-friendly test wrappers that set `CC_RUST_HOME` to a temp directory and clear provider API-key env vars for broad tool baselines.
- Split future edits out of the largest IPC files before adding behavior.
- Consider adding direct `cc-ipc` tests once more protocol ownership moves into that crate.
