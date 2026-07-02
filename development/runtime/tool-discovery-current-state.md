# 工具发现当前实现情况

日期：2026-07-02

范围：内置工具、root-owned 工具、MCP 工具、插件工具、技能工具、延迟工具发现和运行时过滤。

## 总体状态

工具发现已经是分层注册体系。运行时不会只从一个静态列表读取工具，而是由多个 provider 合并：

- `allthecodes-tools` 提供基础工具集合。
- `allthecodes-startup` 组装 root-owned 工具和运行时 provider。
- MCP manager 把已连接服务器的工具和资源暴露给引擎。
- 插件系统提供 runtime plugin tools。
- skill 系统提供技能类工具与搜索元数据。
- deferred tools 机制把非核心工具延迟暴露，通过搜索和显式执行降低上下文体积。

最终进入模型和工具执行层的是经过 policy、feature gate、session gate、重复名去重后的工具集合。

## 基础工具注册

核心注册逻辑在 `crates/allthecodes-tools/src/registry.rs`。

当前主要概念：

- `ToolProvider`：工具来源 provider。
- `ToolRegistryProviders`：provider 集合。
- `install_tool_registry_providers()`：安装进程级 provider。
- `ToolPolicy`：按运行角色过滤工具。
- `ToolSessionGates`：按当前 session 状态过滤工具。

`ToolPolicy` 当前包含：

- `DefaultAgent`
- `Coordinator`
- `CoordinatorWorker`
- `InProcessTeammate`

session gates 当前包含：

- `non_interactive`：隐藏 `AskUserQuestion`。
- `subagent`：隐藏递归 agent/spawn/followup 类工具，避免子 agent 再创建无限嵌套任务。

基础工具集合覆盖文件、任务、延迟工具、product、skills、media、goals、workflow、memory、network、notifications、ask-user、config、structured-output、brief、system status 和 tool search 等类别。

## Root-Owned 工具组装

`crates/allthecodes-startup/src/tool_registry.rs` 负责 root 启动阶段的工具合并。

`root_owned_base_tools()` 当前会加入：

- exec/Bash/PowerShell 相关工具。
- browser 工具。
- MCP resource tools。
- scheduler tools。
- multi-agent v2 工具。
- `AgentTool`、`TaskAgentTool`。
- `SkillTool`。
- worktree 工具。
- LSP 工具。
- `SendMessage`。
- PR activity。
- TeamSpawn aliases。

`root_tool_registry_providers()` 会把 root-owned base provider 与 runtime plugin tool provider 合并。最终查询入口包括：

- `get_all_tools()`
- `get_tools_for_active_session()`
- `get_tools_for_policy(policy)`
- `filter_tools_for_policy(...)`

重复工具名会保留第一次注册结果，并记录重复项，避免后注册来源静默覆盖已有工具。

## MCP 工具发现

MCP 发现位于 `crates/allthecodes-mcp/src/discovery.rs`。

当前配置来源：

- 用户配置：`~/.allthecodes/settings.json`
- 项目配置：`<cwd>/.allthecodes/settings.json`

这里已经按 allthecodes 路径隔离读取 `.allthecodes`，不是原版 Codex 的 `.Codex`。

发现结果带有 scope：

- `DiscoveryScope::User`
- `DiscoveryScope::Project`

project 配置在 legacy merged discovery 中覆盖 user 配置。scoped discovery 会保留单个 server 的解析诊断，便于状态页展示配置错误。

连接管理位于 `crates/allthecodes-mcp/src/manager.rs`：

- disabled server 会进入 disabled 状态，不建立 client。
- 启动连接有重试，当前为 3 次，退避 50 到 250ms。
- 初始化失败会 disconnect 并记录 error 状态。
- ready 后按能力列出 tools/resources。
- 提供 `all_tools()`、`all_resources()`、`list_resources()`、`read_resource()`、`find_client_for_tool()` 等入口。

MCP server 状态会被 runtime snapshot 汇总，保留配置诊断和 manager 当前状态。

## 插件与技能工具

插件工具通过 runtime provider 接入 root registry，主要入口是 `allthecodes_plugins::discover_plugin_tools`。

技能系统有两个面向：

- 可执行技能工具，例如 `SkillTool`。
- tool search 目录中的 skill 元数据，帮助模型发现可用能力。

runtime snapshot 会汇总 plugin info 和 skill info，供 Web 状态页使用。

## 延迟工具发现

延迟工具逻辑在 `crates/allthecodes-tools/src/deferred_tools.rs`。

当前策略：

- 核心工具默认暴露。
- 非核心工具默认延迟。
- 模型可以通过 `SearchExtraTools` 查找延迟工具。
- 模型可以通过 `ExecuteExtraTool` 执行已经发现的延迟工具。

`SearchExtraTools` 支持：

- 普通查询。
- `discover:<query>`：只检查描述/schema，不改变 discovered 状态。
- `select:<tool>`：显式选中工具并加入当前 session 的 discovered set。

索引内容包括工具名、描述、prompt、schema 关键词、MCP server 名等。每次返回的结果数有上限，当前最大为 25。

已发现工具状态按 session 保存到内存结构中，并可以从 compaction boundary、工具结果附件、system compact metadata 中恢复部分 discovered names，避免压缩后完全丢失工具发现上下文。

## ToolSearch 目录

运行时搜索目录由 `crates/allthecodes-engine/src/tool_runtime/tool_search.rs` 和 `crates/allthecodes-tools/src/runtime/tool_search.rs` 支撑。

当前能力：

- 引擎安装合并后的 runtime tool catalog。
- ToolSearch 从 runtime tools 和 skills 构建搜索索引。
- 工具来源会分为 builtin、MCP、plugin、skill 等。
- 工具类别包括 read-only、edit、execution、planning、tasks、agent、LSP、system、tool discovery、skill、MCP、plugin 等。
- 标签会标记 read-only、concurrency-safe、destructive 等属性。
- 查询支持别名、CJK tokenization 和基础 stemming。

ToolSearch 是发现和解释层；延迟工具的实际执行仍通过 `SearchExtraTools`/`ExecuteExtraTool` 管理 discovered 状态和参数调用。

## 当前边界与待补齐点

- ToolSearch 的来源和类别推断部分依赖启发式规则，例如 `mcp__` 前缀和描述文本。
- 重名工具当前是 first wins；不会自动命名空间化后注册冲突。
- deferred discovered state 主要是运行时状态和 compact metadata，不是独立的长期持久索引。
- MCP 连接失败不会阻塞会话启动；失败信息通过状态面暴露，工具不会进入可执行集合。
- 插件/MCP 工具是否可见取决于当前 provider 和 manager 刷新结果；没有把所有外部工具固定写入静态清单。
