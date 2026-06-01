# Command settings UI 覆盖审计

日期: 2026-05-08

范围: 根据 [`docs/COMMAND_UI_REFERENCE.md`](../COMMAND_UI_REFERENCE.md) 审计 Rust
TUI 当前 slash command 的实现情况，记录哪些命令已有专门设置/配置界面，哪些只是
选择器、文本输出、填充 prompt、打开外部编辑器，或只有未接线组件。

本文档只记录计划和缺口，不包含实现代码。

## 0. “专门 UI”的判断标准

本审计中，一个命令只有同时满足下面条件，才算有专门设置/配置 UI：

1. 无参数 slash command 能从 `ui/components/command_surface` 打开
   `CommandSurface` 或等价 TUI overlay。
2. surface 自己持有交互状态，例如 tab、选择、过滤、详情、表单字段、确认动作。
3. Enter 或快捷键会执行明确动作，或提交精确 slash command。
4. 有 snapshot 或 command-surface 测试证明关键可见状态。

纯文本输出、打开 `$EDITOR`、或只把半截命令填进 prompt 都有价值，但不算完整的
专门设置 UI。

## 1. 当前实现事实

当前 `CommandSurface::for_slash_command` 中可从无参数命令打开的 surface 是：

- `/agents`
- `/config` 和 `/settings`
- `/diff`
- `/hooks`
- `/login`
- `/mcp`
- `/memory`
- `/sandbox`
- `/skills`
- `/tasks`
- `/team` 和 `/teams`

`/agent` 单数目前没有注册；当前实现命令是 `/agents`。

核查证据：

- `crates/claude-code-rs/src/ui/components/command_surface/mod.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/tests.rs`
- `target/ui-snapshots/components__03__config_surface_model_theme_effort.txt`
- `target/ui-snapshots/agents__01__agent_generation_and_wizard.txt`
- `target/ui-snapshots/agents__02__agents_core_surfaces.txt`
- `target/ui-snapshots/mcp__01__mcp_surfaces.txt`
- `target/ui-snapshots/permissions__01__permission_component_helpers.txt`

## 2. 已有较完整设置/配置 UI 的命令

### `/config` / `/settings`

状态: 已有较完整专门 UI。

`/config` 会打开 `ConfigSurface`，包含 Status、Model、Theme、Effort、Config、
Editor tabs。Model、Theme、Effort 是 picker；选择后提交 `/config set ...`。
这是当前 Rust TUI 中最接近完整设置界面的实现，应作为后续设置类 surface 的基准。

### `/sandbox`

状态: 首版足够。

`/sandbox` 会打开 tabbed `SandboxSurface`，可提交聚焦的 sandbox/network 命令。
它比 `/config` 小，但有自己的交互状态，并且能从无参数命令直接进入。

### `/login`

状态: 作为命令入口 surface 足够，作为完整认证向导仍是部分实现。

`/login` 会打开 `LoginSurface` 并选择认证方式。OAuth、浏览器继续流程仍依赖文本/
后端行为，因此它是专门入口界面，不是完整内嵌认证 wizard。

## 3. 有 surface，但还不是完整设置/配置 UI

### `/agents`

状态: 目前是部分选择器，不是完整 agent 详情/创建 UI。

当前行为：

- `/agents` 打开 `AgentsSurface`。
- surface 显示 source tabs 和 agent list。
- Enter 提交 `/agents show <agent>`。
- `AgentsSurface::new` 明确关闭创建入口：
  `state.show_create_new = false`。

已有但未通过 `/agents` 接线：

- `ui/agents/agent_detail.rs` 已能渲染完整 agent detail。
- `ui/agents/new_agent_creation/**` 已有 create-agent wizard。
- `target/ui-snapshots/agents__*.txt` 已证明详情与多步向导组件存在。

缺口：

用户期望的“使用 `/agent` 或 `/agents` 查看 agents 详细情况，并且有完整交互逻辑”
目前还没有完整落在 slash command surface 中。当前 `/agents` 更像选择器：选中后
退出 overlay，交给 `/agents show <name>` 文本输出。

建议计划：

1. 保留 `/agents show <name>` 文本命令，供脚本和直接输入使用。
2. 将 `AgentsSurface` 扩展为 list/detail 模式：
   - list mode 继续复用当前 source tabs 和 grouped list；
   - detail mode 复用 `render_agent_detail`；
   - Enter 在 surface 内进入 detail，而不是立即提交文本命令；
   - 如需保留原行为，可用 `s` 等显式快捷键提交 `/agents show <name>`。
3. 只有当创建 wizard 可达时才重新启用 create-new row：
   - `n` 或 create-new row 进入 `CreateAgentWizard`；
   - 保存路径走现有 agent settings 持久化；
   - cancel 回到列表，不写文件。
4. 决定是否新增 `/agent` alias；如果新增，应和 `/agents` 共用 handler 与 surface。
5. 增加 command-surface 测试与 snapshot，覆盖 list、detail、create wizard 入口、
   cancel、save-ready 状态。

### `/mcp`

状态: 部分管理 UI。

当前行为：

- `/mcp` 打开 `McpSurface`。
- 支持 server list 和 status/edit/reconnect/remove 等 action tabs。
- rich MCP 组件已存在，包括 server approval、multiselect approval、desktop import、
  tool list/detail、settings、reconnect、elicitation。

缺口：

无参数 `/mcp` 已经有实用管理面，但 add/edit 仍主要是填充 slash command prompt，
不是完整 in-surface 表单。一些 MCP rich dialog 也还没有统一纳入主 `/mcp`
surface 状态机。

建议计划：

1. 保留现有 action tabs。
2. 为选中 server 增加 in-surface detail mode，展示 tools/capabilities/warnings。
3. 在字段稳定时，把 add/edit 提升为小表单。
4. 尽量把已有 approval/import/tool-detail 组件接入同一状态机。
5. 保留 `/mcp <args>` 作为非交互路径。

### `/hooks`

状态: 部分 settings browser。

当前行为：

- `/hooks` 打开 `HooksSurface`。
- 可浏览 hook event 分类和 settings scope。
- 打开某个 scope 时进入 `$EDITOR`。

缺口：

这是设置浏览器，不是 in-TUI hook 配置编辑器。它不能在界面内创建、编辑、校验、
启用或禁用 hook entry。

建议计划：

1. 保留 scope/event 浏览。
2. 增加 selected hook entry detail mode。
3. 为常见字段提供表单编辑；复杂场景明确 fallback 到 `$EDITOR`。
4. 写入 hook settings 前展示校验反馈。

### `/memory`

状态: 部分 memory manager。

当前行为：

- `/memory` 打开 `MemorySurface`。
- 可浏览 memory targets 和 actions。
- 一些动作仍提交 slash command 或打开/编辑外部文件。

缺口：

它是有用的管理器，但还不是完整 memory 设置表单。auto-memory toggle 和 path
配置仍偏命令/文件驱动。

建议计划：

1. 增加 settings tab，展示 auto-memory enablement 与 scope/path 状态。
2. 保留 file open/edit 为显式动作。
3. 如后续加入 destructive memory 操作，必须有确认。

## 4. 设置/配置类命令但没有专门 UI

### `/permissions` / `/perms`

状态: 缺少 top-level settings UI。

已有组件：

- runtime permission approval dialogs 已存在。
- `ui/permissions/**` 中已有 permission rule list、rule input、workspace directory、
  recent denials、plan mode entry/exit、各类工具审批 renderer。

缺口：

当前没有无参数 `/permissions` command surface。命令 handler 是文本式的，而权限配置
又是最值得拥有专门设置 UI 的区域之一。

建议计划：

1. 新增 `CommandSurface::Permissions`。
2. 提供 tabs：
   - Mode：当前 default permission mode 和安全切换；
   - Rules：allow/ask/deny rules、source、matcher；
   - Workspace：trusted/untrusted workspace directories；
   - Session grants：当前 session 的一次性授权；
   - Recent denials：最近拒绝、命中规则和风险说明。
3. 尽量复用现有 permission rule renderers。
4. 持久化或高风险变更必须显式确认，并提交现有 `/permissions ...` 命令。
5. 增加测试覆盖 `/permissions` 打开、tab 导航、rule detail、add-rule prompt、
   workspace detail、reset confirmation、`/perms` alias。

### `/plugin`

状态: 缺少 top-level UI，已有独立计划。

当前 `/plugin` 是文本式 layered management。已有专门计划：

- [`plugin-ui-port-to-rust-plan-2026-05-08.md`](plugin-ui-port-to-rust-plan-2026-05-08.md)
- [`generic-selectable-command-surface-plan-2026-05-08.md`](generic-selectable-command-surface-plan-2026-05-08.md)

本审计只确认 `/plugin` 仍是 gap；实现时应沿用已有计划，不重复拆新线。

### `/keybindings`

状态: 缺少专门 UI。

当前行为是打开或创建 `~/.cc-rust/keybindings.json`。command palette 能显示 edit
target，但没有 TUI list/editor。

建议计划：

1. 先加只读 keybinding list。
2. 增加冲突检测。
3. enable/disable/edit 动作优先走现有 settings API；复杂编辑明确 fallback 到
   `$EDITOR`。

### `/statusline`

状态: 缺少专门 UI。

当前通过文本命令管理 show、set、clear、enable/disable、test、payload、refresh、
timeout、padding。

建议计划：

1. 新增 `StatusLineSurface`，包含 Config、Preview、Payload、Test tabs。
2. 用表单行展示 enabled、command、refresh interval、timeout、padding。
3. 保留 `/statusline <args>` 作为脚本路径。

### `/ide`

状态: 缺少 dedicated picker。

命令能 detect/select/reconnect IDE MCP bridge，但没有 command surface。

建议计划：

1. 新增 IDE picker/status surface。
2. 展示 detected IDEs、current selection、bridge health、reconnect、clear selection。
3. mutation 复用 `/ide select ...` 和 `/ide reconnect`。

### `/notify`

状态: 缺少专门 UI。

当前是文本 handler，受 feature gate 和后端能力影响。

建议计划：

1. 如果 notification support 被编译进来，增加 status/test/on/off surface。
2. feature-gate 或平台不可用时，把诊断作为 first-class state 展示。

### `/voice`

状态: 缺少专门 UI，runtime voice 当前不可用。

命令保留兼容设置，但真实 voice backend 目前不可用。

建议计划：

1. 在真实 voice backend 存在前，不做完整 voice wizard。
2. 如果加 surface，应只做 diagnostic-only 和 stored flag toggle。

### `/advisor`、`/fast`、`/experimental`、`/model`、`/effort`

状态: 没有各自独立 surface。

这些属于配置类命令。`/model` 和 `/effort` 已经通过 `/config` picker 覆盖，单独
surface 可选。`/advisor`、`/fast`、`/experimental` 更适合并入 `/config` 新 tab，
除非产品上明确需要独立入口。

建议计划：

1. 优先扩展 `/config`，不要制造很多很小的 settings overlay。
2. 如果这些设置经常变更，在 `/config` 增加 Advisor 和 Runtime tabs。
3. 保留 per-command slash handlers，供自动化和直接输入使用。

## 5. 需要接线决策的选择器/dialog 命令

### `/resume`

状态: picker 组件存在，command surface 缺失。

`ui/components/resume_picker.rs` 已存在，但 `/resume` 当前通过命令 handler resume
latest 或 by id。无参数 `/resume` 没有 picker。

建议计划：

1. 新增 `CommandSurface::Resume`。
2. 使用现有 picker data 加载 resume targets。
3. Enter 提交 `/resume <session_id>`。
4. 保留 `/resume recent` 和 `/resume <id>` 文本行为。

### `/skills`

状态: 有选择器 UI，不是设置 UI。

除非后续加入 skill install/edit management，否则不需要为 settings parity 单独处理。

### `/tasks` 和 `/team`

状态: 有管理 surface，不是全局 settings UI。

它们应继续作为任务/团队管理界面。除非未来新增持久化配置，不应归类为全局设置。

## 6. 建议实施顺序

1. 按已有计划完成 `/plugin`。
2. 新增 `/permissions`，因为它是高价值设置项，且已有很多可复用组件。
3. 升级 `/agents`：从 selector 变成 list/detail/create wizard，因为 detail renderer
   和 wizard 已存在但 slash surface 未接线。
4. 新增 `/resume` picker，因为 picker model 已存在且行为风险低。
5. 如需一等设置入口，把 Advisor/Fast/Experimental 扩展进 `/config`。
6. 新增 `/statusline`、`/keybindings`、`/ide` surfaces。
7. 回头增强 `/mcp`、`/hooks`、`/memory`，把 prompt-fill/editor fallback 替换为
   稳定 data model 上的 in-surface forms。

## 7. Definition of done

每个新增或升级的 surface 至少满足：

1. 无参数命令打开 surface；非空参数保留当前文本命令行为。
2. alias 路由一致，例如 `/settings` 到 `/config`；如新增 `/agent`，则到 `/agents`。
3. snapshot tests 覆盖主视图、empty state、导航、detail view、至少一个 action。
4. mutation 复用现有 command handlers 或 settings APIs，并保持 `.cc-rust` 路径隔离。
5. destructive 或 broad mutation 必须确认。
6. 实现完成后再更新 `docs/COMMAND_UI_REFERENCE.md`，不要提前把计划写成事实。

## 8. 后续实现验证命令

文档审计阶段：

```powershell
git diff -- docs/plan/command-settings-ui-coverage-audit-2026-05-08.md
```

实现具体 command surface 时：

```powershell
cargo test -p claude-code-rs command_surface
cargo test -p claude-code-rs permissions
cargo test -p claude-code-rs agents
```

如果涉及 UI snapshot，按现有仓库流程重新生成并审阅相关 `target/ui-snapshots` 或
insta snapshots。

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成（审计已交付，7 个实施建议均未开始）**

已完成：
- 全部审计工作已完成：已记录 11 个已有 Surface 的实现程度（`/config`、`/sandbox`、`/login`、`/agents`、`/mcp`、`/hooks`、`/memory`、`/permissions`、`/plugin`、`/keybindings`、`/statusline` 等）
- 所有建议实施计划已写入 §6（7 项）

未开始（7 项实施建议）：
1. 按已有计划完成 `/plugin` — 未开始
2. 新增 `/permissions` surface — 未开始
3. 升级 `/agents`（selector → list/detail/create wizard）— 未开始
4. 新增 `/resume` picker — 未开始
5. Advisor/Fast/Experimental 扩展进 `/config` — 未开始
6. 新增 `/statusline`、`/keybindings`、`/ide` surfaces — 未开始
7. 增强 `/mcp`、`/hooks`、`/memory` in-surface forms — 未开始
