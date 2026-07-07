# 工具发现当前实现情况

日期：2026-07-07

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

ToolSearch 是精确可调用工具目录：需要工具名、输入 schema、调用示例或
`select:<tool-name>` 精确查找时使用它。延迟工具的实际执行仍通过
`SearchExtraTools`/`ExecuteExtraTool` 管理 discovered 状态和参数调用。

## Domain Discovery 搜索面

本轮新增了和 `ToolSearch` 分开的领域发现面，避免把“找能力”和“取可调用
schema”混在一起：

- `SkillSearch`：从本地 skill registry 搜索 bundled/user/project/plugin/MCP skill。
  复用 `DiscoverSkills` 的本地评分 helper，但输出规范化的 `results[]`，包含
  source、`when_to_use`、match reasons、next action、prefetch/remote-state 占位。
  `DiscoverSkills` 保持原有 `skills[]` 兼容输出。
- `McpSearch`：搜索 MCP server/resource/capability/MCP skill 摘要。结果可包含
  MCP tool 名称和描述摘要，但不返回完整 input schema；精确 schema 仍走
  `ToolSearch(source=mcp, include_schema=true)`。
- `PluginSearch`：搜索 installed/active/marketplace-cache plugin 摘要。结果可列
  plugin skills/tools/MCP contributions，但不返回 plugin tool schema；精确 schema
  仍走 `ToolSearch(source=plugin, include_schema=true)`。

共享类型和 provider boundary 位于
`crates/allthecodes-tools/src/discovery_search.rs`。root runtime 在
`crates/allthecodes/src/command_runtime_bridge.rs` 注入 MCP/plugin discovery
provider，搜索工具不直接依赖 MCP/plugin 实现 crate。provider panic 或未安装时
返回空本地结果和解释性 preview，不触发 reconnect、refresh、install、enable 或
网络请求。

对应 slash command 已接入：

- `/skills search <query>`
- `/mcp search <query>`
- `/plugin search <query>`

这些命令渲染简短结果表、match reasons 和 next action，并提示完整诊断仍在
`/mcp status`、`/plugin status`、`/skills diagnostics`、`SystemStatus`，精确
callable schema 仍在 `ToolSearch`。

## MCP Skill、Skill Prefetch 与 Remote URL Gates

`allthecodes-config` 现在暴露三个发现相关 gate：

- `FEATURE_MCP_SKILLS` / `Feature::McpSkills`：控制 MCP `skill://` resource
  ingestion。关闭时不注册 MCP-provided skills，但普通 MCP tools/resources 保持
  原有发现和执行路径。
- `FEATURE_EXPERIMENTAL_SKILL_SEARCH` / `Feature::ExperimentalSkillSearch`：
  控制本地 skill-search prefetch、turn-zero discovery 和 remote-state 占位。关闭
  时不做 prefetch enrichment，但显式本地 `SkillSearch` 仍可用。
- `FEATURE_REMOTE_URL_DISCOVERY` / `Feature::RemoteUrlDiscovery`：控制远程
  skill/plugin/MCP URL discovery 的占位状态。默认关闭；关闭时 discovery 输出
  `remote_source="feature_disabled"` 或 prefetch `remote_state="feature_disabled"`。
  开启时当前实现仍只返回 `remote_source="deferred"` / `remote_state="deferred"`，
  不做网络访问。

当前 prefetch 只读取本地 skill 元数据（name、description、source、
`when_to_use`、argument hint/name、paths、assets、entry docs、dependencies 和
prompt body）。remote state 只用于说明状态：

- `not_configured`：`FEATURE_EXPERIMENTAL_SKILL_SEARCH` 关闭，未做 prefetch。
- `feature_disabled`：本地 prefetch 已执行，但 `FEATURE_REMOTE_URL_DISCOVERY`
  关闭。
- `deferred`：remote URL discovery gate 已开启，但远程 fetch/registry/install
  仍未实现。

无论上述哪种状态，当前实现都不会 fetch remote URL、刷新远程 registry、安装、
启用或信任远程内容。

## Kairos / Proactive Search Tips

`crates/allthecodes-services/src/search_tips.rs` 提供本地 `SearchTipService`：

- tip kind 覆盖 skill、MCP、plugin。
- 只有 `FEATURE_KAIROS` 或 `FEATURE_PROACTIVE` 开启时生成。
- 对同一 session/kind/id 做去重和 cooldown。
- 持久 dismiss/remind-later 写入
  `~/.allthecodes/search-tip-dismissals.json`（或 `ALLTHECODES_HOME` 下同名文件），
  JSON 形状兼容 LSP recommendation dismissal：`plugin_id`、`dismissed_at`、
  `remind_after_secs`。`remind_after_secs=null`/缺失表示永久 dismiss；有秒数时
  到期后重新允许提示。
- 输出仍复用现有 `PromptSuggestion` / `BackendMessage::Suggestions`，不会新增
  IPC message。
- tip 是 advisory 文本，提示用户运行 `/skills ...`、`/mcp search ...`、
  `/plugin info ...` 或 `ToolSearch ...`；不会自动安装、启用、连接、重载或执行
  skill/plugin/MCP 操作。

headless 路径在 `crates/allthecodes/src/app_runtime_adapters/sdk_mapper.rs` 合并
prefetch-derived skill candidates；Rust TUI 路径在
`crates/allthecodes/src/ui/tui/engine_events.rs` 做同样合并。daemon proactive tick
在 `crates/allthecodes-daemon/src/tick.rs` 中仅写入本地 discovery 摘要
`{ count, remote_state }`，不携带 remote URL。

## 当前边界与待补齐点

- ToolSearch 的来源和类别推断部分依赖启发式规则，例如 `mcp__` 前缀和描述文本。
- Domain discovery 搜索面返回摘要和 next action，不返回完整 callable schemas。
- 重名工具当前是 first wins；不会自动命名空间化后注册冲突。
- deferred discovered state 主要是运行时状态和 compact metadata，不是独立的长期持久索引。
- MCP 连接失败不会阻塞会话启动；失败信息通过状态面暴露，工具不会进入可执行集合。
- 插件/MCP 工具是否可见取决于当前 provider 和 manager 刷新结果；没有把所有外部工具固定写入静态清单。
- 远程 skill/plugin/MCP URL discovery 仍是显式 deferred TODO，并已由
  `FEATURE_REMOTE_URL_DISCOVERY` 独立 gate 控制。当前实现只保留
  `remote_url_todo` / `remote_source` / `remote_state` 之类状态字段，不做网络访问。
