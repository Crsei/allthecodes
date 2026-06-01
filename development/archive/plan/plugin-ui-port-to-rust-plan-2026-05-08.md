# Plugin UI 从 `claude-code-bun` 迁移到 `claude-code-rs` 的执行计划

日期: 2026-05-08

目标范围: 将 `claude-code-bun/src` 中 plugin 的管理体验迁移到
`claude-code-rs`，覆盖用户已知 plugin 来源的安装、更新、卸载、配置、校验、
通知和刷新事件；明确不迁移 marketplace/discover 市场目录能力。

UI 仍优先落在 `rust/crates/claude-code-rs/src/ui`，但 direct-source 安装、
更新、配置和校验需要补齐 `src/plugins` 与 `/plugin` 命令后端。

本文档是执行计划，不包含实现代码。

## 0. 需要你先做决定的方案

下面这些决策会影响实现范围、改动文件、风险和最终与 Bun 版 `/plugin` 界面的对齐程度。每项都给出推荐默认方案，后续如果没有特别指定，可以按推荐默认推进。

### D1. 范围边界: UI 迁移 + direct-source 生命周期，不含市场

推荐默认: 本计划采用 direct-source 生命周期方案。第一阶段先用 Rust 已有 plugin
后端能力做可用 TUI 界面；后续阶段补齐用户已知来源的 install/update/configure/
validate/notification/reload 流程，不实现 marketplace/discover 目录。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 只做 UI 首版 | 主要改 `rust/crates/claude-code-rs/src/ui`，动作通过现有 `/plugin` 命令提交。 | 最快、风险最低；但不能满足 install/update/configure/validate 目标。 |
| B. direct-source 生命周期 | 先做 UI 和已支持能力，再补 `/plugin install <source>`、update、configure、validate、事件通知。 | 本计划采用。用户自己寻找 plugin 来源，Rust 负责安装和生命周期管理。 |
| C. 完整 Bun 市场 parity | 同时迁移 UI、市场目录、marketplace 管理、安装、更新、卸载、配置、校验、通知和刷新事件。 | 当前不采用；范围大且会引入市场发现、远程目录和策略问题。 |

已定范围: 采用 B；C 作为后续独立计划，不阻塞本计划。

### D2. Rust UI 承载方式

推荐默认: 先接入现有 `CommandSurface` overlay。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 使用现有 `CommandSurface` 字符串渲染 | 新增 `ui/components/command_surface/surfaces/plugin.rs`，像 MCP/Skills surface 一样输出文本。 | 最贴合当前 Rust UI 架构和 snapshot 测试；复杂布局会有压缩。 |
| B. 新建更完整的 ratatui 插件组件 | 在 `ui/plugins/` 下做状态化 list/details widget，再由 `CommandSurface` 承载。 | 长期质量更好，代码和测试更多。 |
| C. 改造共享 overlay 行为 | 让 plugin UI 使用更大或全高 overlay。 | 长列表体验更好，但会影响所有 command surface。 |

需要决定: 首版是否允许调整共享 overlay 尺寸，还是必须保持当前居中 overlay。

### D3. 数据读取边界

推荐默认: 新增一个 UI adapter，先直接读取现有 `crate::plugins` API；adapter 保持隔离，后续可替换成 IPC/subsystem 查询。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. UI adapter 直接读 `crate::plugins` | TUI 从 `plugins::loader`、`plugins::get_all_plugins`、`plugins::needs_refresh` 等 API 组装展示模型。 | 简单、可测试；主要服务当前 TUI。 |
| B. 先补 IPC/subsystem query | 增加 plugin 查询命令/事件，让 UI 通过 `PluginEvent::PluginList` 等事件驱动。 | 分层更干净，但第一轮范围更大。 |
| C. 复用 `/plugin` 文本输出 | UI 解析或展示现有命令输出。 | 最快，但很难做稳定的交互详情和动作。 |

需要决定: plugin UI 首版是否可以直接读进程内 plugin 状态。

### D4. 安装、更新、启用、禁用、卸载等变更动作的 UX

推荐默认: 第一阶段已有动作通过 `CommandSurfaceOutcome::Submit` 提交现有 slash
command；Phase 3 之后新增 install/update/configure/validate 也走同一命令路径。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 提交 slash command | Enter 或快捷键提交 `/plugin install <source>`、`/plugin update <id>`、`/plugin enable <id>`、`/plugin disable <id>`、`/plugin uninstall <id>`、`/plugin configure <id>`、`/plugin validate <path>`、`/reload-plugins`。 | 复用命令路径和错误处理；动作后 overlay 通常关闭。 |
| B. surface 内直接修改 | surface 调用 plugin API，原地刷新列表。 | UX 更顺，但容易和命令处理重复。 |
| C. 混合模式 | 危险/持久化动作走命令；纯查看和本地刷新在 surface 内完成。 | 实用折中。 |

需要决定: 动作后是否关闭 overlay 并提交命令，还是停留在 plugin UI 内。

### D5. Discover 和 Marketplaces 的处理方式

推荐默认: 本计划不实现 Discover/Marketplaces，也不提供 `/marketplace` alias。
用户自己寻找 Serena MCP、Context7、GitHub MCP Server、Playwright MCP、
Filesystem MCP、Git MCP、Fetch MCP、Memory MCP、Supabase MCP、Docker MCP
等 plugin 来源，再通过 `/plugin install <source>` 安装。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 排除市场入口 | Rust UI 只显示 Installed、Errors，以及 direct-source install/config/validate 入口。 | 本计划采用；不误导用户，以最小后端面完成生命周期管理。 |
| B. 显示只读说明 | 可在帮助文本中说明“本版本不提供市场目录，请提供 plugin 来源”。 | 可接受，但不能作为 tab 或可交互市场。 |
| C. 补齐 marketplace 后端 | 实现 registry、discover list、marketplace add/remove/update。 | 当前不采用；应拆成后续独立计划。 |

已定范围: 采用 A；B 只允许出现在 help/empty state，不允许伪装成市场功能。

### D6. Plugin Options / Configuration

推荐默认: 本计划正式支持配置。第一轮 UI 可以先显示 schema；后续阶段必须补
option storage、schema validate 和可编辑配置 flow。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 暂缓 | details 中展示 configuration schema，但不提供编辑。 | 只能作为 Phase 1 临时状态，不能作为最终 DoD。 |
| B. 增加 options 存储和表单 | 移植 Bun 的 plugin options 持久化和配置对话框。 | 本计划采用；需要新增后端和校验工作。 |
| C. 暴露 raw JSON 编辑入口 | 给出未来 options JSON 路径或 edit target。 | 轻量，但不是 Bun 等价 UX。 |

已定范围: 采用 B；C 仅可作为 unsupported schema shape 的降级入口。

### D7. Installed 视图是否合并 Plugin 与 MCP

推荐默认: 当能归因到 plugin 时，显示 plugin-contributed MCP 子行；独立 MCP 继续交给 `/mcp`。

| 方案 | 含义 | 影响 |
| --- | --- | --- |
| A. 只显示 plugin 行 | Installed tab 只显示插件和组件计数。 | 最小实现。 |
| B. Plugin 行 + 子 MCP 行 | 尽量复刻 Bun: 插件行后显示其贡献的 MCP 子项。 | 更接近 Bun；需要小心映射 MCP runtime 状态。 |
| C. 完全合并 standalone MCP | 像 Bun 一样把独立 MCP 也并入 Installed 列表。 | 完整度最高，但与 `/mcp` UI 耦合更强。 |

需要决定: plugin UI 应该拥有多少 MCP 展示职责。

## 1. 当前代码事实

### 1.1 Bun 版 plugin UI 入口

Bun 版 plugin UI 是一个 JSX slash-command surface:

- 命令注册: `claude-code-bun/src/commands/plugin/index.tsx`
- JSX 命令入口: `claude-code-bun/src/commands/plugin/plugin.tsx`
- 主 tab UI: `claude-code-bun/src/commands/plugin/PluginSettings.tsx`
  - `PluginSettings` 从 line 721 开始。
  - line 726-739 管理 `viewState`、`activeTab`、输入、错误、结果、子搜索状态。
  - line 768-781 标记 plugin 状态变化，并要求用户运行 `/reload-plugins` 让 session 激活变更。
  - line 943-1006 渲染四个核心 tab: Discover、Installed、Marketplaces、Errors。
- Installed 列表和详情: `claude-code-bun/src/commands/plugin/ManagePlugins.tsx`
  - `ManagePlugins` 从 line 516 开始。
  - line 525-564 读取 MCP clients/tools、plugin errors、flagged plugins、搜索、分页和 view state。
  - line 2245-2348 渲染 plugin details: scope、version、description、author、status、components、errors、actions。
  - line 2692-2785 渲染搜索框、scope header、分页提示、统一 row cell。
- 列表单元格: `claude-code-bun/src/commands/plugin/UnifiedInstalledCell.tsx`
  - line 12 开始处理 plugin、flagged-plugin、failed-plugin、MCP row。
- plugin 启动和刷新 UX: `claude-code-bun/src/hooks/useManagePlugins.ts`
  - line 38 开始 initial plugin load。
  - line 288-305 当 plugin 变更但未 reload 时提示 `/reload-plugins`。
- 相关提示和通知:
  - `claude-code-bun/src/hooks/notifs/usePluginInstallationStatus.tsx`
  - `claude-code-bun/src/hooks/notifs/usePluginAutoupdateNotification.tsx`
  - `claude-code-bun/src/components/ClaudeCodeHint/PluginHintMenu.tsx`
  - `claude-code-bun/src/components/LspRecommendation/LspRecommendationMenu.tsx`

### 1.2 Rust 当前能力

Rust 已经有 plugin 后端和 `/plugin` 文本命令，但没有 plugin interactive command surface。

- Command surface 注册: `rust/crates/claude-code-rs/src/ui/components/command_surface/mod.rs`
  - line 40-54 的 `CommandSurface` enum 当前没有 Plugin variant。
  - line 57-75 的 `for_slash_command` 当前支持 agents/config/diff/hooks/login/mcp/memory/sandbox/skills/tasks/team，不支持 plugin。
  - line 144 之后已有 `render_tabs`、`cycle_index`，可复用给 plugin tab。
- Command surface overlay: `rust/crates/claude-code-rs/src/ui/app/render.rs`
  - line 522-557 的 `render_command_surface_overlay` 用居中边框 overlay 渲染所有 command surface。
- `/plugin` 文本命令: `rust/crates/claude-code-rs/src/commands/plugin_cmd.rs`
  - line 1-22 明确三层模型: installed on disk、enabled/disabled、active in session。
  - line 33-44 支持 list/installed/disabled/errors/status/enable/disable/uninstall。
  - line 127-170 组装 layered row: plugin、version、installed、enabled、active、errors、skills、tools、mcp。
  - line 322-388 enable/disable 会写入状态，并在需要时 emit refresh-needed。
- plugin 后端模型: `rust/crates/claude-code-rs/src/plugins/mod.rs`
  - line 33-90 定义 `PluginSource`、`PluginStatus`、`PluginEntry`。
  - line 374-419 实现 drift 检测和 `needs_refresh`。
  - line 501-640 初始化 plugin，并发现 tools、MCP servers、skills。
- manifest schema: `rust/crates/claude-code-rs/src/plugins/manifest.rs`
  - 已包含 tools、skills、MCP servers、LSP servers、commands、dependencies、configuration。
- plugin 事件: `rust/crates/claude-code-rs/src/ipc/subsystem_events.rs`
  - 已有 `PluginEvent::StatusChanged`、`PluginList`、`RefreshNeeded`、`Reloaded`。
- 可参考的现有 UI surface:
  - `ui/components/command_surface/surfaces/mcp.rs`
  - `ui/components/command_surface/surfaces/skills.rs`
  - `ui/components/command_surface/surfaces/agents.rs`
  - 测试入口: `ui/components/command_surface/tests.rs`

## 2. 目标行为

最终希望 Rust plugin UI 覆盖 Bun 版中不依赖市场目录的用户视角:

1. 用户自行找到 plugin 来源，然后用 `/plugin install <source>` 安装。
2. 管理已安装 plugins。
3. 查看 plugin errors。
4. 查看 plugin details: metadata、scope、status、components、errors。
5. 支持 install、update、enable、disable、uninstall、configure、validate、reload 等动作。
6. plugin 状态变化后显示 `/reload-plugins` 提示，不静默自动刷新。
7. 显示 plugin 贡献的 tools、skills、MCP servers、LSP servers、commands、hooks/agents。
8. 显示安装失败、更新失败、配置失败、校验失败、加载失败等 diagnostics。
9. 保持键盘优先的 TUI 操作方式。
10. 不提供 Discover、Marketplaces、市场排序、安装量、市场管理或远程目录浏览。

非目标:

- 不实现 marketplace registry、known marketplaces、marketplace add/remove/update。
- 不把 Serena MCP、Context7、GitHub MCP Server、Playwright MCP、Filesystem MCP、
  Git MCP、Fetch MCP、Memory MCP、Supabase MCP、Docker MCP 等示例硬编码成
  UI 可安装列表。
- 上述示例如果只是 MCP server，应先通过 plugin manifest 包装后走 `/plugin`；
  否则继续属于 `/mcp` 的安装/配置范围。

第一轮最小可交付目标:

- `/plugin`、`/plugins` 无参数时打开 Rust TUI plugin surface。
- `Installed` 和 `Errors` 基于现有 Rust plugin state 可用。
- 不显示 `Discover` 和 `Marketplaces` tab。
- details view 显示 metadata、安装/启用/激活状态、组件、错误、reload 提示。
- enable/disable/uninstall/reload/status 动作优先通过现有 slash command 执行。
- install/update/configure/validate 在后续阶段补齐真实命令和后端前，不在 UI 中伪装可用。

## 3. 架构计划

### 3.1 新增 `ui/plugins` 展示模块

新增文件:

- `rust/crates/claude-code-rs/src/ui/plugins/mod.rs`
- `rust/crates/claude-code-rs/src/ui/plugins/types.rs`
- `rust/crates/claude-code-rs/src/ui/plugins/plugin_cell.rs`
- `rust/crates/claude-code-rs/src/ui/plugins/plugin_details.rs`
- `rust/crates/claude-code-rs/src/ui/plugins/plugin_list.rs`
- 后续可选:
  - `plugin_install.rs`
  - `plugin_options.rs`
  - `plugin_errors.rs`
  - `plugin_validate.rs`

目的:

- 避免把展示逻辑塞进 `CommandSurface`。
- 延续 `ui/skills`、`ui/agents` 的模块化方式。
- 让 snapshot 测试更小、更稳定。

建议的 UI view-model:

```rust
pub enum PluginTab {
    Installed,
    Install,
    Errors,
}

pub enum PluginView {
    List,
    Details { plugin_id: String },
    Errors,
    InstallSourceInput,
    Configure { plugin_id: String },
    ValidateInput,
    ConfirmUninstall { plugin_id: String, purge: bool },
}

pub enum PluginRowKind {
    Plugin,
    FailedPlugin,
    FlaggedPlugin,
    ChildMcp,
}

pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub source_id: Option<String>,
    pub source_label: String,
    pub status_label: String,
    pub installed: bool,
    pub enabled: bool,
    pub active: bool,
    pub error: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub lsp_servers: Vec<String>,
    pub commands: Vec<String>,
    pub kind: PluginRowKind,
}
```

这些类型先作为 TUI view-model，不作为稳定公共 API。

### 3.2 新增 plugin command-surface adapter

新增:

- `rust/crates/claude-code-rs/src/ui/components/command_surface/adapters/plugin.rs`

职责:

- 从当前 plugin state 构建 `PluginSurfaceSnapshot`。
- 合并:
  - `plugins::loader::load_installed_plugins_report()` 的磁盘状态。
  - `plugins::get_all_plugins()` 的内存 active 状态。
  - `plugins::needs_refresh()` 的 drift 状态。
- 复用 `/plugin list` 当前三层语义:
  - installed: 是否在 `installed_plugins.json`。
  - enabled: `PluginStatus::Installed` / `Disabled` / `Error`。
  - active: 是否已加载到当前 session registry。
- 将 `PluginDiagnostic` 转成 failed/error row。
- 根据 `PluginEntry` 和 manifest-backed discovery 补充 components。
- 如果 D7 选择展示 MCP 子项，使用 `plugins::discover_plugin_mcp_servers_scoped()` 归因 plugin MCP。

adapter 输出要稳定排序，避免 snapshot 测试抖动。

### 3.3 新增 `PluginSurface`

新增:

- `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/plugin.rs`

状态:

- `tab_index`
- `view`
- `selected_index`
- `details_action_index`
- `filter`
- snapshot 或 rows

键盘约定:

| 按键 | 行为 |
| --- | --- |
| Left / `[` | 上一个 tab |
| Right / `]` | 下一个 tab |
| Up / `k` | 上一行 |
| Down / `j` / Tab | 下一行 |
| 普通字符 | 输入过滤条件 |
| Backspace | 删除过滤字符 |
| Enter | list 中打开详情；details 中执行当前 action |
| `e` | enable 当前插件 |
| `d` | disable 当前插件 |
| `u` | uninstall 当前插件 |
| `r` | `/reload-plugins` |
| `s` | `/plugin status` |
| Esc | details 返回 list；list 关闭 overlay |

details actions:

- Enable / Disable
- Uninstall
- Uninstall and purge cache
- Reload plugins
- Open `/plugin status`
- Configure options
- Update from recorded source
- Validate local plugin manifest/path
- 后续: Open homepage/source URL

第一阶段不要在 surface 内直接执行删除文件等危险动作。需要 purge 时提交
`/plugin uninstall <id> --purge`，由现有 command handler 负责。
install、update、configure 和 validate 在对应后端存在前只允许作为 disabled action
或帮助文本出现。

### 3.4 接入 `CommandSurface`

修改:

- `rust/crates/claude-code-rs/src/ui/components/command_surface/mod.rs`
- `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/mod.rs`
- `rust/crates/claude-code-rs/src/ui/components/command_surface/tests.rs`

改动:

- export `PluginSurface`。
- 增加 `CommandSurface::Plugin(PluginSurface)`。
- 在 `for_slash_command` 中对 `plugin` 打开 surface。
- 如果为 `/plugins` 增加 command alias，现有 parser 应映射到 `cmd.name == "plugin"`，surface 仍从 plugin 分支打开。
- 不增加 `/marketplace` alias。
- `title()` 返回 `"Plugins"`。
- `render()` 和 `handle_key()` 分发到 `PluginSurface`。
- 测试 `/plugin` 无参数打开 surface，`/plugin status` 保持走原命令路径。

### 3.5 补齐命令 alias 和 command palette 帮助

修改:

- `rust/crates/claude-code-rs/src/commands/mod.rs`
- `rust/crates/claude-code-rs/src/ui/components/command_palette/metadata.rs`
- 必要时补 `rust/crates/claude-code-rs/src/ui/input/slash_command.rs` 测试。

建议:

- 给 plugin command 增加 aliases:
  - `plugins`
- 扩展 `/plugin` metadata:
  - `/plugin`
  - `/plugins`
  - `/plugin list`
  - `/plugin installed`
  - `/plugin disabled`
  - `/plugin errors`
  - `/plugin status`
  - `/plugin install <source>`
  - `/plugin update <plugin-id>`
  - `/plugin enable <plugin-id>`
  - `/plugin disable <plugin-id>`
  - `/plugin uninstall <plugin-id> [--purge]`
  - `/plugin configure <plugin-id>`
  - `/plugin validate <path>`

保留 edit targets:

- `~/.cc-rust/plugins/installed_plugins.json`
- `~/.cc-rust/plugins/cache`
- plugin options path: `~/.cc-rust/plugins/options/{plugin-id}.json` 或实现阶段确定的等价隔离路径。

### 3.6 刷新与通知

当前 Rust plugin command 已经能 emit `PluginEvent::RefreshNeeded` 和 `PluginEvent::Reloaded`，但 Rust TUI 的 `ui/tui/subsystem_events.rs` 目前主要处理 LSP recommendation 和 LSP command error。

计划:

1. Phase 1: plugin surface 打开时读取 `plugins::needs_refresh()`，在 footer/details 中显示 drift。
2. Phase 2: `ui/tui/subsystem_events.rs` 处理 plugin events:
   - `RefreshNeeded`: 添加低优先级 system info，例如 `Plugins changed. Run /reload-plugins to activate.`
   - `Reloaded { count, had_error }`: 添加成功或 warning system info。
   - `StatusChanged`: 可只 mark dirty，避免过多消息。
3. Phase 3 之后 install/update/configure/uninstall/validate 都要把失败原因写入 diagnostics，
   成功的持久化变更触发 refresh-needed。
4. 不自动刷新。保持 Bun 的模型: plugin 变更后由用户运行 `/reload-plugins` 激活。

## 4. 分阶段执行计划

### Phase 0. 基线和 parity checklist

目标: 在改 UI 前锁定当前 Rust `/plugin` 命令行为。

步骤:

1. 补或确认 `/plugin` command 测试:
   - list 显示 installed/enabled/active。
   - installed/disabled/errors filter 正常。
   - status 显示 drift。
   - enable/disable 在需要时 emit refresh-needed。
   - uninstall 删除 persisted metadata。
2. 建立 Bun -> Rust plugin UI checklist:
   - Installed
   - Errors
   - Details
   - Direct-source install/update
   - Configure
   - Validate
   - Notifications
3. 确认第一阶段不新增依赖。

验证:

```powershell
cargo test -p claude-code-rs plugin_cmd
```

### Phase 1. Installed 和 Errors 可用界面

目标: `/plugin` 打开一个真实可用的 Rust TUI plugin surface。

步骤:

1. 新增 `ui/plugins` rendering helpers 和 row/view-model types。
2. 新增 `adapters/plugin.rs` 组装 installed/error rows。
3. 新增 `surfaces/plugin.rs`:
   - tabs
   - list view
   - filtering
   - selection
   - details view
   - status/drift footer
4. 增加 `CommandSurface::Plugin`。
5. `/plugin` 无参数打开 surface。
6. 增加 `/plugins` alias；不增加 `/marketplace` alias。
7. enable/disable/uninstall/reload/status 通过 `CommandSurfaceOutcome::Submit` 路由到现有 slash command。

验收标准:

- `/plugin` 无参数打开标题为 `Plugins` 的 overlay。
- `/plugin status` 仍执行现有文本命令。
- Installed tab 显示 plugin name、version、source、status、active state、component counts。
- Errors tab 显示 metadata/load diagnostics 和 plugin error rows。
- Details view 显示 description、source、cache path、installed/enabled/active、tools、skills、MCP servers、LSP servers、error detail。
- Install/configure/validate action 在后端落地前必须 disabled 或只显示帮助，不提交不存在的命令。
- overlay 激活时键盘导航不影响 prompt input。

验证:

```powershell
cargo test -p claude-code-rs command_surface
cargo test -p claude-code-rs plugin
```

同时新增 snapshot:

- no plugins empty state。
- enabled plugin。
- disabled plugin。
- plugin with errors。
- details view。
- drift footer。

### Phase 2. Plugin refresh event UX

目标: 对齐 Bun 的“插件状态已变更，请运行 `/reload-plugins`”心智模型。

步骤:

1. 扩展 `ui/tui/subsystem_events.rs` 处理 plugin events。
2. `RefreshNeeded` 显示 system info。
3. `Reloaded` 显示 reload 结果。
4. `PluginSurface` 每次打开都展示当前 drift。
5. 增加 subsystem event tests。

验收标准:

- enable/disable 后，如果 session 与磁盘状态不一致，用户能看到 reload 提示。
- `/reload-plugins` 完成后，用户能看到结果。
- UI 不静默自动刷新 plugin contributions。

验证:

```powershell
cargo test -p claude-code-rs tui::tests
```

### Phase 3. Direct-source install/update 后端补齐

目标: 支持用户自己找到 plugin 来源后安装和更新，不提供市场发现或 marketplace 管理。

注意: 这一阶段不再是纯 `src/ui` 改动，需要补 `src/plugins`、`commands/plugin_cmd.rs`
和 command palette metadata。

后端任务:

1. 定义 direct-source parser，映射到现有 `PluginSource`:
   - local path: `C:\path\to\plugin`、`./plugin-dir`
   - git URL: `https://...git`
   - GitHub repo: `owner/repo` 或 `github:owner/repo`
   - npm package: `npm:<package>[@version]`
2. 增加安装 API:
   - 读取并校验 `.claude-plugin/plugin.json` 或等价 manifest。
   - materialize 到 `~/.cc-rust/plugins/cache/{source-id}/{plugin-id}/`。
   - 写入 `~/.cc-rust/plugins/installed_plugins.json`。
   - 记录 source、cache path、installed_at、updated_at。
3. 增加更新 API:
   - 从已记录 source 更新 cache。
   - 保留启用/禁用状态和 options。
   - 失败时保留上一版可用 cache，不破坏已安装状态。
4. 扩展 slash commands:
   - `/plugin install <source>`
   - `/plugin update <plugin-id>`
   - `/plugin update --all`
5. 安装、更新、卸载后 emit `PluginEvent::StatusChanged` 和 `RefreshNeeded`。
6. 记录 installation/update diagnostics，供 `Errors` tab 展示。

UI 任务:

1. Details action 增加 Update。
2. 增加 direct-source install input 或帮助入口。
3. install/update/remove 动作第一版仍通过 slash command 提交。
4. failed install/update 进入 Errors tab。
5. Empty state 显示 direct-source 安装提示，而不是 Discover/Marketplace 提示。

验收标准:

- `/plugin install <local-path>` 能安装本地 fixture plugin。
- `/plugin install <git-or-github-source>` 至少有解析和错误路径测试；网络路径不要求 e2e。
- `/plugin update <plugin-id>` 能从本地 fixture source 更新 cache。
- install/update 后能看到 reload-needed 提示。
- 失败原因能在 Errors 中看到并有下一步提示。
- 不创建 `known_marketplaces.json`，不增加 marketplace list/add/remove/update 命令。

验证:

```powershell
cargo test -p claude-code-rs plugin_install
cargo test -p claude-code-rs plugin_update
cargo test -p claude-code-rs plugin_cmd
cargo test -p claude-code-rs command_surface
```

### Phase 4. Plugin configuration 和 Validate

目标: 对齐 Bun 中不依赖 marketplace 的 plugin options/configuration/validate flow。

后端任务:

1. 定义 plugin option storage path 和优先级，例如:
   - `~/.cc-rust/plugins/options/{plugin-id}.json`
   - 项目级 override 如果存在，必须使用 `.cc-rust` 隔离路径。
2. 校验 manifest `configuration` schema。
3. 增加读写 option values 的 API。
4. 增加 slash commands:
   - `/plugin configure <plugin-id>`
   - `/plugin validate <path>`
5. `validate` 支持 plugin manifest 文件和 plugin 目录；不校验 marketplace manifest。

UI 任务:

1. details action 增加 Configure options。
2. 增加最小 schema form renderer:
   - string
   - number
   - boolean
   - enum
3. 增加 validate input/result surface。
4. 暂不支持的 schema shape 只读展示。

验收标准:

- Details 能显示 configuration schema。
- 支持字段能编辑并持久化。
- invalid local plugin manifest 或 plugin 目录能输出清晰错误。
- `/plugin validate` 不接受 marketplace 作为成功路径。

验证:

```powershell
cargo test -p claude-code-rs plugin_options
cargo test -p claude-code-rs validate_plugin
```

### Phase 5. Diagnostics、通知和示例 recipe 对齐

目标: 将 install/update/configure/validate 的结果变成用户可见 diagnostics 和通知，
并提供 direct-source 示例 recipe，不迁移市场推荐。

任务:

1. 扩展 `ui/tui/subsystem_events.rs` 或相关 diagnostics 通道，展示:
   - install success/failure
   - update success/failure
   - configure save failure
   - validate failure
   - reload-needed
2. Errors tab 汇总 load/install/update/configure/validate diagnostics。
3. 文档或 help 中加入 direct-source recipe:
   - Serena MCP、Context7、GitHub MCP Server、Playwright MCP、Filesystem MCP、
     Git MCP、Fetch MCP、Memory MCP、Supabase MCP、Docker MCP。
4. 明确 recipe 前提: 这些目标必须有 plugin manifest；否则应走 `/mcp` 配置。
5. LSP recommendation 继续保持独立 surface，除非抽象共享组件能明显减少重复。
6. 不迁移 Bun 的 marketplace install hint、install counts、autoupdate marketplace notification。

验收标准:

- 现有 LSP recommendation 不回退。
- install/update/configure/validate 失败通知能指向 `/plugin` Errors 或 details。
- help/recipe 不把示例 MCP server 伪装成内置市场条目。
- 不出现 marketplace install hint 或 marketplace autoupdate notification。

验证:

```powershell
cargo test -p claude-code-rs lsp_recommendation
cargo test -p claude-code-rs plugin
```

### Phase 6. 长列表和窄终端体验打磨

目标: 让 plugin UI 在真实插件数量下可用。

任务:

1. 如果当前 overlay 太小，评估扩大 `render_command_surface_overlay` 的高度/宽度策略。
2. 增加分页提示。
3. 所有 row 支持宽度感知截断。
4. 长 component list 优先显示 count summary。
5. details sections 固定顺序:
   - Summary
   - State
   - Source/cache
   - Components
   - Errors
   - Actions
6. 80-column 终端不能出现不可读重叠。

验收标准:

- 80 列终端可读。
- 长插件名和长错误信息可预测地截断或换行。
- 当前选中 row/action 清晰可见。

验证:

```powershell
cargo test -p claude-code-rs command_surface
```

如果 renderer 支持宽度参数，补 80/100/120 columns snapshot。

## 5. 文件级改动清单

### Phase 1 预计文件

| 文件 | 改动 |
| --- | --- |
| `rust/crates/claude-code-rs/src/ui/mod.rs` | export 新的 `ui/plugins` 模块。 |
| `rust/crates/claude-code-rs/src/ui/plugins/mod.rs` | 新增 plugin UI 模块入口。 |
| `rust/crates/claude-code-rs/src/ui/plugins/types.rs` | 新增 view-model 类型。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_cell.rs` | 新增 row renderer。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_details.rs` | 新增 details renderer。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_list.rs` | 新增 list/filter renderer。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/adapters/mod.rs` | 增加 plugin adapter module。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/adapters/plugin.rs` | 新增 plugin data adapter。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/mod.rs` | 增加 plugin surface module。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/plugin.rs` | 新增 interactive surface。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/mod.rs` | 增加 `PluginSurface` variant 和路由。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/tests.rs` | 增加行为和 snapshot tests。 |
| `rust/crates/claude-code-rs/src/commands/mod.rs` | 根据 D5 增加 `/plugins` alias；不增加 `/marketplace`。 |
| `rust/crates/claude-code-rs/src/ui/components/command_palette/metadata.rs` | 扩展 `/plugin` usage/help/edit targets。 |

### Phase 2 预计文件

| 文件 | 改动 |
| --- | --- |
| `rust/crates/claude-code-rs/src/ui/tui/subsystem_events.rs` | 处理 plugin refresh/reloaded events。 |
| `rust/crates/claude-code-rs/src/ui/tui/tests.rs` | 增加 plugin event tests。 |

### Phase 3-5 direct-source 生命周期预计文件

| 文件 | 改动 |
| --- | --- |
| `rust/crates/claude-code-rs/src/plugins/mod.rs` | 增加 direct-source install/update/options API 或导出。 |
| `rust/crates/claude-code-rs/src/plugins/loader.rs` | 补 direct-source install metadata 和 diagnostics。 |
| `rust/crates/claude-code-rs/src/plugins/manifest.rs` | 完善 plugin manifest/options 字段校验；不新增 marketplace 校验目标。 |
| `rust/crates/claude-code-rs/src/commands/plugin_cmd.rs` | 增加 install/update/validate/configure 命令；不增加 marketplace 子命令。 |
| `rust/crates/claude-code-rs/src/ipc/subsystem_events.rs` | 需要时补 install/update/configure/validate diagnostics 事件。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_install.rs` | 可选: direct-source install input/rendering。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_options.rs` | 可选: configuration form rendering。 |
| `rust/crates/claude-code-rs/src/ui/plugins/plugin_validate.rs` | 可选: validate input/result rendering。 |

## 6. 兼容性规则

1. Phase 1 不新增依赖。
2. 保留现有 `/plugin <args>` 行为。
3. `/plugin` 无参数打开交互 UI；`/plugin ...` 非空参数继续执行命令。
4. 保持 `.cc-rust` 路径隔离，不读取或写入 Bun/Claude 全局 plugin state。
5. 优先复用现有 command 和 plugin API。
6. plugin mutation 后不自动刷新 contributions，只提示 `/reload-plugins`。
7. install/update/options/validate 后端不存在时，不在 UI 中假装支持。
8. 不新增 marketplace/discover/known_marketplaces 入口、存储或命令。
9. 危险动作必须经过明确命令或确认状态。
10. 示例 MCP server 只有在具备 plugin manifest 时才作为 plugin 安装示例；否则继续交给 `/mcp`。

## 7. 测试计划

Phase 1 最小测试:

- `PluginSurface` 能由 `/plugin` 无参数打开。
- `/plugin status` 不打开 surface，仍执行 handler。
- plugin list 能渲染 installed、disabled、active、inactive、error rows。
- filter 能按 id/name/description/component 缩小结果。
- Enter 能打开 selected row details。
- details action 路由正确:
  - disabled plugin -> `/plugin enable <id>`
  - enabled plugin -> `/plugin disable <id>`
  - uninstall -> `/plugin uninstall <id>`
  - purge -> `/plugin uninstall <id> --purge`
  - reload -> `/reload-plugins`
- `plugins::needs_refresh()` 返回 reason 时显示 drift footer。
- 无插件时 empty state 清晰。

Phase 3-5 最小测试:

- `/plugin install <local-fixture>` 写入 installed metadata 和 cache。
- `/plugin update <id>` 使用已记录 source 更新本地 fixture。
- install/update failure 不破坏上一版 installed metadata。
- `/plugin configure <id>` 能读写支持的 schema 字段。
- `/plugin validate <path>` 能校验 plugin manifest 文件和目录。
- marketplace path/manifest 不被当作成功 validate 目标。
- install/update/configure/validate diagnostics 出现在 Errors 或 system info 中。
- `/marketplace` 不作为 alias 或 command palette entry 出现。

建议命令:

```powershell
cargo test -p claude-code-rs plugin_cmd
cargo test -p claude-code-rs command_surface
cargo test -p claude-code-rs tui::tests
cargo test -p claude-code-rs plugin_install
cargo test -p claude-code-rs plugin_update
cargo test -p claude-code-rs plugin_options
cargo test -p claude-code-rs validate_plugin
```

实现完成后跑更广检查:

```powershell
cargo test -p claude-code-rs
cargo build -p claude-code-rs
```

## 8. 风险和缓解

| 风险 | 缓解 |
| --- | --- |
| Bun UI 暗示 Rust 尚未具备的后端能力。 | 按 D5 排除市场入口，不能伪装 marketplace/discover 已可用。 |
| mutation 行为和 slash command 重复。 | Phase 1 动作统一提交 slash command。 |
| command surface overlay 对 plugin 管理太小。 | 首版先复用；如果 snapshot 显示不可读，再单独改 overlay 尺寸策略。 |
| 全局 plugin registry 让测试相互影响。 | 触碰 registry/disk 的测试使用 serial 或隔离 fixture。 |
| direct-source git/npm install 涉及网络，容易 flaky。 | 自动测试使用本地 fixture；git/npm 网络路径只测解析和错误处理。 |
| 用户把 MCP server 当作 plugin 安装。 | help/recipe 明确要求 plugin manifest；无 manifest 时提示使用 `/mcp`。 |
| update 失败破坏已安装版本。 | 更新先写临时 cache，校验成功后再切换 metadata。 |
| 现有部分文档/注释存在编码异常。 | 新文件保持 UTF-8；不复制乱码文本。 |
| MCP 子项状态可能和 live MCP connection 不一致。 | Phase 1 先标成 contributed/configured；live status 后续再接。 |

## 9. 推荐执行顺序

1. 先确定 D1-D7。
2. 按 Phase 1 实现 `CommandSurface` 版 `/plugin` UI。
3. 跑 command surface snapshots 和 plugin command tests。
4. 实现 Phase 2 plugin event notices。
5. 实现 Phase 3 direct-source install/update。
6. 实现 Phase 4 configuration/validate。
7. 实现 Phase 5 diagnostics、通知和 direct-source recipe。
8. 如需 marketplace/discover，另开独立计划，不混入本计划。

## 10. 第一版 Definition of Done

第一版可交付完成标准:

- `/plugin` 打开 Rust TUI interactive plugin surface。
- Installed 和 Errors view 使用真实 Rust plugin state。
- Details view 能解释一个 plugin 的状态和 components。
- enable/disable/uninstall/reload 动作通过现有命令可用。
- refresh-needed 状态可见。
- 现有 `/plugin <args>` 和 command palette help 不回退。
- 新增 surface 和现有 plugin command path 都有测试覆盖。
- `/plugins` alias 可用；`/marketplace` 不作为本计划交付项。

完整 direct-source 生命周期完成标准:

- `/plugin install <source>` 支持本地 fixture plugin，并有 git/github/npm source 解析测试。
- `/plugin update <plugin-id>` 使用记录的 source 更新，并在失败时保留上一版。
- `/plugin configure <plugin-id>` 能持久化支持的 configuration schema 字段。
- `/plugin validate <path>` 能校验 plugin manifest 文件或目录。
- install/update/configure/validate/uninstall 后的通知和 refresh-needed 行为可见。
- Errors view 能展示安装、更新、配置、校验和加载失败 diagnostics。
- 不实现 Discover、Marketplaces、marketplace registry、marketplace list/add/remove/update。
