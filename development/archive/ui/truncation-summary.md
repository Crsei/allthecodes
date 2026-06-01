# UI 面板截断 / 省略展示汇总

> 记录时间：2026-05-22  
> 来源分析：Rust TUI (claude-code-rs/src/ui/) 代码审计

本文档列出所有 UI 面板中因空间限制而截断/省略展示的内容，按渲染位置分类。

---

## 一、对话记录（消息区域，内联展示）

这些截断发生在主消息流中，直接可见于聊天历史。

### 1.1 助手思考预览

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/assistant_thinking_message.rs:37` |
| 截断方式 | `.chars().take(120)`，仅显示前 120 字符 |
| 展示位置 | 助手消息的思考过程块内 |

### 1.2 助手文本（API 错误）

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/assistant_text_message.rs:166-168` |
| 截断方式 | 手动 `&s[..997]` + `"..."`，上限 1000 字符 |
| 展示位置 | API 错误文本块内 |

### 1.3 工具调用结果

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/render.rs:1851-1871` |
| 截断方式 | `.take(5)`，仅显示前 5 行，剩余显示 `"... {} more lines"` |
| 展示位置 | 工具返回内容块内 |

### 1.4 工具输入 JSON

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/render.rs:2120-2141` |
| 截断方式 | `abbreviate_json()` — 递归截断至 `max_chars`，末尾加 `...` |
| 展示位置 | 工具调用行摘要中（如 `"● Bash(cargo test)"` 旁） |

### 1.5 Bash 输出行

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/user_bash_output_message.rs:71, 160-176` |
| 截断方式 | `truncate_to_width()` — 按 unicode 显示宽度 silent 截断，不加省略号 |
| 展示位置 | bash 输出消息块内 |

### 1.6 附件文件

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/attachment_message.rs:19, 359-364` |
| 截断方式 | 显示 `(truncated)` 标记 |
| 展示位置 | 文件附件块内 |

### 1.7 工具活动摘要

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/rendering/tool_activity.rs:311, 328, 334` |
| 截断方式 | 工具输入摘要截断至 96 字符；JSON 值截断至 48 字符 |
| 展示位置 | 助手消息中合并后的工具活动行 |

### 1.8 欢迎界面（消息区域空状态）

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/components/welcome.rs:62-63, 107-135` |
| 截断方式 | CWD 路径从开头截断（`truncate_start`）；提示文字末尾截断（`truncate_str`） |
| 展示位置 | 无对话时覆盖消息区域 |

### 1.9 转录模式消息限制

| 项目 | 说明 |
|------|------|
| 文件 | `crates/claude-code-rs/src/ui/messages/render.rs:645-658` |
| 截断方式 | `truncate_transcript_messages()` — 仅保留最近 30 条消息 |
| 展示位置 | Transcript view 模式下（`Ctrl+O` 切换） |

---

## 二、专门面板（覆盖层 / 独立视图）

这些截断发生在用户主动打开的面板或系统自动弹出的对话框中。

### 2.1 Diff 面板

| 项目 | 说明 |
|------|------|
| 触发 | `/diff` 命令 |
| 文件 | `crates/claude-code-rs/src/ui/diff/diff_detail_view.rs:10, 23, 34-35, 69-70` |
| 截断方式 | 超过 400 行截断并显示 `(diff truncated (exceeded 400 line limit))`；每行按宽度 silent 截断 |
| 文件 | `crates/claude-code-rs/src/ui/diff/diff_file_list.rs:41-45, 68` |
| 截断方式 | 文件路径从开头截断（`truncate_start_to_width`）；截断文件显示 `(truncated)` |
| 文件 | `crates/claude-code-rs/src/ui/diff/structured_diff.rs:257-476` |
| 截断方式 | 样式 spans 按显示宽度截断；超出 max_lines 的行丢弃 |

### 2.2 命令面板

| 项目 | 说明 |
|------|------|
| 触发 | `/` 快捷键 |
| 文件 | `crates/claude-code-rs/src/ui/command_palette/render.rs:85, 110, 210, 220, 228, 297-306, 346` |
| 截断方式 | 命令描述、标签、用法文本、示例均按可用宽度截断（`truncate()` + `...`） |

### 2.3 权限对话框

| 项目 | 说明 |
|------|------|
| 触发 | 工具需用户审批时自动弹出 |
| 文件 | `crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs:253-326, 794, 924-935` |
| 截断方式 | 反馈文本、提示文本、正文行、警告 spans、按钮文字均用 `truncate_str()` 截断 |
| 文件 | `crates/claude-code-rs/src/ui/permissions/utils.rs:176, 185, 269-286, 305, 331` |
| 截断方式 | 请求摘要截断至 140 字符（`truncate_middle` head...tail）；路径截断至 140 字符；每行截断至 180 字符 |
| 文件 | `crates/claude-code-rs/src/ui/permissions/question_dialog.rs:104-167, 244-256` |
| 截断方式 | 问题文本、答案预览、对话框行均按字符截断 |

### 2.4 审批覆盖层

| 项目 | 说明 |
|------|------|
| 触发 | 工具执行审批流程 |
| 文件 | `crates/claude-code-rs/src/ui/components/approval_overlay.rs:137-145` |
| 截断方式 | 长文本行 `.take(width - 1)` + 单个省略号 `\u{2026}` |

### 2.5 历史搜索对话框

| 项目 | 说明 |
|------|------|
| 触发 | `Ctrl+R` |
| 文件 | `crates/claude-code-rs/src/ui/components/history_search_dialog.rs:169, 175, 213, 218, 249, 382-404` |
| 截断方式 | prompt、来源、首行按 unicode 宽度截断，过长时加 `...` |

### 2.6 命令表面系列（Command Surface 覆盖层）

所有 `/xxx` 命令均打开居中覆盖层，覆盖整个终端。

#### 2.6.1 Resume（恢复会话）

| 项目 | 说明 |
|------|------|
| 触发 | `/resume` |
| 文件 | `crates/claude-code-rs/src/ui/command_surface/surfaces/resume.rs:82, 130-137` |
| 截断方式 | 会话标题截断至 72 字符 |

#### 2.6.2 Remote（远程运行）

| 项目 | 说明 |
|------|------|
| 触发 | `/remote` |
| 文件 | `crates/claude-code-rs/src/ui/command_surface/surfaces/remote.rs:111` |
| 截断方式 | 运行 ID 截断至 26 字符（`truncate_middle` head...tail） |

#### 2.6.3 Agents（代理管理）

| 项目 | 说明 |
|------|------|
| 触发 | `/agents` |
| 文件 | `crates/claude-code-rs/src/ui/agents/utils.rs:118-139` |
| 截断方式 | 代理名称/值用 `truncate_middle()` head...tail 截断 |

### 2.7 状态栏

| 项目 | 说明 |
|------|------|
| 位置 | 底部固定区域（始终可见） |
| 文件 | `crates/claude-code-rs/src/ui/app/render.rs:1019` |
| 截断方式 | 标签文本截断至 28 字符 |

### 2.8 输入框（Chat Composer）

| 项目 | 说明 |
|------|------|
| 位置 | 底部输入区域 |
| 文件 | `crates/claude-code-rs/src/ui/components/chat_composer.rs:115-121` |
| 截断方式 | 提示行过长时 `fit_line()` 截断，加 `\u{2026}` 省略号 |

### 2.9 搜索框

| 项目 | 说明 |
|------|------|
| 位置 | 命令面板/对话框中的搜索输入 |
| 文件 | `crates/claude-code-rs/src/ui/components/search_box.rs:79, 127-139` |
| 截断方式 | 搜索文本按 unicode 宽度 silent 截断（`truncate_by_width`） |

---

## 三、工具函数清单

以下工具函数被多处复用，供参考：

| 函数 | 位置 | 行为 |
|------|------|------|
| `truncate_text(text, max_len)` | `crates/cc-utils/src/messages.rs:149-163` | 字节安全截断 + `...` |
| `truncate_start_to_width(text, max_width)` | `crates/claude-code-rs/src/ui/diff.rs:110-130` | 从开头截断，保留尾部 |
| `truncate_by_width(text, max_width)` | `crates/claude-code-rs/src/ui/diff.rs:110-130` | 按显示宽度 silent 截断 |
| `truncate_middle(value, max_chars)` | `crates/claude-code-rs/src/ui/agents/utils.rs:118-139` | head...tail 截断 |
| `truncate_middle(input, max_chars)` | `crates/claude-code-rs/src/ui/permissions/utils.rs:269-286` | head...tail 截断 |
| `truncate_str(s, max_chars)` | `crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs:925-935` | 字符截断 + `...` |
| `truncate_to_width(text, max_width)` | `crates/claude-code-rs/src/ui/components/history_search_dialog.rs:382-404` | unicode 宽度截断 + `...` |
| `truncate_to_width(text, max_width)` | `crates/claude-code-rs/src/ui/messages/user_bash_output_message.rs:160-176` | unicode 宽度 silent 截断 |
| `abbreviate_json(value, max_chars)` | `crates/claude-code-rs/src/ui/messages/render.rs:2120-2141` | JSON 递归截断 |

---

## 四、专门面板尺寸定义

> 记录时间：2026-05-22  
> 来源：`overlays/mod.rs`, `app/render.rs`, `permissions/`, `command_palette/`

### 4.1 共享基础设施：`panel_layout` + `CenteredOverlayFrame`

**文件**：`crates/claude-code-rs/src/ui/panel_layout.rs`

Rust TUI 现在通过 `PanelSizePreset` / `PanelSizeSpec` 集中记录面板尺寸默认值，并提供 `resolve_rect()` 统一计算安全的居中区域：

- `CommandSurface`: 32..148 宽，5..40 高
- `HistorySearch`: 20..148 宽，8..28 高
- `AgentTree`: 24..140 宽，8..32 高
- `PermissionDialog`: 56..150 宽，最小 8 高，最大随终端高度
- `QuestionDialog`: 90% 终端宽，最小 56 宽，8..18 高
- `BypassPermissionsMode`: 90% 终端宽，最小 64 宽，8..18 高
- `BetterViewPanel`: 固定 140 字符宽

**文件**：`crates/claude-code-rs/src/ui/overlays/mod.rs`

用于渲染居中对话框的通用结构体：

```rust
pub struct CenteredOverlayFrame<'a> {
    pub title: &'a str,
    pub color: Option<&'a str>,
    pub min_width: u16,
    pub max_width: u16,
    pub min_height: u16,
    pub max_height: u16,
}
```

**默认参数**：
- `min_width`: 24
- `max_width`: 96
- `min_height`: 5
- `max_height`: 24

`CenteredOverlayFrame::with_preset()` 可直接使用 `PanelSizePreset`，旧的 `.width()` / `.height()` builder 仍保留兼容。

渲染逻辑（`render_centered_dialog_lines()`）：
- 宽度：`min(area.width - 4, max_width).max(min_width)`
- 高度：`content_line_count.max(min_height).min(max_height)`
- 终端过小时（`area.width < 8` 或 `area.height < 4`）不渲染

### 4.2 共享基础设施：`BetterViewPanel`

**文件**：`crates/claude-code-rs/src/ui/components/better_view_panel.rs`

纯文本面板渲染器，内部尺寸常量：
- `PANEL_WIDTH`: **140** 字符
- `NAV_WIDTH`: **40** 字符（左侧导航列）
- `DETAIL_WIDTH`: **95** 字符

始终输出 140 字符宽的面板行，外部容器（如 `CenteredOverlayFrame`）负责缩放/截断。

### 4.3 各面板尺寸

#### Command Surface（命令表面）

**文件**：`crates/claude-code-rs/src/ui/app/render.rs:817-839`

```rust
CenteredOverlayFrame::new(surface.title())
    .color("accent")
    .width(32, 148)
    .height(5, 40)
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 32 |
| 最大宽度 | 148 |
| 最小高度 | 5 |
| 最大高度 | 40 |
| 自适应 | 高度基于内容行数 |

#### Permission Dialog（权限对话框）

**文件**：`crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs:169-183`

未使用 `CenteredOverlayFrame`，自行计算居中 Rect：

```rust
let dialog_width = area.width.saturating_sub(2).clamp(56, 150).min(area.width);
let preferred_height = footer_height.saturating_add(if self.is_typing_feedback() { 14 } else { 12 });
let dialog_height = preferred_height.min(area.height).max(8);
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 56 |
| 最大宽度 | 150 |
| 最小高度 | 8 |
| 最大高度 | area.height（屏幕高度） |
| 首选高度 | 12（正常）或 14（反馈模式）+ footer_height |

#### Question Dialog（询问对话框）

**文件**：`crates/claude-code-rs/src/ui/permissions/question_dialog.rs:73-78`

未使用 `CenteredOverlayFrame`：

```rust
let dialog_width = (area.width * 90 / 100).max(56).min(area.width);
let dialog_height = 18u16.min(area.height).max(8);
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 56 |
| 最大宽度 | area.width（屏幕宽度） |
| 首选宽度 | 90% 屏幕宽度 |
| 最小高度 | 8 |
| 最大高度 | 18 |

#### History Search（历史搜索）

**文件**：`crates/claude-code-rs/src/ui/app/render.rs:841-873`

```rust
CenteredOverlayFrame::new("History Search")
    .color("accent")
    .width(20, 148)
    .height(8, 28)
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 20 |
| 最大宽度 | 148 |
| 最小高度 | 8 |
| 最大高度 | 28 |

#### Agent Tree（代理线程树）

**文件**：`crates/claude-code-rs/src/ui/app/render.rs:875-910`

```rust
CenteredOverlayFrame::new("Agent Threads")
    .color("accent")
    .width(24, 140)
    .height(8, 32)
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 24 |
| 最大宽度 | 140 |
| 最小高度 | 8 |
| 最大高度 | 32 |

#### Diff 面板

继承 Command Surface 的约束（作为 DiffSurface 变体）：

| 约束 | 值 |
|------|-----|
| 最小宽度 | 32 |
| 最大宽度 | 148 |
| 最小高度 | 5 |
| 最大高度 | 40 |

#### Command Palette（命令面板）

**文件**：`crates/claude-code-rs/src/ui/command_palette/mod.rs:19-25`, `render.rs:24`

不使用 `CenteredOverlayFrame`，渲染在底部面板区域，全屏宽度。

| 约束 | 值 |
|------|-----|
| 宽度 | 全屏宽度 |
| 最大高度 | ~29 行（MAX_ROWS 20 + RESERVED 7 + BORDER 2） |
| 终端限制 | `area.height - 4` |
| 列宽 | `COMMAND_COLUMN_WIDTH = 34`（固定） |

#### Bypass Permissions Mode（旁路权限模式）

**文件**：`crates/claude-code-rs/src/ui/permissions/bypass_permissions_mode_dialog.rs:57-61`

```rust
let dialog_width = (area.width * 90 / 100).max(64).min(area.width);
let dialog_height = 18u16.min(area.height).max(8);
```

| 约束 | 值 |
|------|-----|
| 最小宽度 | 64 |
| 最大宽度 | area.width（屏幕宽度） |
| 首选宽度 | 90% 屏幕宽度 |
| 最小高度 | 8 |
| 最大高度 | 18 |

### 4.4 统一性分析

已有统一的内部尺寸配置机制。`panel_layout.rs` 集中保存 overlay/dialog/panel 的默认尺寸，`CenteredOverlayFrame` 和权限相关弹窗均从 preset 获取尺寸约束。

仍保留独立布局的面板：

1. **Command Palette**：底部全宽面板，尺寸由命令行数、帮助区和底部输入区域共同决定，不使用居中 overlay。
2. **消息区域内联内容**：随主消息流宽度渲染，不属于专门面板尺寸配置。

该机制目前是 Rust 内部 preset，不接入用户 settings。

---

## 五、观察

- **对话记录**中最明显的省略是：工具调用结果仅显示前 5 行、Bash 输出按宽度 silent 截断。
- **专门面板**中省略最突出的是：Diff 面板 400 行上限、权限对话框的 head...tail 摘要截断。
- silent 截断（不加省略号）可能让用户察觉不到内容被截断，尤其是 `user_bash_output_message.rs` 和 `diff.rs` 中的 `truncate_by_width`。
