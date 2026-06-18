# TUI 提示面板上移到对话栏上方计划

> 日期：2026-06-18
> 范围：Rust TUI，主要位于 `crates/allthecodes/src/ui/`
> 目标：将输入相关提示面板、slash command surface、权限/提问类短面板从居中或输入栏下方展示，调整为优先贴在对话输入栏上方展示。

---

## 1. 目标

当前 Rust TUI 中存在两类面板展示方式：

1. 输入相关面板已经进入 bottom pane，但顺序是 `input -> completion_popup -> command_palette`，导致补全和命令面板显示在输入栏下方。
2. `CommandSurface`、history search、agent tree、permission/question/bypass dialogs 仍通过 centered overlay 居中展示。

本计划要建立一个统一的 prompt-adjacent 布局策略：

- 输入相关浮层贴在输入栏上方。
- 短交互面板优先贴在输入栏上方，而不是居中打断会话上下文。
- 大型阅读/详情面板保留居中或独立全屏策略，避免强行压缩可读区域。
- 布局逻辑集中实现，避免在每个 dialog 中手写坐标。

---

## 2. 当前代码入口

| 功能 | 当前文件 | 当前行为 |
|------|----------|----------|
| 主渲染布局 | `crates/allthecodes/src/ui/app/render.rs` | 计算 message area、bottom pane、overlay 渲染顺序 |
| bottom pane 高度/区域 | `crates/allthecodes/src/ui/components/bottom_pane.rs` | 定义 input、completion_popup、command_palette 等区域顺序 |
| prompt 输入框 | `crates/allthecodes/src/ui/prompt_input.rs` | 输入栏具体渲染 |
| slash command palette | `crates/allthecodes/src/ui/command_palette/render.rs` | 在 bottom pane 中渲染 |
| slash command surface | `crates/allthecodes/src/ui/command_surface/mod.rs` | `render()` 返回文本，外层居中渲染 |
| centered overlay | `crates/allthecodes/src/ui/overlays/mod.rs` | `render_centered_dialog_lines()` 统一居中文本弹层 |
| panel 尺寸预设 | `crates/allthecodes/src/ui/panel_layout.rs` | `PanelSizePreset` 和 `centered_rect()` |
| permission dialog | `crates/allthecodes/src/ui/permissions/dialog_overlay.rs` | 自行 resolve centered rect |
| question dialog | `crates/allthecodes/src/ui/permissions/question_dialog.rs` | 自行 resolve centered rect |
| bypass dialog | `crates/allthecodes/src/ui/permissions/bypass_permissions_mode_dialog.rs` | 自行 resolve centered rect |

现有测试中 `command_palette_renders_below_prompt_input` 固定了旧行为，需要改为新行为。

---

## 3. 设计方案

### 3.1 新增 prompt-adjacent 布局模型

建议在 `panel_layout.rs` 中新增通用方法，或拆出新文件 `prompt_overlay_layout.rs`：

```rust
pub enum PanelAnchor {
    Centered,
    PromptAdjacent { prompt_area: Rect },
}

pub fn prompt_adjacent_rect(
    terminal: Rect,
    prompt_area: Rect,
    spec: PanelSizeSpec,
    preferred_height: u16,
) -> Option<Rect>
```

规则：

- `prompt_area.y` 以上作为优先可用空间。
- 面板底部贴近 `prompt_area.y`，中间保留 0 到 1 行 gap，具体按现有视觉风格决定。
- 宽度默认使用终端宽度减 padding，再按 `PanelSizeSpec` clamp。
- 高度按 `preferred_height`、preset max、上方可用空间三者取最小。
- 当上方可用空间低于 `min_height` 时，回退到 centered 或全宽小面板，保证可操作。

### 3.2 新增 prompt-adjacent overlay renderer

在 `overlays/mod.rs` 中新增类似接口：

```rust
pub fn render_prompt_adjacent_dialog_lines(
    frame: CenteredOverlayFrame<'_>,
    body: Vec<Line<'static>>,
    terminal: Rect,
    prompt_area: Rect,
    buf: &mut Buffer,
    colors: &ThemeColors,
    style: Style,
)
```

注意：

- 可以先复用 `CenteredOverlayFrame` 的 title、color、min/max width/height 字段。
- 后续如果命名不合适，再重命名为 `OverlayFrame` 或 `PanelFrame`。
- `render_centered_dialog_lines()` 保留，服务大型 modal。

### 3.3 重排 bottom pane

修改 `BottomPaneHeights::split()` 的区域顺序，使输入相关面板在输入栏上方：

目标顺序：

```text
spinner
suggestions
paste_notice
completion_popup
command_palette
command_arg_help
input
notification
agent_footer
status
```

同步更新：

- `BottomPaneHeights::total()` 不变。
- `BottomPaneAreas` 字段可保持不变，仅调整 `split()` mapping。
- `app/render.rs` 中相关注释和渲染顺序。
- 测试 `command_palette_renders_below_prompt_input` 改为断言 palette row 小于 prompt row。

### 3.4 迁移 CommandSurface

先迁移 `CommandSurface`，因为它是 slash 命令交互最常用的居中面板：

- 修改 `render_command_surface_overlay()` 接收 `prompt_area: Option<Rect>`。
- 有 `prompt_area` 时使用 prompt-adjacent renderer。
- 无 `prompt_area` 时回退 centered，覆盖 transcript/focus/tiny terminal 等场景。

### 3.5 迁移短交互 dialogs

第二批迁移：

- `HistorySearchDialog`
- `AgentTreeDialog`
- `PermissionDialog`
- `QuestionDialog`
- `BypassPermissionsModeDialog`

建议策略：

- permission/question/bypass 作为 prompt-adjacent 默认目标。
- history search 和 agent tree 需要确认高度；如果超过可用空间，则保持 centered 或使用大面板 preset。
- 每个 dialog 的 `render()` 可以增加 `anchor: Option<Rect>` 参数，或者通过包装函数在 `app/render.rs` 统一定位。

### 3.6 保留大型 modal

以下类型不建议纳入本轮 prompt-adjacent：

- diff detail / structured diff
- long task detail
- file preview / better view panel
- 需要大量上下文阅读的 full panel

这些面板应继续使用 centered/fullscreen 策略。

---

## 4. 实施阶段

### Phase 1：输入相关面板上移

修改：

- `components/bottom_pane.rs`
- `app/render.rs`
- `app/tests.rs`

验收：

- slash palette 出现在 prompt 上方。
- completion popup 出现在 prompt 上方。
- status/footer 仍在底部，不与 prompt 或 palette 重叠。

### Phase 2：新增通用 prompt-adjacent 布局工具

修改：

- `panel_layout.rs`
- `overlays/mod.rs`

新增测试：

- 上方空间充足时，rect bottom <= prompt_area.y。
- 上方空间不足时，高度裁剪或 fallback。
- 小终端时 rect 不越界。

### Phase 3：迁移 CommandSurface

修改：

- `app/render.rs`
- `overlays/mod.rs`
- 相关 command surface snapshot / buffer tests

验收：

- `/model`、`/permissions`、`/tasks` 等 surface 出现在输入栏上方。
- Esc、Enter、方向键行为不变。
- 面板关闭后 prompt focus 不丢失。

### Phase 4：迁移 permission/question 类 dialogs

修改：

- `permissions/dialog_overlay.rs`
- `permissions/question_dialog.rs`
- `permissions/bypass_permissions_mode_dialog.rs`
- `app/render.rs`

验收：

- 工具权限请求优先显示在输入栏上方。
- `AskUserQuestion` 输入仍能编辑、提交、取消。
- bypass permissions 警告在小终端仍可完整操作。

### Phase 5：迁移或保留其它 overlay

评估：

- `HistorySearchDialog`
- `AgentTreeDialog`

决策标准：

- 短列表、短提示：迁移到 prompt-adjacent。
- 长列表、阅读空间需求高：保留 centered。

---

## 5. 测试计划

最小测试集：

```bash
cargo test -p allthecodes app::tests::command_palette
cargo test -p allthecodes panel_layout
cargo test -p allthecodes overlays
```

完整验证：

```bash
cargo build --workspace --release
```

需要新增或更新的断言：

- `command_palette_renders_above_prompt_input`
- `completion_popup_renders_above_prompt_input`
- `command_surface_renders_above_prompt_when_prompt_area_exists`
- `permission_dialog_stays_above_prompt_or_falls_back_on_tiny_terminal`

---

## 6. 风险与处理

| 风险 | 处理 |
|------|------|
| 面板和输入栏重叠 | 用统一 rect helper，测试检查 `overlay.y + overlay.height <= prompt_area.y` |
| 小终端空间不足 | 明确 fallback，不强制 prompt-adjacent |
| 大型面板可读性下降 | 只迁移短交互面板，大型 modal 保留 centered |
| 截断后看不到关键按钮 | permission/question 类 footer 固定保留，body 可裁剪 |
| 旧 snapshot 大量变化 | 分 phase 更新 snapshot，避免混合行为变更 |

---

## 7. 完成标准

- 输入相关 panel 默认显示在 prompt 上方。
- `CommandSurface` 默认显示在 prompt 上方。
- permission/question 类短交互 dialog 可显示在 prompt 上方，并在小终端可回退。
- centered overlay 仍可用于大型 modal。
- 相关单测和 release build 通过，且无新增 warning。
