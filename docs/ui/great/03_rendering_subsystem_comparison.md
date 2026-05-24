# 渲染子系统对比：Rust (ratatui) vs TypeScript (Ink/React)

> **日期**：2026-05-17
> **范围**：Markdown、主题、进度条、旋转指示器、闪烁效果、工具活动、差异渲染、虚拟滚动、历史单元格和语法高亮

---

## 汇总表

| 领域 | Rust | TypeScript | Rust 完成度 | 主要差距 |
|------|------|------------|-------------------|----------|
| Markdown（核心解析） | `pulldown_cmark` + LRU 缓存 | `marked` + 500 条目令牌缓存 | 3/5 | 表格基础已补；仍缺链接化与完整 token 兼容 |
| Markdown 渲染 | `markdown_render.rs` 外观层 + 样式保留换行 | `markdown.ts` 中的完整令牌格式化器 | 3/5 | 未处理图片/OSC 8 链接/转义/定义/删除/HTML |
| 流式 Markdown | `MarkdownStreamCollector` 缓冲 + render cache | 带单调边界追踪的 `StreamingMarkdown` | 2/5 | 无真正后缀增量解析；仅缓存未变帧 |
| 主题 | 预组合样式 + 命名主题构造 | 支持按主题名称自定义的主题系统 | 4/5 | 未接入设置/主题选择/自定义颜色 |
| 进度条 | 字符串函数 + styled line helper | 带 `fillColor`/`emptyColor` 的 React 组件 | 3/5 | 部分调用路径仍是纯文本 |
| 旋转指示器 | Braille 动画，缓冲区渲染 | Ink 旋转指示器组件 | 4/5 | 无滴答间隔管理 |
| 闪烁效果 | per-instance 逐字符正弦波动画 | 与 `TextHighlight` 分割器集成 | 3/5 | 未与高亮分段管线集成 |
| 工具活动 | 完整状态机 + JSON 摘要 + styled compact line | 内联 React 渲染 | 4/5 | 输出预览仍是简单字符串 |
| 差异（行级） | `similar` crate，带样式和词级高亮的缓冲区渲染 | 带 `StructuredDiff` 的 `diff` 模块 | 4/5 | 词级能力基础已补，复杂 diff 仍未完全 TS 对齐 |
| 差异（结构化） | 完整代码块解析 + 行号 | `StructuredPatchHunk` 类型 | 4/5 | 结构上等价 |
| 差异（对话框/文件列表） | `/diff` 来源切换 + 分页列表 + 详情视图 | 完整对话框组件 + 键盘导航 | 3/5 | 缺少颜色、差异语法高亮、PageUp/Down/详情滚动 |
| 文件编辑差异 | 统计信息 + 结构化代码块 | 懒加载、Suspense、上下文感知的代码块调整 | 3/5 | 无异步加载、无上下文感知的代码块调整 |
| 语法高亮 | feature-gated `syntect` fenced code highlighting | 基于 WASM 的 Shiki（`cliHighlight`） | 2/5 | 默认构建 fallback；差异主路径颜色保真不足 |
| 虚拟滚动 | O(log n) 二分查找，宽度感知 | Ink 虚拟列表 | 4/5 | 成熟，有测试覆盖 |
| 历史单元格 | 类型化枚举 + 提示/对话渲染 | 消息列表组件 | 3/5 | 简单字符串输出，无样式渲染 |
| Git 差异获取 | `git2` raw helper + `/diff` 数据适配层 | 调用 `git` CLI | 4/5 | 无 CLI 回退；raw helper 信息较少 |

---

## 2026-05-18 修复后遗留项

本轮针对 `worktree-ui-plan3` 先修复了构建失败、Markdown 表格状态机、流式 Markdown `commit()` 语义、词级 diff 两侧内容串线，以及 `syntect` feature 编译问题。验证命令：

- `cargo check -p claude-code-rs`
- `cargo check -p claude-code-rs --features syntect`
- `cargo test -p claude-code-rs ui::`

修复后仍需追踪的差距：

- **Markdown 表格**：已有基础表格解析、边框、列宽和对齐；仍缺少 TS `MarkdownTable` 的 ANSI 感知换行、多行单元格布局和更完整的复杂内容处理。
- **Markdown 链接化**：仍无 OSC 8 终端超链接、GitHub Issue/PR 引用链接化，以及 `mailto:` 特例处理。
- **Markdown 兼容性**：列表嵌套、有序列表字母/罗马数字编号、`def`/`del`/`html` 等 token 仍未按 TS 行为完整对齐。
- **流式 Markdown**：已修复渲染缓存不应推进 `commit()` 边界的问题，并保留纯文本 fast path；仍没有 TS `StreamingMarkdown` 的稳定前缀/不稳定后缀增量解析和块级滚动锚定。
- **语法高亮**：`syntect` feature 下可对 Markdown fenced code 做 token 着色；默认构建仍按 feature-gated fallback 输出 `theme.code` 单色样式。差异详情/文件编辑预览的主路径仍以字符串快照为边界，无法保留 token 级颜色。
- **主题**：新增命名主题构造能力，但尚未接入设置、主题选择 UI 或用户自定义颜色覆盖。
- **进度条与工具活动**：工具活动 compact 渲染已复用 styled line 和 styled progress bar；任务状态等其他字符串入口仍是纯文本进度条，工具输出预览也仍是简单字符串。
- **闪烁文本**：已改为 per-instance `ShimmerAnimation`；仍未接入 TS 的 `TextHighlight` 分段/组合管线，也没有主题色键配置。
- **差异渲染**：ratatui 行级 diff 已处理相邻 `-/+` 的词级高亮，结构化 diff 也有 styled renderer；差异对话框/文件列表仍缺完整键盘导航接线、轮次差异集成和异步/懒加载体验。
- **Git 差异获取**：已有 stats helper 和包含 untracked 的 status summary；`get_git_diff()` 仍没有 Git CLI fallback，也不会把未追踪文件内容纳入 diff 正文。
- **历史单元格**：已有 styled history helper；屏幕消息列表仍未切换到完整 styled history cell 渲染，也没有时间戳、UUID、token 用量等元数据展示。

---

## 领域：Markdown 渲染

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/markdown.rs`（259 行）
- `../../../crates/claude-code-rs/src/ui/rendering/markdown_render.rs`（63 行）
- `../../../crates/claude-code-rs/src/ui/rendering/markdown_stream.rs`（74 行）

### TS 文件
- `../../../../claude-code-bun/src/components/Markdown.tsx`（236 行）
- `../../../../claude-code-bun/src/utils/markdown.ts`（382 行）
- `../../../../claude-code-bun/src/components/MarkdownTable.tsx`
- `../../../../claude-code-bun/src/utils/textHighlighting.ts`

### Rust 完成度：3/5

### 当前状态
Rust Markdown 使用 `pulldown_cmark` 配合 LRU 缓存（256 条目），支持：标题（H1 粗体+下划线，H2+ 粗体）、粗体、斜体、内联代码（`theme.code`）、围栏/缩进代码块、无序/有序列表（单层字母前缀）、链接（下划线）、块引用（暗淡 `|` 前缀）、水平分割线、软/硬换行和基础表格。解析器选项中已启用删除线。

`markdown_render.rs` 外观层将输出包装为 ratatui `Text`，并提供 `wrap_lines()` 工具函数；样式保留换行已补齐，换行后的 span 会继续携带原样式。

`MarkdownStreamCollector` 缓冲原始文本，保留纯文本 fast path，并缓存未变化的渲染帧；它仍不做真正的后缀级增量重新解析。

### 与 TS 相比缺失的功能
- **表格能力仍弱于 TS**——Rust 已有基础表格，但 TS `MarkdownTable` 具备 ANSI 感知换行、多行布局和更完整的复杂单元格处理。
- **默认构建无语法高亮**——`syntect` feature 下已有 fenced code token 着色；默认构建仍使用单色 fallback。TS 为代码块使用懒加载的 WASM Shiki（`cliHighlight`）。
- **无超链接**（OSC 8）——TS 将链接包装为可点击的终端超链接。Rust 将其渲染为纯下划线文本。
- **无 GitHub Issue/PR 引用链接化**（`owner/repo#123`）。
- **列表嵌套能力较基础**——Rust 使用 `list_stack` 提供缩进式嵌套，但没有 TS 侧更完整的列表格式化能力。
- **无嵌套列表的有序列表字母/罗马数字编号**。
- **无 `mailto:` 处理**——TS 跳过邮件链接的 OSC 8。
- **无 `def`/`del`/`html` 令牌处理**（在 TS 中返回为空，在 Rust 中未处理）。
- **无真正流式增量解析**——TS `StreamingMarkdown` 使用单调边界追踪，仅重新解析不稳定的后缀。Rust 仅缓存未变化帧。
- **Markdown 语法快速路径较弱**——Rust 已有字符级 fast path 处理纯文本，仍没有 TS 那种完整正则检测策略。

### 影响
用户会看到基础 Markdown 表格，但复杂表格仍会降级。默认构建下代码块仍没有语法高亮。长流式响应仍可能产生不必要的完整重新解析成本。超链接不可点击。

---

## 领域：主题

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/theme.rs`（117 行）

### TS 文件
- `../../../../claude-code-bun/packages/@ant/ink/src/theme/`（多个主题文件）

### Rust 完成度：4/5

### 当前状态
主题定义了 25 个预组合的 `Style` 字段，涵盖：助手/用户/系统名称、工具名称/结果、错误/警告/信息、提示、边框、代码、思考、暗淡、标题、粗体、斜体、链接、差异添加/删除/上下文/头部、已选/未选。所有颜色在 `Default::default()` 中硬编码为 RGB。`Theme::new()` 构造函数仅调用 `Default::default()`，没有任何自定义。

### 与 TS 相比缺失的功能
- **无命名主题切换**——TS 支持多个主题名称（例如 `"dark"`、`"light"`、`"tron"`），具有不同的调色板。Rust 有一个硬编码主题。
- **无带自定义颜色的 `new()`**——`new()` 方法不接受参数；自定义需要使用结构体字面量构造。
- **主题样式覆盖仍不均匀**——`selected`/`unselected` 已被 command palette、permission overlay 等路径使用，但不少文本型渲染 helper 仍输出无主题样式字符串。
- **无来自设置/配置的颜色覆盖**——TS 主题可以通过用户设置自定义。

### 影响
视觉自定义受限。用户无法在亮/暗主题之间切换。固定的 RGB 值可能无法在所有终端配色方案上良好呈现。

---

## 领域：进度条

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/progress_bar.rs`（53 行）

### TS 文件
- `../../../../claude-code-bun/src/components/design-system/ProgressBar.tsx`（86 行）
- `../../../../claude-code-bun/src/components/design-system/ProgressBar.tsx`（1 行，再导出）

### Rust 完成度：3/5

### 当前状态
纯函数 `render_progress_bar(ratio, width) -> String`，使用 9 个 Unicode 块字符（`" ▏▎▍▌▋▊▉█"`）。与 TS 使用相同的字符集和算法。通过钳位处理 NaN、负值和溢出比率。有边界情况的单元测试。

该函数在 `tool_activity.rs` 中用于内联进度渲染（10 个字符宽）。

### 与 TS 相比缺失的功能
- **无颜色支持**——TS `ProgressBar` 有 `fillColor` 和 `emptyColor` 属性，类型为 `keyof Theme`。Rust 返回没有样式的纯字符串。
- **不是 UI 组件**——Rust 是返回 `String` 的纯函数；TS 是与 Ink 渲染管线集成的 React 组件。
- **无可配置宽度**——宽度在调用点硬编码（工具活动中始终为 10）；TS 接受任意宽度属性。

### 影响
工具活动输出中的进度条显示为无颜色的文本，而不是视觉上带有样式的条。

---

## 领域：旋转指示器

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/spinner.rs`（96 行）

### TS 文件
- Ink 框架 `useInput` / `../../../../claude-code-bun/packages/@ant/ink/` 中的旋转指示器组件

### Rust 完成度：4/5

### 当前状态
`SpinnerState` 结构体，使用 10 个 Braille 字符（`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`）进行动画帧追踪，与 TS 相同。提供 `tick()`、`start()`、`stop()`、`set_message()`、`render()` 方法。渲染到 ratatui 缓冲区中，使用主题样式（帧使用 `theme.info`，消息使用 `theme.dim`）。非活跃旋转指示器不渲染任何内容。

### 与 TS 相比缺失的功能
- **无集成的滴答定时器**——`tick()` 必须在外部按间隔调用。TS 旋转指示器与 Ink 的渲染循环集成。
- **无帧率控制**——80ms 间隔在组件内不可配置。
- **无颜色自定义**——始终为帧使用 `theme.info`，为消息使用 `theme.dim`。

### 影响
基本使用在功能上等价。正常运行中没有可见的用户层面差距。

---

## 领域：闪烁文本

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/shimmer.rs`（41 行）

### TS 文件
- `../../../../claude-code-bun/src/utils/textHighlighting.ts`（`TextHighlight` 类型中的闪烁效果）

### Rust 完成度：3/5

### 当前状态
`shimmer_spans(text)` 函数使用正弦波动画创建逐字符样式的区间。使用全局 `OnceLock<Instant>` 记录经过时间，这意味着动画阶段在所有闪烁实例之间共享。颜色在灰度范围（RGB 120-220）内扫描。

### 与 TS 相比缺失的功能
- **全局定时器状态**——所有闪烁实例共享相同的动画阶段，使它们全部同步脉冲。TS 按实例计时允许独立动画。
- **未与文本高亮管线集成**——TS 闪烁效果是 `TextHighlight` 段上的 `shimmerColor` 属性，可与其他高亮类型组合。Rust 闪烁效果是独立的。
- **无颜色自定义**——硬编码的灰度范围。TS 允许指定主题颜色键。
- **逐字符开销**——每个字符创建一个 `Span`，对于长字符串来说开销很大。

### 影响
闪烁效果可用但粗糙。当多个闪烁元素可见时，所有实例同步动画是明显的。

---

## 领域：工具活动

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/tool_activity.rs`（319 行）
- `../../../crates/claude-code-rs/src/ui/tasks/render_tool_activity.rs`（39 行）
- `../../../crates/claude-code-rs/src/ui/tool_activity.rs`（遗留文件，在 `ui/` 根目录）

### TS 文件
- 在 `../../../../claude-code-bun/src/components/Messages.tsx` / `MessageRow.tsx` 中内联渲染
- 通过 `AppStateStore` 追踪工具状态

### Rust 完成度：4/5

### 当前状态
`ToolActivity` 结构体带有 5 状态机（Queued/Running/Succeeded/Failed/Cancelled）。追踪：名称、面向用户的名称、参数摘要、状态、摘要文本、错误摘要、经过时间（毫秒）、进度（可选的完成/总数）、输出行数、输出预览（前几行）。提供 `compact_line()` 用于分组显示，`transcript_block()` 用于详细视图，以及 `display_call()` 用于格式化 `ToolName(args)` 输出。

包含 `render_grouped_activity()` 用于聚合多个工具活动，以及从任务系统桥接的 `render_task_tool_activity()`。

JSON 输入摘要，带有优先键提取（`path`、`command`、`pattern`、`url` 等），并回退到前 3 个键。使用 `insta` 进行快照测试。

### 与 TS 相比缺失的功能
- **`compact_line()` 中无带样式/颜色的输出**——状态标签、名称和摘要为纯文本。TS 使用主题感知着色渲染工具调用块。
- **无丰富的输出预览**——仅将前几行存储为字符串；无工具输出的结构化渲染。
- **无流式进度更新**——页面级进度作为快照更新；TS 可以在 Read/Edit 工具中流式传输增量文件内容。
- **`render_grouped_activity` 产生纯文本**——未与 ratatui 渲染集成；输出是 `String`，不是带样式的 `Line`。

### 影响
工具活动在文本上完整，但与 TS 相比视觉上平淡。`compact_line` 中缺少样式化输出意味着对话中内联的工具进度缺乏视觉层次。

---

## 领域：差异渲染

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/diff.rs`（261 行）——核心差异模块
- `../../../crates/claude-code-rs/src/ui/diff/diff_detail_view.rs`——详情视图
- `../../../crates/claude-code-rs/src/ui/diff/diff_dialog.rs`——对话框状态机
- `../../../crates/claude-code-rs/src/ui/diff/diff_file_list.rs`——文件列表
- `../../../crates/claude-code-rs/src/ui/diff/file_edit_diff.rs`——文件编辑统计
- `../../../crates/claude-code-rs/src/ui/diff/structured_diff.rs`——结构化代码块解析
- `../../../crates/claude-code-rs/src/ui/rendering/get_git_diff.rs`——Git 差异获取

### TS 文件
- `../../../../claude-code-bun/src/components/diff/DiffDetailView.tsx`
- `../../../../claude-code-bun/src/components/diff/DiffDialog.tsx`
- `../../../../claude-code-bun/src/components/diff/DiffFileList.tsx`
- `../../../../claude-code-bun/src/components/StructuredDiff.tsx`
- `../../../../claude-code-bun/src/components/StructuredDiffList.tsx`
- `../../../../claude-code-bun/src/components/FileEditToolDiff.tsx`

### Rust 完成度：3/5

### 当前状态
Rust 差异子系统是最完整的渲染领域，有多个反映 TS 组件结构的子模块。关键能力：

- **行级差异**：使用 `similar` crate 进行 `TextDiff::from_lines()`。渲染时带行号（旧/新）、+/- 前缀、主题着色（diff_add/diff_remove/diff_context/diff_header）和相邻 `-/+` 的词级高亮。显示"N more lines..."截断提示。
- **结构化差异**：`StructuredDiffHunk` 带有头部解析、行级详情（旧/新行号、类型）、带截断的格式化输出 `render_structured_diff_hunks()`，以及可保留样式的 `render_structured_diff_hunks_styled()`。
- **差异数据模型**：`DiffFile`、`DiffStats`、`DiffData` 类型匹配 TS `DiffData` 结构。支持 is_binary、is_large_file、is_truncated、is_untracked 标志。
- **文件列表**：带 `MAX_VISIBLE_FILES=5` 的分页渲染，文件统计显示。
- **差异对话框**：双模式（列表/详情）状态机，支持多个 `DiffSource`；`/diff` command surface 已接入 Staged/Unstaged 来源切换、文件选择和详情视图。
- **文件编辑差异**：使用 `TextDiff` 的 `unified_hunk_lines_from_edit()`、`file_edit_diff_stats()`、`format_file_edit_summary()`。
- **Git 差异获取**：`get_git_diff()` 通过 `git2` 库支持已暂存、未暂存和组合模式。

`diff_detail_view`、`diff_file_list` 和 `DiffSource` 数据模型已经被 `/diff` command surface 活跃使用；`diff_dialog::render_diff_dialog_lines()` 仍主要作为文本快照/组合 helper 存在，而不是完整独立 overlay。

### 与 TS 相比缺失的功能
- **词级差异仍是基础版**——Rust 已能处理相邻 `-/+` 行的词级高亮，但还没有 TS `StructuredDiff` 的完整字符级/语义级细节。
- **差异中语法高亮接线不完整**——结构化 styled renderer 有高亮入口，但主文件编辑预览/详情仍经过字符串边界，颜色保真不足。
- **无轮次差异集成**——TS `DiffDialog` 支持"轮次差异"（每轮对话差异快照）。Rust `DiffSource` 有数据模型但是死代码。
- **无交互式键盘导航**——TS 差异对话框支持箭头键、Page Up/Down 进行文件选择。Rust 对话框是无输入处理的死代码状态机。
- **无异步加载**——TS `FileEditToolDiff` 使用 `Suspense` + `use()` 进行懒加载差异计算，带占位渲染。Rust `file_edit_diff` 是同步的。
- **无上下文感知的代码块调整**——TS `adjustHunkLineNumbers()` 在文件内容自差异生成后发生变化时调整代码块上下文。Rust 没有等价物。
- **独立 diff dialog helper 未完全产品化**——`render_diff_dialog_lines()` 可用于组合快照，但实际 UI 通过 `CommandSurface::Diff` 和 `BetterViewPanel` 组装，尚不是 TS 那种完整 overlay 组件。

### 影响
最显著的面向用户的影响已经从"完全没有词级 diff"收窄为"复杂 diff 和语法高亮保真不足"。三个死代码/弱接线子模块仍表明基于对话框的差异浏览体验（列表视图 -> 带键盘导航的详情视图）尚未完整接入 UI。

---

## 领域：Git 差异获取

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/get_git_diff.rs`（113 行）

### TS 文件
- `../../../../claude-code-bun/src/hooks/useDiffData.ts`（通过 shell 命令使用 `git` CLI）
- `../../../../claude-code-bun/src/utils/diff.ts`

### Rust 完成度：4/5

### 当前状态
使用 `git2` 库（libgit2 绑定）进行原生 Git 操作。`get_git_diff()` 支持三种 raw diff 模式：已暂存（HEAD 到索引）、未暂存（索引到工作目录）、组合（两者皆有，带章节标题）。有 `get_status_summary()` 返回格式化的 Git 状态输出，带标记（A/M/D/??/ M/ D）。`/diff` command surface 走单独的数据适配层，会从 `git2::Diff` 解析 `DiffStats`、文件列表、hunks 和 `is_untracked` 标志。

### 与 TS 相比缺失的功能
- **无 `git diff` CLI 回退**——TS 调用 `git diff` CLI，可与任何 Git 版本配合使用。Rust 依赖 `git2` 库，可能不支持所有 Git 特性。
- **raw `get_git_diff()` 无差异统计信息解析**——TS 从输出中解析差异统计信息。Rust 的 raw helper 返回原始字符串；`/diff` 适配层已通过 `diff.stats()` 解析统计信息。
- **raw `get_git_diff()` 对未追踪文件展示有限**——`get_status_summary()` 和 `/diff` 数据适配层能看到未追踪文件；raw patch 字符串 helper 仍主要面向已存在 diff 内容。

### 影响
常见情况下功能等价。`git2` 依赖比调用 shell 更可靠，但可能滞后于 Git CLI 的特性。

---

## 领域：虚拟滚动

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/virtual_scroll.rs`（347 行）

### TS 文件
- Ink 框架虚拟列表（在 `../../../../claude-code-bun/packages/@ant/ink/src/` 中）
- `../../../../claude-code-bun/src/components/Messages.tsx`（消息列表）

### Rust 完成度：4/5

### 当前状态
完整的虚拟滚动实现，具有：
- **每条消息高度缓存**：维护逻辑行数和宽度感知的换行后可视行数。
- **前缀和偏移数组**：两个数组（逻辑偏移、可视偏移）支持在 O(log n) 时间内对可见范围进行二分查找。
- **过扫描**：在视口上下额外渲染 40 行以实现平滑滚动。
- **宽度感知的缓存失效**：在终端大小调整时，可视高度完全重新计算，但逻辑高度保持不变。
- **通过 `partition_point()` 进行二分查找**：将滚动偏移映射到可见消息索引范围。
- **全面的测试覆盖**：宽度变更重新计算、可视范围边界测试、失效测试。

### 与 TS 相比缺失的功能
- **集成方式较手动**——`App` 的 prompt/transcript 渲染路径已经调用 `ensure_up_to_date()`，`render_messages()` 使用 `visual_range()` 渲染可见消息；但与 TS React 组件相比，缓存更新仍由调用方显式管理。
- **无焦点/选择追踪**——仅处理可见性，不处理哪个消息当前是"活跃的"。
- **逻辑高度 helper 仍是兼容/诊断路径**——主渲染器已经使用可视偏移；`total_lines()` 和 `visible_range()`（逻辑）仍保留给旧调用或测试场景。

### 影响
虚拟滚动实现成熟且经过良好测试，并已接入主消息渲染路径。剩余差距主要是 React 式封装、焦点/选择追踪，以及逻辑高度 helper 的历史兼容残留。

---

## 领域：历史单元格

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/history_cell.rs`（77 行）

### TS 文件
- `../../../../claude-code-bun/src/components/MessageRow.tsx`（消息行渲染）
- `../../../../claude-code-bun/src/types/message.ts`（消息类型层次结构）

### Rust 完成度：3/5

### 当前状态
类型化的 `HistoryCell` 枚举，有 6 个变体：User、Assistant、System、Tool（名称+摘要）、Diff（路径+添加/删除）、Status。`HistoryRenderMode` 枚举有 Prompt 和 Transcript 模式。`render_history()` 将所有单元格用换行符连接。使用带有 `format!()` 的纯文本，无样式。

### 与 TS 相比缺失的功能
- **无样式渲染**——输出是纯文本，无颜色、无缩进、无主题应用。TS 消息渲染使用完整的 Ink 组件树，带有主题感知的样式。
- **无丰富的工具调用显示**——工具单元格仅存储名称+摘要字符串。TS 渲染带参数、持续时间和带样式输出预览的工具调用。
- **无代码块格式化**——助手文本原样存储；历史单元格内无 Markdown 渲染。
- **无消息元数据**（时间戳、UUID、用量信息）。

### 影响
历史单元格作为提示/对话导出的简单文本表示。它们不用于屏幕上的渲染。对于预期的用例（上下文窗口显示），这个差距是可接受的。

---

## 领域：语法高亮

### Rust 文件
- `../../../crates/claude-code-rs/src/ui/rendering/syntax_highlight.rs`
- `../../../crates/claude-code-rs/src/ui/rendering/markdown.rs`
- `../../../crates/claude-code-rs/Cargo.toml` — `syntect` 仍是 optional feature

### TS 文件
- `../../../../claude-code-bun/src/utils/cliHighlight.ts`（WASM Shiki 加载器）
- `../../../../claude-code-bun/src/components/Markdown.tsx`（Suspense + use(highlight)）
- `../../../../claude-code-bun/src/utils/markdown.ts`（formatToken 高亮集成）

### Rust 完成度：2/5

### 当前状态
Rust 已新增 `syntax_highlight.rs`，在 `syntect` feature 下对 fenced code block 做 token 级着色，并从 `CodeBlockKind::Fenced(lang)` 提取语言标识符。默认 feature 仍为空，因此默认构建会走 `theme.code` 单色 fallback。

TS 实现通过 WASM 加载 Shiki（`cliHighlight.ts`），使用 `Suspense` 进行懒加载。在约 50ms 的加载期间，显示无高亮的回退内容。加载后，对每个代码块调用 `highlight.highlight(text, { language })`，对于不支持的语言回退到 `plaintext`。语言检测来自围栏代码块的信息字符串。

### 与 TS 相比缺失的功能
- **默认构建不启用语法高亮**——需要 `--features syntect` 才能获得 token 级着色。
- **差异主路径颜色保真不足**——结构化 styled renderer 有高亮入口，但文件编辑预览/详情等字符串边界会丢失颜色。
- **无语言选择器 UI**——不支持 TS 侧类似语言覆盖/选择的交互。
- **无流式代码块增量高亮**——仍不是逐行维护 highlighter 状态的增量模式。

### 补齐计划
详见 `docs/ui/great/plans/plan-06-syntax-highlighting.md`：
- 阶段 1-3 已有基础落地：syntect 集成、fenced lang 提取、token → ratatui spans。
- 阶段 4 仅有结构化 styled renderer 入口，主 UI 字符串边界仍需继续接线。
- 阶段 5: 语言选择器 UI（模糊匹配 + 覆盖层下拉选择）仍未实现。
- 阶段 6: 流式代码块增量高亮（逐行 `HighlightLines::highlight()`）仍未实现。
- 估算: ~600 行新代码（`#[cfg(feature = "syntect")]` 编译门控）

### 影响
默认构建下 Markdown 输出和差异代码块仍显示为纯单色文本。启用 `syntect` 后 Markdown fenced code 有彩色 token，但差异视图和字符串预览还不能完整保留语法颜色。

---

## 横切关注点

### 渲染管线集成
Rust 渲染模块主要是纯函数或字符串生成器，而 TS 组件是与 Ink 渲染循环集成的 React 组件。`rendering/` 中的几个 Rust 模块仍是工具库形态，但虚拟滚动和 `/diff` 已经进入实际 UI 路径。差异体验的剩余问题不是完全未接线，而是 overlay 形态、样式层级、详情滚动和快捷键覆盖仍弱于 TS。

### 测试覆盖
Rust 模块通常有单元测试（markdown、progress_bar、shimmer、tool_activity、structured_diff、virtual_scroll），有些使用 `insta` 进行快照测试。当前也已有 `runtime/visual_regression.rs` 覆盖 foundational / interactive UI surfaces。与 TS 相比，缺口更准确地说是缺少基于真实终端尺寸、颜色样式和交互序列的端到端视觉回归覆盖。

### 流式支持
TS 有成熟的流式优化：
- `StreamingMarkdown` 带单调边界追踪（避免重新解析稳定的前缀）
- 流式过程中的块级滚动锚定
- 工具调用的流式进度指示器

Rust 的 `MarkdownStreamCollector` 已有纯文本 fast path 和未变化帧缓存，但没有稳定前缀/不稳定后缀的真正增量解析。在长流式会话中，这种差异仍会明显。

### 颜色/样式保真度
TS 使用 ANSI 转义序列（通过 `chalk`）和 Ink 的组件样式，允许逐字符粒度。Rust ratatui `Style` 在 `Span` 级别工作。`markdown_render.rs` 的换行函数已保留 span 样式；剩余问题集中在字符串边界会丢失颜色，以及部分渲染入口尚未切换到 styled line。

### 依赖方法
TS 使用 WASM（Shiki）和 JavaScript 库（`marked`），它们提供丰富的功能集，但有懒加载代价。Rust 使用原生 Rust 库（`pulldown_cmark`、`similar`、`git2`），这些库更快但内置功能较少。这种权衡使 TS 在功能丰富性方面占优，而 Rust 在原始解析性能方面占优。
