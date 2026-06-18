# 组件对等性对比：Rust vs TypeScript UI

**日期**：2026-05-18
**范围**：Ratatui 后端（`rust/crates/claude-code-rs/src/ui/`）、TypeScript 设计系统（`src/components/design-system/`）以及下游 Ink 主题组件（`claude-code-bun/packages/@ant/ink/src/theme/`）

---

## 汇总表

| 组件 | Rust ratatui | TS design-system | @ant/ink theme | 评级 |
|-----------|:----------:|:--------------:|:-------------:|:------:|
| Dialog | 部分 | 完整 | 完整 | 4/5 |
| Divider | 完整 | 完整 | 完整 | 5/5 |
| Pane | 部分 | 完整 | 完整 | 4/5 |
| ThemedBox | 部分 | 完整 | 完整 | 4/5 |
| ThemedText | 部分 | 完整 | 完整 | 4/5 |
| ThemeProvider | 部分 | 完整 | 完整 | 3/5 |
| KeyboardShortcutHint | 部分 | 完整 | 完整 | 4/5 |
| ListItem | 部分 | 完整 | 完整 | 4/5 |
| ProgressBar | 完整 | 完整 | 完整 | 5/5 |
| Byline | 部分 | 完整 | 完整 | 4/5 |
| LoadingState | 部分 | 完整 | 完整 | 4/5 |
| FuzzyPicker | 部分 | 完整 | 完整 | 4/5 |
| Ratchet | 部分 | 完整 | 完整 | 4/5 |
| StatusIcon | 完整 | 完整 | 完整 | 5/5 |
| Tabs | 部分 | 完整 | 完整 | 3/5 |
| Spinner | 缺失 | 缺失 | 完整 | 不适用 |
| SearchBox | 4/5 | 缺失 | 完整 | 不适用 |
| 欢迎屏幕 | 4/5 | 缺失 | 缺失 | 不适用 |
| 状态组件/状态行 | 3/5 | 缺失 | 缺失 | 不适用 |
| 批准/权限 UI | 3/5 | 缺失 | 缺失 | 不适用 |
| 底部面板堆栈 | 1/5 | 缺失 | 缺失 | 不适用 |
| 聊天组件 | 2/5 | 缺失 | 缺失 | 不适用 |
| 分页器/滚动覆盖层 | 4/5 | 缺失 | 缺失 | 不适用 |
| 功能面板 | 2/5 | 缺失 | 缺失 | 不适用 |
| 会话恢复选择器 | 2/5 | 缺失 | 缺失 | 不适用 |

**图例**：1=桩/缺失，2=部分状态，3=功能可用但有限，4=接近完整，5=功能完整

**补齐状态**：第一轮 Rust ratatui 设计系统原语已落地。剩余差距集中在配置主题名映射、Dialog overlay 事件派发和 Tabs 完整交互契约，已记录到 [`../../KNOWN_ISSUES.md`](../../KNOWN_ISSUES.md) 的 `UI-006`、`UI-007`、`UI-008`。

---

## 第1章：设计系统原语

这些组件构成了 TypeScript 代码库中的原子 UI 词汇。它们是 Rust 实现中最基础的缺口。

### 组件：Dialog

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/overlays/dialog.rs`
**TS design-system 路径**：`src/components/design-system/Dialog.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
Rust 现在有通用 Dialog 组件，覆盖标题/副标题、主题色 accent、可选边框、输入指南、`Pane` 包装、Esc/n 取消，以及 Ctrl+C/Ctrl+D 连续触发保护。`Dialog::handle_key()` 已提供组件级按键路由 helper。

#### 剩余差距
- `OverlayStack` 仍主要保存 overlay metadata，尚未集中持有具体 Dialog 并派发真实 key events。
- 真实权限/确认弹窗路径还需要接入 `DialogEvent::Cancel` / `DialogEvent::Exit` 后才能声明 runtime parity。
- 详见 [`../../KNOWN_ISSUES.md`](../../KNOWN_ISSUES.md) `UI-007`。

#### 影响
组件层容器已补齐，但调用方还不能默认获得统一的 Dialog 事件语义。

---

### 组件：Divider

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/components/divider.rs`
**TS design-system 路径**：`src/components/design-system/Divider.tsx`
**Rust 完整度**：5/5

#### Rust 实现状态
Rust 现在有共享 Divider 组件，支持主题颜色 key、自定义重复字符、宽度、左右 padding、标题居中和窄宽度回退，并有单元测试覆盖。

#### 剩余差距
当前没有独立开放差距；后续只需要在更多调用方替换手写分隔线。

#### 影响
共享视觉分隔符已经可用，后续收益取决于调用方迁移覆盖率。

---

### 组件：Pane

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/components/pane.rs`
**TS design-system 路径**：`src/components/design-system/Pane.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
Rust 现在有通用 Pane 组件，委托 Divider 渲染顶部边界，支持主题色、水平 padding、顶部 padding、modal 内部跳过分隔线，并作为 Dialog/ThemedBox 的共享容器基础。

#### 剩余差距
- ratatui 没有 Ink/React 的 `flexShrink` 布局模型；高度稳定性需要由调用方布局约束保证。
- 现有 `/config`、`/permissions` 等调用方还未全部迁移到 Pane。

#### 影响
共享容器已存在，但斜杠命令屏幕仍需要逐步迁移到它，才能获得一致的彩色顶部边框和填充。

---

### 组件：ThemeProvider / ThemedBox / ThemedText

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/theme/`、`crates/claude-code-rs/src/ui/components/themed_box.rs`、`crates/claude-code-rs/src/ui/components/themed_text.rs`
**TS design-system 路径**：`src/components/design-system/ThemeProvider.tsx`、`ThemedBox.tsx`、`ThemedText.tsx`
**Rust 完整度**：3/5

#### Rust 实现状态
Rust 现在有 design theme 层：`ThemeName`、`ThemeSetting::Auto`、`ThemeProvider`、完整 `ThemeColors` 表、`resolve_color()`、`dim_style()`、settings 读写 helper，以及 `ThemedBox` / `ThemedText`。`App` 会从用户 settings 初始化主题，TUI 会按 `app_state.settings.theme` 同步当前主题。

#### 剩余差距
- `auto` 当前基于 `$COLORFGBG` 推断，没有 OSC 11 实时监视器。
- `/config theme` 允许的 `solarized`、`monokai`、`nord` 等主题名尚未映射到 design theme palette，未知值会 fallback。详见 [`../../KNOWN_ISSUES.md`](../../KNOWN_ISSUES.md) `UI-006`。
- 主题预览/取消工作流还未形成完整交互。
- 旧组件中的硬编码颜色还需要逐步迁移到 `ThemeProvider`。

#### 影响
主题基础设施已经存在，但调用方迁移和完整主题名覆盖尚未完成。

---

### 组件：KeyboardShortcutHint

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/components/keyboard_shortcut.rs`
**TS design-system 路径**：`src/components/design-system/KeyboardShortcutHint.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
Rust 现在有 `ShortcutHint`、styled hint 渲染、legacy string 渲染和 byline 风格的多提示组合。Dialog 已复用该组件渲染输入指南。

#### 剩余差距
- 尚未统一读取用户自定义 keybindings 来生成所有调用方的提示文案。
- 旧页脚字符串还需要逐步替换为 `ShortcutHint`。

#### 影响
结构化提示组件已可用，但旧调用方迁移前仍可能出现不一致提示。

---

### 组件：ListItem

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/components/list_item.rs`
**TS design-system 路径**：`src/components/design-system/ListItem.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
Rust 现在有通用 ListItem 组件，支持 focused/selected/disabled 状态、pointer、check marker、description、styled mode 和主题颜色。

#### 剩余差距
- 截断列表的上/下滚动指示器仍由具体 picker/list 控制。
- `declareCursor` 属于 Ink/React 可访问性语义，ratatui 端尚无等价抽象。
- 功能面板、恢复选择器等旧列表还需要迁移到共享 ListItem。

#### 影响
共享列表项视觉语言已存在，但调用方迁移仍未完成。

---

### 组件：ProgressBar

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/rendering/progress_bar.rs`
**TS design-system 路径**：`src/components/design-system/ProgressBar.tsx`
**Rust 完整度**：5/5

#### Rust 实现状态
Rust 现在有共享 `ProgressBar` widget 和低层 `render_progress_bar()`。它支持 Unicode ⅛ 块精度、ratio clamp、零宽处理、主题填充/空白颜色，以及自定义 `fill_color` / `empty_color`。

#### 剩余差距
当前没有独立开放差距；任务状态调用方已可通过 `progress_bar_styled()` 复用 styled Line。

#### 影响
进度条组件 parity 已收口。

---

### 组件：Byline / LoadingState / FuzzyPicker / Ratchet

**Rust ratatui 路径**：`crates/claude-code-rs/src/ui/components/keyboard_shortcut.rs`、`loading_state.rs`、`fuzzy_picker.rs`、`ratchet.rs`
**TS design-system 路径**：`src/components/design-system/Byline.tsx`、`LoadingState.tsx`、`FuzzyPicker.tsx`、`Ratchet.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
- **Byline**：由 `ShortcutHint` / `render_hints_styled()` 提供共享键盘提示行，支持多提示分隔、粗体快捷键和 styled span 输出。
- **LoadingState**：提供 Braille spinner、可选粗体/暗色文本、subtitle、frame 管理和 snapshot 覆盖。
- **FuzzyPicker**：组合 `SearchBox`、`ListItem`、Byline 和 preview，支持上下方向布局、empty message、match label、visible window 和 item focus。
- **Ratchet**：实现最大高度锁定，支持 `Always` / `Offscreen` 模式、visible 状态和 reset，避免内容高度收缩导致布局跳动。

#### 剩余差距
- 这些组件仍主要是纯渲染原语，过滤、选中、事件派发和 tick 调度由调用方持有。
- 旧 picker/loading/list 调用方还需要逐步迁移到共享组件。

#### 影响
设计系统的中层组件已经可复用，剩余工作主要是调用方接线和交互所有权收口。

---

## 第2章：状态与图标组件

### 组件：StatusIcon

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/status_icon.rs`
**TS design-system 路径**：`src/components/design-system/StatusIcon.tsx`
**@ant/ink theme 路径**：`claude-code-bun/packages/@ant/ink/src/theme/StatusIcon.tsx`
**Rust 完整度**：5/5

#### Rust 实现状态
- **Ratatui**：`status_icon.rs` 现在提供 `Success`、`Error`、`Warning`、`Info`、`Pending`、`Loading` 六种语义状态，渲染 Unicode 图标，使用主题色，支持 `with_space`，并保留 legacy label / legacy severity mapping。

#### 剩余差距
当前没有独立开放差距；后续只需要把旧状态文本调用方迁移到 `StatusIcon::render()`。

#### 影响
状态图标组件 parity 已收口。

---

## 第3章：标签页与导航

### 组件：Tabs

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/tabs.rs`
**TS design-system 路径**：`src/components/design-system/Tabs.tsx`
**@ant/ink theme 路径**：`claude-code-bun/packages/@ant/ink/src/theme/Tabs.tsx`
**Rust 完整度**：3/5

#### Rust 实现状态
- **Ratatui**：`tabs.rs` 现在有 `Tab` / `Tabs` 结构，支持 selected state、next/previous wrap、主题色 header、full-width header、`content_height` 字段和 legacy `render_tabs()` 兼容函数。
#### 缺失功能（对比 TS design-system）
- 标签页头部焦点状态管理（`headerFocused` / `blurHeader` / `focusHeader`）
- 子组件协调键盘交互的 `useTabHeaderFocus()` 钩子
- 受控模式（`selectedTab` + `onTabChange` 属性）
- 用于 `tabs:next` / `tabs:previous` 动作的 `useKeybindings` 集成
- 模态框滚动引用集成（`useModalScrollRef`）
- 固定高度标签内容的 `contentHeight` 属性（防止布局偏移）
- 从聚焦内容使用 Tab/Left/Right 导航的 `navFromContent` 属性
- 标签头部下方附加内容的 `banner` 属性
- 终端宽度感知内容区域的 `useFullWidth`
- 与子箭头键处理器冲突解决的 `disableNavigation` 属性
- 完整内容 panes 渲染和调用方级 keyboard action 集成

详见 [`../../KNOWN_ISSUES.md`](../../KNOWN_ISSUES.md) `UI-008`。

#### 影响
ratatui 后端已有标签头部组件，但缺乏斜杠命令屏幕（如 `/config`、`/help` 和 `/permissions`）所需的完整交互式标签面板语义（焦点管理、键盘导航、模态框协调）。

---

## 第4章：欢迎屏幕与状态

### 组件：欢迎屏幕

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/welcome.rs`
**TS design-system 路径**：缺失（无专用组件——嵌入在 `LogoV2/` 中）
**Rust 完整度**：4/5

#### Rust 实现状态
- **Ratatui**：功能完整的欢迎面板渲染器，具有：
  - 带边框面板和强调色标题
  - 版本、模型、会话 ID（截断）、CWD 显示
  - 工具提示行（"Enter to send, /help for commands"）
  - 窄终端紧凑模式（单行回退）
  - 用于布局计算的 `welcome_height_for()` 集成
  - Unicode 安全字符串截断
  - 全面的测试覆盖率（6 个测试）
  - 无 ASCII 标志（出于紧凑性的设计选择）

#### 缺失功能
- 主题感知颜色（目前在 ratatui 中硬编码为 RGB 常量）
- 动画标志（见于 TS `LogoV2/` 组件）
- 基于用户上下文的提示/入门建议
- 键盘快捷键提示集成
- 来自最近历史的会话恢复建议

#### 影响
Rust 欢迎屏幕是功能性的，与上游欢迎显示相当。主题和动画方面存在微小差距。

---

### 组件：状态组件 / 状态行

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/status_widget.rs`
**TS design-system 路径**：缺失（嵌入在应用级组件中）
**Rust 完整度**：3/5

#### Rust 实现状态
- **Ratatui**：基于文本的状态行，具有：
  - `StatusSnapshot` 数据模型：模型、CWD、权限模式、沙箱、费用、运行中的工具、活动代理、子系统状态
  - 带有标签/值/严重度三元组的 `StatusIndicator`，用于可扩展指示器
  - `render_line()` 单行紧凑格式（竖线分隔）
  - `render_details()` 多行展开格式
  - 与 `StatusIcon` 的严重度标签集成
  - 良好的测试覆盖率，使用 `insta` 快照

#### 缺失功能（ratatui）
- 无实际的终端渲染——仅为下游显示生成字符串
- 输出中无颜色/样式（全部为纯文本）
- 详情视图无 `BetterViewPanel` 集成
- 不支持自定义状态行命令

#### 影响
ratatui 状态组件提供了坚实的数据模型，但无法直接渲染带样式的输出。

---

## 第5章：权限与批准 UI

### 组件：批准覆盖层

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/approval_overlay.rs`
**TS design-system 路径**：嵌入在 `src/components/permissions/` 中
**Rust 完整度**：3/5

#### Rust 实现状态
- **Ratatui**：结构良好的批准覆盖层，具有：
  - `ApprovalKind` 枚举，覆盖 Bash、FileEdit、WebFetch、MCP、UserInput、Fallback
  - `ApprovalChoice` 枚举：AllowOnce、AllowAlways、Deny、EditRequest
  - 通过 `BetterViewPanel` 的基于网格的布局，包含请求详情、决策列表和页脚
  - 风险分类（`approval_risk()` 返回人类可读的风险标签）
  - 默认 `fail_closed`（出错时拒绝）
  - 长命令字符串的 `fit_line()` 截断

#### 缺失功能（ratatui）
- EditRequest 支持（变体存在但与 Deny 没有视觉区别）
- 默认选项中未包含 `Always Allow` 选择
- 无文件编辑差异的视觉预览
- 待处理状态无动画指示器
- 无内联显示的键盘热键字母

#### 影响
ratatui 批准覆盖层覆盖了基本批准流程，但仍缺少若干完整交互细节。

---

## 第6章：输入与搜索

### 组件：SearchBox

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/search_box.rs`
**@ant/ink theme 路径**：`claude-code-bun/packages/@ant/ink/src/theme/SearchBox.tsx`
**Rust 完整度**：4/5

#### Rust 实现状态
ratatui 后端的实现出人意料地完善：
- 构建器模式，流畅 API（`.placeholder()`、`.focused()`、`.prefix()`、`.cursor_offset()`、`.borderless()`、`.width()`）
- 在正确偏移处插入可视光标字符（`|`）
- 通过 `UnicodeWidthChar` 的 Unicode 感知宽度截断
- `is_terminal_focused` 状态区分终端/失焦光标与输入聚焦光标
- 查询为空时显示占位符文本
- 使用 `insta` 快照的相当全面的测试覆盖率（5 个场景：聚焦带占位符、聚焦带光标、失焦、无边框、宽度截断）

#### 缺失功能
- 主题感知边框颜色（目前默认为 ANSI 样式）
- 加载状态无 `Spinner` 集成
- 无历史搜索集成
- 无文件路径自动完成支持
- 无 `/` 斜杠命令补全提示

#### 影响
SearchBox 是 ratatui 后端中最完整的组件之一。对于基本搜索/过滤操作是功能性的。

---

## 第7章：分页器与覆盖层

### 组件：分页器覆盖层

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/pager_overlay.rs`
**Rust 完整度**：4/5

#### Rust 实现状态
实现完善的可滚动分页器：
- `StaticOverlay` 用于静态文本内容，具有完整的键盘导航：
  - Up/Down/k/j 单行滚动
  - PageUp/PageDown/space 翻页滚动
  - Home/End 跳转
  - Esc/q 关闭
- 包装 `StaticOverlay` 的 `TranscriptOverlay`，用于聊天记录显示
- 调度到任一变体的 `Overlay` 枚举
- 滚动边界保护
- 用于视口切片的 `render_offset_content()` 工具函数
- PageDown 行为的测试覆盖

#### 缺失功能
- 分页内容内无搜索/查找
- 无进度指示器（"第 45/200 行"）
- 无鼠标滚轮支持
- 宽内容无水平滚动
- ratatui `Line` 显示中无 ANSI 转义渲染

#### 影响
一个坚实、功能完善的分页器。与 `less` 风格的基本功能相当。缺少搜索和进度显示。

---

## 第8章：功能面板

### 组件：功能面板

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/feature_panels.rs`
**TS design-system 路径**：嵌入在斜杠命令屏幕中
**Rust 完整度**：2/5

#### Rust 实现状态
- **Ratatui**：功能面板导航的标签化表单状态：
  - `FeaturePanelKind` 枚举，包含 9 种面板类型（MCP、Agents、Teams、LSP、Settings、Sandbox、Plugins、Skills、Tasks）
  - `PanelState`：Ready、NeedsConfig、Running、Error
  - 带有选项和描述的 `FormTab` 系统
  - 徽章计数显示
  - 带有 `TabbedFormEvent` 响应的按键事件处理
  - 用于扁平文本面板列表的 `render_panel_index()`

#### 缺失功能（ratatui）
- 无视觉渲染——一切通过 `render_lines()` 以纯文本形式呈现
- 无内联状态指示器（彩色圆点、状态徽章）
- 无配置操作表单
- 无连接状态指示器（在线/离线/错误）
- 无可滚动面板内容

#### 影响
ratatui 功能面板仅是一个状态管理层，没有统一的标签化面板导航。

---

## 第9章：聊天与编辑器

### 组件：聊天组件

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/chatwidget.rs`
**Rust 完整度**：4/5

#### Rust 实现状态
- **Ratatui**：包装 `App` 结构的薄适配器：
  - 用于初始化参数的 `ChatWidgetInit`（模型、后端、会话、CWD、语音设置）
  - 委托调用的 `add_message()`、`messages()`、`clear_messages()`、`set_streaming()`
  - 转发到 `App` 的 `handle_key_event()`
  - 用于 App 生命周期管理的 `from_app()` / `into_app()`

#### 缺失功能（ratatui）
- ratatui 的 `ChatWidget` 不是 UI 组件——它是一个状态管理边界
- 没有消息、编辑器或对话历史的实际渲染
- 所有聊天渲染必须通过下游的 `App` 方法进行

#### 影响
ratatui 聊天组件的范围正确，作为一个适配器。

---

## 第10章：会话与恢复

### 组件：恢复选择器

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/resume_picker.rs`
**Rust 完整度**：2/5

#### Rust 实现状态
会话恢复的数据模型：
- 包含 session_id、title、CWD、last_modified、message_count 的 `SessionTarget`
- 按最新优先排序的会话列表的 `ResumePicker`
- `SessionPickerAction` 枚举：MoveUp/Down、PageUp/Down、Select、Cancel
- `apply()` 方法返回 `Option<SessionSelection>`（终端动作返回 Some）
- 与 `cc_session::storage::list_sessions()` 集成的 `load_resume_targets()`

#### 缺失功能
- 无视觉渲染
- 无搜索/过滤
- 无删除会话功能
- 无会话预览（最后一条消息片段）
- 无日期格式化（"2 小时前"）
- 无键盘提示页脚
- 长列表无滚动指示器

#### 影响
一个正确但最小的数据模型。没有用户可交互的实际 UI。

---

## 第11章：底部面板

### 组件：底部面板

**Rust ratatui 路径**：`rust/crates/claude-code-rs/src/ui/components/bottom_pane.rs`
**Rust 完整度**：1/5

#### Rust 实现状态
视图堆栈状态机：
- `BottomPaneView` 枚举：Composer、Approval、Selection、Status
- `name()` 字符串标签
- 基于堆栈的 push/pop 导航
- `focused_view()` 查看堆栈顶部
- `render_lines()` 生成文本字符串（非渲染）

#### 缺失功能
- 无实际终端渲染
- 视图之间无视觉分隔
- 视图切换无动画
- 无高度计算
- 无聊天编辑器渲染集成
- 视图之间无焦点管理

#### 影响
底部面板是一个概念草图——仅有状态管理而无 UI。实际底部栏布局必须在其他地方实现。

---

## 主要发现

### 1. 根本差距：Rust 设计系统已建立，但调用方迁移未完成

TypeScript 代码库有 16 个可复用的设计系统组件（`Dialog`、`Divider`、`Pane`、`ThemedBox`、`ThemedText`、`ThemeProvider`、`KeyboardShortcutHint`、`ListItem`、`ProgressBar`、`StatusIcon`、`Tabs`、`Byline`、`Ratchet`、`FuzzyPicker`、`LoadingState`、`color`），具有一致的主题、键盘处理和无障碍支持。Rust 代码库现在已经补齐第一轮共享组件和主题原语，但大量调用方仍停留在旧的手写渲染、字符串页脚或局部状态模型上。

### 2. Ratatui 后端：状态重、渲染轻

ratatui 组件（`crates/claude-code-rs/src/ui/components/`）过去主要是状态管理层；本轮新增的 design-system 组件已经开始提供 styled `Line` / `Span` 渲染。但一些核心界面（功能面板、恢复选择器、底部面板）仍然偏状态模型，尚未全面切到共享渲染原语。

### 3. 主题系统接线是影响最大的剩余缺口

运行时主题系统已经存在，但完整 parity 还需要：
- 补齐 `/config theme` 暴露主题名到 design palette 的映射。
- 把旧组件中的硬编码颜色迁移到 `ThemeProvider`。
- 用 OSC 11 或等价机制补齐实时终端背景检测。
- 为主题预览/取消工作流补调用方交互。

### 4. Rust 中存在但 TypeScript 中不存在的组件

一些组件仅存在于 Rust 代码库中：
- `SearchBox`（ratatui）：构建器模式的搜索框，带光标定位
- `PagerOverlay`（ratatui）：可滚动文本分页器，带键盘导航
这些代表 Rust 优先的特性，可以为 TypeScript 的改进提供参考。

### 5. 测试覆盖率的差异

ratatui 组件有合理的测试覆盖率。新增 design-system 组件覆盖了 Divider、ListItem、KeyboardShortcutHint、LoadingState、FuzzyPicker、Ratchet、StatusIcon、ProgressBar、ThemeProvider、Dialog 等单元或 snapshot 测试；旧组件仍以 `welcome.rs`、`search_box.rs`、`status_widget.rs` 等为主要覆盖点。
