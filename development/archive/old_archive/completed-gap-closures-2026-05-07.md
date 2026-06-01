# 2026-05-07 已确认实现内容归档

本文件保存从活跃状态文档中迁出的完成历史。活跃 TODO 只看 [../IMPLEMENTATION_GAPS.md](../IMPLEMENTATION_GAPS.md)，当前状态只看 [../WORK_STATUS.md](../WORK_STATUS.md)。

## Extensibility closure

已归档的分阶段证据：

- [extensibility-phase0-baseline-scope-2026-05-06.md](extensibility-phase0-baseline-scope-2026-05-06.md)
- [extensibility-phase1-mcp-lifecycle-2026-05-06.md](extensibility-phase1-mcp-lifecycle-2026-05-06.md)
- [extensibility-phase2-remote-https-sse-2026-05-06.md](extensibility-phase2-remote-https-sse-2026-05-06.md)
- [extensibility-phase3-mcp-oauth-2026-05-06.md](extensibility-phase3-mcp-oauth-2026-05-06.md)
- [extensibility-phase4-mcp-streamable-http-2026-05-06.md](extensibility-phase4-mcp-streamable-http-2026-05-06.md)
- [extensibility-phase5-custom-agent-safety-2026-05-06.md](extensibility-phase5-custom-agent-safety-2026-05-06.md)
- [extensibility-phase6-integration-closure-2026-05-06.md](extensibility-phase6-integration-closure-2026-05-06.md)

当前结论：

- MCP lifecycle: `/mcp connect` / `disconnect` / `reconnect` 与 IPC lifecycle commands 走 shared runtime `McpManager`。
- Remote SSE: `https://` SSE 支持 secure endpoint validation、redirect rejection、redacted URL logging、header protection、JSON-RPC POST routing 与 401/403 `auth-needed`。
- OAuth: remote SSE 支持 metadata、manual PKCE start/complete、token storage/refresh/clear/status、redaction、IPC auth events 与 `Authorization` injection。
- Streamable HTTP: 支持 POST JSON-RPC、JSON/SSE response、`MCP-Session-Id`、`MCP-Protocol-Version`、optional GET SSE listener、DELETE cleanup、OAuth header reuse 与 secure loopback/remote URL validation。
- Custom agent safety: child agents 继承 parent `ToolPermissionContext`；user/project `permissionMode` 只从 default parent mode 应用；plugin `permissionMode` 忽略；`disallowedTools` 优先；支持 deny-all 与 namespaced MCP wildcards；拒绝未知 editable `isolation` 和 `maxTurns: 0`。
- Runtime registry refresh: `QueryEngineDeps::refresh_tools()` 从 shared runtime `McpManager` 重建 dynamic MCP wrappers，保留 native MCP-named tools，刷新 ToolSearch，并在 model call 前运行。

边界：

- active client-side Extensibility runtime 无剩余 gap。
- WebSocket 仍是 unsupported/custom，因为不在当前标准 MCP transport matrix。
- custom-agent `skills` / `hooks` / `plugin` / `mcpServers` 字段为 parsed-but-inactive future fields，后续激活必须复用 Phase 5 permission inheritance contract。

## Previously open gaps now closed

以下内容已从活跃 gap 文档迁出，后续只在 archive 中保留历史：

- IPC `clear_messages`: `QueryEngine::clear_messages()` 与 `/clear` ingress 路径已清空后端历史并广播 `conversation_replaced`。
- 权限 Phase 2 hook: `run_pre_tool_hooks` 结果已折入中心 permission decision，deny/ask/allow 按 spec 顺序生效。
- Vim 状态机: normal/insert/visual、基础 navigation、operator、single-key 与 visual selection 已覆盖。
- Agent Teams 用户面: `/team`、`TeamSpawn`、`SendMessage`、Team Dashboard 与 in-process backend 已完成；pane backend 是 intentional crop。
- Tasks V2 / TodoWrite: task-list isolation、monotonic IDs、locks、owner claim、activeForm/metadata、dependency updates、teammate owner release 与 web provider-diff docs 已关闭。
- Ratatui P0/P1: shell/diff/search/history/progress/tool-activity foundation，以及 settings/tasks/status/MCP/file-edit render surfaces 已完成；residuals 转入 [../KNOWN_ISSUES.md](../KNOWN_ISSUES.md)。
- Runtime storage unification: `CC_RUST_HOME` / `~/.cc-rust/` 路径隔离、用户文档 [../STORAGE.md](../STORAGE.md) 与历史 plan/spec 已归档到 [archive/superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md](superpowers/plans/2026-04-18-phase1-runtime-storage-unification.md) 和 [archive/superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md](superpowers/specs/2026-04-18-phase1-runtime-storage-unification-design.md)。

## Tool parity subitems closed

已从 active gap 表迁出的补齐项：

- FileWriteTool: temporary file + rename、restore backup、size limit、permission preservation、binary rejection。
- FileReadTool: symlink canonicalize/metadata、UTF-8/UTF-16/BOM detection、UTF-8 lossy fallback、大文件分页与 `next_offset`。
- SkillTool: dependency resolution、version conflict、compatible version、hot reload、frontmatter diagnostics。
- LSP: ranged `didChange`、passive `publishDiagnostics`、completion 与 diagnostics snapshot。
- BashTool: heredoc validation、git operation tracking、process tree / cancellation semantics。
- PowerShellTool: destructive command detection、high-risk validator rules、native parser invalid fail-closed、native AST metadata gate、parameter binding safety、CLM TypeName validation。
- Sandbox: shell filesystem preflight、fail-closed user surface。
- FileEditTool: read-after conflict detection、lock/readonly preflight、edit backup history、auto-indent correction、live transcript diff preview path。
- AgentTool: tool allow/deny filtering, tool definition de-duplication, team context inheritance, multi-agent dispatch entry。
- WebFetch: redirect policy、Content-Type dispatch、environment proxy / `NO_PROXY` support、Cookie/credential boundary。
- TaskTools: `TaskOutput` block/timeout, upstream task taxonomy, remote supervisor metadata, recoverable marker, restore poll timer reset, remote review timeout guard。
- PlanMode: conservative classifier, workflow persistence, implementation task linkage, team approval mailbox flow, dedicated plan-file write allowlist。

If any of these regress, open a new item in [../KNOWN_ISSUES.md](../KNOWN_ISSUES.md) instead of re-expanding active TODO docs.
