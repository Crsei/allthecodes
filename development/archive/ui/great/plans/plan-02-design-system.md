# Plan 02: Design System Components — 补齐计划

> **目标**: 补齐 16 个 TS `@anthropic/ink` design-system 组件到 Rust (ratatui) 后端
> **上游参考**: `claude-code-bun/packages/@ant/ink/src/theme/` 下的对应实现
> **Rust 现有**: `crates/claude-code-rs/src/ui/components/` 的现有实现
> **生成日期**: 2026-05-18
> **状态**: 规划中

---

## 执行策略概述

### 分层依赖

```
层 1 (基础设施)       层 2 (原子组件)        层 3 (复合组件)
───────────────── ──────────────────── ───────────────────
ThemeProvider        Divider              Dialog
color (解析工具)     ThemedBox            Pane
                     ThemedText           FuzzyPicker
                     KeyboardShortcutHint
                     Byline
                     StatusIcon
                     ProgressBar
                     LoadingState
                     Ratchet
                     Tabs
                     ListItem
```

### 实现策略选择

**方案 A: ratatui Widget (推荐)** — 为每个组件实现 `ratatui::widgets::Widget` trait，
使其可渲染为 `ratatui::Buffer`。这是正确的 Rust TUI 抽象方式。

**方案 B: 字符串渲染 + Style** — 沿用现有模式，组件返回 `Vec<Line<'static>>` 或 `String`
外加 `Style`。适用于简单组件，但缺乏 widget 组合性。

**推荐**: 层 2 原子组件用方案 B（轻量，与现有代码风格一致），层 3 复合组件用方案 A。

### 文件位置

- 基础设施: `crates/claude-code-rs/src/ui/theme/` (新建目录)
- 原子组件: `crates/claude-code-rs/src/ui/components/` (现有目录)
- 复合组件: `crates/claude-code-rs/src/ui/overlays/` (新建目录，用于 Dialog 等)

---

## 组件补齐计划

---

### 1. ThemeProvider — 运行时主题切换 (P0)

| 维度 | 内容 |
|------|------|
| **当前状态** | Rust 无主题系统。所有颜色硬编码为 ANSI/RGB 常量 (`Color::Rgb(190, 140, 255)`)。OpenTUI 前端使用静态 `theme.ts` 调色板，无运行时切换。 |
| **目标状态** | 全局 `Theme` 结构体，可通过 `ThemeName` 切换。主题持久化到 `~/.cc-rust/`。 |
| **TS 参考** | `ThemeProvider.tsx` (157行) + `theme-types.ts` (640行) |
| **工作量** | 3-4 天 |
| **依赖** | 无 |

#### 实现步骤

1. **定义 Theme 结构体**
   - 在 `src/ui/theme/` 下新建 `mod.rs`，定义 `Theme` 包含 6 种预定义主题的数据。
   - 类型为 `HashMap<&'static str, Color>` 或结构体字段（推荐结构体，编译时类型安全）。
   - 每种子主题的颜色键与 TS `theme-types.ts` 的 `Theme` 接口对齐（约 60 个 key）。

2. **定义 ThemeName 枚举**
   ```rust
   pub enum ThemeName {
       Dark,
       Light,
       LightDaltonized,
       DarkDaltonized,
       LightAnsi,
       DarkAnsi,
   }
   ```

3. **实现 `get_theme()` 函数**
   - 将 TS 的 `getTheme()` 函数直接移植。
   - 每个 `ThemeName` 对应一个完整的颜色表。

4. **实现 ThemeProvider**
   - `ThemeProvider` 结构体，当前活动主题的持有者。
   - `fn current(&self) -> &Theme` —— 获取当前主题。
   - `fn set_theme(&mut self, name: ThemeName)` —— 切换主题。
   - 使用 `OnceCell` 或类似机制实现全局可访问性。

5. **持久化**
   - `load_theme_setting()` — 从 `~/.cc-rust/config.toml` 或 `config` 模块读取。
   - `save_theme_setting()` — 写入持久化配置。
   - 支持 `ThemeSetting::Auto`（跟随终端背景）。

6. **Auto 模式**
   - 终端启动时检查 `$COLORFGBG` 环境变量。
   - 可选：通过 OSC 11 转义序列轮询终端背景色（需要 crossterm 支持）。

7. **注册到 App**
   - 将 ThemeProvider 添加到 `App` 结构体。
   - 所有渲染函数从 App 获取当前主题。

---

### 2. color — 主题色解析工具 (P0)

| 维度 | 内容 |
|------|------|
| **当前状态** | 无。所有颜色硬编码。 |
| **目标状态** | `resolve_color()` / `ColorExt` trait 用于将主题 key 解析为 ratatui `Color`。 |
| **TS 参考** | `color.ts` (31行) + `ThemedBox.tsx` 中的 `resolveColor()` 函数 |
| **工作量** | 0.5 天 |
| **依赖** | ThemeProvider |

#### 实现步骤

1. **实现 `resolve_color` 函数**
   ```rust
   pub fn resolve_color(color: &str, theme: &Theme) -> Option<Color>;
   ```
   - 支持直接值：`rgb(r,g,b)`、`#hex`、`ansi256(n)`、`ansi:<name>`
   - 支持主题 key 查找
   - 返回 `Option<Color>`，`None` 表示未设置

2. **实现 `ColorExt` trait**（可选）
   - 为 `Color` 添加 `dimmed()` 方法（透明度兼容 ANSI dim）。
   - 使用主题的 `inactive` 颜色做 dimming（与粗体兼容）。

---

### 3. Divider — 分隔线 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 手动 `"\u{2500}".repeat()` 散落在各处。 |
| **目标状态** | `Divider` 结构体，返回 `Vec<Line>`。支持主题颜色、带标题居中、填充、自定义字符。 |
| **TS 参考** | `Divider.tsx` (91行) |
| **工作量** | 0.5 天 |
| **依赖** | ThemeProvider, color |

#### 实现步骤

1. 在 `components/divider.rs` 新建 `Divider` 结构体：
   - `width: Option<usize>` — 宽度（默认终端宽度）
   - `color: Option<&'static str>` — 主题 key
   - `char: char` — 重复字符（默认 `─`）
   - `padding: usize` — 缩进填充
   - `title: Option<String>` — 居中对齐的标题

2. 实现 `Divider::render(&self, theme: &Theme, term_width: usize) -> Vec<Line>`。

3. 支持居中对齐标题。使用 Unicode 宽度感知的字符串截断。

4. 在 `ui/mod.rs` 注册模块。

---

### 4. ThemedBox — 主题感知容器 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。`border` 和 `background` 颜色直接在调用处硬编码。 |
| **目标状态** | 将主题 key 解析为边框/背景颜色的容器包装器。 |
| **TS 参考** | `ThemedBox.tsx` (105行) |
| **工作量** | 1 天 |
| **依赖** | ThemeProvider, color, Pane |

#### 实现步骤

1. 在 `components/themed_box.rs` 新建 `ThemedBox` 结构体：
   - 包装 `Pane`（层叠效果：仅边框/背景色解析层）。
   - 字段：`border_color`, `border_top_color`, `border_bottom_color`, etc. 为 `Option<&'static str>`。
   - `background_color: Option<&'static str>`。

2. 实现 `render()` 方法。返回 `Vec<Line>`。

3. 与 Pane 集成——当不指定显式边框时，退化为普通 Pane。

---

### 5. ThemedText — 主题感知文本 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 所有文本使用 `Span::raw()`，无限主题感知。 |
| **目标状态** | 解析主题 key 为文本颜色的 `Text` 包装器。支持 `dimColor`（兼容粗体）、`bold`、`italic`、`color`、`backgroundColor`。 |
| **TS 参考** | `ThemedText.tsx` (120行) |
| **工作量** | 1 天 |
| **依赖** | ThemeProvider, color |

#### 实现步骤

1. 在 `components/themed_text.rs` 新建 `ThemedText` 结构体：
   - `color: Option<&'static str>` — 主题 key 或原始颜色
   - `background_color: Option<&'static str>` — 主题 key
   - `dim_color: bool` — 使用 theme.inactive 淡色
   - `bold`, `italic`, `underline`, `strikethrough`, `inverse` 等

2. 实现 `render()` 方法，返回 `Vec<Span>` 或直接 `String` 加生成的 Style。

3. 与 KeyboardShortcutHint 集成——ShortcutHint 使用 ThemedText 做模式。

---

### 6. KeyboardShortcutHint — 键盘快捷键提示 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 42 行的 `ShortcutHint` + `render_shortcut_hints()`。输出为 `"Enter select \| Esc close"` 通过竖线分隔。 |
| **目标状态** | 支持结构化渲染：快捷键加粗、可选括号包裹、可配置动作文本。 |
| **TS 参考** | `KeyboardShortcutHint.tsx` (53行), `ConfigurableShortcutHint.tsx` (24行) |
| **工作量** | 0.5 天 |
| **依赖** | ThemedText |

#### 增强步骤

1. 升级 `ShortcutHint` 结构体：
   - 添加 `parens: bool`（是否用括号包裹）
   - 添加 `bold: bool`（快捷键文本加粗）

2. 升级 `render_shortcut_hints()`：
   - 接受 `Theme` 参数
   - 输出 `Vec<Span>` 而非纯字符串
   - 使用 Byline 风格的中间点分隔符

3. （可选）添加 `ConfigurableShortcutHint`，允许从用户配置的键绑定解析快捷键标签。

---

### 7. Byline — 页脚提示 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。多提示用 `" \| "` 硬编码分隔（如 `render_shortcut_hints()` 的 join 调用）。 |
| **目标状态** | 将多个键盘提示以中间点 ` · ` 连接的结构化组件。 |
| **TS 参考** | `Byline.tsx` (55行) |
| **工作量** | 0.5 天 |
| **依赖** | KeyboardShortcutHint |

#### 实现步骤

1. 在 `components/byline.rs` 新建 `Byline` 结构体：
   - 接受 `Vec<ShortcutHint>` 或 `Vec<Span>`。
   - 用 ` · ` 连接非空项。

2. 实现 `render()` 返回 `Vec<Span>`，使用 ThemedText 做 dim 样式。

3. 集成：替换 `render_shortcut_hints()` 的现有调用者（`approval_overlay.rs`、`better_view_panel.rs` 等）。

---

### 8. Pane — 面板容器 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。`BetterViewPanel` (187行) 是领域专用，非通用 Pane 组件。 |
| **目标状态** | 通用 Pane 组件，支持主题着色的顶部分隔线、水平填充、顶部间距，及模态框感知（模态框内跳过双重边框）。 |
| **TS 参考** | `Pane.tsx` (58行) + `Divider.tsx` |
| **工作量** | 1 天 |
| **依赖** | Divider, ThemeProvider, modalContext |

#### 实现步骤

1. 在 `components/pane.rs` 新建 `Pane` 结构体：
   - `color: Option<&'static str>` — 顶部分隔线颜色
   - `padding_x: usize`（默认 2）
   - `padding_top: usize`（默认 1）
   - `children: Vec<Vec<Line>>`

2. 实现 `render()` 方法：
   - 非模态框：Divider + 2px 水平填充 + 顶部间距
   - 模态框内：跳过 Divider，仅填充

3. 与 `BetterViewPanel` 的关系：Pane 是渲染层通用容器；`BetterViewPanel` 持续用于带有选中/详情的分割面板布局，其顶/底边框可改用 Pane。

---

### 9. Dialog — 通用对话框 (P0)

| 维度 | 内容 |
|------|------|
| **当前状态** | 仅有 `PermissionDialog`（357行，领域专用）。无通用对话框框架。 |
| **目标状态** | 通用 Dialog 组件，支持标题/副标题、Esc/n 取消、Ctrl+C/D 退出确认、输入指南、可隐藏边框。 |
| **TS 参考** | `Dialog.tsx` (88行) + 40+ 个 TS 对话框组件 |
| **工作量** | 2-3 天 |
| **依赖** | ThemeProvider, Pane, KeyboardShortcutHint, Byline, modalContext |

#### 实现步骤

1. **基础知识设施：叠加层栈**（0.5 天）
   - 在 `src/ui/overlays/` 新建 `overlay_stack.rs`：
     - `OverlayStack` 结构体，z-indexed 栈。
     - push/pop/peek 操作。
     - 活跃覆盖层的事件路由。
   - 在 `src/ui/overlays/mod.rs` 注册。
   - 集成到 `tui/` 主循环。

2. **modalContext 移植**（0.25 天）
   - `use_is_inside_modal()` — 在 Pane/Dialog 中查询是否在模态框内。
   - `use_modal_scroll_ref()` — 获取可滚动引用。

3. **Dialog 组件**（1 天）
   - 在 `src/ui/overlays/dialog.rs` 实现 `Dialog` 结构体：
     - `title`, `subtitle`, `children`
     - `on_cancel` 回调
     - `color: Option<&'static str>`（默认 `permission`）
     - `hide_input_guide`, `hide_border`
     - `is_cancel_active: bool`（当内嵌 TextInput 聚焦时禁用取消）

4. **退出快捷键**（0.5 天）
   - Ctrl+C/D：首次按下显示"再按一次退出"，第二次退出。
   - Esc/n：直接取消（当 `is_cancel_active` 为 true 时）。

5. **输入指南**（0.25 天）
   - 默认：Byline + KeyboardShortcutHint 显示 Enter/Esc。
   - 暂挂退出时：显示 `Press X again to exit`。
   - 可定制 `input_guide` 字段。

6. **适配现有 Dialog**
   - 将 `PermissionDialog` 迁移为使用通用 Dialog 组件。
   - 将 `HistorySearchDialog` 部分迁移。

7. **40+ 个 TS 对话框迁移**（参考）：
   TS 参考中约 40+ 个对话框（主要位于 `claude-code-bun/packages/@ant/ink/src` 及上游 `cc/src`）。优先实现高频使用的：
   - P0: `PermissionDialog`（已有，迁移）— 0.5 天
   - P1: `SettingsDialog`（/config 屏幕）— 1 天
   - P1: `CommandPaletteDialog` — 0.5 天
   - P2: 其余对话框（根据使用频率排序）

---

### 10. ListItem — 列表项 (P1)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。所有选择 UI 自定义渲染。 |
| **目标状态** | 通用列表项，支持焦点指示器（`❯`）、选择复选（`✓`）、滚动提示（`↑`/`↓`）、描述文本、禁用状态。 |
| **TS 参考** | `ListItem.tsx` (189行) |
| **工作量** | 1 天 |
| **依赖** | ThemeProvider, color |

#### 实现步骤

1. 在 `components/list_item.rs` 新建 `ListItem` 结构体：
   - `is_focused: bool` — 是否聚焦（显示 `❯`）
   - `is_selected: bool` — 是否选中（显示 `✓`）
   - `show_scroll_up`, `show_scroll_down` — 滚动提示
   - `styled: bool` — 自动应用状态颜色（默认为 true）
   - `disabled: bool` — 禁用状态
   - `description: Option<String>`
   - `children: Vec<Span>`

2. 实现 `render()` 方法：
   - 焦点指示器使用 `suggestion` 颜色。
   - 选中复选使用 `success` 颜色。
   - 禁用时 dim 文本且无指示器。
   - 描述文本使用 `inactive` 颜色，2px 左侧缩进。

3. 适配：
   - 替换 `feature_panels.rs` 中的选中行。
   - 替换 `resume_picker.rs` 中的会话列表渲染。
   - 替换 `history_search_dialog.rs` 中的列表行。

---

### 11. StatusIcon — 状态图标 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 33 行最小枚举：4 个变体（Ok/Warning/Error/Attention），纯文本标签（"ok"/"warn"/"error"/"attention"）。 |
| **目标状态** | 6 种语义状态（`success`, `error`, `warning`, `info`, `pending`, `loading`），Unicode 图标（`✓`, `✗`, `⚠`, `ℹ`, `○`, `…`），主题感知颜色。 |
| **TS 参考** | `StatusIcon.tsx` (69行) |
| **工作量** | 0.5 天 |
| **依赖** | ThemeProvider, color |

#### 增强步骤

1. 扩展 `StatusIcon` 枚举：
   ```rust
   pub enum StatusIcon {
       Success,
       Error,
       Warning,
       Info,
       Pending,
       Loading,
   }
   ```

2. 添加方法：
   - `fn icon(&self) -> &'static str` — Unicode 图标字符
   - `fn color_key(&self) -> Option<&'static str>` — 主题 key 或 None（dim 回退）
   - `fn render(&self, theme: &Theme, with_space: bool) -> Span`

3. 保持与 `StatusSeverity` 的兼容映射（`StatusSeverity::Ok` → `StatusIcon::Success` 等）。

4. 适配 `status_widget.rs`。

---

### 12. ProgressBar — 进度条 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 53 行的 `render_progress_bar()` 字符串函数，支持 Unicode ⅛ 块精度，但无主题颜色、独立 widget。 |
| **目标状态** | ratatui Widget 组件，支持填充/空白主题颜色、自动比例钳制、可配置宽度。 |
| **TS 参考** | `ProgressBar.tsx` (50行) |
| **工作量** | 0.5 天 |
| **依赖** | ThemeProvider, color |

#### 增强步骤

1. 在 `rendering/progress_bar.rs` 扩展为 `ProgressBar` 结构体：
   - `ratio: f64`（范围 0.0-1.0）
   - `width: usize`
   - `fill_color: Option<&'static str>`
   - `empty_color: Option<&'static str>`

2. 实现 `render(&self, theme: &Theme) -> Span`。

3. 将现有 `render_progress_bar()` 保留为底层函数，`ProgressBar::render()` 在其上包装颜色处理。

---

### 13. Tabs — 标签页 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 28 行最小函数：空格分隔的字符串，选中标签 `[selected]`，非选中 ` label `。无键盘处理、无样式、无颜色。 |
| **目标状态** | 完整的标签页系统：受控/非受控模式、焦点管理、键盘导航（←/→/Tab）、主题颜色、模态框滚动引用集成。 |
| **TS 参考** | `Tabs.tsx` (300 行+) |
| **工作量** | 2 天 |
| **依赖** | ThemeProvider, modalContext, color, Pane |

#### 实现步骤

1. 在 `components/tabs.rs` 重写 `Tabs` 结构体：
   - `tabs: Vec<Tab>` — 标签数据（id, title, children）
   - `selected: usize` — 当前选中
   - `color: Option<&'static str>` — 标签颜色
   - `use_full_width: bool` — 扩展至终端宽度
   - `content_height: Option<usize>` — 固定内容高度

2. 添加 `Tab` 结构体和 `TabContent` 渲染器——仅渲染选中的标签内容。

3. 添加 `TabsHeader`：
   - 标签水平排列，当前选中标签突出显示。
   - 高亮标签：使用 `color` 作为背景，`inverseText` 作为文本颜色。
   - 支持标题前缀（如 `"Config:" + 标签`）。

4. 键盘导航：
   - Header 聚焦时：←/→ 切换标签。
   - 通过 `selected` 属性和回调支持受控模式。

5. 可选：水平滚动溢出处理（参考 OpenTUI `TagTabs.tsx` 的 `planTagTabs()`）。

6. 集成到 `/config`, `/help`, `/permissions` 等命令界面。

---

### 14. LoadingState — 加载状态 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。各组件的加载状态内联处理。 |
| **目标状态** | Spinner + 加载文本组合。支持粗体标题、副标题、暗淡模式。 |
| **TS 参考** | `LoadingState.tsx` (67行), `Spinner.tsx` (20行) |
| **工作量** | 0.5 天 |
| **依赖** | ThemeProvider |

#### 实现步骤

1. 在 `components/loading_state.rs` 新建 `LoadingState` 结构体：
   - `message: String`
   - `bold: bool`
   - `dim_color: bool`
   - `subtitle: Option<String>`

2. 实现 `Spinner`（基于字符串，与 ratatui 兼容）：
   - 10 帧的 Braille spinner（`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`）。
   - 需要定时器或帧索引（在调用处管理）。
   - 对于纯文本输出，返回当前帧字符。

3. `LoadingState::render()` 返回 `Vec<Line>`，Spinner + 消息。

---

### 15. Ratchet — 进度指示器 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 不存在。 |
| **目标状态** | "棘轮"组件：确保内容区域高度不会缩小，只在展开时增长。用于内容高度变化时防止布局偏移的渐进式高度锁定。 |
| **TS 参考** | `Ratchet.tsx` (45行) |
| **工作量** | 1 天 |
| **依赖** | 无（纯布局辅助） |

#### 实现步骤

1. 在 `components/ratchet.rs` 新建 `Ratchet` 结构体：
   - `lock: RatchetLock` 枚举（`Always`, `Offscreen`）
   - 追踪最大内容高度。
   - 当内容高度 < 已记录的最大高度时，强制最小高度为最大高度。

2. 实现考虑因素：
   - 在 ratatui 中，这需要父级协调——Ratchet 不直接控制高度。
   - 实现为高度约束的包装器：`render()` 输出包含 `min_height` 信息的结构体。
   - 调用者（如 `Pane`）在布局计算中使用 `min_height`。

---

### 16. FuzzyPicker — 模糊搜索选择器 (P2)

| 维度 | 内容 |
|------|------|
| **当前状态** | 仅有 `fuzzy_match.rs`（123 行算法）。无 UI 组件。 |
| **目标状态** | 完整的模糊搜索选择器 UI：SearchBox + ListItem 列表 + Byline 快捷提示 + 可选预览面板。 |
| **TS 参考** | `FuzzyPicker.tsx` (312行) |
| **工作量** | 2-3 天 |
| **依赖** | SearchBox(已有), ListItem, Byline, KeyboardShortcutHint, Pane, color |

#### 实现步骤

1. 在 `components/fuzzy_picker.rs` 新建 `FuzzyPicker`（泛型）结构体：
   - 泛型参数 `T` 代表项类型。
   - `title: String`
   - `items: Vec<T>` — 候选列表（调用者已过滤）
   - `render_item: fn(&T, bool) -> Vec<Span>` — 每项渲染
   - `render_preview: Option<fn(&T) -> Vec<Line>>` — 可选的预览渲染器
   - `on_select: Box<dyn Fn(T)>` — 选择回调
   - `on_cancel: Box<dyn Fn()>` — 取消回调
   - `on_query_change: Box<dyn Fn(&str)>` — 搜索文本变化
   - `visible_count: usize`
   - `direction: PickerDirection`（Down/Up）
   - `match_label: Option<String>`
   - `empty_message: String`

2. 键盘处理：
   - Up/Down（或 Ctrl+P/Ctrl+N）：聚焦上/下。
   - Enter：选中当前项。
   - Tab/Option+Tab：次要操作（引用的 `mention` 等）。
   - Esc：取消。
   - 文本输入：更新查询并通知调用者。

3. 窗口滚动计算（参考 FuzzyPicker.tsx 的 `windowStart`/`visible` 逻辑）。

4. 渲染：
   - 顶部：Pane + 标题 + SearchBox。
   - 中间：ListItem 列表 + 可选预览。
   - 底部：Byline 快捷键提示。

5. 集成：
   - 命令面板（已有245行，可集成 FuzzyPicker）。
   - 历史搜索（已有516行 `history_search_dialog.rs`，可迁移或继承 FuzzyPicker）。
   - `/config` 中的选项选择器。

---

## 工作量汇总

| # | 组件 | 优先级 | 工作量 | 依赖 | 类型 |
|---|------|--------|--------|------|------|
| 01 | ThemeProvider (含 theme定义) | P0 | 3-4 天 | 无 | 基础设施 |
| 02 | color (解析工具) | P0 | 0.5 天 | 01 | 基础设施 |
| 03 | Dialog (通用 + 叠加层栈) | P0 | 2-3 天 | 01, 02, 04, 06, 07, 08 | 复合 |
| 04 | Divider | P1 | 0.5 天 | 01, 02 | 原子 |
| 05 | ThemedBox | P1 | 1 天 | 01, 02, 08 | 原子 |
| 06 | ThemedText | P1 | 1 天 | 01, 02 | 原子 |
| 07 | KeyboardShortcutHint (增强) | P1 | 0.5 天 | 06 | 原子 |
| 08 | Byline | P1 | 0.5 天 | 07 | 原子 |
| 09 | Pane | P1 | 1 天 | 04, 01 | 原子 |
| 10 | ListItem | P1 | 1 天 | 01, 02 | 原子 |
| 11 | StatusIcon (增强) | P2 | 0.5 天 | 01, 02 | 原子 |
| 12 | ProgressBar (增强) | P2 | 0.5 天 | 01, 02 | 原子 |
| 13 | Tabs (重写) | P2 | 2 天 | 01, modal, 02 | 复合 |
| 14 | LoadingState | P2 | 0.5 天 | 01 | 原子 |
| 15 | Ratchet | P2 | 1 天 | 无 | 工具 |
| 16 | FuzzyPicker | P2 | 2-3 天 | 10, 08, 07, 09, SearchBox | 复合 |

**总计**: 17-21 天（约 4-5 周单人全时）

---

## 推荐执行顺序

### 冲刺 1 (Week 1-2): 基础设施 + 高频组件

```
Day 1-4:   ThemeProvider + theme 定义 + color 工具    [01, 02]
Day 5:     Divider                                     [04]
Day 6-7:   ThemedText + KeyboardShortcutHint + Byline   [06, 07, 08]
Day 8-9:   Pane + ThemedBox                             [09, 05]
Day 10:    叠加层栈基础设施                              [03 sub]
```

### 冲刺 2 (Week 3-4): 复合组件 + 核心增强

```
Day 11-12: Dialog (通用)                                [03]
Day 13:    ListItem                                     [10]
Day 14-15: Tabs (重写)                                  [13]
Day 16:    StatusIcon + ProgressBar (增强)              [11, 12]
Day 17:    LoadingState + Ratchet                       [14, 15]
```

### 冲刺 3 (Week 5): 复合选择器 + 集成

```
Day 18-20: FuzzyPicker                                  [16]
Day 21:    集成 + 测试                                    [迁移]
```

---

## 集成检查清单

- [ ] ThemeProvider 接入 `App` 结构体，所有渲染器传递 `theme` 引用
- [ ] Divider/Byline 替换现有硬编码分隔符 (`" | "`, `"\u{2500}".repeat()`)
- [ ] ListItem 替换 `feature_panels.rs`、`resume_picker.rs`、`history_search_dialog.rs` 的选中行
- [ ] StatusIcon 替换 `status_widget.rs` 中的纯文本标签
- [ ] ProgressBar 替换 `task_status_utils.rs` 中的直接 `render_progress_bar()` 调用
- [ ] Dialog 集成 PermissionDialog（现有 357 行）
- [ ] Tabs 替换 BetterViewPanel 后的标签化命令屏幕
- [ ] FuzzyPicker 集成命令面板和历史搜索
- [ ] 移除 `#[allow(dead_code)]` 从已接线组件中
- [ ] 测试覆盖：所有组件至少有一个 snapshot 测试

---

## 参考文件索引

| 组件 | TS 参考路径 |
|------|-------------|
| Dialog | `claude-code-bun/packages/@ant/ink/src/theme/Dialog.tsx` |
| Divider | `claude-code-bun/packages/@ant/ink/src/theme/Divider.tsx` |
| Pane | `claude-code-bun/packages/@ant/ink/src/theme/Pane.tsx` |
| ThemedBox | `claude-code-bun/packages/@ant/ink/src/theme/ThemedBox.tsx` |
| ThemedText | `claude-code-bun/packages/@ant/ink/src/theme/ThemedText.tsx` |
| ThemeProvider | `claude-code-bun/packages/@ant/ink/src/theme/ThemeProvider.tsx` |
| KeyboardShortcutHint | `claude-code-bun/packages/@ant/ink/src/theme/KeyboardShortcutHint.tsx` |
| ListItem | `claude-code-bun/packages/@ant/ink/src/theme/ListItem.tsx` |
| ProgressBar | `claude-code-bun/packages/@ant/ink/src/theme/ProgressBar.tsx` |
| StatusIcon | `claude-code-bun/packages/@ant/ink/src/theme/StatusIcon.tsx` |
| Tabs | `claude-code-bun/packages/@ant/ink/src/theme/Tabs.tsx` |
| FuzzyPicker | `claude-code-bun/packages/@ant/ink/src/theme/FuzzyPicker.tsx` |
| Byline | `claude-code-bun/packages/@ant/ink/src/theme/Byline.tsx` |
| LoadingState | `claude-code-bun/packages/@ant/ink/src/theme/LoadingState.tsx` |
| Ratchet | `claude-code-bun/packages/@ant/ink/src/theme/Ratchet.tsx` |
| Spinner | `claude-code-bun/packages/@ant/ink/src/theme/Spinner.tsx` |
| color | `claude-code-bun/packages/@ant/ink/src/theme/color.ts` |
| theme-types | `claude-code-bun/packages/@ant/ink/src/theme/theme-types.ts` |
| modalContext | `claude-code-bun/packages/@ant/ink/src/theme/modalContext.ts` |

| 组件 | Rust 现有路径 |
|------|---------------|
| status_icon | `crates/claude-code-rs/src/ui/components/status_icon.rs` |
| keyboard_shortcut | `crates/claude-code-rs/src/ui/components/keyboard_shortcut.rs` |
| tabs | `crates/claude-code-rs/src/ui/components/tabs.rs` |
| fuzzy_match | `crates/claude-code-rs/src/ui/components/fuzzy_match.rs` |
| progress_bar | `crates/claude-code-rs/src/ui/rendering/progress_bar.rs` |
| approval_overlay | `crates/claude-code-rs/src/ui/components/approval_overlay.rs` |
| better_view_panel | `crates/claude-code-rs/src/ui/components/better_view_panel.rs` |
| history_search_dialog | `crates/claude-code-rs/src/ui/components/history_search_dialog.rs` |
| feature_panels | `crates/claude-code-rs/src/ui/components/feature_panels.rs` |
| search_box | `crates/claude-code-rs/src/ui/components/search_box.rs` |
