# 执行计划：输入/编辑器补齐 — 6 个缺失功能

> **生成日期**: 2026-05-18
> **来源**: `docs/ui/great/06_missing_features_summary.md` §3
> **参考实现**:
> - TS: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/components/PromptInput/`
> - Rust: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/prompt_input.rs`
> - Rust Vim: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/input/vim.rs`
> - Rust ChatComposer: `crates/claude-code-rs/src/ui/components/chat_composer.rs`
> - Rust Clipboard: `crates/claude-code-rs/src/ui/input/clipboard_paste.rs`

---

## 总览

| # | 功能 | Rust 行数 | TS 参考总行数 | 当前状态 | 目标状态 | 估算工作量 |
|---|------|:----------:|:-------------:|:--------:|:--------:|:----------:|
| 1 | 多行输入 | ~400 (PromptInput) | ~4200 (全部 PromptInput/) | 单行 | 完整多行编辑 | ~600 行 |
| 2 | 文本高亮 | 0 | ~400 (highlighting + shimmer) | 纯文本 | 8+ 种高亮类型 | ~500 行 |
| 3 | 撤销/重做 | 0 + 映射为空操作 | ~132 (useInputBuffer) | 无操作 | 50 条目撤销缓冲区 | ~150 行 |
| 4 | 图片粘贴 | 470 (死代码) | ~400 (imagePaste + imageStore) | 死代码未接线 | 从死代码到接线 | ~100 行 |
| 5 | 输入建议 | 0 | ~250 (useTypeahead + ghost) | 无内联补全 | 斜杠命令内联建议 | ~300 行 |
| 6 | ChatComposer | 126 (纯状态) | 集成在 PromptInput(2650) | 状态模型无控件 | 完整 Widget 实现 | ~350 行 |

**总工作量估算**: ~2000 行新 Rust 代码（不含测试）

**关键设计原则**:
- 所有六项功能共享同一个数据模型 (`PromptInput.input: String` + `cursor_position: usize`)，改造时应避免破坏已有功能（Ctrl 快捷键、水平滚动、粘贴、历史搜索集成）。
- TS 的 PromptInput.tsx（2650 行）整合了所有功能，但 Rust 应采用模块化拆分：每个功能作为一个独立的模块/子系统，通过 trait 或消息与 `PromptInput` 核心交互。
- Vim 模式（916 行完整状态机）已连接到 `PromptInput`，但 `Undo` 映射为空操作。补齐撤销/重做时应优先复用现有的 `VimAction::Undo` 路由。

---

## 1. 多行输入

### 当前状态
`PromptInput`（`prompt_input.rs:13-24`）明确定义为单行输入。Enter 和 Shift+Enter 都提交输入。渲染函数（`render_with_context`，`prompt_input.rs:185-284`）假设高度为 1 行，使用水平滚动显示长文本。在 `app/render.rs:184-193` 中，`render_with_context` 被分配一个固定高度的 `Rect` 区域。

### 目标状态
支持多行文本编辑：
- Enter 插入换行符；仅在没有输入内容时，Ctrl+Enter 或命令面板提交
- Shift+Enter 提交（与 TS `getNewlineInstructions` 一致的交换逻辑）
- 光标在行间上下移动（↑/↓ 只在首行/末行时触发历史/页脚导航）
- 输入区域根据行数动态扩展高度（最多 N 行，超出后启用内部滚动视口）
- 文本换行（word wrap）显示在输入区域内

### TS 参考
| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| multiline prop | `types/textInputTypes.ts:60-63` | `BaseTextInputProps.multiline` 布尔标志 |
| 换行渲染 | `BaseTextInput.tsx` | `useTextInput` hook 处理换行和视口 |
| 多行光标 | `useVimInput.ts` | `useTextInput` 跟踪 `cursorLine`/`cursorColumn` |
| newline 指令 | `PromptInput/utils.ts:17-32` | `getNewlineInstructions()` 决定 Enter vs Shift+Enter |
| 行感知历史 | `PromptInput.tsx:1063-1093` | `isCursorOnFirstLine`/`isCursorOnLastLine` 决定 ↑/↓ 是否触发历史 |
| 输入视口 | `PromptInput.tsx:222-223` | `MIN_INPUT_VIEWPORT_LINES = 3`，`maxVisibleLines` 属性 |

### Rust 参考
- `prompt_input.rs:13-24` — 单行数据结构
- `prompt_input.rs:185-284` — 单行渲染
- `app/input.rs:180-186` — ↑/↓ 始终触发历史（没有行感知）
- `app/render.rs:184-193` — 固定高度输入区域

### 实施步骤
- [ ] 步骤 1: 扩展 `PromptInput` 数据结构，支持多行：添加行缓存、行高计算、视口偏移。
- [ ] 步骤 2: 修改 `handle_key`：Enter 插入 `\n` 而非提交，Shift+Enter 提交，↑/↓ 行间移动光标（仅首行/末行触发外部历史/页脚导航），Home/End 移动到行首/行尾。
- [ ] 步骤 3: 实现多行渲染：计算可见行数，渲染带换行的输入区域，支持内部滚动视口。
- [ ] 步骤 4: 更新 `app/render.rs` 中分配给 PromptInput 的区域高度，使其随内容动态变化（上限 N 行）。
- [ ] 步骤 5: 更新 `app/input.rs` 中 ↑/↓ 的历史触发逻辑，加入 `isCursorOnFirstLine`/`isCursorOnLastLine` 检查。
- [ ] 步骤 6: 添加 `multiline` 配置选项（对应 TS `multiline` prop），默认启用。
- [ ] 步骤 7: 为现有 Ctrl+U/E/A/W/K 快捷键确认行为在多行模式下正确（Ctrl+U 清除全部，Ctrl+A/E 移动到文本首/尾，Ctrl+W 删除前一个单词，Ctrl+K 删除到行尾）。

### 依赖
- 无（独立修改，但渲染区域计算需要与 app/render.rs 协调）

### 估算工作量
~600 行（数据结构扩展 100 + 按键处理 150 + 渲染 250 + 集成 100）

---

## 2. 文本高亮（彩虹色、@提及、/命令、token 预算）

### 当前状态
所有输入以 `Span::raw` 纯文本渲染（`prompt_input.rs:263-276`）。无高亮支持。

### 目标状态
在输入渲染中支持 8+ 种高亮类型：
- **/command** 高亮（蓝色 `theme.suggestion`）
- **@name** 团队成员提及高亮（使用对应成员的 agent 颜色）
- **彩虹色关键词**（ultrathink/ultraplan/ultrareview/buddy 触发器，逐字符循环颜色）
- **btw** 侧问触发器（黄色）
- **token budget** 标记（蓝色）
- **slack channel** 提及（蓝色）
- **[Image #N]** 药丸（光标在 start 时反色）
- **历史搜索**匹配（黄色）
- **语音听写**临时范围（暗淡）

### TS 参考
| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| TextHighlight 类型 | `utils/textHighlighting.ts:17-24` | `{start, end, color, dimColor?, inverse?, shimmerColor?, priority}` |
| segmentTextByHighlights | `utils/textHighlighting.ts:34-59` | 高亮排序、去重叠、分段算法 |
| combinedHighlights 计算 | `PromptInput.tsx:715-858` | 8+ 种高亮源合并 |
| ShimmeredInput 渲染 | `PromptInput/ShimmeredInput.tsx:17-116` | 分段 + 动画 shimmer 渲染 |
| 彩虹色函数 | `utils/thinking.ts:findThinkingTriggerPositions + getRainbowColor` | 逐字符颜色 |
| 命令位置 | `utils/suggestions/commandSuggestions.ts` | `findSlashCommandPositions` |
| 提及位置 | `PromptInput.tsx:649-689` | `memberMentionHighlights` 正则匹配 |
| token budget | `utils/tokenBudget.ts` | `findTokenBudgetPositions` |

### Rust 参考
- `prompt_input.rs:260-278` — 当前纯文本渲染
- `prompt_input.rs:27-31` — `PromptInputRenderContext` 只有 hint/placeholder/mode_indicator

### 实施步骤
- [ ] 步骤 1: 定义 `TextHighlight` 结构体：`{start: usize, end: usize, color: Option<ThemeColor>, dim: bool, inverse: bool, priority: u8}`（适配 ratatui `Style` 而非 TS 的 `keyof Theme`）。
- [ ] 步骤 2: 实现 `segment_text_by_highlights()` 函数：高亮按 start 排序 → 按 priority 去重叠 → 分段切割。
- [ ] 步骤 3: 在 `PromptInput` 中添加 `highlights: Vec<TextHighlight>` 字段，提供 `set_highlights()` 方法。
- [ ] 步骤 4: 改造 `render_with_context()`，当 `highlights` 非空时使用分段渲染（多 `Span` 代替单 `Span::raw`）。
- [ ] 步骤 5: 实现 `/command` 检测：在用户输入时扫描 `displayedValue` 中的 `/command_name` 模式，与已注册命令列表匹配。
- [ ] 步骤 6: 实现 `@name` 提及检测：通过团队上下文匹配已知成员名称，分配颜色。
- [ ] 步骤 7: 实现关键词触发器检测（ultrathink/ultraplan/btw/buddy）：扫描已知关键字位置。
- [ ] 步骤 8: 实现 token budget 和 slack channel 标记检测。
- [ ] 步骤 9: 集成到渲染管线：`App` 在每一帧计算 `highlights` 向量并传递给 `PromptInput`。
- [ ] 步骤 10: 添加可选的 shimmer 动画支持（`HighlightedInput` 风格的逐帧更新），仅在彩虹色高亮存在时启用。

### 依赖
- 与多行输入（#1）共享渲染管线 — 高亮分段应在多行渲染之上叠加

### 估算工作量
~500 行（高亮类型/分段 150 + 检测逻辑 150 + 集成渲染 150 + shimmer 动画 50）

---

## 3. 撤销/重做

### 当前状态
`VimAction::Undo`（`input/vim.rs:94-95`）在正常模式下按 `u` 触发，但在 `app/input.rs:691` 映射到 `AppAction::None`（无操作）。全局搜索 `undo_history`/`undo_stack` 零结果。

### 目标状态
支持撤销/重做编辑操作：
- 编辑历史缓冲区（50 条记录，1 秒去抖）
- 每次文本变更时保存快照（文本 + 光标位置）
- `VimAction::Undo` (`u` in normal mode) 和 `Ctrl+_` 触发撤销
- `Ctrl+R` 或自定义快捷键触发重做（TS 没有显式重做，但 `useInputBuffer` 支持通过 buffer index 恢复）
- 撤销后恢复光标位置

### TS 参考
| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| useInputBuffer 实现 | `hooks/useInputBuffer.ts:1-132` | `BufferEntry {text, cursorOffset, pastedContents, timestamp}` |
| pushToBuffer | `hooks/useInputBuffer.ts:34-67` | 1s 去抖、50 条目上限、最后条目去重 |
| undo 调用 | `PromptInput.tsx:969-973` | `{pushToBuffer, undo, canUndo, clearBuffer}` |
| onChange 中 push | `PromptInput.tsx:1029-1031` | 每次值变化时 push |
| 快捷键绑定 | `PromptInputHelpMenu.tsx:29` | `chat:undo` 绑定到 `ctrl+_` |

### Rust 参考
- `input/vim.rs:94-95` — `VimAction::Undo` 枚举变体
- `input/vim.rs:394` — `KeyCode::Char('u')` → `VimAction::Undo`
- `app/input.rs:691` — `VimAction::Undo` → `AppAction::None`（当前空操作）
- `app/input.rs:194-201` — vim 动作应用流程

### 实施步骤
- [ ] 步骤 1: 新增 `undo_history.rs` 模块，定义 `UndoBuffer` 结构体和 `UndoEntry { text: String, cursor_position: usize }`。
- [ ] 步骤 2: 实现 `push()`：添加条目，上限 50 条，1 秒去抖，最后条目去重。
- [ ] 步骤 3: 实现 `undo()`：返回上一个条目并维护 index（支持多次撤销）。
- [ ] 步骤 4: 实现 `redo()`：前进到下一个条目。
- [ ] 步骤 5: 将 `UndoBuffer` 集成到 `App` 状态中。
- [ ] 步骤 6: 在 `PromptInput::handle_key()` 每次变更前调用 `push()`。
- [ ] 步骤 7: 修改 `app/input.rs:691`：将 `VimAction::Undo` 连接到 `UndoBuffer::undo()`。
- [ ] 步骤 8: 添加 Ctrl+`_` / Ctrl+`/`（标准终端撤销快捷键）处理。
- [ ] 步骤 9: 添加可选的撤销提示显示（如 TS 的 `Ctrl+_ to undo`）。

### 依赖
- 与多行输入（#1）共享光标模型，但实现可独立进行

### 估算工作量
~150 行（UndoBuffer 结构 80 + 集成 50 + 快捷键 20）

---

## 4. 图片粘贴

### 当前状态
`input/clipboard_paste.rs`（470 行）包含完整的跨平台图片粘贴代码：
- Windows: PowerShell `Get-Clipboard -Format Image`
- macOS: `pngpaste` 命令
- Linux: `wl-paste` 或 `xclip`
- WSL: Windows 回退 + 路径转换
- 所有代码完整，会编码为 PNG 并写入临时文件，返回 `(Vec<u8>, PastedImageInfo)`

但该模块从未被事件处理器调用。`Event::Paste` 只触发文本粘贴（`app/input.rs:305-314`）。Ctrl+V 在按键处理中没有绑定。也没有 `[Image #N]` 药丸插入逻辑。

### 目标状态
用户粘贴图片时：
- Ctrl+V 检测剪贴板中的图片（优先于文本）
- 图片编码为 base64 PNG，存储在 `pasted_contents` 映射中
- 输入中插入 `[Image #N]` 药丸占位符
- 提交时，药丸展开为实际的 `image_block` 内容
- 药丸渲染为不可编辑的反色区域（光标接触时反色，光标在内时吸附到边缘）

### TS 参考
| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| getImageFromClipboard | `utils/imagePaste.ts` | 检查剪贴板是否有图片 |
| storeImage | `utils/imageStore.ts` | 存储 base64 并分配 ID |
| formatImageRef | `history.ts` | 生成 `[Image #N]` 格式 |
| onImagePaste 回调 | `PromptInput.tsx:参数` | paste → store → insert ref |
| 图片药丸渲染 | `PromptInput.tsx:691-697, 715-731` | `imageRefPositions` + 反色高亮 |
| 光标吸附 | `PromptInput.tsx:707-713` | 光标在药丸内时吸附到边界 |
| 提交时展开 | `PromptInput.tsx` | 提交时将 `[Image #N]` 替换为 image_block |

### Rust 参考
- `input/clipboard_paste.rs:51-56` — `paste_image_as_png()` 入口函数（完整代码，`feature = "image"` 开关）
- `input/clipboard_paste.rs:367-378` — `pasted_image_format()` 路径扩展名检测
- `prompt_input.rs:142-146` — `paste_text()` 文本粘贴入口
- `app/input.rs:305-314` — `handle_paste_event()` 当前仅处理文本

### 实施步骤
- [ ] 步骤 1: 确认 `feature = "image"` 的启用状态和依赖（`image` crate、`tempfile`）。确保 `Cargo.toml` 中这些依赖已声明。
- [ ] 步骤 2: 在 `PromptInput` 中添加 `pasted_images: Vec<(usize, Vec<u8>, ImageInfo)>` 字段，管理 `[Image #N]` 药丸。
- [ ] 步骤 3: 实现 `paste_image()` 方法：调用 `clipboard_paste::paste_image_as_png()` → base64 编码 → 分配 ID → 插入 `[Image #N]` → 存储数据。
- [ ] 步骤 4: 在 `app/input.rs` 的按键处理中添加 Ctrl+V 检测：尝试图片粘贴，若无图片则回退到文本粘贴（`Event::Paste` 已有处理）。
- [ ] 步骤 5: 在渲染中添加药丸占位符：检测 `[Image #N]` 模式，渲染为带样式的不可编辑块（光标吸附逻辑）。
- [ ] 步骤 6: 确保提交时 `[Image #N]` 被展开为实际的 image_block 内容。

### 依赖
- `image` crate 和 `tempfile` crate（已在 `Cargo.toml` 中声明为 optional）
- 提交管道需要支持 image_block 展开

### 估算工作量
~100 行（接线 30 + 药丸管理 40 + 渲染 30）（存疑的 clipboard_paste.rs 已有 470 行）

---

## 5. 输入建议 / 自动补全（inline ghost text）

### 当前状态
命令面板（`command_palette/mod.rs`）处理 `/` 命令，但作为模态叠加层显示，而非内联建议。没有输入过程中的虚影文本（ghost text）或自动补全。没有底部建议列表。

### 目标状态
用户输入 `/` 命令时：
- 输入中显示灰色虚影文本提示命令补全（如 `/comm` → `/commit`，`mit` 为虚影）
- 按 Tab 接受补全
- 底部区域显示建议列表（命令描述、参数提示）
- 非命令输入时，根据上下文显示适当的提示（文件路径、agent 名称等）

### TS 参考
| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| useTypeahead | `hooks/useTypeahead.js` | 基于输入产生 `InlineGhostText` |
| InlineGhostText | `types/textInputTypes.ts:11-16` | `{text, fullCommand, insertPosition}` |
| 建议列表 | `PromptInputFooterSuggestions.tsx` | `SuggestionItem` 列表 + 渲染 |
| 建议集成 | `PromptInput.tsx:587-596, PromptInputFooter.tsx` | suggestions → footer 渲染 |
| 接受补全 | `BaseTextInput.tsx` | Tab 键接受 ghost text |

### Rust 参考
- `command_palette/mod.rs` — `/` 命令模态面板
- `command_palette/filter.rs` — 现有模糊匹配算法
- `command_palette/metadata.rs` — 命令元数据

### 实施步骤
- [ ] 步骤 1: 定义 `InlineSuggestion` 结构体：`{text: String, insert_position: usize, full_command: String}`。
- [ ] 步骤 2: 实现 `suggest_command()` 函数：当输入以 `/` 开头时，与注册的命令名称模糊匹配，生成补全文本。
- [ ] 步骤 3: 在 `PromptInput` 中添加 `inline_suggestion: Option<InlineSuggestion>` 字段和 Tab 接受逻辑。
- [ ] 步骤 4: 改造渲染：在光标后渲染暗淡的虚影文本。
- [ ] 步骤 5: 实现底部建议渲染区域，复用 `render_suggestions()`（现有在 `app/render.rs` 中）。
- [ ] 步骤 6: 添加建议导航（↑/↓ 切换建议，Tab 接受，Esc 关闭）。
- [ ] 步骤 7: 扩展建议源到文件路径和 agent 名称（可后续迭代）。

### 依赖
- 与多行输入（#1）和高亮（#2）共享渲染区域

### 估算工作量
~300 行（数据模型 50 + 匹配逻辑 80 + 渲染 100 + 导航集成 70）

---

## 6. ChatComposer — 从状态模型到完整 Widget

### 当前状态
`ChatComposerState`（`components/chat_composer.rs:21-26`）是一个纯状态结构体（126 行），包含 `input: String`、`mode: ComposerMode`、`busy: bool`、`queued: VecDeque<String>`。`render_lines()` 方法（`chat_composer.rs:104-115`）用 `\n` 替换换行符后输出调试字符串，不是真正的 ratatui Widget。

### 目标状态
ChatComposer 作为完整的 ratatui Widget：
- 管理输入状态（委托给 `PromptInput`）
- 管理模式（Insert/VimNormal/Slash/Busy）
- 管理提交队列（忙时排队）
- 渲染输入区域 + 底部状态行 + 排队命令状态
- 与事件循环和按键处理集成

### TS 参考
TS 中没有独立的 ChatComposer 组件 — 所有功能集成在 `PromptInput.tsx`（2650 行）中。关键集成点：

| 引用点 | 文件:行号 | 关键内容 |
|--------|:----------|----------|
| 队列管理 | `PromptInputQueuedCommands.tsx` | 排队命令渲染 |
| 模式管理 | `PromptInput.tsx:226-269` | props: mode, onModeChange |
| 忙状态 | `PromptInput.tsx` | `isLoading` prop 控制 |
| 提交逻辑 | `PromptInput.tsx` | `onSubmit` 回调 |
| 底部状态 | `PromptInputFooter.tsx` | 模式/状态指示器 |

### Rust 参考
- `components/chat_composer.rs:1-126` — 当前纯状态模型
- `prompt_input.rs:1-473` — 底层输入控件
- `app/input.rs:1-700` — 按键处理路由
- `app/render.rs:170-218` — 底部区域渲染

### 实施步骤
- [ ] 步骤 1: 将 `ChatComposerState` 重构为 `ChatComposer` 结构体，包含 `PromptInput` 实例代替裸 `String`。
- [ ] 步骤 2: 实现 `handle_key()` 方法：委托按键处理到 `PromptInput`，处理模式切换和忙状态。
- [ ] 步骤 3: 实现 `render()` ratatui Widget：渲染输入区域 + 模式指示器 + 排队命令状态行。
- [ ] 步骤 4: 将 `ChatComposer` 集成到 `App` 中：替换 `App.prompt: PromptInput` 为 `App.composer: ChatComposer`（或共存）。
- [ ] 步骤 5: 更新 `app/render.rs`：底部区域由 ChatComposer 统一渲染（包含 input + 模式行 + 页脚）。
- [ ] 步骤 6: 更新 `app/input.rs`：按键路由通过 ChatComposer 转发到 PromptInput。
- [ ] 步骤 7: 适配现有功能：历史搜索、命令面板、粘贴等通过 ChatComposer 访问。

### 依赖
- 多行输入（#1）和高亮（#2）——ChatComposer 应封装升级后的 PromptInput
- 撤销/重做（#3）——通过 ChatComposer 暴露

### 估算工作量
~350 行（重构 100 + Widget 实现 150 + 集成 100）

---

## 综合时间线建议

```
Phase 1（独立、无阻塞）:
  ├── 3. 撤销/重做 (150 行) — 独立模块，不影响渲染
  └── 4. 图片粘贴 (100 行 + 470 行已存在) — 接线为主

Phase 2（渲染核心改造）:
  ├── 1. 多行输入 (600 行) — 最大改造，影响 render.rs
  └── 2. 文本高亮 (500 行) — 叠加在多行渲染之上

Phase 3（集成功能）:
  ├── 5. 输入建议 (300 行) — 依赖多行和高亮渲染区域
  └── 6. ChatComposer (350 行) — 封装所有上述功能

总计: ~2000 行新代码（不含测试）
```

---

## 风险评估

| 风险 | 影响 | 概率 | 缓解措施 |
|------|:----:|:----:|----------|
| 多行输入破坏现有快捷键行为 | 高 | 中 | 保留所有现有 Ctrl 快捷键并在 UI 测试中验证 |
| 高亮渲染性能（逐帧计算） | 中 | 低 | 仅在高亮变更时重算（memoization），shimmer 可选帧率 |
| 图片粘贴的跨平台兼容性 | 中 | 中 | clipboard_paste.rs 已有平台分支，在 WSL/X11/Wayland/macOS 上测试 |
| 撤销缓冲区内存使用（50 条 x 大输入） | 低 | 低 | 50 上限 + 去重 + 去抖已控制内存 |
| 命令建议与命令面板重叠 | 中 | 低 | 清晰划分：命令面板=叠加层激活，内联建议=输入时被动提示 |
