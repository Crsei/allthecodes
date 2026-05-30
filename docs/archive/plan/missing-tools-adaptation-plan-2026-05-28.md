# allthecodes 缺失工具更新适配计划

> 生成日期: 2026-05-28
> 范围: 基于 `docs/reference/tool-comparison-3projects.md` gap 分析，规划 claude-code-bun / codex 已有但 allthecodes (rust) 尚未实现的工具
> 目标: 分优先级补齐工具生态，覆盖权限注册、系统提示词、Tool trait 实现、前端渲染四个层面

---

## 1. 当前工具生态快照

| 来源 | 工具数量 | 说明 |
|------|----------|------|
| **claude-code-bun** (upstream) | ~67 | 最全，含 cron/MCP-resource/通知等特色工具；本次补充自 commit `efc218d8` |
| **allthecodes** (rust，当前) | ~66+ | ✅ 已完成 P0/P1/P2 工具补齐，P3 部分实现 |
| **codex** (OpenAI) | ~31 | 独特的有 apply_patch、goal 管理、multi-agent v2 等 |

### 当前 allthecodes 已实现的工具概览（截至 2026-05-29）

**allthecodes-tools 自有工具** (38+):
Read / Write / Edit / Glob / Grep / SafeWrite / Bash / PowerShell / REPL / Sleep /
AskUserQuestion / Config / StructuredOutput / SendUserMessage / WebFetch / WebSearch /
EnterPlanMode / ExitPlanMode / Brief / SystemStatus / ToolSearch / TodoWrite /
Task / TaskList / TaskUpdate / TaskOutput / TaskStop / NotebookEdit /
CtxInspect / Monitor / SendUserFile / Snip / TerminalCapture / ReviewArtifact / RemoteTrigger / ListPeers /
SearchExtraTools / ExecuteExtraTool

**外部 provider 注册工具** (~24):
Agent / TaskAgent / Skill / Lsp / SendMessage / TeamSpawn /
SubscribePrActivity / UnsubscribePrActivity / SubscribePR /
EnterWorktree / ExitWorktree /
mcp__computer-use__screenshot / mcp__computer-use__left_click / mcp__computer-use__right_click /
mcp__computer-use__middle_click / mcp__computer-use__double_click /
mcp__computer-use__type_text / mcp__computer-use__key /
mcp__computer-use__scroll / mcp__computer-use__mouse_move /
mcp__computer-use__cursor_position

**engine 层注册工具** (~4):
ListMcpResources / ReadMcpResource / CronCreate / CronDelete / CronList / WebBrowser

**与 bun 等效但实现不同**:
- `subscribe_pr_activity` / `unsubscribe_pr_activity` (bun: `SubscribePR`)
- `ToolSearch` (BM25-based, bun: `SearchExtraTools`)

### claude-code-bun 完整工具清单 (commit `efc218d8`, 2026-05-29)

**CORE_TOOLS — 始终加载的 28 个核心工具**:

| 工具名 | 源文件 | 说明 |
|--------|--------|------|
| `Bash` | `BashTool/BashTool.tsx` | Shell 命令执行（Linux/macOS） |
| `Shell` | `BashTool/BashTool.tsx` | Bash 别名（与 Bash 同一工具） |
| `Read` | `FileReadTool/FileReadTool.ts` | 文件读取 |
| `Edit` | `FileEditTool/FileEditTool.ts` | 文件精确替换编辑 |
| `Write` | `FileWriteTool/FileWriteTool.ts` | 文件完整写回 |
| `Glob` | `GlobTool/GlobTool.ts` | 文件 glob 模式匹配 |
| `Grep` | `GrepTool/GrepTool.ts` | 内容搜索（ripgrep） |
| `NotebookEdit` | `NotebookEditTool/NotebookEditTool.ts` | Jupyter notebook cell 编辑 |
| `Agent` | `AgentTool/AgentTool.ts` | 启动子 agent |
| `AskUserQuestion` | `AskUserQuestionTool/AskUserQuestionTool.ts` | 向用户提问 |
| `TaskCreate` | `TaskCreateTool/TaskCreateTool.ts` | 创建任务 |
| `TaskGet` | `TaskGetTool/TaskGetTool.ts` | 获取任务详情 |
| `TaskList` | `TaskListTool/TaskListTool.ts` | 列出任务 |
| `TaskUpdate` | `TaskUpdateTool/TaskUpdateTool.ts` | 更新任务 |
| `TaskOutput` | `TaskOutputTool/TaskOutputTool.ts` | 获取任务输出 |
| `TaskStop` | `TaskStopTool/TaskStopTool.ts` | 停止任务 |
| `TodoWrite` | `TodoWriteTool/TodoWriteTool.ts` | 写入待办事项 |
| `EnterPlanMode` | `EnterPlanModeTool/EnterPlanModeTool.ts` | 进入计划模式 |
| `ExitPlanMode` | `ExitPlanModeTool/ExitPlanModeV2Tool.ts` | 退出计划模式 |
| `VerifyPlanExecution` | `VerifyPlanExecutionTool/VerifyPlanExecutionTool.ts` | 验证计划执行 |
| `WebFetch` | `WebFetchTool/WebFetchTool.ts` | 网页内容获取 |
| `WebSearch` | `WebSearchTool/WebSearchTool.ts` | 网页搜索 |
| `LSP` | `LSPTool/LSPTool.ts` | LSP 代码智能 |
| `Skill` | `SkillTool/SkillTool.ts` | 技能执行 |
| `Sleep` | `SleepTool/SleepTool.ts` | 等待指定时间 |
| `SearchExtraTools` | `SearchExtraToolsTool/SearchExtraToolsTool.ts` | 搜索延迟工具 |
| `ExecuteExtraTool` | `ExecuteTool/ExecuteTool.ts` | 执行延迟发现的工具 |
| `SyntheticOutput` / `StructuredOutput` | `SyntheticOutputTool/SyntheticOutputTool.ts` | 结构化输出 |

**ALWAYS_IMPORTED — 始终注册但 feature/env-gated 的工具**:

| 工具名 | 源文件 | gating | 备注 |
|--------|--------|--------|------|
| `REPL` | `REPLTool/REPLTool.ts` | `USER_TYPE === 'ant'` | REPL 模式（隐藏基础工具） |
| `SuggestBackgroundPR` | `SuggestBackgroundPRTool/SuggestBackgroundPRTool.ts` | `USER_TYPE === 'ant'` | 建议后台 PR |
| `CronCreate` | `ScheduleCronTool/CronCreateTool.ts` | 始终注册 | 创建 cron 定时任务 |
| `CronDelete` | `ScheduleCronTool/CronDeleteTool.ts` | 始终注册 | 删除 cron 任务 |
| `CronList` | `ScheduleCronTool/CronListTool.ts` | 始终注册 | 列出 cron 任务 |
| `RemoteTrigger` | `RemoteTriggerTool/RemoteTriggerTool.ts` | `AGENT_TRIGGERS_REMOTE` | 远程触发 |
| `Monitor` | `MonitorTool/MonitorTool.ts` | `MONITOR_TOOL` | 监控（目录存在，.ts 文件为空 stub） |
| `SendUserFile` | `SendUserFileTool/SendUserFileTool.ts` | `KAIROS` | 发送文件给用户 |
| `PushNotification` | `PushNotificationTool/PushNotificationTool.ts` | `KAIROS / KAIROS_PUSH_NOTIFICATION` | 推送通知 |
| `SubscribePR` | `SubscribePRTool/SubscribePRTool.ts` | `KAIROS_GITHUB_WEBHOOKS` | 订阅 PR 活动 |
| `OverflowTest` | `OverflowTestTool/OverflowTestTool.ts` | `OVERFLOW_TEST_TOOL` | 溢出测试（空常量名） |
| `workflow` | `WorkflowTool/WorkflowTool.ts` | `WORKFLOW_SCRIPTS` | 工作流脚本引擎 |
| `VerifyPlanExecution` | `VerifyPlanExecutionTool/VerifyPlanExecutionTool.ts` | `CLAUDE_CODE_VERIFY_PLAN` env | 计划验证 |

**ALWAYS_LOADED — 无 gate 始终加载的工具**:

| 工具名 | 源文件 | 说明 |
|--------|--------|------|
| `SendUserMessage` / `Brief` | `BriefTool/BriefTool.ts` | 发送消息给用户（双名称） |
| `Config` | `ConfigTool/ConfigTool.ts` | 配置读写 |
| `ListMcpResourcesTool` | `ListMcpResourcesTool/ListMcpResourcesTool.ts` | 列出 MCP server 资源 |
| `ReadMcpResourceTool` | `ReadMcpResourceTool/ReadMcpResourceTool.ts` | 读取 MCP 资源内容 |
| `EnterWorktree` | `EnterWorktreeTool/EnterWorktreeTool.ts` | 进入 git worktree |
| `ExitWorktree` | `ExitWorktreeTool/ExitWorktreeTool.ts` | 退出 git worktree |
| `TeamCreate` | `TeamCreateTool/TeamCreateTool.ts` | 创建团队 |
| `TeamDelete` | `TeamDeleteTool/TeamDeleteTool.ts` | 删除团队 |
| `SendMessage` | `SendMessageTool/SendMessageTool.ts` | 向队友发送消息 |
| `LocalMemoryRecall` | `LocalMemoryRecallTool/LocalMemoryRecallTool.ts` | 本地记忆回忆 |
| `VaultHttpFetch` | `VaultHttpFetchTool/VaultHttpFetchTool.ts` | Vault HTTP 获取 |

**DEFERRED/OPTIONAL — 不自动注册，需 SearchExtraTools 发现的工具**:

| 工具名 | 源文件 | 说明 |
|--------|--------|------|
| `CtxInspect` | `CtxInspectTool/CtxInspectTool.ts` | 上下文窗口检视 |
| `DiscoverSkills` | `DiscoverSkillsTool/DiscoverSkillsTool.ts` | 发现可用技能 |
| `ListPeers` | `ListPeersTool/ListPeersTool.ts` | 列出本地会话 |
| `MCPTool` | `MCPTool/MCPTool.ts` | 动态 MCP 工具调用（按 server 注入） |
| `McpAuthTool` | `McpAuthTool/McpAuthTool.ts` | MCP 认证 |
| `PowerShell` | `PowerShellTool/PowerShellTool.ts` | Windows PowerShell 执行 |
| `ReviewArtifact` | `ReviewArtifactTool/ReviewArtifactTool.ts` | 审查工作制品 |
| `Snip` | `SnipTool/SnipTool.ts` | 历史消息压缩 |
| `TerminalCapture` | `TerminalCaptureTool/TerminalCaptureTool.ts` | 终端输出捕获 |
| `WebBrowser` | `WebBrowserTool/WebBrowserTool.ts` | 浏览器渲染页面获取 |
| `Tungsten` | `TungstenTool/TungstenTool.ts` | 虚拟终端（stub，未实现） |
| `TestingPermission` | `testing/TestingPermissionTool.tsx` | 测试权限工具 |

**动态工具（由 MCP/plugin 生成）**:
- `mcp__<serverName>__<toolName>` — 所有已连接 MCP server 注册的 tools
- `plugin__<pluginName>__<toolName>` — 已安装插件注册的 tools

---

## 2. 缺失工具清单与分级

### P0 — 核心体验缺口（影响基础交互完整性） ✅ **已全部实现**

| # | 工具 | bun | codex | 缺失影响 | 实现状态 |
|---|------|-----|-------|----------|---------|
| 1 | **NotebookEdit** | ✅ | — | Jupyter 用户无法在会话中编辑 notebook cell | ✅ `notebook_edit.rs` 完整实现，6 个单元测试 |
| 2 | **ListMcpResources / ReadMcpResource** | ✅ | list_mcp_resources / read_mcp_resource | MCP 资源探知能力缺失 | ✅ `mcp_resource_tools.rs` 完整实现，4 个单元测试 |

### P1 — 中优先级（提升自动化/调度/协作能力） ✅ **已全部实现**

| # | 工具 | bun | codex | 缺失影响 | 实现状态 |
|---|------|-----|-------|----------|---------|
| 3 | **CronCreate / CronDelete / CronList** | ✅ | — | 无法定时执行任务/轮询 | ✅ `scheduler_tools.rs` 完整实现 |
| 4 | **WebBrowser** | ✅ | — | 缺少浏览器内内容获取能力 | ✅ `browser_tool.rs` 完整实现，支持 text/screenshot/both |
| 5 | **Monitor** | ✅ | — | 长时后台任务监控缺失 | ✅ `product_tools.rs` 完整实现 |
| 6 | **SendUserFile** | ✅ | — | 缺少直接向用户发送文件的能力 | ✅ `product_tools.rs` 完整实现 |

### P2 — 低优先级（产品体验增强） ✅ **已全部实现**

| # | 工具 | bun | codex | 缺失影响 | 实现状态 |
|---|------|-----|-------|----------|---------|
| 7 | **SubscribePR** (完整版) | ✅ | — | 当前 `subscribe_pr_activity` 功能不全 | ✅ `pr_activity.rs` 完整实现，含 webhook 路由 |
| 8 | **TerminalCapture** | ✅ | — | 终端输出捕获 | ✅ `product_tools.rs` 完整实现 |
| 9 | **ReviewArtifact** | ✅ | — | 审查工作制品 | ✅ `product_tools.rs` 完整实现 |
| 10 | **Snip** | ✅ | — | 历史消息压缩 | ✅ `product_tools.rs` + `cc-compact` 完整实现 |
| 11 | **CtxInspect** | ✅ | — | 上下文窗口检视 | ✅ `product_tools.rs` 完整实现 |
| 12 | **RemoteTrigger** | ✅ | — | 远程触发其他 Claude Code 实例 | ✅ `product_tools.rs` 完整实现 |
| 13 | **ListPeers** | ✅ | — | 本地会话发现 | ✅ `product_tools.rs` 完整实现 |

### P3 — 暂缓（需要基础设施或不属于当前核心目标）

| # | 工具 | bun | codex | 说明 | 实现状态 |
|---|------|-----|-------|------|---------|
| 14 | LocalMemoryRecall | ✅ | — | 需要跨会话内存存储层 | ✅ `phase5::LocalMemoryRecall` JSON/文件索引 v1 |
| 15 | VaultHttpFetch | ✅ | — | 需要加密凭据存储 | ✅ `phase5::VaultHttpFetch` HTTPS + vault credential_ref |
| 16 | PushNotification | ✅ | — | 需要移动端基础设施 | ✅ `phase5::PushNotification` local audit + HTTPS webhook provider |
| 17 | DiscoverSkills | ✅ | — | Skill 发现，优先级低 | ✅ `phase5::DiscoverSkills` |
| 18 | VerifyPlanExecution | ✅ | — | 计划验证工作流 | ✅ `phase5::VerifyPlanExecution` |
| 19 | workflow | ✅ | — | 工作流脚本引擎 | ✅ `phase5::Workflow` Rust-native durable workflow spec |
| 20 | ExecuteExtraTool | ✅ | — | 延迟工具执行 | ✅ `deferred_tools.rs` 完整实现 |
| 21 | **SearchExtraTools** (含延迟工具系统) | ✅ | — | 工具发现 + 延迟加载 | ✅ `deferred_tools.rs` 完整实现，CORE_TOOLS 边界定义 |
| 22 | **apply_patch** | — | ✅ | Tree-sitter AST 感知的语义化 patch | ✅ `phase5::ApplyPatch` JSON `{patch}` 接口 |
| 23 | **Goal 管理** (get_goal / create_goal / update_goal) | — | ✅ | 目标管理系统 | ✅ `GetGoal` / `CreateGoal` / `UpdateGoal` |
| 24 | **Multi-agent v2** (send_message / followup_task / list_agents / close_agent / wait_agent) | — | ✅ | 增强型多 agent 通信 | ✅ `SendMessage` + `FollowupTask` / `ListAgents` / `CloseAgent` / `WaitAgent` |
| 25 | view_image | — | ✅ | 已可通过 computer-use 截图覆盖 | ✅ `phase5::ViewImage` |

#### P3 剩余未实现项（整理）

| 项目 | 来源 | 缺失能力 | 当前阻塞/备注 |
|------|------|----------|----------------|
| — | — | — | 2026-05-29 已完成 Phase 5 剩余工具首版实现；后续只保留增强项跟踪 |

#### P3 部分实现项（整理）

| 项目 | 已有内容 | 缺失内容 | 下一步 |
|------|----------|----------|--------|
| `PushNotification` | CLI `/notify` 命令 stub | 真实移动端 provider 未接入 | Tool schema、权限确认、local audit provider、HTTPS webhook provider 已落地 |
| `Workflow` | `plan_workflow.rs` 内部状态管理 | 自动 agent 调度仍可增强 | 独立 `Workflow` Tool 已支持 start/status/cancel 和 `{data_root}/workflows/` 持久化 |
| Multi-agent v2 | `SendMessage` 已实现 | 与 in-process runtime 的完成状态可继续深化 | 已新增 `FollowupTask` / `ListAgents` / `CloseAgent` / `WaitAgent`，复用 team config + mailbox |

#### P3 已完成但仍属本阶段的项

| 项目 | 实现状态 |
|------|----------|
| `SearchExtraTools` | ✅ 已实现 select/discover/keyword 查询模式 |
| `ExecuteExtraTool` | ✅ 已实现 discovery guard + 委托执行 |
| 延迟工具系统 | ✅ 已实现 `CORE_TOOLS` 边界、跨 turn discovered tools 持久化和 API 请求工具过滤 |

---

## 3. 各工具适配方案

### 3.1 NotebookEdit (P0) ✅ **已实现**

**状态 (当前)**: 完整实现。文件 `allthecodes-tools/src/fs/notebook_edit.rs`，包含 `NotebookEditTool` 结构体，完整 Tool trait 实现（name/description/input_json_schema/call/validate_input 等）。注册于 `fs/mod.rs`，被 `tool_registry.rs` 集成。6 个单元测试覆盖替换/插入/删除/read-before-edit/cache 刷新/stale 拒绝。✅

**实现方案（历史参考）**:
- 新工具文件: `allthecodes-tools/src/notebook_edit.rs`
- 依赖: `serde_json` 解析 cell 标识 + 内容；本地 `.ipynb` 文件读写复用现有 `Read`/`Write` 的文件操作能力
- 核心能力:
  - 读取 notebook 文件并解析 cell 结构
  - 替换指定 cell 的源代码
  - 支持插入新 cell、删除 cell
  - 支持 code/markdown cell 类型切换
- 注册: `allthecodes-tools` 的 `fs::tools()` 中追加，或独立模块

**估算**: ~400 行 Rust，复用 `allthecodes-tools` 的 tool trait / result / 错误处理

### 3.2 ListMcpResources / ReadMcpResource (P0) ✅ **已实现**

**状态 (当前)**: 完整实现。文件 `allthecodes-engine/src/mcp_resource_tools.rs`，包含 `ListMcpResourcesTool` 和 `ReadMcpResourceTool` 两个完整 Tool 实现。均 read-only、concurrency-safe。注册于 `mcp_resource_tools::tools()`，被 `tool_registry.rs` 集成。4 个单元测试覆盖静态契约/runtime manager/错误路径/binary 处理。✅

**实现方案（历史参考）**:
- 两个工具均可放在 `allthecodes-mcp` crate 中作为外部 provider 注册
- `ListMcpResources`: 查询所有已连接 MCP server 的 resource 列表，返回名称/URI/MIME 类型
- `ReadMcpResource`: 按 URI 读取具体 resource 内容
- 需要扩展 `McpManager` 暴露 resource 枚举和读取接口

**估算**: ~300 行 Rust，主要工作量在 `McpManager` 接口扩展

### 3.3 Cron 套件 (P1) ✅ **已实现**

**状态 (当前)**: 完整实现。文件 `allthecodes-services/src/scheduler_tools.rs`，包含 `CronCreateTool` / `CronDeleteTool` / `CronListTool` 三个完整 Tool 实现。注册于 `scheduler_tools::tools()`，被 `tool_registry.rs` 集成。系统提示词已集成。✅

**实现方案（历史参考）**:
- 新 crate `allthecodes-cron` 或直接放入 `allthecodes-tools/src/cron/`
- 依赖: `cron` crate (Rust 版 cron 解析)
- 三个工具:
  - `CronCreate`: 传入 cron 表达式 + prompt + (可选) recurring 标志
  - `CronDelete`: 按 job ID 删除
  - `CronList`: 列出所有活跃 cron job
- 存储: 内存 + `~/.allthecodes/cron_jobs.json` 持久化
- 注意: 与现有 `allthecodes-daemon` 的集成，确保 cron 在 daemon 模式下存活

**估算**: ~600 行 Rust

### 3.4 WebBrowser (P1) ✅ **已实现**

**状态 (当前)**: 完整实现。文件 `allthecodes-engine/src/browser_tool.rs`，包含 `WebBrowserTool`，支持 text / screenshot / both 提取模式。注册于 `browser_tool::tools()`，被 `tool_registry.rs` 集成。有单元测试。✅

**实现方案（历史参考）**:
- 方案 A (推荐): 利用现有 `allthecodes-browser` cmar 的 Chrome/CDP 集成能力
- 方案 B: 借助 `headless_chrome` crate（如需要全功能浏览器）
- 核心功能: 打开 URL → 等待渲染 → 提取文本/截图 → 返回结构化内容
- 权限: 需要单独的浏览器访问许可（安全敏感）

**估算**: ~500 行 Rust，依赖 `allthecodes-browser` 能力

### 3.5 Monitor (P1) ✅ **已实现**

**状态 (当前)**: 完整实现。位于 `allthecodes-tools/src/product_tools.rs`，包含 `MonitorTool`（完整 Tool trait），支持权限检查、输入验证、实时输出流式传输。权限 UI 在 `monitor_permission_request/` 目录。✅

### 3.6 SendUserFile (P1) ✅ **已实现**

**状态 (当前)**: 完整实现。位于 `allthecodes-tools/src/product_tools.rs`，包含 `SendUserFileTool`，支持文件读取、大小限制、UTF-8/二进制检测、结构化附件。✅

### 3.7 其余 P2 工具 ✅ **已全部实现**

| 工具 | 实际文件 | 实现状态 |
|------|---------|---------|
| **TerminalCapture** | `allthecodes-tools/src/product_tools.rs` (`TerminalCaptureTool`) | ✅ 完整实现 |
| **ReviewArtifact** | `allthecodes-tools/src/product_tools.rs` (`ReviewArtifactTool`)，有独立权限 UI | ✅ 完整实现 |
| **Snip** | `allthecodes-tools/src/product_tools.rs` (`SnipTool`) + `allthecodes-compact/src/snip.rs` | ✅ 完整实现 |
| **CtxInspect** | `allthecodes-tools/src/product_tools.rs` (`CtxInspectTool`) | ✅ 完整实现 |
| **RemoteTrigger** | `allthecodes-tools/src/product_tools.rs` (`RemoteTriggerTool`)，支持 HTTP 提交/幂等键/审计日志 | ✅ 完整实现 |
| **ListPeers** | `allthecodes-tools/src/product_tools.rs` (`ListPeersTool`)，读取 daemon state + peers config | ✅ 完整实现 |
| **SubscribePR (完整)** | `allthecodes-teams/src/pr_activity.rs` (`SubscribePrTool`/`SubscribePrActivityTool`)，含 webhook 路由 | ✅ 完整实现 |

### 3.8 SearchExtraTools / ExecuteExtraTool — 延迟工具发现与执行系统 (P3)

bun 版的核心设计：**不在 API 请求中发送所有工具的 schema**，仅在初始化时发送核心工具 (CORE_TOOLS, 38 个)，其余工具（MCP 工具 + 非核心内置工具）通过延迟发现按需加载。

#### 3.8.1 架构总览

```
模型（只看到 CORE_TOOLS schema）
        │
        ▼
  SearchExtraTools (核心工具，schema 始终可见)
        │  query="select:CronCreate" / "schedule task"
        ▼
  返回延迟工具名列表  ──→  extractDiscoveredToolNames() 提取
        │                   并持久化到后续 API request 的 filteredTools 中
        ▼
  ExecuteExtraTool (核心工具，schema 始终可见)
        │  tool_name + params
        ▼
  查找全量注册表 → discovery guard 检查 → 委托 targetTool.call()
```

#### 3.8.2 allthecodes 当前状态（截至 2026-05-29）

- ✅ 已有 `ToolSearch` 工具（BM25 算法），可搜索当前注册的所有工具
- ✅ 有 agent 级别的子进程工具搜索能力
- ✅ **已有 `CORE_TOOLS` / `is_deferred_tool` 的延迟加载边界定义** — 在 `allthecodes-tools/src/deferred_tools.rs` 中定义
- ✅ **已有 `ExecuteExtraTool`** — 延迟工具执行的包装器，在 `deferred_tools.rs` 中完整实现
- ✅ **已有 `SearchExtraTools`** — 在 `deferred_tools.rs` 中完整实现，支持 select/discover/keyword 三种查询模式
- ✅ `extract_discovered_tool_names()` — 支持跨 turn、structured attachment、legacy tool reference 与 compact metadata 恢复
- ✅ API 请求时的工具过滤机制 — 由 deferred tool gate 控制，仅发送 core + discovered 工具 schema

#### 3.8.3 实现方案

**步骤 A: 定义核心工具边界**

- 在 `allthecodes-engine` 中定义 `CORE_TOOLS: HashSet<String>`，包含 38 个核心工具名（参考 bun 的 `src/constants/tools.ts`）
- 实现 `is_deferred_tool(tool_name) -> bool` 函数
- 核心工具包括：Read, Write, Edit, Glob, Grep, Bash, NotebookEdit, Agent, AskUserQuestion, WebFetch, WebSearch, Sleep, LSP, Skill, EnterPlanMode, ExitPlanMode, TaskCreate/Get/List/Update/Output/Stop, TodoWrite, SearchExtraTools(新), ExecuteExtraTool(新), StructuredOutput, VerifyPlanExecution

**步骤 B: 实现 SearchExtraTools**

- 新文件: `allthecodes-tools/src/search_extra_tools.rs`
- 工具名: `SearchExtraTools`
- 输入: `query: string`, `max_results?: number`（默认 5）
- 输出: `{ matches: string[], query, total_deferred_tools, already_loaded: string[] }`
- 支持的查询模式：
  - `select:<name>` — 精确查找，支持 `select:A,B,C` 多选
  - `discover:<query>` — 发现模式，返回工具描述 + schema 但不触发最终加载
  - 普通关键词 — BM25 + TF-IDF 混合搜索（复用现有 `ToolSearch` 的 BM25 能力）
  - `+<term>` 前缀 — 强制匹配
- `is_concurrency_safe()`, `is_read_only()` 均返回 true

**步骤 C: 实现 ExecuteExtraTool**

- 新文件: `allthecodes-tools/src/execute_extra_tool.rs`
- 工具名: `ExecuteExtraTool`
- 输入: `tool_name: string`, `params: Record<string, unknown>`
- 输出: `{ result: unknown, tool_name: string }`
- 核心逻辑：
  1. 从全量工具注册表按名称查找目标工具
  2. **Discovery guard**: 如果启用了延迟加载，检查目标工具是否已被搜索发现；未发现则返回错误 "use SearchExtraTools first"
  3. 检查 target.is_enabled()
  4. 校验入参 target.validate_input()
  5. 检查权限 target.check_permissions()
  6. 委托 target.call() 执行并包装结果

**步骤 D: 发现状态持久化**

- 在 `allthecodes-engine` 的会话状态中维护 `discovered_tools: HashSet<String>`
- 实现 `extract_discovered_tool_names(messages: &[Message]) -> HashSet<String>`：
  - 解析 `SearchExtraTools` 的 tool result（支持 legacy `tool_reference` block 和新 plain-text 格式）
  - 解析 `deferred_tools_delta` attachment 格式
  - 支持 compaction 后的边界标记 `compact_metadata.pre_compact_discovered_tools`
- 在构建 API request 时：`filtered_tools = core_tools ∪ discovered_tools ∪ {SearchExtraTools, ExecuteExtraTool}`

**步骤 E: API 层工具过滤**

- 在 `allthecodes-engine` 的 query builder 中：
  - 检查是否启用 `search_extra_tools` 特性
  - 如启用，将 `filtered_tools` 用于 API 请求（只发送选定工具的 schema）
  - 否则保持当前行为（发送所有工具 schema）

**估算**: ~1200 行 Rust（4 个模块各 ~300 行）

#### 3.8.4 与现有 ToolSearch 的关系

| 维度 | 现有 ToolSearch | 新增 SearchExtraTools |
|------|----------------|----------------------|
| 使用者 | 子 agent / 主进程内部搜索 | **模型**在 API 调用中搜索延迟工具 |
| 搜索范围 | 当前会话全量工具 | 仅延迟工具（非 CORE_TOOLS） |
| 输出类型 | 工具定义供代码使用 | 工具名 + 描述供模型选择 |
| 配套机制 | 无 | + ExecuteExtraTool 执行 + 发现状态追踪 |
| 部署阶段 | 已存在 | Phase 5（或独立并行） |

现有 `ToolSearch` 的 BM25 能力可作为 `SearchExtraTools` 的基础搜索引擎复用。`SearchExtraTools` 在此基础上增加 TF-IDF 索引层（可选）和 `select:`/`discover:` 模式支持。
## 4. 架构改动影响

### 4.1 新增文件/模块

```
crates/allthecodes-tools/src/
├── notebook_edit.rs          # P0: NotebookEdit 工具
├── cron/
│   ├── mod.rs                # P1: Cron 工具模块
│   ├── create.rs
│   ├── delete.rs
│   └── list.rs
├── web_browser.rs            # P1: WebBrowser 工具
├── monitor.rs                # P1: Monitor 工具
├── send_user_file.rs         # P1: SendUserFile 工具
├── ctx_inspect.rs            # P2: CtxInspect 工具
├── terminal_capture.rs       # P2: TerminalCapture 工具
├── review_artifact.rs        # P2: ReviewArtifact 工具
└── snip.rs                   # P2: Snip 工具

crates/allthecodes-mcp/src/
├── resources.rs              # P0: ListMcpResources / ReadMcpResource 实现
└── resource_tools.rs         # P0: Tool trait 适配器

crates/allthecodes-cron/      # P1: 可选独立 crate
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── job.rs
│   ├── scheduler.rs
│   └── storage.rs
```

### 4.1b 延迟工具系统新增文件 (P3)

```
crates/allthecodes-engine/src/
├── deferred_tools.rs         # P3: CORE_TOOLS 边界定义 + is_deferred_tool + discovered_tools 状态管理
├── tool_filters.rs           # P3: API 请求时的工具过滤逻辑（仅发核心 + 已发现工具）
└── extract_discovery.rs      # P3: extract_discovered_tool_names() 消息历史扫描

crates/allthecodes-tools/src/
├── search_extra_tools.rs     # P3: SearchExtraTools 工具实现
└── execute_extra_tool.rs     # P3: ExecuteExtraTool 工具实现
```

### 4.2 需修改的现有文件

| 文件 | 改动 | 优先级 |
|------|------|--------|
| `crates/allthecodes-tools/src/fs/mod.rs` | 追加 NotebookEdit 到 `tools()` | P0 |
| `crates/allthecodes-tools/src/registry.rs` | 追加新工具的注册 | P0-P2 |
| `crates/allthecodes-startup/src/tool_registry.rs` | 追加 MCP resource 工具 provider | P0 |
| `crates/allthecodes-mcp/src/lib.rs` | 暴露 Resource 查询接口 | P0 |
| `crates/allthecodes-mcp/src/manager.rs` | 新增 resource 路由/缓存 | P0 |
| `crates/allthecodes-permissions/src/rules.rs` | 注册新工具的权限规则（部分已有） | P0-P2 |
| `crates/allthecodes/src/ui/permissions/permission_request_router.rs` | 追加新工具 UI 权限路由（部分已有） | P0-P2 |
| `crates/allthecodes-engine/src/lib.rs` | 导出 `deferred_tools` 模块，集成提取/过滤 | P3 |
| `crates/allthecodes-engine/src/query_builder.rs` | API 请求构建时根据 `CORE_TOOLS + discovered` 过滤工具列表 | P3 |
| `crates/allthecodes-tools/src/registry.rs` | 追加 `SearchExtraTools` 和 `ExecuteExtraTool` 注册 | P3 |
| `crates/allthecodes/src/state/session.rs` | 会话状态中维护 `discovered_tools` HashSet | P3 |
| `crates/allthecodes-startup/src/tool_registry.rs` | 确保 `SearchExtraTools` 和 `ExecuteExtraTool` 始终注册且不会延迟 | P3 |

### 4.3 系统提示词改动

新工具需要加入系统提示词，引导模型在合适场景调用：
- NotebookEdit → 在检测到 `.ipynb` 文件编辑时优先选择
- ListMcpResources / ReadMcpResource → 在需要访问 MCP server 数据时激活
- Cron 套件 → 在需要定时/周期性任务时调用
- WebBrowser → 在 WebFetch 无法获取渲染后内容时建议使用
- SearchExtraTools / ExecuteExtraTool → 系统提示词需说明：核心工具直接调用，非核心工具先 `SearchExtraTools` 搜索再通过 `ExecuteExtraTool` 执行；附带延迟工具列表供模型参考

---

## 5. 执行路线图

### Phase 0 — 基线冻结与实现边界 (P0 准备, 2-3 天)

Phase 0 不做运行时行为改动，目标是把 P0 工具的上游契约、当前 Rust 接线点、测试基线和命名边界冻结下来，避免 Phase 1 一边实现一边重新判定范围。

#### 0A. 上游契约冻结

参考路径：
- `claude-code-bun/packages/builtin-tools/src/tools/NotebookEditTool/NotebookEditTool.ts`
- `claude-code-bun/packages/builtin-tools/src/tools/ListMcpResourcesTool/ListMcpResourcesTool.ts`
- `claude-code-bun/packages/builtin-tools/src/tools/ReadMcpResourceTool/ReadMcpResourceTool.ts`
- `claude-code-bun/src/services/mcp/client.ts`

输出物：
- 记录上游参考 commit、allthecodes 当前 commit、对照日期。
- 固定 `NotebookEdit` 输入字段：`notebook_path`、`cell_id`、`new_source`、`cell_type`、`edit_mode`。
- 固定 `NotebookEdit` 输出字段：`new_source`、`cell_id`、`cell_type`、`language`、`edit_mode`、`error`、`notebook_path`、`original_file`、`updated_file`。
- 固定 MCP resource 工具输入/输出结构：`server` 可选过滤、`uri` 必填读取、`contents[]` 支持 text 与 blob。
- 明确工具命名策略：Phase 1 默认采用 allthecodes 当前 CamelCase 内置工具风格；若选择与 bun 完全兼容，应显式采用 `ListMcpResourcesTool` / `ReadMcpResourceTool`，否则在本计划中记录保留差异并说明映射关系。

#### 0B. 当前 Rust 接线点清单

确认并记录以下当前状态：
- `crates/allthecodes-permissions/src/rules.rs` 已把 `NotebookEdit` 纳入 edit/AcceptEdits 规则。
- `crates/allthecodes/src/ui/permissions/permission_request_router.rs` 已有 `PermissionRouteKind::NotebookEdit`、`Monitor`、`ReviewArtifact` 路由。
- `crates/allthecodes-mcp/src/client/mod.rs` 已实现 `list_resources()` 与 `read_resource()`。
- `crates/allthecodes-mcp/src/manager.rs` 只有 `all_resources()` 快照，缺少按 server/URI 定位和读取的 manager API。
- `crates/allthecodes-startup/src/tool_registry.rs` 目前只注册 root-owned 工具、plugin 工具；MCP resource helper tools 还没有 provider 接线。

输出物：
- 在本文件追加一段 “Phase 0 Baseline Snapshot”，列出上述状态与缺口。
- 若发现本计划中路径使用旧写法（例如缺少 `crates/` 前缀），在 Phase 0 同步修正文档路径，避免后续任务误改旧路径。

#### 0C. 行为边界与非目标

Phase 1 必做：
- `NotebookEdit` 必须要求 Read-before-edit，并复用 `FileStateCache` 防止 stale edit。
- `.ipynb` 必须保留除目标 cell 外的 notebook metadata、cell metadata、nbformat/nbformat_minor。
- code cell 修改或插入时必须清空 `outputs` 并把 `execution_count` 置为 `null`。
- `source` 必须兼容 notebook 中的 string 与 string array 表示；写回策略以 Phase 0 冻结记录为准。
- MCP resource 工具必须只读取当前已连接 MCP server，不隐式启动未配置 server。
- MCP resource 工具必须 read-only、concurrency-safe，不进入 edit 权限路径。

Phase 1 非目标：
- 不实现 Cron / WebBrowser / Monitor / SendUserFile。
- 不引入新的 MCP transport。
- 不重构 MCP tool wrapper 的既有命名规则。
- 不做完整 binary blob 持久化目录设计；若 ReadMcpResource 遇到 blob，Phase 1 只允许采用已有 allthecodes 数据目录下的最小安全落盘方案，或返回明确的 “binary unsupported in Phase 1” 错误。

#### 0D. 验证基线

Phase 0 结束前运行并记录结果：

```
cargo test -p allthecodes-startup tool_registry -- --nocapture
cargo test -p allthecodes-mcp --lib -- --nocapture
cargo test -p allthecodes-tools fs:: -- --nocapture
cargo test -p allthecodes-permissions rules -- --nocapture
cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture
```

如果某个命令已有非本任务失败，Phase 0 要记录失败用例、首个错误和是否阻塞 Phase 1；不要在 Phase 1 中把无关历史失败混入 P0 工具实现。

#### Phase 0 退出条件

- [x] 上游契约、Rust 接线点、命名策略已记录。
- [x] Phase 1 文件写入范围明确。
- [x] Phase 1 非目标明确，未把 P1/P2 工具提前混入。
- [x] 当前测试基线已运行或记录无法运行原因。
- [x] 没有运行时行为变更；Phase 0 只产生文档变更。

#### Phase 0 Implementation Snapshot (2026-05-29)

参考版本：
- allthecodes: `e131e6e8180cd41763d09c4ccf5efcd6aab9fef8`
- claude-code-bun: `2cc9a7daef652286ba8ad27f3489d713004059b7` (→ 2026-05-29 已合并 `upstream/main` 至 `efc218d8ef654665b2cd10a1a43caaf2069e965e`，新增 autofix-pr 等特性)

冻结契约：
- `NotebookEdit` 使用 `notebook_path`、`cell_id`、`new_source`、`cell_type`、`edit_mode`。
- `NotebookEdit` 返回 `new_source`、`cell_id`、`cell_type`、`language`、`edit_mode`、`error`、`notebook_path`、`original_file`、`updated_file`。
- MCP resource 工具采用 allthecodes CamelCase 名称：`ListMcpResources`、`ReadMcpResource`。
- `ListMcpResources` 输入为可选 `server`，输出 resource 列表并包含 `server`、`uri`、`name`、`description`、`mimeType`。
- `ReadMcpResource` 输入为必填 `server` 与 `uri`，输出 `contents[]`；binary blob 不直接写入模型上下文。

基线结果：
- `cargo test -p allthecodes-startup tool_registry -- --nocapture`: 通过，7 passed。
- `cargo test -p allthecodes-tools fs:: -- --nocapture`: 通过，72 passed。
- `cargo test -p allthecodes-permissions rules -- --nocapture`: 通过，52 passed。
- `cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture`: 通过，8 passed。
- `cargo test -p allthecodes-mcp --lib -- --nocapture`: sandbox 内因 loopback/OAuth socket 权限失败；按批准的 escalated 命令重跑通过，60 passed。

### Phase 1 — P0 工具快速补齐 (1-2 周) ✅ **已完成**

Phase 1 只交付 `NotebookEdit` 与 MCP resource helper tools。建议拆成 5 个可独立 review 的切片，按顺序执行；前两个 NotebookEdit 切片可并行做只读调研，但代码落地应串行，避免同一 fs 模块冲突。

**当前状态**: 全部 5 个子阶段（1A-1E）已完成。NotebookEdit 在 `fs/notebook_edit.rs`，MCP resource tools 在 `engine/src/mcp_resource_tools.rs`。系统提示词已集成，ToolSearch 索引已更新，权限 UI 已就位。

#### 1A. NotebookEdit 数据模型与纯函数 ✅

写入范围：
- `crates/allthecodes-tools/src/fs/notebook_edit.rs`
- `crates/allthecodes-tools/src/fs/mod.rs`
- 必要时补充 `crates/allthecodes-tools/Cargo.toml`，但优先使用已有 `serde_json`、`uuid`、`chrono`、`similar`、`tokio`。

实现要点：
- 定义 `NotebookEditTool`，工具名为 `NotebookEdit`。
- 定义内部 `NotebookDocument` / `NotebookCell` serde 表示，保留未知字段，避免丢弃上游或 Jupyter 扩展 metadata。
- 支持 `edit_mode`: `replace`、`insert`、`delete`，默认 `replace`。
- 支持 `cell_type`: `code`、`markdown`；`insert` 时必填或按上游兼容策略默认 `code`，具体以 Phase 0 冻结结果为准。
- cell 定位顺序与 bun 对齐：先按真实 cell `id` 查找，再支持 `cell-N` 数字索引格式。
- `replace` 指向末尾后一格时按上游行为转为 `insert`。
- 修改 code cell 时清空 `outputs`，`execution_count` 写为 `null`。
- 新增 nbformat 4.5+ cell 时生成 cell `id`；低版本 notebook 不强制写入 `id`。
- 用纯函数覆盖 JSON parse、cell 查找、cell 修改、写回 JSON 构造，降低工具 I/O 测试成本。

测试要求：
- 有真实 `.ipynb` fixture 或 inline JSON fixture。
- 覆盖 replace by id、replace by `cell-N`、insert after cell、insert at beginning、delete、cell type switch。
- 覆盖 code cell 输出清理。
- 覆盖 string source 与 array source 输入。
- 覆盖 invalid JSON、非 `.ipynb`、cell 不存在、非法 edit_mode。

#### 1B. NotebookEdit 文件 I/O、权限与结果渲染数据 ✅

写入范围：
- `crates/allthecodes-tools/src/fs/notebook_edit.rs`
- `crates/allthecodes-tools/src/fs/mod.rs`
- 如 TUI 现有 NotebookEdit permission preview 需要字段补齐，仅修改 `crates/allthecodes/src/ui/permissions/notebook_edit_permission_request/`

实现要点：
- `input_json_schema()` 与 Phase 0 冻结契约一致。
- `validate_input()` 执行路径校验、扩展名校验、read-before-edit、stale read 校验、notebook JSON 校验、cell 定位校验。
- `get_path()` 返回 `notebook_path`，`is_destructive()` 返回 true，`is_read_only()` 返回 false，`is_concurrency_safe()` 返回 false。
- `check_permissions()` 复用现有文件写权限决策路径；不要新增独立权限语义，除非 Phase 0 证明现有 API 不可复用。
- `call()` 使用 `safe_write_text()` 原子写回，更新 `FileStateCache`，并复用 `edited_text_file_message()` 产生 edited attachment。
- 触发 `FileChanged` hook，operation 建议为 `notebook_edit`，payload 至少包含 `file_path`、`edit_mode`、`cell_id`、`cell_type`、`safe_write`。
- `ToolResult.data` 返回冻结输出字段；`model_content` 返回简洁文字，不把完整 notebook 重复塞入模型结果。
- `display_preview` 提供 TUI 可消费的 JSON，至少含 `kind: "notebook_edit"`、`path`、`edit_mode`、`cell_id`、`hunk_lines` 或摘要。

测试要求：
- Read → NotebookEdit → Read 不返回 stale 内容。
- 未 Read 直接编辑失败。
- Read 后文件被外部修改时失败。
- safe write 失败时返回 tool error data，不 panic。
- registry 测试能发现 `NotebookEdit` 且 schema 合法。

#### 1C. MCP manager resource API ✅

写入范围：
- `crates/allthecodes-mcp/src/manager.rs`
- `crates/allthecodes-mcp/src/lib.rs`
- `crates/allthecodes-mcp/src/client/client_tests.rs` 或 manager 专用测试模块

实现要点：
- `list_resources(server: Option<&str>) -> Result<Vec<McpResourceWithServer>>`，结果包含 `server`、`uri`、`name`、`description`、`mime_type`。
- `read_resource(server: &str, uri: &str) -> Result<ReadResourceResult>`，按 server 精确定位，区分 “server 不存在”、“server 未连接”、“server 不支持 resources”、“resource read failed”。
- 可选增加 `find_client_for_resource(server, uri)` 或 `client_for_server(server)`；不要暴露可变 client 引用到不必要的外层。
- 如果已有 `resources/list_changed` 通知路径，应记录是否会刷新 `client.resources`；Phase 1 至少保证手动 list 后结果不 stale。
- manager API 保持 read-only 语义，不影响现有 `all_tools()` / MCP tool wrapper。

测试要求：
- 多 server 聚合列表。
- 按 server 过滤列表。
- server 不存在错误包含可用 server 列表。
- 读取 resource 时调用正确 server。
- 单 server 失败不影响其他 server list，或明确记录 Phase 1 是否 fail-fast。

#### 1D. MCP resource Tool adapter 与注册 ✅

推荐实现位置：
- `crates/allthecodes-engine/src/mcp_resource_tools.rs`，如果需要访问运行时 MCP manager。
- 或 `crates/allthecodes-mcp/src/resource_tools.rs`，前提是不会造成 `allthecodes-mcp -> allthecodes-tools` 依赖环。
- 注册点：`crates/allthecodes-startup/src/tool_registry.rs`

实现要点：
- 实现 `ListMcpResources` 与 `ReadMcpResource` 两个 `Tool`。
- 二者 `is_read_only()`、`is_concurrency_safe()` 返回 true，`is_destructive()` 返回 false。
- `ListMcpResources` 输入：`server?: string`。
- `ReadMcpResource` 输入：`server: string`、`uri: string`。
- `ToolResult.data` 对齐 Phase 0 冻结契约。
- 文本 resource 进入 `model_content`；binary blob 不直接把 base64 放入上下文。Phase 1 若未实现安全落盘，应返回明确错误并在本计划记录剩余项。
- provider 注册后，`get_all_tools()` 包含两个工具且不与 MCP server 动态工具重名。
- 如果最终工具名选择 bun 兼容形式，所有测试和文档统一使用 `ListMcpResourcesTool` / `ReadMcpResourceTool`，不要混用。

测试要求：
- `test_all_tools_have_unique_names` 通过。
- `test_all_tools_have_schema` 通过。
- read-only/concurrency-safe 断言。
- 空 resource list 返回 “No resources found” 类似提示。
- server 过滤和 read 错误路径有稳定消息。

#### 1E. 系统提示词、ToolSearch、TUI/headless 验证 ✅

写入范围：
- `crates/allthecodes-engine/src/system_prompt/` 下现有动态/静态提示词文件。
- `crates/allthecodes-tools/src/tool_search.rs` 仅在新工具没有被通用索引自然覆盖时修改。
- `crates/allthecodes/src/ui/messages/` 或 permission preview 文件仅在现有渲染无法展示新结果时修改。

实现要点：
- 系统提示词说明：编辑 `.ipynb` 优先用 `NotebookEdit`，不要用 `Edit` 直接改 JSON，除非 NotebookEdit 不可用。
- 系统提示词说明：需要 MCP server 资源时先 list，再 read；不要猜 URI。
- ToolSearch 能用 “jupyter / notebook / ipynb / mcp resources” 找到对应工具。
- TUI permission router 已有 NotebookEdit route；Phase 1 只补缺失字段，不重写权限 UI。
- headless JSONL 输出包含新工具 result，不破坏现有 attachment/message schema。

验证命令：

```
cargo test -p allthecodes-tools notebook_edit -- --nocapture
cargo test -p allthecodes-mcp --lib -- --nocapture
cargo test -p allthecodes-startup tool_registry -- --nocapture
cargo test -p allthecodes-engine mcp_resource -- --nocapture
cargo test -p allthecodes ui::permissions::permission_request_router -- --nocapture
cargo build --workspace --release
git diff --check
```

### Phase 2 — 调度与自动化 (P1, 3-4 周) ✅ **已完成**

所有 Phase 2 工具已实现：Cron 套件（`allthecodes-services/src/scheduler_tools.rs`）+ WebBrowser（`allthecodes-engine/src/browser_tool.rs`）。

```
Week 3-4:
  [Cron 套件]
    - 实现 cron 解析、调度器、持久化
    - 注册三个工具
    - 集成 daemon 生命周期

  [WebBrowser]
    - 对接 allthecodes-browser 的 CDP 能力
    - 权限模型（安全敏感: 需要单独授权）
    - 前端渲染（全页面/截图/文本提取）
```

### Phase 3 — 产品体验 (P1-P2, 5-7 周) ✅ **已完成**

所有 Phase 3 工具已实现：Monitor、SendUserFile、TerminalCapture、Snip、CtxInspect、ReviewArtifact（均在 `allthecodes-tools/src/product_tools.rs`）。Snip 压缩逻辑在 `allthecodes-compact/src/snip.rs`。权限 UI 已就位。

```
Week 5:
  [Monitor + SendUserFile]
    - 后台监控 + 文件发送

Week 6-7:
  [TerminalCapture + Snip + CtxInspect + ReviewArtifact]
    - 终端捕获、历史压缩、上下文检视、制品审查
    - 按依赖关系排序：CtxInspect 最简单（纯读取），Snip 需要 compaction 层
```

### Phase 4 — 协作与远程 (P2, 8-9 周) ✅ **已完成**

所有 Phase 4 工具已实现：RemoteTrigger、ListPeers、SubscribePR（完整版）（均在 `allthecodes-tools/src/product_tools.rs` 和 `allthecodes-teams/src/pr_activity.rs`）。

```
Week 8-9:
  [RemoteTrigger + ListPeers + SubscribePR 补齐]
    - 远程触发需要先定义 CCR API 协议
    - ListPeers 基于已有 IPC transport
    - SubscribePR 补齐 bun 版功能
```

### Phase 5 — 延迟工具系统与评估 (P3, 持续) 🟡 **部分完成**

Phase 5 引入 SearchExtraTools/ExecuteExtraTool 延迟工具加载系统，并对其他 P3 工具做持续评估。

**已完成**: SearchExtraTools + ExecuteExtraTool 已在 `allthecodes-tools/src/deferred_tools.rs` 中完整实现，含 CORE_TOOLS 边界定义、select/discover/keyword 查询模式、discovery guard 委托执行、跨 turn 发现状态恢复和 API 请求工具过滤。

```
Phase 5a (可选，提前):
  [SearchExtraTools + ExecuteExtraTool]
    - 定义 CORE_TOOLS 边界（38 个核心工具）
    - 实现 SearchExtraTools 工具（select/discover/keyword 三种查询模式）
    - 实现 ExecuteExtraTool 工具（discovery guard + 委托执行）
    - 实现 extract_discovered_tool_names() 跨 turn 状态持久化
    - 在 API 请求构建器中添加工具过滤逻辑
    - 部署后：API 请求中工具 schema 数量从 ~60 降至 ~40+discovered

Phase 5b (持续):
  - LocalMemoryRecall → ✅ JSON/文件索引 v1
  - VaultHttpFetch → ✅ HTTPS-only + vault credential_ref
  - PushNotification → ✅ local audit provider + HTTPS webhook provider
  - DiscoverSkills → ✅ Skill registry 发现 Tool
  - VerifyPlanExecution → ✅ plan workflow / task / todo read-only 验证
  - Workflow → ✅ Rust-native durable workflow Tool
  - ApplyPatch → ✅ JSON `{patch}` Codex patch grammar Tool
  - Goal 管理 → ✅ `GetGoal` / `CreateGoal` / `UpdateGoal`
  - Multi-agent v2 → ✅ 复用 teams/coordinator config + mailbox
  - ViewImage → ✅ 本地图片 content block Tool
```

#### Phase 5 Implementation Snapshot (2026-05-29)

- `SearchExtraTools` / `ExecuteExtraTool` 注册于 `allthecodes-tools/src/deferred_tools.rs`，并通过 `allthecodes-tools/src/registry.rs` 进入核心工具集合。
- `ALLTHECODES_DEFERRED_TOOL_LOADING` / `CC_RUST_DEFERRED_TOOL_LOADING` 控制 API 请求过滤；开启后 `prepare_model_request()` 只发送 `CORE_TOOLS ∪ discovered_tools`。
- `extract_discovered_tool_names()` 支持 SearchExtraTools JSON/text result、`deferred_tools_delta` structured attachment、legacy `tool_reference(s)` 字段，以及 `compact_metadata.pre_compact_discovered_tools`。
- auto-compact 后会把当前 session 的 discovered tools 写入 compact boundary metadata，便于 session 恢复后继续过滤工具 schema。
- `discover:<query>` 只返回 schema/描述，不加载工具；`select:<tool>` 或普通关键词匹配会写入 `deferred_tools_delta` 并让下一轮请求包含对应工具 schema。
- Phase 5 剩余工具已新增到 `allthecodes-tools/src/phase5/`：`DiscoverSkills`、`ViewImage`、`GetGoal`、`CreateGoal`、`UpdateGoal`、`VerifyPlanExecution`、`Workflow`、`ApplyPatch`、`LocalMemoryRecall`、`VaultHttpFetch`、`PushNotification`。
- Multi-agent v2 工具新增到 `allthecodes-teams/src/multi_agent_v2.rs`：`ListAgents`、`FollowupTask`、`WaitAgent`、`CloseAgent`；复用现有 team config 与 mailbox，不新增第二套 agent registry。
- 新持久化路径集中在 `allthecodes-config/src/paths.rs`：`goals/`、`workflows/`、`vault/`、`notifications/`。
- 验证命令：`cargo test -p allthecodes-tools phase5 -- --nocapture`、`cargo test -p allthecodes-startup tool_registry -- --nocapture`、`cargo test -p allthecodes-config paths -- --nocapture`、`cargo test -p allthecodes-teams --lib -- --nocapture`。

---

## 6. 依赖与阻塞（实现状态评估）

| 工具 | 外部依赖 | 内部依赖 | 风险 | 实际实现 |
|------|---------|---------|------|---------|
| NotebookEdit | `serde_json` (已有) | 无 | 低 | ✅ 已实现 |
| ListMcpResources | 无 | allthecodes-mcp resource 接口暴露 | 低 | ✅ 已实现 |
| ReadMcpResource | 无 | allthecodes-mcp resource 接口暴露 | 低 | ✅ 已实现 |
| Cron | `cron` crate | daemon 生命周期 | 中（daemon 集成） | ✅ 已实现 |
| WebBrowser | `allthecodes-browser` / `headless_chrome` | 浏览器检测、权限系统 | 中（安全/平台差异） | ✅ 已实现 |
| Monitor | 无 | task store、daemon | 低 | ✅ 已实现 |
| RemoteTrigger | 无 | CCR API 协议定义 | 中（协议未定） | ✅ 已实现 |
| TerminalCapture | 无 | PTY/进程管理 | 低 | ✅ 已实现 |
| SubscribePR | GitHub API | teams 基础设施 | 低（已有基础） | ✅ 已实现 |
| CtxInspect | 无 | QueryEngine 上下文统计 API | 低 | ✅ 已实现 |
| Snip | 无 | `allthecodes-compact` | 低 | ✅ 已实现 |
| SearchExtraTools | 无 | allthecodes-tools registry + 现有 ToolSearch BM25 能力 | 低 | ✅ 已实现 |
| ExecuteExtraTool | 无 | allthecodes-tools registry + discovered_tools 状态 + deferred_tools 模块 | 低 | ✅ 已实现 |
| 延迟工具系统 | 无 | allthecodes-engine 的 query_builder + 会话状态 | 中 | ✅ 已实现（CORE_TOOLS + 发现状态 + API 请求过滤） |
| SendUserFile | 无 | — | 低 | ✅ 已实现 |
| ReviewArtifact | 无 | — | 低 | ✅ 已实现 |
| ListPeers | 无 | IPC transport | 低 | ✅ 已实现 |

---

## 7. 验收标准（截至 2026-05-29）

### Phase 0 验收 ✅

- [x] 已记录上游参考 commit、allthecodes 当前 commit、对照日期
- [x] 已冻结 `NotebookEdit`、`ListMcpResources`、`ReadMcpResource` 的输入/输出契约
- [x] 已确认 P0 工具命名策略，且文档内没有混用未说明的别名
- [x] 已记录 Rust 现有接线点、缺失接口、Phase 1 写入范围
- [x] 已运行 Phase 0 基线命令，或记录每个无法运行命令的原因和首个错误
- [x] Phase 0 未引入运行时行为变更

### Phase 1 验收 ✅

- [x] NotebookEdit 可编辑 `.ipynb` 文件中的任意 cell
- [x] NotebookEdit 支持插入/删除 cell 和切换 cell 类型
- [x] NotebookEdit 强制 Read-before-edit，并能拒绝 stale edit
- [x] NotebookEdit 修改 code cell 时清空 outputs 且重置 execution_count
- [x] ListMcpResources 列出所有已连接 MCP server 的资源
- [x] ReadMcpResource 按 URI 读取 resource 内容
- [x] MCP resource 工具为 read-only、concurrency-safe，且不进入 edit 权限路径
- [x] 所有 P0 工具在 `allthecodes_tools_base_tools()` 或 provider 中注册
- [x] 所有 P0 工具有单元测试
- [x] Phase 1 验证命令通过，或文档记录非本任务失败的首个错误与剩余风险

### Phase 2 验收 ✅

- [x] Cron 支持标准 5 字段表达式
- [x] Cron job 在 daemon 模式下正确触发
- [x] Cron job 持久化跨进程重启存活
- [x] WebBrowser 能获取 JS 渲染后的页面内容
- [x] WebBrowser 有独立的权限提示（安全模型）

### Phase 3 验收 ✅

- [x] Monitor 后台运行并返回定期状态更新
- [x] SendUserFile 正确发送文件内容给用户
- [x] TerminalCapture 捕获 PTY 输出
- [x] Snip 对历史消息执行 compaction
- [x] CtxInspect 返回有意义的上下文统计

### Phase 4 验收 ✅

- [x] RemoteTrigger 支持 HTTP 远程触发
- [x] ListPeers 发现本地 + 远程对等点
- [x] SubscribePR 完整 PR 订阅（含 webhook 路由）

### Phase 5 验收 🟡

- [x] SearchExtraTools 实现 select/discover/keyword 查询模式
- [x] ExecuteExtraTool 实现 discovery guard + 委托执行
- [x] CORE_TOOLS 边界定义
- [x] extract_discovered_tool_names() 跨 turn 持久化
- [x] API 请求工具过滤（仅发核心工具 schema）
- [x] LocalMemoryRecall — allthecodes memory / auto_memory / transcripts 检索 v1
- [x] VaultHttpFetch — HTTPS-only + credential_ref + redaction
- [x] PushNotification — local audit provider + HTTPS webhook provider
- [x] DiscoverSkills — Skill registry 发现 Tool
- [x] VerifyPlanExecution — plan workflow / task / todo read-only 验证
- [x] Workflow — Rust-native durable workflow Tool
- [x] ApplyPatch — JSON `{patch}` Codex patch grammar Tool
- [x] Goal 管理 — `GetGoal` / `CreateGoal` / `UpdateGoal`
- [x] Multi-agent v2 拓展（`FollowupTask` / `ListAgents` / `CloseAgent` / `WaitAgent`）
- [x] ViewImage — 本地图片 content block Tool

### 通用 ✅

- [x] 所有新工具通过 `test_all_tools_have_unique_names` 测试
- [x] 所有新工具通过 `test_all_tools_have_schema` 测试
- [x] 所有新工具有完整的 JSON schema
- [x] 权限系统为每个新工具注册相应规则
- [x] TUI 和 headless 前端正确渲染新工具的输出
