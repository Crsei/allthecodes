# 插件系统移植缺口

对应: `claude-code-bun/src/utils/plugins/`  
Rust 对应: `cc-plugins/`

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| pluginLoader.ts | 3,305 | 核心插件加载器 |
| marketplaceManager.ts | 2,643 | 市场管理 |
| installedPluginsManager.ts | 1,268 | 安装元数据管理 |
| schemas.ts | 1,683 | Zod 模式定义 |
| validatePlugin.ts | 903 | 插件验证 |
| mcpbHandler.ts | 966 | MCPB 二进制插件格式 |
| loadPluginCommands.ts | 946 | 命令加载 |
| loadPluginAgents.ts | 348 | Agent 定义加载 |
| loadPluginHooks.ts | 287 | Hook 加载 |
| loadPluginOutputStyles.ts | 178 | 输出样式加载 |
| marketplaceHelpers.ts | 592 | 市场工具函数 |
| mcpPluginIntegration.ts | 634 | MCP 服务器集成 |
| pluginInstallationHelpers.ts | 600 | 安装工具 |
| dependencyResolver.ts | 305 | 依赖解析 |
| reconciler.ts | 265 | 状态同步 |
| refresh.ts | 223 | 刷新逻辑 |
| pluginDirectories.ts | 178 | 路径解析 |
| pluginIdentifier.ts | 123 | 标识符解析 |
| zipCache.ts + zipCacheAdapters.ts | 406+ | ZIP 缓存 |
| pluginAutoupdate.ts | 278 | 自动更新 |
| ...其他 15+ 文件 | ~2,000+ | 策略、块列表、遥测等 |
| **合计** | **~20,468** | |

## Rust 已实现 / 已部分实现

### `crates/cc-plugins/` (完整度 ~50%)
- **manifest**: `crates/cc-plugins/src/manifest.rs` 已覆盖 `plugin.json` 的核心字段：tools、skills、mcpServers、lspServers、commands、dependencies、configuration；但 commands/hooks/outputStyles 等字段还没有完整 runtime loader。
- **loader**: `crates/cc-plugins/src/loader.rs` 从 `installed_plugins.json` 加载，兼容 V2 对象格式和 V1 裸数组格式；会校验已安装插件的 `plugin.json`，并把错误写入诊断或 `PluginStatus::Error`。
- **refresh**: `crates/cc-plugins/src/refresh.rs` + `crates/cc-commands/src/reload_plugins_cmd.rs` 支持 `/reload-plugins` 热重载插件注册表和插件 skills。
- **tools**: `crates/cc-plugins/src/tools.rs` 的 `PluginToolWrapper` 已把带 `runtime.type = "stdio"` 的插件工具接入 `cc_tools::tool::Tool`；`crates/start-up/src/tool_registry.rs` 把 `cc_plugins::discover_plugin_tools` 注入真实工具注册表。
- **skills**: `cc_plugins::discover_plugin_skill_definitions()` 已在 `crates/claude-code-rs/src/main.rs`、`crates/claude-code-rs/src/command_runtime_bridge.rs`、`crates/cc-commands/src/skills_cmd.rs` 进入启动和 `/skills reload` 路径。
- **MCP servers**: `cc_plugins::discover_plugin_mcp_servers[_scoped]()` 已能把 manifest `mcpServers` 转成 `cc_mcp::McpServerConfig`；`crates/cc-mcp/src/discovery.rs` 也有 plugin hook 入口。当前非测试注册点在 `crates/claude-code-rs/src/app_runtime_adapters/mod.rs` 的 subsystem/headless event sink 安装路径，普通 `main.rs` B.3d 调用 `discover_mcp_servers()` 之前未看到等价前置注册，因此普通启动路径仍需补齐或调整顺序。
- **plugin state command**: `crates/cc-commands/src/plugin_cmd.rs` 已支持 `/plugin list|installed|disabled|errors|status|enable|disable|uninstall [--purge]`，但没有 `/plugin install` 或 marketplace 浏览。
- **事件系统 / IPC**: `crates/cc-plugins/src/mod.rs` 的 `PluginSubsystemEvent` 已经由 `crates/claude-code-rs/src/app_runtime_adapters/mod.rs` 适配到 `cc_ipc_protocol::subsystem_events::PluginEvent`；`crates/claude-code-rs/src/app_subsystem_handlers.rs` 可返回 `PluginList`、`Reloaded`、`StatusChanged`。
- **LSP 声明解析**: `crates/cc-lsp-service/src/mod.rs` 已有 `load_manifest_lsp_declaration()` 和 `LspConfigProvider`，可以解析插件 `lspServers` 字段；但当前未看到非测试 runtime 调用 `set_config_provider()` 将插件 LSP 声明真正注入 `configured_server_configs()`。
- **LSP 推荐外壳**: `crates/claude-code-rs/src/ui/lsp_recommendation/`、`crates/cc-commands/src/lsp_cmd.rs`、`crates/claude-code-rs/src/app_subsystem_handlers.rs` 已有推荐 UI、设置持久化和 IPC 回复处理；`yes` 仍明确提示安装路径未接线。

## Rust 缺失的主要功能

### 1. Marketplace 管理 (2,643 行) — 仅有数据模型/路径，核心缺失
- 多来源市场: URL、GitHub、npm、file
- 本地缓存管理
- 安装/更新/删除
- 自动更新
- 策略、块列表/允许列表
- GCS 启动检查
- 市场发现和浏览
- Rust 现状：`PluginSource`、`MarketplaceEntry`、`marketplaces_dir()`、`known_marketplaces_path()`、cache 扫描已经存在；但没有 `marketplace.rs`、下载器、索引刷新、浏览、安装或更新 runtime。

### 2. MCPB Handler (966 行) — 完全缺失
- MCP Bundle 二进制插件格式
- 下载、解压、清单提取
- 用户配置输入处理
- 内容哈希和缓存

### 3. 命令/Agent/Hook/输出样式加载 (~1,800 行) — skills 已接线，其余主要缺失
- `loadPluginCommands.ts`: 插件命令加载、Markdown frontmatter 解析
- `loadPluginAgents.ts`: Agent 定义加载
- `loadPluginHooks.ts`: Hook 配置加载
- `loadPluginOutputStyles.ts`: 输出样式加载
- Rust 现状：plugin skills 已从 manifest `skills` 字段加载到 `cc-skills`；plugin commands 只有 manifest DTO (`CommandContribution`)，没有命令注册/执行；plugin agents 只有 IPC/UI 类型能表达 `Plugin` 来源，没有 loader；plugin hooks/output styles 未从插件 manifest 或插件目录加载。

### 4. 插件验证 (903 行) — 基础 manifest 校验已实现，完整验证缺失
- 清单字段验证
- 目录结构检查
- 文件类型验证
- YAML frontmatter 验证
- Rust 现状：`validate_manifest()` 校验名称、版本、重复工具、stdio runtime command；`loader.rs` 会在加载时产生 manifest/metadata 诊断。缺少独立验证命令、目录结构/文件类型扫描、插件命令/agent/hook/output-style frontmatter 校验。

### 5. 安装管理 (1,268 行) — 仅有 metadata 读写和卸载，安装缺失
- `installed_plugins.json` V1→V2 迁移
- 安装追踪 (全局)
- 作用域管理 (user/project/local)
- Git 感知的检出路径检测
- Rust 现状：`loader.rs` 能读 V1/V2 并写 V2，`plugin_cmd.rs` 能 enable/disable/uninstall，`uninstall_plugin()` 可移除 metadata 并可选 purge cache；但没有 installer、作用域化安装、全局安装追踪、依赖下载或 Git checkout 检测。

### 6. MCPB 插件集成 (634 行) — MCP server manifest 集成已实现，MCPB 缺失
- MCP 服务器配置
- 集成到 MCP 管理器
- Rust 现状：普通 `plugin.json` 的 `mcpServers` 已有 manifest-to-`McpServerConfig` 转换和 `cc-mcp` plugin hook；但普通启动时的 hook 前置注册仍需补齐。`.mcpb` bundle 解析、用户配置输入、hash/cache 和从 bundle 生成 MCP server config 仍缺失。

### 7. LSP 插件集成和推荐 — 解析/UI 已有，runtime 注入和推荐生成缺失
- `lspPluginIntegration.ts`: LSP 服务器配置
- `lspRecommendation.ts`: 基于使用的推荐
- Rust 现状：`cc-lsp-service` 可解析插件 `lspServers`，推荐 UI/设置已存在；缺少从 `cc-plugins` 收集 enabled 插件 LSP 配置并调用 `cc_lsp_service::set_config_provider()` 的 root wiring，也缺少基于文件/使用情况发出真实 `RecommendationRequest` 和安装动作。

### 8. 插件生命周期功能 — refresh/drift 部分已实现，其余缺失
- `pluginAutoupdate.ts`: 自动更新调度和检查
- `pluginVersioning.ts`: 版本兼容性检查
- `zipCache.ts`: ZIP 下载缓存
- `reconciler.ts`: 状态同步
- Rust 现状：`refresh.rs`、`needs_refresh()`、`compute_drift()` 能做会话注册表刷新和磁盘/内存 drift 提示；没有自动更新、版本兼容策略、ZIP 缓存或完整 reconciler。

### 9. 策略和安全 — 工具权限已有，插件策略缺失
- `pluginBlocklist.ts`: 插件块列表
- `pluginFlagging.ts`: 插件标记
- `pluginPolicy.ts`: 策略管理
- `orphanedPluginFilter.ts`: 孤立插件过滤
- Rust 现状：`PluginToolWrapper` 会走 `ToolPermissionContext`，read-only 工具可免问，写工具默认询问/在 plan mode 拒绝；但缺少 marketplace/plugin 级 blocklist、allowlist、flagging、policy 和 orphaned 过滤。

### 10. 遥测和统计 — 缺失
- `installCounts.ts`: 安装计数
- `fetchTelemetry.ts`: 遥测获取
- `hintRecommendation.ts`: 推荐提示
- Rust 现状：插件事件已能进入 IPC subsystem event；未看到接入 `cc-observability` 的安装/使用统计或远端 telemetry。

## 主要缺失功能的 Rust 修改位置

| 缺失功能 | 需要修改或新增的 Rust 位置 |
|----------|----------------------------|
| Marketplace 索引/浏览/刷新 | `crates/cc-plugins/src/marketplace.rs`（新增）、`crates/cc-plugins/src/lib.rs`、`crates/cc-plugins/src/mod.rs`、`crates/cc-commands/src/plugin_cmd.rs` |
| 插件安装/更新/删除完整流程 | `crates/cc-plugins/src/installation.rs`（新增）、`crates/cc-plugins/src/loader.rs`、`crates/cc-plugins/src/mod.rs`、`crates/cc-commands/src/plugin_cmd.rs`、`crates/claude-code-rs/src/app_subsystem_handlers.rs` |
| 多来源下载：URL/GitHub/npm/file | `crates/cc-plugins/src/sources.rs`（新增）、`crates/cc-plugins/src/marketplace.rs`（新增）、`crates/cc-plugins/src/installation.rs`（新增） |
| ZIP/cache 管理 | `crates/cc-plugins/src/zip_cache.rs`（新增）、`crates/cc-plugins/src/installation.rs`（新增）、`crates/cc-plugins/src/mod.rs` |
| MCPB bundle 处理 | `crates/cc-plugins/src/mcpb.rs`（新增）、`crates/cc-plugins/src/installation.rs`（新增）、`crates/cc-mcp/src/discovery.rs`、`crates/claude-code-rs/src/app_runtime_adapters/mod.rs`、`crates/claude-code-rs/src/main.rs` |
| 插件命令加载/注册/执行 | `crates/cc-plugins/src/commands.rs`（新增）、`crates/cc-commands/src/plugin_commands.rs`（新增）、`crates/cc-commands/src/lib.rs`、`crates/claude-code-rs/src/command_runtime_bridge.rs` |
| 插件 Agent 定义加载 | `crates/cc-plugins/src/agents.rs`（新增）、`crates/cc-services/src/agent_definitions/mod.rs`、`crates/cc-commands/src/agents_cmd.rs`、`crates/claude-code-rs/src/app_subsystem_handlers.rs` |
| 插件 Hook 加载 | `crates/cc-plugins/src/hooks.rs`（新增）、`crates/cc-config/src/settings.rs`、`crates/cc-engine/src/hooks/registration.rs`、`crates/cc-commands/src/hooks_cmd.rs`、`crates/claude-code-rs/src/main.rs` |
| 插件输出样式加载 | `crates/cc-plugins/src/output_styles.rs`（新增）、`crates/cc-engine/src/output_style.rs`、`crates/cc-config/src/validation.rs`、`crates/cc-commands/src/config_cmd.rs` |
| 完整插件验证 | `crates/cc-plugins/src/validation.rs`（新增）、`crates/cc-plugins/src/manifest.rs`、`crates/cc-plugins/src/loader.rs`、`crates/cc-commands/src/plugin_cmd.rs` |
| 依赖解析和安装顺序 | `crates/cc-plugins/src/dependency_resolver.rs`（新增）、`crates/cc-plugins/src/installation.rs`（新增）、`crates/cc-plugins/src/manifest.rs` |
| 自动更新/版本兼容/reconcile | `crates/cc-plugins/src/autoupdate.rs`（新增）、`crates/cc-plugins/src/versioning.rs`（新增）、`crates/cc-plugins/src/reconciler.rs`（新增）、`crates/cc-plugins/src/refresh.rs` |
| 插件 LSP runtime 注入 | `crates/cc-plugins/src/lsp.rs`（新增）、`crates/cc-lsp-service/src/mod.rs`、`crates/claude-code-rs/src/app_runtime_adapters/mod.rs` 或 `crates/claude-code-rs/src/main.rs` |
| LSP 推荐生成和安装动作 | `crates/cc-lsp-service/src/recommendation.rs`（新增）、`crates/claude-code-rs/src/app_subsystem_handlers.rs`、`crates/claude-code-rs/src/ui/lsp_recommendation/lsp_recommendation_menu.rs`、`crates/cc-commands/src/lsp_cmd.rs`、`crates/cc-commands/src/plugin_cmd.rs` |
| 插件策略/块列表/允许列表/标记 | `crates/cc-plugins/src/policy.rs`（新增）、`crates/cc-plugins/src/blocklist.rs`（新增）、`crates/cc-plugins/src/flagging.rs`（新增）、`crates/cc-config/src/settings.rs`、`crates/cc-plugins/src/loader.rs` |
| 安装统计/遥测/推荐提示 | `crates/cc-plugins/src/telemetry.rs`（新增）、`crates/cc-observability/src/event.rs`、`crates/cc-observability/src/context.rs`、`crates/claude-code-rs/src/app_runtime_adapters/mod.rs` |
| 插件配置 schema 和用户输入 | `crates/cc-plugins/src/configuration.rs`（新增）、`crates/cc-config/src/settings.rs`、`crates/cc-commands/src/plugin_cmd.rs`、`crates/claude-code-rs/src/app_subsystem_handlers.rs` |

## 关键差异总结

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| Plugin manifest schema | Zod schemas (1,683 行) | PluginManifest | ✅ 核心字段已移植，完整验证不足 |
| 插件加载 | pluginLoader.ts (3,305 行) | loader.rs | ✅ 部分移植 |
| 热重载 | refresh.ts | refresh.rs + /reload-plugins | ✅ 已接 runtime |
| 工具包装器 | 直接执行 JS | PluginToolWrapper | ⚠️ 已接工具注册表，但仅 stdio 子进程 |
| Plugin skills | loadPluginAgents/skills 相关路径 | manifest skills + cc-skills | ✅ skills 已接启动和 reload |
| Plugin MCP servers | mcpPluginIntegration.ts | discover_plugin_mcp_servers + cc-mcp hook | ⚠️ 转换/hook 已有，普通启动前置注册待补齐 |
| **Marketplace** | **完整市场 (2,643 行)** | **只有模型/路径** | **核心缺口** |
| **MCPB Handler** | **完整处理 (966 行)** | **无** | **核心缺口** |
| **命令加载** | **loadPluginCommands (946 行)** | **只有 manifest DTO** | **功能缺口** |
| **Agent 加载** | **loadPluginAgents (348 行)** | **只有 Plugin 来源类型/UI 过滤** | **功能缺口** |
| **插件验证** | **validatePlugin (903 行)** | **validate_manifest 基础校验** | **功能缺口** |
| **安装管理** | **~1,200 行** | **metadata enable/disable/uninstall** | **功能缺口** |
| 自动更新 | pluginAutoupdate.ts | 无 | 功能缺口 |
| LSP 集成 | lspPluginIntegration.ts | parser/UI 已有，provider 未接 | 功能缺口 |
| 策略/块列表 | ~5 文件 | 仅工具权限 | 安全缺口 |

## 插件运行时差异

Bun 版本可以直接运行 JavaScript 插件，因为 Bun 原生支持 JS/TS。  
Rust 版本目前只支持通过 `PluginToolWrapper` 以 stdio 子进程执行插件工具，需要插件目录中已有可执行命令或系统已安装的二进制文件；还没有 marketplace 下载、bundle 解包或 JS/TS 原生运行时。
