# 执行计划：Agent 导航 UI 接线

> **生成日期**: 2026-05-18
> **来源**: `docs/ui/great/06_missing_features_summary.md` §6
> **参考实现**:
> - TS: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/components/agents/` (14 个文件, 2286 行)
> - TS AgentNavigationFooter: `AgentNavigationFooter.tsx` (19 行)
> - TS AgentsList: `AgentsList.tsx` (285 行)
> - TS AgentDetail: `AgentDetail.tsx` (133 行)
> - TS AgentEditor: `AgentEditor.tsx` (228 行)
> - TS AgentsMenu: `AgentsMenu.tsx` (346 行)
> - Rust 导航状态: `crates/claude-code-rs/src/ui/app/agent_navigation.rs` (184 行, `#[allow(dead_code)]`)
> - Rust agents 目录: `crates/claude-code-rs/src/ui/agents/` (29 个文件, ~1800 行)
> - Rust 代理导航页脚: `agents/agent_navigation_footer.rs` (28 行)
> - Rust agents_list: `agents/agents_list.rs` (110 行)
> - Rust agents_menu: `agents/agents_menu.rs` (81 行)
> - Rust 渲染管线: `app/render.rs` (681 行)
> - Rust App 结构体: `app.rs` (153 行结构体)

---

## 当前状态

Rust 的 Agent 系统处于**代码已实现但未接线**的独特状态:

### 代理导航数据模型 (`app/agent_navigation.rs`)
- `AgentNavigationState` 结构体: 使用 `BTreeMap<String, AgentThreadEntry>` 维护线程状态
- `upsert()` / `mark_closed()` / `remove()` / `clear()` — CRUD 方法完整
- `ordered_threads()` — 插入顺序维护
- `adjacent_thread_id()` — 方向导航 (`Previous`/`Next`)
- `active_agent_label()` — 多代理时返回标签
- `render_agent_tree()` — 文本树渲染（含活动/关闭状态、角色、线程 ID）
- 测试覆盖: 插入顺序导航、树渲染 — 有单元测试
- **整模块标记 `#[allow(dead_code)]`** — 未在 `App` 结构体中引用

### 代理 UI 组件 (`agents/`)
- `agent_navigation_footer.rs` (28行) — 页脚组件（存在但未接入渲染管线）
- `agents_list.rs` (110行) — 代理列表（纯文本，含排序、源分组、键盘导航 stub）
- `agents_menu.rs` (81行) — 代理菜单（纯文本，选项显示）
- `agent_detail.rs` — 代理详情视图
- `agent_editor.rs` — 代理编辑器
- `tool_selector.rs`, `color_picker.rs`, `model_selector.rs` — 选择器组件
- `new_agent_creation/` — 11 步向导（25+ 文件）
- `generate_agent.rs`, `validate_agent.rs`, `types.rs`, `utils.rs` — 辅助模块

**关键差距**: 以上所有组件主要输出纯文本且为 `String` 返回类型，未集成 ratatui 样式的渲染。`agent_navigation_footer.rs` 存在但未被 `render.rs` 调用。

### App 结构体
- 无 `AgentNavigationState` 字段
- 无 `render_agent_tree()` 在渲染管线中调用
- 页脚、树面板、列表、菜单等在渲染管线中无对应插槽

## 目标状态

1. **`AgentNavigationState` 接入 App** — 作为 `App` 结构体字段，在消息/线程状态变更时更新
2. **代理导航页脚渲染** — 在多代理活跃且在非主线程时，在状态栏上方/内部显示导航提示（`Press ↑↓ to navigate · Enter to select · Esc to go back`）
3. **代理树面板** — 可通过按键切换显示的覆盖层，展示所有代理线程状态
4. **代理列表/菜单** — 将现有 `agents_list.rs` 和 `agents_menu.rs` 转为 ratatui Style 渲染，接入覆盖层系统
5. **协调器状态面板** — 显示协调器/队友的摘要状态
6. **移除 `#[allow(dead_code)]`** — 验证数据模型和渲染路径均被使用

## TS 参考

### `AgentNavigationFooter.tsx` (19 行)
- 显示导航操作说明
- 集成 `useExitOnCtrlCDWithKeybindings()` 用于退出确认
- 使用 `<Text dimColor>` 渲染

### `AgentsList.tsx` (285 行)
- 按 `AGENT_SOURCE_GROUPS` 分组显示代理
- "创建新代理"选项
- 键盘导航（↑↓ 选择，Enter 确认，Esc 返回）
- 内置/自定义代理区分（dimColor 标记内置）
- 覆盖/被覆盖标记（override/warning）
- 模型和记忆信息显示
- 空状态提示

### `AgentDetail.tsx` (133 行)
- 显示代理配置详情
- 编辑/删除操作

### `AgentEditor.tsx` (228 行)
- 代理配置文件编辑器
- 设置变更管理

### `AgentsMenu.tsx` (346 行)
- 代理选择菜单（来源筛选: all/built-in/user/plugin）
- 支持从菜单创建新代理
- 与键盘导航集成的列表

## Rust 当前代码

### `app/agent_navigation.rs` (184 行)
```rust
pub struct AgentNavigationState {
    entries: BTreeMap<String, AgentThreadEntry>,
    order: Vec<String>,
}
```
方法: `upsert()`, `mark_closed()`, `remove()`, `clear()`, `ordered_threads()`, `adjacent_thread_id()`, `active_agent_label()`, `render_agent_tree()`

### `agents/agent_navigation_footer.rs` (28 行)
存在纯文本页脚结构，但未被调用。

### `agents/mod.rs` (218 行)
模块导出所有 agent 组件，`#[cfg(test)]` 测试块测试了 `agent_detail`、`agents_list`、`agent_file_utils`、`agent_navigation_footer` 的集成。测试中存在设备路径硬编码（`F:/repo/`），需修复。

### `app/render.rs` (681 行)
底部区域布局：
```
spinner (流式时) → suggestions → paste_notice → input → command_palette
→ command_arg_help → status_bar
```
**无 agent 导航插槽**。

### `command_surface/surfaces/agents.rs`
已存在 `/agents` 命令表面，使用 `render_agent_detail()`。

## 分步实施

### 阶段 1: 将 AgentNavigationState 接入 App（~80 行）

- [ ] 1.1 在 `App` 结构体添加 `agent_nav: AgentNavigationState` 字段
- [ ] 1.2 在 `App::new()` 中初始化 `AgentNavigationState::default()`
- [ ] 1.3 添加 `App::update_agent_navigation()` 方法，从消息列表同步代理线程状态（遍历 `Message` 中的 agent 相关事件 → `upsert`/`mark_closed`）
- [ ] 1.4 在流式消息更新后或收到引擎事件时调用该方法
- [ ] 1.5 移除 `mod agent_navigation;` 上的 `#[allow(dead_code)]` 属性

### 阶段 2: 代理导航页脚（~60 行）

- [ ] 2.1 在 `App` 中添加 `show_agent_footer: bool` 字段（在多代理模式下启用）
- [ ] 2.2 在 `render.rs` 中检测多代理状态: `self.agent_nav.ordered_threads().len() > 1`
- [ ] 2.3 在底部区域增加可选的代理导航页脚行（位于状态栏上方或内部）
- [ ] 2.4 调用 `agent_navigation_footer` 渲染: `self.agent_nav.active_agent_label(current_thread_id)` 和导航提示
- [ ] 2.5 当 `show_agent_footer` 为 true 时显示，状态栏信息中显示当前代理标签

### 阶段 3: 代理树面板覆盖层（~120 行）

- [ ] 3.1 在 `AppAction` 枚举添加 `ToggleAgentTree` / `ShowAgentTree` 变体
- [ ] 3.2 在 `App` 中添加 `agent_tree_visible: bool` 或 `agent_tree_dialog: Option<AgentTreeDialog>`，引用 `AgentNavigationState`
- [ ] 3.3 新建 `agent_tree_dialog.rs`: 覆盖层面板，使用 `render_agent_tree()` 但以 `Vec<Line>`（ratatui 样式）替代纯文本
- [ ] 3.4 在 `render.rs` 覆盖层段（permission_dialog / history_search_dialog 同级）渲染
- [ ] 3.5 支持键盘导航（↑↓ 选择线程，Enter 切换到选中的线程）
- [ ] 3.6 按键绑定: 将某个键绑定动作（如 `agents:tree`）分发至 `ToggleAgentTree`

### 阶段 4: 代理列表视图（~150 行）

- [ ] 4.1 新建 `agent_list_dialog.rs` 或扩展 `agents/agents_list.rs` 为 ratatui widget
- [ ] 4.2 转换现有纯文本渲染为 `Vec<Line>` 带样式输出（代理名、模型、源、覆盖状态）
- [ ] 4.3 键盘导航集成（↑↓ 选择，Enter 进入详情，Esc 关闭）
- [ ] 4.4 支持源筛选（all/built-in/user/plugin）
- [ ] 4.5 在覆盖层系统中渲染（与 history_search 同级）

### 阶段 5: 协调器/队友状态面板（~80 行）

- [ ] 5.1 在 `AgentNavigationState` 或新增 `AgentCoordinatorState` 中追踪协调器/队友状态
- [ ] 5.2 覆盖层面板显示所有队友的活跃摘要（名称、角色、状态、工具计数）
- [ ] 5.3 在渲染管线中按需显示

### 阶段 6: 移除死代码+清理（~20 行）

- [ ] 6.1 从 `app.rs` 和所有使用 `AgentNavigationState` 的文件中移除 `#[allow(dead_code)]`
- [ ] 6.2 修复 `agents/mod.rs` 测试中的设备路径硬编码（`F:/repo/`）
- [ ] 6.3 验证 `render_agent_tree()` 被 `agent_tree_dialog` 调用而非孤立

### 阶段 7: 测试（~100 行）

- [ ] 7.1 `AgentNavigationState` 接入 App 的集成测试（模拟消息更新 → 验证状态同步）
- [ ] 7.2 代理树面板渲染快照测试
- [ ] 7.3 代理列表视图键盘导航测试
- [ ] 7.4 多代理场景的端到端渲染测试（页脚显示、线程切换）

## 工作量估算

| 阶段 | 内容 | 估算行数 | 复杂度 |
|:----:|------|:--------:|:------:|
| 1 | AgentNavigationState 接入 App | ~80 | 低 |
| 2 | 代理导航页脚 | ~60 | 低 |
| 3 | 代理树面板覆盖层 | ~120 | 中 |
| 4 | 代理列表视图（ratatui 化） | ~150 | 中 |
| 5 | 协调器状态面板 | ~80 | 中 |
| 6 | 死代码清理 | ~20 | 低 |
| 7 | 测试 | ~100 | 低-中 |
| **合计** | | **~610** | |

## 依赖/前提

1. **现有 agents/ 目录是基础** — 所有 UI 组件已存在纯文本版本，本计划主要工作是将它们接入渲染管线并添加样式
2. **数据流**: 代理线程状态从 `Message` 数组驱动 — 需要 `App::update_agent_navigation()` 在消息变更后同步更新
3. **按键绑定**: 需在 `input.rs` 中添加 agents 相关的按键处理（切换树面板、列表导航）
4. **与 CommandSurface 关系**: 已有 `/agents` 命令表面 (`command_surface/surfaces/agents.rs`)，树面板/列表可作为增强替代或补充
5. **最小外部依赖**: 全部使用现有 ratatui 原语和现有模块

### 注意事项
- 代理编辑器和创建向导（`new_agent_creation/` 中的 11 步向导）**不在本计划范围内** — 它们已有独立实现，本计划聚焦导航和列表渲染
- 本计划完成后，`agents/` 目录中所有组件应编入渲染管线，数据模型不再有 `dead_code`

## 实施后遗留问题（2026-05-20）

本计划的核心接线已经完成：`AgentNavigationState` 已进入 `App` 状态，agent 树覆盖层、导航页脚、按键动作和相关渲染测试已接入生产路径。`cargo check -p claude-code-rs` 和 `cargo build --workspace --release` 未产生 Rust 生产警告。

仍需在后续计划中跟踪：

1. `agents/` 中的列表、菜单、详情和编辑器仍主要作为上游镜像/辅助表面存在，当前落地重点是导航树和页脚；若要求完整 TS parity，需要继续把 `AgentsList`、`AgentsMenu`、`AgentDetail`、`AgentEditor` 的完整交互接入覆盖层或命令表面。
2. 协调器/队友状态目前只覆盖导航所需的线程标签和状态摘要；TS 侧更完整的队友摘要、工具计数和协调器面板仍是后续 UI parity 工作。
3. `cargo test -p claude-code-rs ui:: -- --nocapture` 仍会在测试目标中暴露部分 agent 辅助/编辑模块的 `dead_code` 警告；这不影响生产构建，但需要在相关表面真正接线或显式测试门控时继续收敛。
