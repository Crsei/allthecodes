# Tool Comparison: claude-code-bun vs claude-code-rust vs codex

Generated: 2026-05-22

## Overview

| Project | Language | Built-in Tool Count | Positioning |
|---------|----------|---------------------|-------------|
| **claude-code-bun** | TypeScript/Bun | ~62 | Upstream official, most comprehensive |
| **claude-code-rust** | Rust | ~47 | Rust port, full build stage |
| **codex** (OpenAI) | Rust | ~31 | OpenAI's coding agent, minimalist design |

---

## 1. Common Tools (All Three)

| Function | claude-code-bun | claude-code-rust | codex |
|----------|----------------|-----------------|-------|
| Shell execution | `Bash` | `Bash` | `shell_command` / `exec_command` |
| Web search | `WebSearch` | `WebSearch` | `web_search` (hosted) |
| Web fetch | `WebFetch` | `WebFetch` | — |
| Sub-agent | `Agent` | `Agent` | `spawn_agent` |
| User interaction | `AskUserQuestion` | `AskUserQuestion` | `request_user_input` |
| LSP | `LSP` | `LSP` | — |
| Image viewing | — (inline) | — | `view_image` |

## 2. bun + rust Shared (Not in codex)

| Function | bun | rust | Notes |
|----------|-----|------|-------|
| File read/write/edit | `Read`/`Write`/`Edit` | `Read`/`Write`/`Edit` | codex uses `apply_patch` |
| Glob / Grep | `Glob`/`Grep` | `Glob`/`Grep` | codex has no standalone tools |
| Skill system | `Skill` | `Skill` | codex has no equivalent |
| Plan mode | `EnterPlanMode`/`ExitPlanMode` | `EnterPlanMode`/`ExitPlanMode` | codex uses `update_plan` |
| Task system | `TaskCreate`/`TaskGet`/`TaskUpdate`/`TaskList` | `TaskCreate`/`TaskGet`/`TaskUpdate`/`TaskList` | codex has no equivalent |
| TodoWrite | `TodoWrite` | `TodoWrite` | codex has no equivalent |
| SendUserMessage/Brief | `SendUserMessage` | `Brief` | codex has no equivalent |
| Background task mgmt | `TaskStop`/`TaskOutput` | `TaskStop`/`TaskOutput` | codex has no equivalent |
| Config | `Config` | `Config` | codex has no equivalent |
| Tool search | `SearchExtraTools` | `ToolSearch` | codex has `tool_search` |
| Sleep | `Sleep` | `Sleep` | codex has no equivalent |
| PowerShell | `PowerShell` | `PowerShell` | codex has no equivalent |
| REPL | `REPL` | `REPL` | codex has no equivalent |

## 3. bun + codex Shared (Not in rust)

| Function | bun | codex | Notes |
|----------|-----|-------|-------|
| MCP resources | `ListMcpResources`/`ReadMcpResource` | `list_mcp_resources`/`read_mcp_resource` | **rust missing** |
| Notebook editing | `NotebookEdit` | — | Neither codex nor rust have it |

## 4. codex-Only Tools

| Tool | Description |
|------|-------------|
| `apply_patch` | Freeform tool, unified file editing via grammar |
| `update_plan` | Plan management (different from bun/rust plan mode) |
| `request_permissions` | Request additional permissions from user |
| `request_plugin_install` | Suggest plugin installation |
| `view_image` | View local image files |
| `exec` / `wait` | Code Mode — freeform code execution |
| `get_goal` / `create_goal` / `update_goal` | Goal management system |
| `write_stdin` | Write to PTY session |
| `spawn_agents_on_csv` / `report_agent_job_result` | CSV batch agent jobs |
| `image_generation` | Image generation (hosted) |
| `list_mcp_resource_templates` | MCP resource templates |
| Multi-agent v2 | `send_message`/`followup_task`/`list_agents`/`close_agent`/`wait_agent` |

## 5. bun-Only Tools

| Tool | Description |
|------|-------------|
| `LocalMemoryRecall` | Cross-session local memory recall |
| `VaultHttpFetch` | Authenticated HTTPS via encrypted vault |
| `CronCreate`/`CronDelete`/`CronList` | Scheduled cron jobs |
| `TeamCreate`/`TeamDelete` | Team management |
| `ListPeers` | Discover other local sessions (UDS inbox) |
| `WebBrowser` | Browser-based content fetch |
| `TerminalCapture` | Terminal output capture |
| `Monitor` | Long-running background monitor |
| `RemoteTrigger` | Remote Claude Code trigger via CCR API |
| `SendUserFile` | Send file to user |
| `PushNotification` | Push notification to mobile |
| `SubscribePR` | GitHub PR webhook subscription |
| `ReviewArtifact` | Artifact review with annotations |
| `Snip` | Historical message compression |
| `DiscoverSkills` | Skill discovery search |
| `VerifyPlanExecution` | Verify plan execution |
| `workflow` | Workflow scripts |
| `ExecuteExtraTool` | Deferred tool execution |
| `CtxInspect` | Context window inspection |
| `StructuredOutput` | Structured output format |

## 6. rust-Only Tools

| Tool | Description |
|------|-------------|
| `SendMessage` / `TeamSpawn` | Team messaging and spawning |
| `subscribe_pr_activity` / `unsubscribe_pr_activity` | PR activity subscription |
| `EnterWorktree` / `ExitWorktree` | Git worktree isolation |
| `SystemStatus` | Subsystem status query |
| `StructuredOutput` | Structured output (JSON/CSV/table) |
| Computer Use suite (10 tools) | `screenshot`/`left_click`/`right_click`/`middle_click`/`double_click`/`type_text`/`key`/`scroll`/`mouse_move`/`cursor_position` |

---

## Gap Analysis: rust vs bun

### Missing in rust (exist in bun)

| Tool | Priority | Notes |
|------|----------|-------|
| `NotebookEdit` | Medium | Jupyter notebook editing |
| `ListMcpResources`/`ReadMcpResource` | High | MCP resource access |
| `CronCreate`/`CronDelete`/`CronList` | Medium | Scheduled tasks |
| `LocalMemoryRecall` | Medium | Cross-session memory |
| `VaultHttpFetch` | Low | Authenticated HTTP |
| `ListPeers` | Low | Peer session discovery |
| `WebBrowser` | Medium | Browser-based fetch |
| `TerminalCapture` | Low | Terminal output capture |
| `Monitor` | Low | Background monitoring |
| `RemoteTrigger` | Low | Remote agent triggers |
| `SendUserFile` | Low | File sending |
| `PushNotification` | Low | Mobile push |
| `SubscribePR` | Medium | PR webhook (partial: rust has `subscribe_pr_activity`) |
| `ReviewArtifact` | Low | Artifact review |
| `Snip` | Medium | History compression |
| `DiscoverSkills` | Low | Skill discovery |
| `VerifyPlanExecution` | Low | Plan verification |
| `workflow` | Low | Workflow scripts |
| `ExecuteExtraTool` | Low | Deferred tool exec |
| `CtxInspect` | Low | Context inspection |

### Unique advantage of rust over bun

| Feature | Details |
|---------|---------|
| Computer Use suite | 10 tools for desktop automation |
| `SystemStatus` | Unified subsystem status query |
| `ToolSearch` | BM25-based tool catalog search |
