# Rust TUI 通用长列表选择界面组件执行计划

日期: 2026-05-08

目标范围: 在实现 plugin UI 之前，先沉淀一个轻量、可复用的长列表选择组件，服务
`/plugin` 首版，并为后续 `/skills`、`/mcp`、`/agents` 等 command surface 的重复逻辑收敛提供路径。

相关计划:

- `rust/docs/plan/plugin-ui-port-to-rust-plan-2026-05-08.md`

## 0. 结论与默认方案

推荐方案: 需要做通用组件，但不要先做大型 UI 框架。正确顺序是:

1. 先抽一个薄的 `SelectableList` / `ListSurface` 通用状态与渲染辅助。
2. 用 `/plugin` UI 作为第一个真实落地场景验证它。
3. 再把 `/skills` 迁过去。
4. 再评估 `/mcp`。
5. 最后评估 `/agents`，因为它有 source grouping、create-new、shadowing 等特例。

不推荐:

- 不推荐在 plugin UI 之前做一轮大规模 `/agents`、`/skills`、`/mcp` 重构。
- 不推荐把通用组件设计成业务无关但功能过满的“UI 框架”。
- 不推荐直接让 plugin UI 再单独复制一套 selected index、filter、pagination 和 footer hints。

第一版应是“足够小的共享基础设施”:

- 管 selected index。
- 管 visible index。
- 管 filter。
- 管 page/window。
- 管通用键盘事件。
- 管 empty state 和 footer hints。
- 不管具体业务字段。
- 不直接知道 plugin、agent、skill、mcp 的数据结构。

## 1. 当前代码事实

Rust TUI 里已经存在多个相似但各自实现的选择界面:

### 1.1 已有通用雏形

`rust/crates/claude-code-rs/src/ui/components/selection_surface.rs`

已有能力:

- `SelectionItem { id, label, description, enabled }`
- `SelectionSurface { title, items, selected, filter }`
- Up/Down/Tab/k/j 导航。
- Enter 选择。
- Esc 关闭。
- Backspace 和 Ctrl+U 编辑 filter。
- 使用 `SearchBox` 渲染 filter。
- 使用 fuzzy match 排序。

局限:

- item shape 太简单，只适合 command picker 级别的列表。
- 不能表达分组 header。
- 不能表达多行 row。
- 不能表达 row 右侧状态列。
- 不能表达 plugin details/actions。
- 没有 pagination/windowing 的完整控制。
- selected index 是 visible-relative，但缺少对复杂列表的稳定 API。

### 1.2 `/agents` 当前实现

`rust/crates/claude-code-rs/src/ui/agents/agents_list.rs`

已有能力:

- 按 source filter 后列出 agents。
- 支持 create-new 伪行。
- 支持 source grouping。
- 支持 selected row。

重复点:

- 自己维护 `selected_index`。
- 自己实现 `move_next` / `move_prev`。
- 自己渲染 marker 和列表。

特例:

- 有 create-new row。
- 有 source grouping。
- 有 agent shadowing。
- 有 model/memory suffix。

结论: `/agents` 不适合作为第一个迁移对象，应最后处理。

### 1.3 `/mcp` 当前实现

`rust/crates/claude-code-rs/src/ui/mcp/mcp_list_panel.rs`

已有能力:

- 展示 MCP server 列表。
- 支持 selected row。
- 支持循环上下移动。
- row 内显示 kind、status、tools count、command/url。

重复点:

- 自己维护 selected index。
- 自己渲染列表、marker、empty state。

特例:

- `/mcp` command surface 还有 action tabs: Status/Edit/Reconnect/Remove。
- MCP row 有二级文本 command/url。

结论: `/mcp` 是第二批迁移对象，适合验证 action tabs + two-line rows。

### 1.4 `/skills` 当前实现

`rust/crates/claude-code-rs/src/ui/skills/skills_menu.rs`

已有能力:

- filter by name/description。
- selected row。
- empty state。
- 多行 row: 主行 + source 行。

重复点:

- 自己 filter。
- 自己维护 selected index。
- 自己渲染 marker。

结论: `/skills` 最适合作为 plugin 之后的第一批迁移对象。

### 1.5 `/plugin` 需求会放大重复

根据 plugin 迁移计划，`/plugin` 需要:

- tabs。
- installed/errors/discover/marketplaces 多视图。
- searchable long list。
- grouped rows。
- plugin row / failed plugin row / child MCP row 多种 row 类型。
- details view。
- actions。
- pagination。
- refresh-needed footer。

如果 plugin UI 不先沉淀通用列表组件，会继续复制 skills/mcp/agents 已有问题。

## 2. 目标组件边界

建议新增模块:

- `rust/crates/claude-code-rs/src/ui/components/selectable_list.rs`

也可以命名为:

- `list_surface.rs`
- `long_list.rs`

推荐名称: `selectable_list.rs`。原因是它不是完整 surface，不负责 overlay，也不负责业务动作，只负责可选择列表的状态和文本渲染辅助。

### 2.1 组件负责什么

第一版负责:

- 保存 `selected`。
- 保存 `filter`。
- 根据 filter 计算 visible indices。
- 提供 `selected_item`。
- 提供 `move_next` / `move_prev`。
- 提供可配置的循环或 clamp 导航。
- 提供 page/window 计算。
- 处理通用键:
  - Up / Down
  - k / j
  - Tab
  - Backspace
  - Ctrl+U
  - 普通字符输入 filter
- 渲染 search line。
- 渲染 empty state。
- 渲染 scroll indicators。
- 渲染 footer hints。

### 2.2 组件不负责什么

第一版不负责:

- 不知道 plugin/skill/mcp/agent 的业务字段。
- 不直接提交 slash command。
- 不处理 details view。
- 不处理 marketplace install/update。
- 不处理 plugin option forms。
- 不直接管理 `CommandSurfaceOutcome`。
- 不替代 `CommandSurface` overlay。
- 不重写 ratatui layout 系统。

### 2.3 最小 API 草案

建议状态类型:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectableListState {
    pub selected: usize,
    pub filter: String,
    pub visible_offset: usize,
    pub wrap_navigation: bool,
}
```

建议 item trait 或轻量 view-model:

```rust
pub trait SelectableListItem {
    fn id(&self) -> &str;
    fn searchable_text(&self) -> Vec<&str>;
    fn enabled(&self) -> bool {
        true
    }
}
```

如果不希望第一版引入 trait，可先用非泛型 view-model:

```rust
pub struct ListItemView {
    pub id: String,
    pub search_terms: Vec<String>,
    pub enabled: bool,
}
```

推荐第一版采用非泛型 `ListItemView`，降低 lifetime/trait 复杂度。业务 surface 自己保留原始 row 列表，通过 adapter 生成 `ListItemView` 用于选择和过滤。

建议事件:

```rust
pub enum SelectableListEvent {
    None,
    Selected(String),
    FilterChanged,
}
```

建议渲染辅助:

```rust
pub struct ListRenderConfig<'a> {
    pub title: &'a str,
    pub empty_label: &'a str,
    pub filter_placeholder: &'a str,
    pub footer_hint: &'a str,
    pub page_height: usize,
}
```

第一版可以先返回 `Vec<usize>` 和状态，让业务自己拼字符串；不要急着做复杂 renderer。

## 3. 与 `CommandSurface` 的关系

`SelectableList` 不应该替代 `CommandSurface`。

现有结构:

- `CommandSurface` 负责决定哪个 slash command 打开哪个 overlay。
- 各 surface 负责业务状态和动作。
- `render_command_surface_overlay` 负责 overlay 框架。

新增后结构:

```text
CommandSurface::Plugin
  -> PluginSurface
      -> SelectableListState
      -> plugin adapter rows
      -> plugin-specific render_row/render_details

CommandSurface::Skills
  -> SkillsSurface
      -> SelectableListState
      -> skill-specific render_row

CommandSurface::Mcp
  -> McpSurface
      -> action tabs
      -> SelectableListState
      -> mcp-specific render_row
```

这样做的好处:

- 通用组件只沉淀列表交互，不绑业务。
- 每个 command surface 仍能保留自己的 action 语义。
- plugin UI 不会因为抽象不足而复制基础状态逻辑。

## 4. 分阶段执行计划

### Phase 0. 锁定现状与测试基线

目标: 在抽组件前确认当前列表 surface 行为，避免重构时丢功能。

步骤:

1. 阅读并记录这些文件的行为:
   - `ui/components/selection_surface.rs`
   - `ui/skills/skills_menu.rs`
   - `ui/mcp/mcp_list_panel.rs`
   - `ui/agents/agents_list.rs`
   - `ui/components/command_surface/surfaces/skills.rs`
   - `ui/components/command_surface/surfaces/mcp.rs`
   - `ui/components/command_surface/surfaces/agents.rs`
2. 确认现有 snapshot 和行为测试覆盖:
   - `ui/components/command_surface/tests.rs`
   - `ui/skills/snapshots`
   - `ui/mcp/snapshots`
   - `ui/agents/snapshots`
3. 不改 agents/mcp/skills 行为，只新增组件测试。

验收标准:

- 现有 command surface tests 仍能作为迁移后的行为对照。
- 形成一个明确迁移顺序: plugin -> skills -> mcp -> agents。

验证:

```powershell
cargo test -p claude-code-rs command_surface
```

### Phase 1. 新增 `SelectableListState`

目标: 抽出最小通用状态，不接入业务 surface。

新增文件:

- `rust/crates/claude-code-rs/src/ui/components/selectable_list.rs`

修改:

- `rust/crates/claude-code-rs/src/ui/mod.rs` 或对应 components module export。

第一版能力:

- `ListItemView`
- `SelectableListState`
- `visible_indices(items)`
- `selected_item(items)`
- `move_next(items)`
- `move_prev(items)`
- `set_filter`
- `clear_filter`
- `handle_filter_key`
- `window(items, page_height)`
- `can_scroll_up`
- `can_scroll_down`

建议测试:

- empty list 不 panic。
- selected index 在 filter 后归零。
- filter 能按多个 search terms 匹配。
- selected item 返回 visible item。
- navigation 支持 wrap 或 clamp。
- windowing 让 selected row 保持可见。

验收标准:

- 新组件不影响任何现有 UI。
- 新组件不依赖 plugin/mcp/skills/agents。

验证:

```powershell
cargo test -p claude-code-rs selectable_list
```

### Phase 2. 用 `/plugin` 首版验证通用组件

目标: plugin UI 使用 `SelectableListState`，避免复制 skills/mcp 的列表逻辑。

依赖:

- `rust/docs/plan/plugin-ui-port-to-rust-plan-2026-05-08.md` 的 Phase 1。

步骤:

1. 新增 plugin UI rows。
2. plugin adapter 生成:
   - `Vec<PluginRow>`
   - `Vec<ListItemView>`
3. `PluginSurface` 内嵌 `SelectableListState`。
4. `PluginSurface` 自己负责:
   - tab 状态。
   - details view。
   - actions。
   - plugin-specific row rendering。
5. `SelectableListState` 负责:
   - filter。
   - selected visible index。
   - page/window。
   - generic navigation。

验收标准:

- `/plugin` 无参数打开 UI。
- Installed 和 Errors view 能处理长列表、filter、选择和 details。
- plugin UI 没有自己重复实现复杂 visible-index 映射。

验证:

```powershell
cargo test -p claude-code-rs plugin command_surface
```

### Phase 3. 迁移 `/skills`

目标: 用最简单的现有 surface 验证通用组件可以替代旧 filter/selection 逻辑。

修改:

- `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/skills.rs`
- `rust/crates/claude-code-rs/src/ui/skills/skills_menu.rs`

步骤:

1. `SkillsSurface` 从 `selected_index + filter` 改为 `SelectableListState`。
2. `SkillMenuItem` 生成 `ListItemView`。
3. 保持现有显示:
   - name
   - enabled/disabled
   - description
   - source
4. 保持现有快捷键:
   - type to filter
   - Backspace
   - Enter details
   - `r` reload
   - `d` diagnostics

验收标准:

- skills snapshot 变化可解释。
- `/skills` filter 和 Enter 行为不回退。
- source 行仍显示。

验证:

```powershell
cargo test -p claude-code-rs skills command_surface
```

### Phase 4. 迁移 `/mcp`

目标: 验证 action tabs + two-line rows 与通用 list state 共存。

修改:

- `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/mcp.rs`
- `rust/crates/claude-code-rs/src/ui/mcp/mcp_list_panel.rs`

步骤:

1. `McpSurface` 保留 `action_index`。
2. `McpListPanelState` 改用或包装 `SelectableListState`。
3. MCP server 生成 `ListItemView`。
4. row rendering 保持:
   - name
   - kind
   - status
   - tools count
   - command/url 二级行
5. 验证 action tabs 不受 filter/list 影响。

验收标准:

- `/mcp` 的 Status/Edit/Reconnect/Remove tabs 不回退。
- Up/Down/Enter 行为不回退。
- 后续可加 filter，但如果现在不加，也要让 selected/window 逻辑复用。

验证:

```powershell
cargo test -p claude-code-rs mcp command_surface
```

### Phase 5. 评估 `/agents`

目标: 决定 `/agents` 是否值得迁移，避免为了统一而破坏复杂行为。

评估点:

- create-new row 是否适合用 generic pseudo item。
- source grouping 是否能通过 row header 表达。
- shadowing 和 source tabs 是否清晰。
- agent generation wizard 是否受影响。

可能结果:

| 结果 | 含义 |
| --- | --- |
| A. 迁移 | 用 `SelectableListState` 管 visible items 和 selected index，保留 agent-specific row rendering。 |
| B. 部分迁移 | 只复用 navigation/window/filter helper，不改 row/grouping。 |
| C. 暂不迁移 | 保持 agents 独立实现，因为特例多于收益。 |

推荐默认: 先选 B 或 C，不要在 plugin UI 主线期间动 `/agents`。

验收标准:

- 如果迁移，agent snapshots 必须稳定。
- create-new、source tabs、shadowed label、model/memory label 不回退。

验证:

```powershell
cargo test -p claude-code-rs agents command_surface
```

## 5. 文件级执行清单

### 新增文件

| 文件 | 用途 |
| --- | --- |
| `rust/crates/claude-code-rs/src/ui/components/selectable_list.rs` | 通用长列表选择状态和辅助。 |

### 首轮可能修改文件

| 文件 | 修改 |
| --- | --- |
| `rust/crates/claude-code-rs/src/ui/mod.rs` | export 新组件模块。 |
| `rust/crates/claude-code-rs/src/ui/components/selection_surface.rs` | 暂不替换；后续可考虑复用 `SelectableListState`。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/tests.rs` | 增加通用行为或 plugin 使用场景测试。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/plugin.rs` | plugin 首个使用方。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/skills.rs` | Phase 3 迁移。 |
| `rust/crates/claude-code-rs/src/ui/skills/skills_menu.rs` | Phase 3 渲染改造。 |
| `rust/crates/claude-code-rs/src/ui/components/command_surface/surfaces/mcp.rs` | Phase 4 迁移。 |
| `rust/crates/claude-code-rs/src/ui/mcp/mcp_list_panel.rs` | Phase 4 渲染改造。 |
| `rust/crates/claude-code-rs/src/ui/agents/agents_list.rs` | Phase 5 评估后再决定。 |

## 6. 组件设计约束

1. 不新增依赖。
2. 不引入复杂泛型，除非非泛型 view-model 明显不够。
3. 不把业务行为塞进通用组件。
4. 不在第一阶段替换所有现有 surface。
5. 通用组件必须可单测。
6. 通用组件必须支持空列表。
7. filter 后 selected 必须保持合法。
8. page/window 必须确保 selected row 可见。
9. row rendering 由业务 surface 控制。
10. command action 由业务 surface 转成 `CommandSurfaceOutcome`。

## 7. 测试计划

### 7.1 `SelectableListState` 单元测试

覆盖:

- empty list。
- 单项列表。
- 多项列表。
- wrap navigation。
- clamp navigation。
- filter 命中 label/id/description/component search terms。
- filter 后 selected reset。
- selected item 跟随 visible indices。
- windowing 在 selected 超出页面时滚动。
- disabled item 不触发 select。

建议命令:

```powershell
cargo test -p claude-code-rs selectable_list
```

### 7.2 command surface 回归测试

覆盖:

- `/plugin` 新 surface。
- `/skills` 迁移后 filter 和 Enter。
- `/mcp` 迁移后 action tabs。
- `/agents` 若迁移，则覆盖 source tabs 和 create-new。

建议命令:

```powershell
cargo test -p claude-code-rs command_surface
```

### 7.3 snapshot 测试

需要关注:

- 80 列宽。
- 长名称。
- 长描述。
- 空状态。
- selected row。
- filter 有结果。
- filter 无结果。
- pagination up/down indicators。

## 8. 风险和缓解

| 风险 | 缓解 |
| --- | --- |
| 过早抽象导致组件复杂化。 | 第一版只抽状态和基础窗口计算，不抽业务 row。 |
| 迁移范围膨胀，拖慢 plugin UI。 | plugin-first，不先批量迁移 agents/mcp/skills。 |
| 现有 `/agents` 特例太多。 | agents 最后评估，可以不迁。 |
| snapshot 大量变化。 | 每个 surface 单独迁移，单独更新 snapshot。 |
| filter 语义与旧实现不一致。 | Phase 3 迁移 skills 时明确接受 fuzzy 或保持 substring，不能静默改变。 |
| list state 和 business rows 双重索引出错。 | 统一通过 visible indices 获取 selected item。 |
| overlay 高度不足。 | 组件支持 windowing；overlay 尺寸优化单独处理。 |

## 9. 与 plugin UI 计划的关系

本计划应作为 plugin UI 计划中 D2 的具体落地方案。

建议将 D2 默认方案调整为:

> 首版仍使用现有 `CommandSurface` overlay，但在实现 `PluginSurface` 前新增一个薄的 `SelectableListState` 通用组件。Plugin UI 是第一个使用方；后续按收益逐步迁移 `/skills`、`/mcp`、`/agents`。

这样兼顾:

- 不做大型 UI 框架。
- 不继续复制长列表逻辑。
- 不阻塞 plugin UI。
- 给后续 command surfaces 留出统一路径。

## 10. 第一版 Definition of Done

第一版完成标准:

- 新增 `SelectableListState` 或同等命名的通用长列表状态组件。
- 组件有独立单元测试。
- plugin UI 首版使用它管理列表选择、filter 和 windowing。
- 没有批量重构 `/agents`、`/skills`、`/mcp`。
- 不新增依赖。
- 不改变现有 `/skills`、`/mcp`、`/agents` 行为。
- 文档中的后续迁移顺序明确。

## 11. 固定中等推理执行拆分

本计划的可执行会话拆分已落到:

- `.omx/plans/generic-selectable-command-surface-medium-session-plan-2026-05-08.md`
- `scripts/tmp/run-generic-selectable-command-surface-sessions.ps1`

执行约束:

- 所有会话固定使用 `model_reasoning_effort="medium"`。
- 通过更小的会话边界、明确 owned files、禁止范围和测试门槛来缩短推理成本，不降低代码质量要求。
- 默认执行范围只覆盖第一版 DoD: 新增通用 `SelectableListState`、接入 `/plugin` 首版、完成测试和文档闭环。
- `/skills`、`/mcp`、`/agents` 迁移继续作为后续独立 lane，不在首版脚本中批量执行。

执行入口:

```powershell
.\scripts\tmp\run-generic-selectable-command-surface-sessions.ps1
```

从指定阶段继续:

```powershell
.\scripts\tmp\run-generic-selectable-command-surface-sessions.ps1 -StartAt 3
```
