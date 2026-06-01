# TypeScript vs Rust 目录结构差异对比

> 生成时间: 2026-04-01
> TypeScript 源码: `cc/src/` (18 根文件, 35 子目录, ~2100+ 文件)
> Rust 源码: `cc/rust/src/` (2 根文件, 16 子目录, 94 文件)

---

## 1. 顶层目录结构对照

| TypeScript `src/` | Rust `src/` | 状态 | 说明 |
|---|---|---|---|
| `services/api/` | `api/` | **已重组** | 从 services 子目录提升为顶级模块 |
| `services/analytics/` | `analytics/` | **已重组** | 同上 |
| `services/compact/` | `compact/` | **已重组** | 同上 |
| `services/mcp/` | `mcp/` | **已重组** | 同上 |
| `services/oauth/` + `utils/secureStorage/` | `auth/` | **已重组** | 认证相关合并为独立模块 |
| `utils/permissions/` | `permissions/` | **已重组** | 从 utils 子目录提升为顶级模块 |
| `utils/settings/` + CLAUDE.md 相关 | `config/` | **已重组** | 设置和配置合并 |
| `components/` + `ink/` | `ui/` | **已重组** | UI 相关合并为单一模块 |
| `memdir/` + 会话相关 | `session/` | **已重组** | 会话管理统一 |
| `QueryEngine.ts` + `query.ts` | `engine/` | **已重组** | 核心引擎逻辑提取为独立模块 |
| `state/` | `types/app_state.rs` | **已合并** | 状态管理合入 types |
| `commands/` | `commands/` | **部分实现** | TS: 85+ 命令, Rust: 5 个 |
| `tools/` | `tools/` | **部分实现** | TS: 42 个工具目录, Rust: 20 个工具文件 |
| `query/` | `query/` | **已对齐** | 结构类似 |
| `remote/` | `remote/` | **已对齐** | 结构类似 |
| `types/` | `types/` | **已对齐** | 结构类似 |
| `utils/` | `utils/` | **大幅精简** | TS: 565 文件, Rust: 6 文件 |

---

## 2. Rust 中缺失的 TS 目录（未移植）

### 核心功能目录

| TS 目录 | 文件数 | 功能 | 优先级建议 |
|---|---|---|---|
| `entrypoints/` | 13 | CLI 入口与 SDK 入口 | Rust 合并到 `main.rs` |
| `bootstrap/` | 1 | 启动状态初始化 | 可合入 engine |
| `state/` | 6 | AppState 状态管理 | 部分合入 types |
| `context/` | 9 | React Context 提供者 | 需 Rust 替代方案 |
| `hooks/` | 104 | React Hooks | 需 Rust 替代方案 |
| `constants/` | 21 | 常量定义 | 需移植 |
| `schemas/` | 1 | JSON Schema | 可合入 types |

### 网络/远程功能目录

| TS 目录 | 文件数 | 功能 |
|---|---|---|
| `bridge/` | 31 | 远程控制/桥接模式 |
| `server/` | 3 | 服务器模式 |
| `upstreamproxy/` | 2 | 上游代理 |
| `cli/` | 19 | CLI 传输层 (SSE, WebSocket 等) |
| `coordinator/` | 1 | 多 Agent 协调 |

### UI/UX 相关目录

| TS 目录 | 文件数 | 功能 |
|---|---|---|
| `components/` | 389 | React/Ink UI 组件 |
| `ink/` | 97 | 终端渲染引擎 |
| `screens/` | 3 | 屏幕视图 |
| `keybindings/` | 14 | 快捷键绑定 |
| `vim/` | 5 | Vim 模式 |
| `outputStyles/` | 1 | 输出样式 |

### 辅助功能目录

| TS 目录 | 文件数 | 功能 |
|---|---|---|
| `skills/` | 23 | 技能系统 |
| `plugins/` | 2 | 插件系统 |
| `tasks/` | 12 | 任务管理 (Dream, Agent, Shell) |
| `memdir/` | 8 | 会话记忆存储 |
| `migrations/` | 11 | 数据迁移 |
| `native-ts/` | 4 | 原生 TS 实现 (color-diff, yoga) |
| `assistant/` | 1 | Kairos 助手模式 |
| `buddy/` | 6 | Companion Sprite |
| `moreright/` | 1 | 扩展权限 |
| `voice/` | 1 | 语音模式 |

### services/ 中未单独移植的子模块

| TS 子目录 | 功能 | Rust 中状态 |
|---|---|---|
| `services/lsp/` | LSP 集成 | 缺失 |
| `services/oauth/` | OAuth 流程 | 部分合入 auth/ |
| `services/plugins/` | 插件管理 | 缺失 |
| `services/AgentSummary/` | Agent 摘要 | 缺失 |
| `services/MagicDocs/` | 文档生成 | 缺失 |
| `services/PromptSuggestion/` | 提示建议 | 缺失 |
| `services/SessionMemory/` | 会话记忆 | 部分合入 session/ |
| `services/autoDream/` | 自动 Dream | 缺失 |
| `services/extractMemories/` | 记忆提取 | 缺失 |
| `services/policyLimits/` | 策略限制 | 缺失 |
| `services/remoteManagedSettings/` | 远程设置 | 缺失 |
| `services/settingsSync/` | 设置同步 | 缺失 |
| `services/teamMemorySync/` | 团队同步 | 缺失 |
| `services/tips/` | 提示信息 | 缺失 |
| `services/toolUseSummary/` | 工具使用摘要 | 缺失 |
| `services/tools/` | 工具服务 | 缺失 |

---

## 3. 根文件对比

### TypeScript 根文件 (18 个)

```
QueryEngine.ts      → rust/src/engine/  (已重组为模块)
Tool.ts             → rust/src/types/tool.rs
Task.ts             → 缺失 (任务系统未完整移植)
commands.ts         → rust/src/commands/mod.rs
context.ts          → 缺失 (React Context, 需替代)
cost-tracker.ts     → 缺失
costHook.ts         → 缺失
dialogLaunchers.tsx → 缺失 (UI 相关)
history.ts          → 缺失
ink.ts              → 缺失 (Ink 引擎, 已用 ratatui 替代)
interactiveHelpers.tsx → 缺失
main.tsx            → rust/src/main.rs
projectOnboardingState.ts → 缺失
query.ts            → rust/src/query/loop_impl.rs
replLauncher.tsx    → 缺失
setup.ts            → 缺失
tasks.ts            → 缺失
tools.ts            → rust/src/tools/registry.rs
```

### Rust 根文件 (2 个)

```
main.rs             ← main.tsx + entrypoints/cli.tsx
shutdown.rs         ← 新增 (优雅关闭逻辑)
```

---

## 4. 工具 (tools/) 对比

### TS → Rust 映射

| TypeScript 工具目录 | Rust 文件 | 状态 |
|---|---|---|
| `AgentTool/` | `agent.rs` | ✅ 已实现 |
| `AskUserQuestionTool/` | `ask_user.rs` | ✅ 已实现 |
| `BashTool/` | `bash.rs` | ✅ 已实现 |
| `FileEditTool/` | `file_edit.rs` | ✅ 已实现 |
| `FileReadTool/` | `file_read.rs` | ✅ 已实现 |
| `FileWriteTool/` | `file_write.rs` | ✅ 已实现 |
| `GlobTool/` | `glob_tool.rs` | ✅ 已实现 |
| `GrepTool/` | `grep.rs` | ✅ 已实现 |
| `NotebookEditTool/` | `notebook_edit.rs` | ✅ 已实现 |
| `EnterPlanModeTool/` | `plan_mode.rs` | ✅ 已实现 |
| `ExitPlanModeTool/` | `plan_mode.rs` | ✅ 合并 |
| `EnterWorktreeTool/` | `worktree.rs` | ✅ 已实现 |
| `ExitWorktreeTool/` | `worktree.rs` | ✅ 合并 |
| `SkillTool/` | `skill.rs` | ✅ 已实现 |
| `ToolSearchTool/` | `tool_search.rs` | ✅ 已实现 |
| `WebFetchTool/` | `web_fetch.rs` | ✅ 已实现 |
| `WebSearchTool/` | `web_search.rs` | ✅ 已实现 |
| `TaskCreateTool/` + 其他 Task 工具 | `tasks.rs` | ✅ 合并 |
| — | `execution.rs` | 🆕 新增 (工具执行引擎) |
| — | `hooks.rs` | 🆕 新增 (工具钩子) |
| — | `orchestration.rs` | 🆕 新增 (工具编排) |
| — | `registry.rs` | 🆕 新增 (工具注册表) |
| `BriefTool/` | — | ❌ 缺失 |
| `ConfigTool/` | — | ❌ 缺失 |
| `LSPTool/` | — | ❌ 缺失 |
| `MCPTool/` | — | ❌ 缺失 |
| `McpAuthTool/` | — | ❌ 缺失 |
| `ListMcpResourcesTool/` | — | ❌ 缺失 |
| `ReadMcpResourceTool/` | — | ❌ 缺失 |
| `PowerShellTool/` | — | ❌ 缺失 |
| `REPLTool/` | — | ❌ 缺失 |
| `RemoteTriggerTool/` | — | ❌ 缺失 |
| `ScheduleCronTool/` | — | ❌ 缺失 |
| `SendMessageTool/` | — | ❌ 缺失 |
| `SleepTool/` | — | ❌ 缺失 |
| `SyntheticOutputTool/` | — | ❌ 缺失 |
| `TeamCreateTool/` | — | ❌ 缺失 |
| `TeamDeleteTool/` | — | ❌ 缺失 |
| `TodoWriteTool/` | — | ❌ 缺失 |
| `TungstenTool/` | — | ❌ 缺失 |
| `WorkflowTool/` | — | ❌ 缺失 |

---

## 5. 命令 (commands/) 对比

### Rust 已实现 (5 个)

| Rust 命令 | TS 对应 |
|---|---|
| `clear.rs` | `commands/clear/` |
| `compact.rs` | `commands/compact/` |
| `config_cmd.rs` | `commands/config/` |
| `diff.rs` | `commands/diff/` |
| `help.rs` | `commands/help/` |

### TS 中存在但 Rust 缺失的命令 (80+)

<details>
<summary>点击展开完整列表</summary>

```
add-dir, advisor, agents, ant-trace, autofix-pr, backfill-sessions,
branch, break-cache, bridge, bridge-kick, brief, btw, bughunter,
chrome, color, commit, commit-push-pr, context, copy, cost,
createMovedToPluginCommand, ctx_viz, debug-tool-call, desktop,
doctor, effort, env, exit, export, extra-usage, fast, feedback,
files, good-claude, heapdump, hooks, ide, init, init-verifiers,
insights, install, install-github-app, install-slack-app, issue,
keybindings, login, logout, mcp, memory, mobile, mock-limits,
model, oauth-refresh, onboarding, output-style, passes, perf-issue,
permissions, plan, plugin, pr_comments, privacy-settings,
rate-limit-options, release-notes, reload-plugins, remote-env,
remote-setup, rename, reset-limits, resume, review, rewind,
sandbox-toggle, security-review, session, share, skills, stats,
status, statusline, stickers, summary, tag, tasks, teleport,
terminalSetup, theme, thinkback, thinkback-play, ultraplan,
upgrade, usage, version, vim, voice
```

</details>

---

## 6. 关键结构差异总结

### Rust 的改进设计

1. **扁平化模块**: TS 中 `services/api/`, `services/compact/`, `services/mcp/` 等深层嵌套被提升为顶级模块
2. **合并相关工具**: `EnterPlanMode` + `ExitPlanMode` → `plan_mode.rs`; 所有 Task 工具 → `tasks.rs`
3. **新增 engine 模块**: 将 `QueryEngine.ts` + `query.ts` 中的核心逻辑独立为 `engine/`
4. **新增 session 模块**: 统一会话生命周期管理 (storage, resume, transcript)
5. **新增 shutdown.rs**: 独立的优雅关闭逻辑

### 需要对齐的方向

1. **utils/ 严重不足**: TS 有 565 文件覆盖 bash, git, github, permissions, settings, sandbox 等; Rust 仅 6 文件
2. **commands/ 覆盖率低**: 5/85+ (约 6%)
3. **UI 层简化**: TS 有 `components/` (389) + `ink/` (97) + `hooks/` (104); Rust 仅 `ui/` (9 文件) — 符合 local-first 策略但需持续补充
4. **缺少插件/技能系统**: `skills/`, `plugins/` 完全缺失
5. **缺少任务子系统**: `tasks/` (DreamTask, LocalAgentTask 等) 完全缺失
6. **缺少网络功能目录**: `bridge/`, `cli/transports/`, `server/`, `coordinator/` — 按计划降优先级

---

## 7. 建议的 Rust 目录结构补充

```
rust/src/
├── main.rs
├── shutdown.rs
├── analytics/
├── api/
├── auth/
├── commands/          ← 需大量补充
├── compact/
├── config/
│   ├── constants.rs   ← 新增: 对应 TS constants/
│   └── ...
├── engine/
├── mcp/
├── permissions/
├── query/
├── remote/
├── session/
├── tools/             ← 需补充 ~20 个工具
├── types/
│   ├── schemas.rs     ← 新增: 对应 TS schemas/
│   └── ...
├── ui/                ← 需持续补充
│   ├── keybindings.rs ← 新增
│   └── ...
├── utils/             ← 需大量补充
│   ├── bash/          ← 新增: 对应 TS utils/bash/
│   ├── git/           ← 新增: 对应 TS utils/git/
│   ├── permissions/   ← 已有顶级 permissions/, 可跳过
│   ├── sandbox/       ← 新增
│   ├── shell/         ← 新增
│   └── ...
├── skills/            ← 新增: 对应 TS skills/
└── plugins/           ← 新增: 对应 TS plugins/
```
