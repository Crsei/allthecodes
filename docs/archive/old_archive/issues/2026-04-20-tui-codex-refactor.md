# Issue: 基于 `codex-rs/tui` 重构 cc-rust 现有 TUI

Labels: `enhancement`, `ui`, `refactor`, `large`
Priority: `P2`（视战略确认结果调整）
Upstream: `F:\AIclassmanager\cc\codex\codex-rs\tui`（Apache-2.0，复用需保留原 license header + NOTICE）

## 0. 背景

`src/ui/` 当前实现是 cc-rust 最早的 ratatui 前端，`CLAUDE.md` 里已将其标注为
"TUI legacy"。近一年 codex 官方把自家 ratatui TUI 迭代到了远超我们的体量与质量，
整个栈（流式 markdown、HistoryCell、BottomPane view stack、kill-buffer 文本区、
paste-burst、审批 overlay、pager、frame requester）都值得参考。

两套 TUI 的体量对比（实测 `wc -l`）：

| 维度 | 当前 cc-rust (`src/ui/`) | 参考 codex (`codex-rs/tui/src/`) |
|------|-------------------------|----------------------------------|
| 文件数 | 19 | 100+ |
| 总行数 | ~8,000 | ~93,000 |
| 核心循环 | `tui.rs` 818 行 | `tui.rs` 613 行 + `chatwidget.rs`（本体 + 子模块）>15k 行 |
| App 状态 | `app.rs` 1,432 行（单体）| `app.rs` + `app/` 子目录分散 ~3k 行 |
| 输入区 | `prompt_input.rs` 259 行 | `bottom_pane/chat_composer.rs` 8,175 + `textarea.rs` 2,469 行 |
| Markdown | `markdown.rs` 266 行 | `markdown_render.rs` 1,134 + `markdown_stream.rs` 725 + `wrapping.rs` 1,407 行 |
| 审批 | `permissions.rs` 242 行（单对话框）| `bottom_pane/approval_overlay.rs` 1,465 行 |

依赖层面两边 **兼容**：都用 `ratatui 0.29` + `crossterm 0.28`。
codex 额外启用了几个 `unstable-*` feature（见 §3）以及自家 patch 到
`nornagon/color-query` 分支的 ratatui/crossterm——这是我们接下来要做的第一个
决策（§3 依赖策略）。

> **许可证**：codex-rs 根目录 `LICENSE` 为 Apache-2.0。直接复用 `.rs` 文件需要：
> - 保留文件原有的 license/版权注释（codex 源码里多数没有 per-file header，
>   但需要在 `NOTICE` 或 `THIRD_PARTY.md` 里声明本仓库包含来自
>   `https://github.com/openai/codex` 的 Apache-2.0 代码）
> - 不需要把 cc-rust 整体改成 Apache-2.0，只要声明即可

## 1. 战略前置决策（必须先拉齐）

`src/ui/` 目前是 "legacy"，`ui/ink-terminal` 是主推前端。**大规模 TUI 重构前**
必须确定以下之一：

### 选项 A：Legacy 保活、挑重点补强

投入控制在 1-2 周。只复用 codex 的 **Tier 1** 模块（纯工具函数、无 core 耦合），
修掉现在最明显的痛点：markdown、换行、Windows 粘贴、kill-buffer。

对应本文 **Phase 1 + Phase 2a + Phase 4a**。

### 选项 B：把 Rust TUI 升格为一等公民

彻底把 `src/ui/` 拉到 codex 级别，覆盖审批 overlay、流式 markdown、HistoryCell
架构、pager 等。工作量 ~2-3 个月（一个人全职）。会让 `src/ui/` 与
`ui/ink-terminal` 在能力上基本打平，headless/daemon 场景下可以直接用 Rust TUI，
不再强依赖 bun。

对应本文 **Phase 1 → Phase 10** 全量执行。

**推荐默认走选项 A**，把选项 B 中每个 Phase 保留为独立 issue，按需解锁。
文档后续按此假设写：Phase 1/2a/4a 是 must-have，其余 Phase 是 nice-to-have。

## 2. 代码复用分层（Reuse Tiers）

按照"与 codex 专属类型耦合程度"把 codex TUI 的文件划分成四层。

### Tier 1 — 纯工具，可几乎原样搬运（~10k 行可用）

无 `codex_core` / `codex_protocol` / `codex_app_server_protocol` 耦合，只依赖
ratatui / crossterm / pulldown-cmark / unicode-segmentation 等公共 crate。
搬运工作 = 复制 + 改 `use` 路径 + 跑编译。

| codex 文件 | 行数 | 用途 | 对应替换 cc-rust |
|-----------|------|------|-----------------|
| `wrapping.rs` | 1407 | 自适应 unicode 换行 | 新增；当前走 `textwrap` |
| `live_wrap.rs` | — | 流式前缀裁剪 | 新增 |
| `markdown_render.rs` | 1134 | pulldown-cmark → ratatui Lines | 替换 `ui/markdown.rs` |
| `markdown.rs` | 121 | 薄封装 `append_markdown` | 同上 |
| `markdown_stream.rs` | 725 | 流式 markdown（增量渲染）| 新增 |
| `text_formatting.rs` | 580 | `proper_join` / 截断 / env 展示 | 新增 |
| `line_truncation.rs` | — | 中部省略渲染 | 新增 |
| `render/line_utils.rs` | — | `line_to_static` / `prefix_lines` / `push_owned_lines` | 新增 |
| `render/highlight.rs` | — | 代码块高亮（syntect/pulldown-cmark）| 新增 |
| `insert_history.rs` | — | scrolling-region 历史行插入（非 altscreen）| 新增 |
| `shimmer.rs` | 80 | 动效文本 | 新增 |
| `frames.rs` | — | 多套 spinner 帧序 | 可替换 `ui/spinner.rs` 帧表 |
| `ascii_animation.rs` | — | 启动动画 | 可选：替换 welcome |
| `color.rs` / `style.rs` | — | 样式助手 | 新增 |
| `terminal_palette.rs` | 439 | 终端色彩检测 | 新增 |
| `terminal_title.rs` | 224 | OSC 标题设置 | 新增 |
| `diff_render.rs` | — | diff 可视化 | 替换 `ui/diff.rs` |
| `key_hint.rs` | — | `KeyBinding` 渲染助手 | 整合进 `ui/keybindings` |

### Tier 2 — 中等适配，需要小改（~15k 行有价值）

仅依赖少量协议类型（可以替换成 cc-rust 自己的 `types::message` / `permissions`）。

| codex 文件 | 行数 | 耦合 | 备注 |
|-----------|------|------|------|
| `bottom_pane/textarea.rs` | 2469 | `user_input::TextElement` | 可替换成自己的 element 类型 |
| `bottom_pane/paste_burst.rs` | 576 | 纯 crossterm | Windows 粘贴关键 |
| `bottom_pane/chat_composer_history.rs` | 424 | — | 几乎无耦合 |
| `bottom_pane/selection_popup_common.rs` | 869 | — | 通用选择列表原语 |
| `bottom_pane/list_selection_view.rs` | 1843 | — | 通用列表选择器 |
| `bottom_pane/file_search_popup.rs` | 184 | `codex_file_search::FileMatch` | @file 自动补全 |
| `bottom_pane/command_popup.rs` | 482 | slash command 元数据 | 斜杠命令弹窗 |
| `bottom_pane/footer.rs` | 1750 | 多处 core 引用 | 需要较多适配 |
| `bottom_pane/scroll_state.rs` | 115 | — | 通用 |
| `custom_terminal.rs` | — | ratatui 扩展 backend | 支持 insert_history |
| `tui/frame_rate_limiter.rs` | — | — | ~60fps 节流 |
| `tui/frame_requester.rs` | — | — | 显式帧请求（替换 dirty 标志）|
| `tui/event_stream.rs` | — | — | EventBroker |
| `pager_overlay.rs` | 1305 | 依赖 history_cell trait | 可替换 transcript mode |
| `status_indicator_widget.rs` | 461 | — | 流式动画指示器 |
| `theme_picker.rs` | 654 | codex theme 结构 | 适配我们的 `ui/theme.rs` |
| `tooltips.rs` | 415 | — | 上下文帮助 |

### Tier 3 — 深度耦合，需要重写或大改（~30k 行谨慎）

高度依赖 codex 的 app-server 协议、plugins、collab、rollout、onboarding 流程。

| codex 文件 | 耦合点 | 在 cc-rust 该怎么做 |
|-----------|-------|-------------------|
| `chatwidget.rs` + 子模块 | `codex_app_server_protocol::*`、`codex_core::plugins::*` | 参考其 **架构**，但重写；把事件换成我们的 `SdkMessage` |
| `history_cell.rs` | 所有 trait 实现都引 codex protocol | **trait 本身可搬**，具体 cell 实现需自己写 |
| `exec_cell/*` | `codex_protocol::protocol::ExecCommand*Event` | 映射到我们的 `BashTool` 生命周期 |
| `bottom_pane/chat_composer.rs` | `user_input::TextElement`、`LocalImageAttachment` | 保留 state machine 骨架，改类型 |
| `bottom_pane/approval_overlay.rs` | `ExecApprovalRequestEvent` 等 | 换成我们的 `permissions::*` 请求 |
| `bottom_pane/mcp_server_elicitation.rs` | `rmcp` | 只在 MCP 功能场景才需要 |
| `streaming/*` | app-server 协议 | 重写为适配 `services/api/*` 的流式响应 |
| `multi_agents.rs` | codex collab | 不搬（我们用自己的 Agent tool 模式）|

### Tier 4 — 不适用（codex 专属，直接跳过）

`app_server_*`、`local_chatgpt_auth.rs`、`oss_selection.rs`、`model_migration.rs`、
`model_catalog.rs`、`update_action.rs` / `update_prompt.rs` / `updates.rs`、
`skills_helpers.rs`、`onboarding/*`（他们的登录流和我们的 OAuth 不一样）、
`collaboration_modes.rs`、`debug_config.rs`、`resume_picker.rs`（除非做 session 选择器）。

## 3. 依赖策略

### 3.1 ratatui feature 扩展

codex 启用了四个 `unstable-*` feature：

```toml
ratatui = { version = "0.29", features = [
    "scrolling-regions",              # insert_history.rs 依赖
    "unstable-backend-writer",        # custom_terminal.rs 依赖
    "unstable-rendered-line-info",    # 精确高度测量（history cell）
    "unstable-widget-ref",            # WidgetRef trait（借用式渲染）
] }
```

**行动项**：Phase 0 把这四个 feature 加入 cc-rust 的 `Cargo.toml`，验证
当前代码无 breaking change。

### 3.2 是否跟随 codex 的 crossterm/ratatui patch 分支

codex 在 workspace Cargo.toml 打了两个 patch 指向 `nornagon/color-query` 分支：

```toml
[patch.crates-io]
crossterm = { git = "...", branch = "nornagon/color-query" }
ratatui = { git = "...", branch = "nornagon-v0.29.0-patch" }
```

这些 patch 主要是为了终端色彩查询（OSC 10/11）。**建议暂不跟随**——除非我们
Phase 8 要做 `terminal_palette.rs`。如果不 patch，`terminal_palette.rs` 可能
部分功能退化，但主流程不受影响。

### 3.3 新增 crate 依赖

| crate | 用途 | 推荐版本 |
|-------|------|---------|
| `pulldown-cmark` | markdown 解析 | workspace 已有则沿用 |
| `unicode-segmentation` | 字形簇 | 已有 |
| `unicode-width` | 显示宽度 | 已有 |
| `ratatui-macros` | line!/span! 宏 | 新增 |
| `regex-lite` | markdown_render 内部 | 新增（轻量，无 regex crate 依赖）|
| `textwrap` | chat_composer 内部用 | 已有 |
| `dunce` | Windows 路径规范化 | 新增（或自行处理）|
| `image` + `base64` | 本地图片附件 | 仅在 Phase 6 开启时需要 |

## 4. 分阶段 Issue 列表

每个 Phase 建议拆成独立 GitHub/issue 文件单独追踪。顺序严格按依赖关系，除
非标注 `(parallel)` 可并行。

---

### Phase 0 — 准备期（S，1-2 天）

**目标**：把依赖与许可证前置条件搞齐，让后续 Phase 能直接 `cargo build`。

- [ ] `Cargo.toml` 打开 ratatui 的四个 `unstable-*` feature，跑 `cargo build --release` 确认无 warning
- [ ] 新增 `NOTICE` 文件，声明本仓库 `src/ui/` 之后将包含来自 OpenAI codex 的
      Apache-2.0 代码
- [ ] 在 `docs/` 下新增 `docs/tui-codex-upstream.md`，记录：
      - 上游路径：`F:\AIclassmanager\cc\codex\codex-rs\tui`
      - 我们 fork 时对应的上游 git commit（`git -C F:\AIclassmanager\cc\codex rev-parse HEAD`）
      - 每个复用文件的上游路径 → cc-rust 路径映射（后续 Phase 填充）
- [ ] 选定命名前缀：我建议新目录 `src/ui2/` 或 `src/tui/`，与现有 `src/ui/`
      并存一段时间，渐进迁移；不要原地覆盖

**验收**：`cargo build --release` 0 warning，`NOTICE` 到位，上游 commit 记录。

---

### Phase 1 — Tier 1 基础原语（M，3-5 天）

**目标**：把纯工具函数搬进来，不改任何业务逻辑，为后续 Phase 打底。

搬运清单（按编译依赖顺序）：

1. `render/line_utils.rs`（无外部依赖）
2. `wrapping.rs` + `live_wrap.rs`
3. `text_formatting.rs`
4. `line_truncation.rs`
5. `style.rs` + `color.rs`
6. `key_hint.rs`
7. `markdown_render.rs`（依赖 `wrapping` + `line_utils`）
8. `markdown.rs`（薄封装）
9. `shimmer.rs` + `frames.rs`（可选）

改动点：

- 所有 `use codex_utils_string::normalize_markdown_hash_location_suffix` 换成
  我们仓库里的等价函数，或者直接内联（normalize 实现很短）
- 所有 `use crate::style::*` 等保持，但需要把 `pub(crate)` 收紧到模块可见度
- `markdown_render.rs` 里 `display_path_for` 用了 codex 的 path 归一化，
  需要换成 `path_utils::relativize_to_home` 或我们自己的版本

**验收**：
- 新模块 `src/tui/primitives/` 编译通过
- 为 `wrapping`、`markdown_render` 写 2-3 个 smoke test（复用 codex 的
  `markdown_render_tests.rs` 里简单 case）

---

### Phase 2a — 替换现有 markdown 渲染（S，1-2 天，阻塞 2b）

**目标**：让当前 `messages.rs` 用 Phase 1 的 `markdown_render`。

- [ ] `messages.rs::render_assistant_message` 中，把 `ui/markdown.rs` 的调用
      换成 `tui::primitives::markdown::append_markdown`
- [ ] `virtual_scroll.rs` 里测量高度的地方切换到新渲染器
- [ ] 删除旧 `ui/markdown.rs`（或标 `#[deprecated]`）
- [ ] 视觉回归：跑 `tests/` 里的 TUI snapshot 测试（如果没有，新增一个
      针对 assistant 消息的 terminal snapshot）

**验收**：显示包含 `# 标题 / * 列表 / `code``` 的 markdown 时能正确渲染，
      长 URL 在窄屏下不再截断成乱码。

---

### Phase 2b — `HistoryCell` trait 架构（L，1-2 周）

**目标**：把 `Vec<Message>` 渲染模型换成 codex 风格的 `HistoryCell` trait。

这是后续 Phase 5/6/7 的前提。

- [ ] 在 `src/tui/history/` 新建：
      - `mod.rs`
      - `cell.rs`（trait 定义，直接抄 codex `history_cell.rs` 第 96-180 行）
      - `assistant.rs`（assistant 消息 cell）
      - `user.rs`（user 消息 cell）
      - `system.rs`（info / error / compact boundary）
      - `plain.rs`（纯文本 cell，fallback）
- [ ] `ui/app.rs::messages: Vec<Message>` 旁边新增
      `history_cells: Vec<Box<dyn HistoryCell>>`，通过 adapter 从 Message 构造
- [ ] `virtual_scroll.rs` 的高度测量接入
      `HistoryCell::desired_height(width)`（比自写计算更准确，因为它用的是
      ratatui `Paragraph::line_count`）
- [ ] `messages.rs::render_messages` 换成遍历 cells 调用
      `HistoryCell::display_lines(width)`
- [ ] 保留 `Message` 作为"持久化层类型"，`HistoryCell` 只是渲染层

**验收**：
- 当前所有消息类型都能用 HistoryCell 渲染
- Ctrl+O 进入 transcript 模式仍然正常
- Session resume（`session/` 模块）读回的消息能正确重建 cell 列表

---

### Phase 3 — TextArea + BottomPane 骨架（L，1-2 周）

**目标**：用 codex 的 `TextArea`（2500 行）替换现有 `PromptInput`（259 行），
并引入 `BottomPane` 的 view stack 骨架。

搬运：

1. `bottom_pane/textarea.rs`
   - 替换 `codex_protocol::user_input::TextElement` 为自己的
     `types::ui::TextElement`（只需要 `id / range / name` 三个字段）
   - 保留 kill-buffer（Ctrl+K / Ctrl+Y）语义
2. `bottom_pane/chat_composer_history.rs`
   - 几乎无耦合，直接搬
3. `bottom_pane/scroll_state.rs`
4. `bottom_pane/selection_popup_common.rs`
5. `bottom_pane/bottom_pane_view.rs`（trait 定义）
6. `bottom_pane/mod.rs` 骨架（只保留 BottomPane struct，暂不接 ChatComposer）

API 层：

- `BottomPane::push_view(Box<dyn BottomPaneView>)`
- `BottomPane::handle_key(KeyEvent) -> CancellationEvent`
- 新 `AppAction::PushView / PopView` 把 overlay 机制暴露给上层

**验收**：
- 现有 Ctrl+C 中断、Esc 返回、history Up/Down 行为 100% 保留
- 输入 `#` 前缀后 kill (Ctrl+K) + yank (Ctrl+Y) 工作
- Windows 下中文 IME 输入仍然正常（paste-burst 在 Phase 4 才加）

---

### Phase 4a — Paste-burst（S，2-3 天，可独立）

**目标**：解决 Windows / 旧终端下粘贴会被拆成逐字符 KeyEvent 的老问题。

- [ ] 搬运 `bottom_pane/paste_burst.rs`（576 行，纯状态机）
- [ ] 在 `TextArea::handle_key_event` 里按 codex 的 "Integration Points"
      注释接入
- [ ] 配置开关：`CLAUDE_CODE_DISABLE_PASTE_BURST=1`

**验收**：在 Windows `cmd.exe` 和 Windows Terminal 中粘贴 500 字中文段落能
一次性落入输入框，不出现 UI 闪烁。

---

### Phase 4b — ChatComposer + 斜杠/文件弹窗（L，2-3 周）

**目标**：把 `PromptInput` 升级为完整的 `ChatComposer`，带斜杠命令和 @file 弹窗。

- [ ] `bottom_pane/command_popup.rs` + `slash_commands.rs`
      - 适配我们的 36 个斜杠命令注册表（`src/commands/`）
- [ ] `bottom_pane/file_search_popup.rs`
      - 适配我们的文件搜索（复用 `tools/glob.rs` 或新建一个轻量 MRU 表）
- [ ] `bottom_pane/chat_composer.rs`（主体）
      - 替换 `LocalImageAttachment` → 自己的类型
      - 移除 remote image row 逻辑（我们不做远程图片 v1）
      - 简化 mention system：只保留 `@file`，砍掉 `$mention`
- [ ] `bottom_pane/list_selection_view.rs`（通用选择器，给未来 /resume 用）

**验收**：
- 输入 `/` 弹出命令列表，方向键选择 + Enter 触发
- 输入 `@s` 弹出文件搜索
- `/help` 等现有命令仍能正常执行

---

### Phase 5 — ExecCell（工具调用可视化）（L，1-2 周）

**目标**：Bash / FileRead / FileEdit 等工具调用用 "cell 内流式" 的形式渲染，
而不是当前的"静态文本块"。

- [ ] 搬运 `exec_cell/` 整个目录（model.rs + render.rs + mod.rs）
- [ ] 改造成以 cc-rust 的 `ToolUse` + `ToolResult` 为数据源
- [ ] `query.rs` 里发送工具调用开始/结束时，往 `ChatWidget` 推对应事件
- [ ] 折叠行数上限：参照 codex `TOOL_CALL_MAX_LINES`
- [ ] 输出增量：每次 tool stdout chunk 到达就更新 cell

**验收**：跑 `bash ls -la /tmp` 能看到工具名高亮 + 输出逐行追加 + 结束后
显示 exit code；长输出自动折叠并提示 "X more lines"。

---

### Phase 6 — 审批 Overlay（M，1 周）

**目标**：把现在的 `PermissionDialog` 换成 codex 的审批 overlay（支持
命令预览、diff 预览、"always allow" 等选项）。

- [ ] 搬运 `bottom_pane/approval_overlay.rs`（1465 行）
- [ ] 数据适配：我们的
      `permissions::ToolPermissionContext` → codex 的
      `ApprovalRequest` 结构
- [ ] 三种请求形态分别接入：
      - `Bash` 命令审批（显示 cmd + cwd）
      - `FileEdit / FileWrite` 审批（显示 diff）
      - 普通工具审批（显示 input JSON）
- [ ] 结果回写：`Approved` / `ApprovedForSession` / `Denied` / `Abort`

**验收**：
- 当 `autoApprove=false` 时，运行 `Write` 工具会弹出 diff 预览 overlay
- Shift+↑/↓ 在 overlay 内滚动
- `y/n/a` 对应通过/拒绝/session-approve

---

### Phase 7 — 流式 Markdown Pipeline（M，1 周）

**目标**：把 assistant 消息流式渲染从"累加 raw text"升级为"增量 markdown"。

- [ ] 搬运 `markdown_stream.rs`（725 行）
- [ ] 改造 `tui.rs::handle_sdk_message` 里 `ContentBlockDelta` 分支：
      - 把 delta 喂给 `MarkdownStream::push(delta)`
      - 每帧从 stream 拉取已定型的 Lines，更新 active cell
- [ ] `HistoryCell::transcript_animation_tick()` 机制（codex 原有）用于光标闪烁
- [ ] 流结束时 `finalize()` 产出完整 Lines，替换活跃 cell

**验收**：流式输出时 `# 标题` 一旦完整就立刻以标题样式渲染；代码块在 ``` 闭合
前用 plain 渲染，闭合后重新高亮。

---

### Phase 8 — FrameRequester + EventStream 重构（M，1 周，可选）

**目标**：把当前的 `dirty` bool + 16ms 固定 tick 换成 codex 的 frame requester
（按需请求帧 + 60fps 上限节流）。

- [ ] 搬运 `tui/frame_rate_limiter.rs` + `frame_requester.rs`
- [ ] 搬运 `tui/event_stream.rs`（EventBroker，为 overlay 提供事件 fork）
- [ ] `custom_terminal.rs`（支持 insert_history 的 ratatui 扩展 backend）
- [ ] 切换 `tui.rs` 主循环到 `tokio_stream` 驱动

**验收**：空闲时 CPU 占用显著下降；流式消息时帧率稳定 ≤ 60fps。

可选扩展：

- `insert_history.rs`：在非 altscreen 模式下把已定型的消息滚出到 scrollback，
  体验更像 `less`。对应需要新增 `--no-altscreen` CLI flag。

---

### Phase 9 — Pager Overlay / Transcript 2.0（M，1 周，可选）

**目标**：用 codex 的 pager overlay 替换当前 `transcript.rs`，支持搜索高亮、
面包屑、行号模式。

- [ ] 搬运 `pager_overlay.rs`（1305 行）
- [ ] 对接 HistoryCell 列表作为数据源
- [ ] 保留我们现在的 Ctrl+O 循环（Prompt → Transcript → Focus）

**验收**：Ctrl+O 进入 transcript，`/` 搜索，`n/N` 跳转，搜索词高亮。

---

### Phase 10 — Theme Picker + Status Indicator 升级（S，3-5 天，可选）

**目标**：
- 用 codex 的动画 spinner (`status_indicator_widget`) 替换 `ui/spinner.rs`
- 加入 `theme_picker.rs`，让 `/theme` 命令有可视化选择器

- [ ] 搬运 `status_indicator_widget.rs`（461 行）
      - 适配输入：当前 streaming 状态 + 阶段标签（"Thinking..." / "Running bash..." / "Retrying..."）
- [ ] 搬运 `theme_picker.rs` + `theme_picker` 相关 popup

**验收**：streaming 期间的 spinner 和 codex 观感一致；`/theme` 打开选择器。

---

## 5. 不做的事（明确排除）

- **不搬 `chatwidget.rs` 全量**：他们的 ChatWidget 深度依赖
  `codex_app_server_protocol` 的 ServerNotification 事件模型，我们有自己的
  `SdkMessage` 流。学架构，不照抄。
- **不做多 agent 协作视图** (`multi_agents.rs`)：我们有 Agent tool，不走
  collab 协议。
- **不搬 onboarding / login 流**：我们的 OAuth PKCE 与 codex 完全不同。
- **不搬 update_prompt 自更新**：cc-rust 靠 `cargo install`，不内建更新器。
- **不跟随 ratatui/crossterm patch 分支**（见 §3.2），除非遇到具体阻塞。

## 6. 验证策略

### 6.1 回归基线

Phase 1/2a 落地前，先建一条基线：

- [ ] 用 `tests/e2e/` 新增 3 个 TUI snapshot 测试，覆盖：
      - 欢迎屏
      - 一轮完整 user → assistant（含 markdown）对话
      - 一次 Bash 工具调用 + 审批 + 输出

所有 Phase 的 PR 都必须跑过这三个 snapshot。差异需要人工 review 后更新
snapshot baseline。

### 6.2 每 Phase 的新测试

- Phase 1：`wrapping` / `markdown_render` 单测（复用 codex 的 case，改 import）
- Phase 2b：`HistoryCell::desired_height` 与旧 `virtual_scroll` 算出的高度
  在 10 条消息上一致
- Phase 3：TextArea 的 kill-buffer 往返
- Phase 4a：paste-burst 能合并 500 字符连续 `KeyEvent::Char`
- Phase 4b：`/h` 能触发 command popup 并正确过滤
- Phase 5：ExecCell 在模拟 stdout 流下正确显示折叠
- Phase 6：三种审批形态的 yes/no/abort 路径
- Phase 7：流式 markdown 在 `#` 出现但未 \n 时不误判为标题

## 7. 风险与开放问题

| 风险 | 缓解 |
|------|------|
| ratatui unstable feature 将来 breaking | 锁定 `ratatui = "=0.29.0"`，升级时统一跑回归 |
| codex 上游持续变动 | Phase 0 记下上游 commit；后续按需 cherry-pick 而不是 rebase 全量 |
| Tier 3 模块适配成本估低 | 每个 Phase 开工前先 spike 一天，校准估算再继续 |
| `src/ui/` 与新 `src/tui/` 双系统并行期代码膨胀 | Phase 2b 结束后设一个"只留新系统"的 cut-off PR |
| 现有 `ui/ink-terminal` 前端不受影响 | 两者通过 IPC 解耦，本 issue 完全不触碰 `ui/` 目录 |

开放问题（需要在开始前拍板）：

1. **选项 A 还是选项 B**？（§1）
2. **新命名前缀**：`src/tui/` vs `src/ui2/` vs 在 `src/ui/` 内加子模块？
3. **是否接受 Apache-2.0 声明**？（需要更新 `README.md` + 新增 `NOTICE`）
4. **是否同意 Phase 8 改主循环**？改了会影响 `ipc/headless.rs` 的共享代码路径
   （需要复查 `run_tui` 与 `headless::event_loop` 的重叠度）
5. **Phase 4a 是否提前到 Phase 1 之后就做**？粘贴体验是当前痛点，独立可做。

## 8. 预期收益（概估）

即便只做 Phase 1 + 2a + 4a（选项 A 最小集）：
- 修复 Windows 粘贴
- markdown 渲染显著提升（代码块高亮、表格、链接）
- 长 URL 换行不再错位
- 代码量从 ~8k 涨到 ~14k，但单体 `app.rs` 不增加

做完选项 B 全量：
- `src/tui/` ≈ 25-30k 行，能力追平 codex
- 为将来把 `src/ui/ratatui` 升为一等公民提供条件
- headless / daemon 下不再强依赖 ink-terminal

---

> 本 issue 是 **总体规划**。确认战略方向后，每个 Phase 建议拆成独立 issue：
> 文件命名 `docs/issues/2026-04-XX-tui-phase-N-<name>.md`。Phase 0 不需要
> 单独 issue，直接随 Phase 1 的 PR 一起走。
