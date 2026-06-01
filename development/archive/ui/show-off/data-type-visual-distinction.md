# TUI 数据类型可视化区分方案

> 本文档分析 allthecodes TUI 系统中，不同数据类型如何被区分并展示到前端终端界面。
>
> 框架：ratatui 0.29 + crossterm 0.28
> Markdown 解析：pulldown-cmark 0.12
> 代码高亮：syntect 5（可选）

---

## 章节详细索引

| 章节 | 详细索引文章 | 重点 |
|---|---|---|
| 一、核心数据类型枚举 | [01-core-data-types.md](data-type-visual-distinction/01-core-data-types.md) | `Message`、`ContentBlock`、系统消息、工具状态、可见/隐藏/间接展示清单 |
| 二、六层区分机制 | [02-six-layer-distinction.md](data-type-visual-distinction/02-six-layer-distinction.md) | 角色样式、内容块细分、用户/系统/附件消息、工具状态支持边界 |
| 三、主题系统 | [03-theme-system.md](data-type-visual-distinction/03-theme-system.md) | 6 套主题、`ThemeColors`、`ThemeProvider`、语义颜色到 ratatui `Style` 的映射 |
| 四、聚合显示优化 | [04-aggregation-optimizations.md](data-type-visual-distinction/04-aggregation-optimizations.md) | `GroupedToolUse` / `CollapsedReadSearch` 支持与不支持的工具矩阵 |
| 五、渲染管线 | [05-rendering-pipeline.md](data-type-visual-distinction/05-rendering-pipeline.md) | 数据模型、预处理、布局、消息分发、专用渲染器、Markdown/Buffer 输出 |
| 六、关键技术文件索引 | [06-key-files-index.md](data-type-visual-distinction/06-key-files-index.md) | 按职责、目录、常查问题组织的源码入口索引 |
| 七、区分策略总结 | [07-distinction-strategy-summary.md](data-type-visual-distinction/07-distinction-strategy-summary.md) | 新增消息/内容块/工具类型时的维护清单、风险与测试建议 |

---

## 一、核心数据类型枚举

定义在 `crates/allthecodes-types/src/message.rs` 中。

### 1.1 顶层 `Message` 枚举

| 变体 | 含义 | 关键子字段 |
|---|---|---|
| `Message::User(UserMessage)` | 用户输入 | 提示词、命令、工具结果 |
| `Message::Assistant(AssistantMessage)` | 模型响应 | 文本、工具调用、思维过程 |
| `Message::System(SystemMessage)` | 系统事件 | 压缩边界、API 错误、信息提示 |
| `Message::Progress(ProgressMessage)` | 进度事件 | 工具/钩子执行进度 |
| `Message::Attachment(AttachmentMessage)` | 附件事件 | 文件编辑、队列命令、最大轮次等 |

### 1.2 `ContentBlock` 子类型

位于助理/用户消息内部的内容块：

| 变体 | 用途 |
|---|---|
| `Text` | 普通文本 / Markdown 富文本 |
| `ToolUse` | 工具调用请求 |
| `ServerToolUse` | MCP 服务端工具调用 |
| `ToolResult` | 工具执行结果 |
| `Thinking` | 模型思维链 |
| `RedactedThinking` | 被截断的思维链 |
| `ConnectorText` | 连接器文本 |
| `Image` | 图片数据 |

### 1.3 `SystemSubtype` 系统消息子类型

| 变体 | 含义 |
|---|---|
| `CompactBoundary` | 对话压缩边界 |
| `MicrocompactBoundary` | 微压缩边界（隐藏） |
| `ApiError` | API 调用错误 |
| `Informational(Info)` | 普通信息提示 |
| `Informational(Warning)` | 警告信息 |
| `Informational(Error)` | 错误信息 |
| `LocalCommand` | 本地命令执行 |
| `Warning` | 通用警告 |

### 1.4 工具调用状态 (`ToolState`)

| 状态 | 含义 |
|---|---|
| `Queued` | 已排队待执行 |
| `Running` | 正在执行 |
| `Succeeded` | 执行成功 |
| `Failed` | 执行失败 |
| `Cancelled` | 已取消 |

---

## 二、六层区分机制

### 第一层：消息角色 → 颜色 + 样式

映射定义在 `crates/allthecodes/src/ui/rendering/theme.rs` 的 `Theme` 结构体。

| 角色 | 特效 | Dark 主题颜色 | RGB | 用途 |
|---|---|---|---|---|
| `assistant_name` | **Bold** | 紫色 | `190,140,255` | 助理名称标签 |
| `user_name` | **Bold** | 浅蓝 | `100,200,255` | 用户名称标签 |
| `system_name` | *Italic* | 灰色 | `180,180,180` | 系统名称标签 |
| `tool_name` | **Bold** | 金色 | `255,200,100` | 工具名称标签 |
| `tool_result` | 普通 | 中灰 | `160,160,160` | 工具输出内容 |
| `error` | **Bold** | 红色 | `255,100,100` | 错误信息 |
| `warning` | 普通 | 琥珀色 | `255,200,80` | 警告信息 |
| `info` | 普通 | 天蓝 | `130,200,255` | 信息提示 |
| `thinking` | *Italic* | 暗灰 | `120,120,120` | 思维过程 |
| `heading` | **Bold + Underlined** | 白色 | `255,255,255` | 标题 |
| `link` | Underlined | 蓝色 | `100,180,255` | 链接 |
| `code` | Fg+Bg | 淡黄 on 深灰 | `220,220,180` on `40,40,40` | 行内代码 |
| `dim` | 普通 | 暗灰 | `100,100,100` | 次要文本 |
| `bold` | **Bold** | 继承前景 | — | 粗体文本 |
| `italic` | *Italic* | 继承前景 | — | 斜体文本 |

语法高亮专用（代码块内部）：

| 角色 | 颜色 | RGB |
|---|---|---|
| `syntax_keyword` | 粉红 |  |
| `syntax_string` | 绿色 |  |
| `syntax_comment` | 灰色斜体 |  |
| `syntax_type` | 青色 |  |
| `syntax_function` | 蓝色 |  |
| `syntax_number` | 橙色 |  |
| `syntax_builtin` | 暖黄 |  |

差异对比专用：

| 角色 | 颜色 |
|---|---|
| `diff_add` | 绿色 |
| `diff_remove` | 红色 |
| `diff_context` | 灰色 |
| `diff_header` | 蓝色 + Bold |

### 第二层：内容块内细分（同一条消息内）

以 Assistant 消息为例，不同 `ContentBlock` 变体的渲染效果：

```
# 文本块 → 完整 Markdown 渲染（标题、粗体、代码、表格等）
这是普通的回复文本

# 工具调用 → 特定图案 + 颜色
  ● Read(file.rs)              ← tool_name（金色加粗）
   ⎿  Bash(command)            ← 续行标记
  ● TodoWrite                  ← 特定工具名
   [x] Task 1                  ← 复选框格式

# 思维过程 → 特殊标记 + 暗色斜体
∴ Thinking...                 ← thinking（暗灰斜体）

# 错误 → 红色加粗
Error occurred: API timeout   ← error
```

不同工具的调用显示差异化：

| 工具 | 显示格式 |
|---|---|
| **Bash / PowerShell** | `"  ● Ran"` / `"  ● Running"` / `"  ● Failed [error]"` |
| **Edit / Write** | `"  ● Edited"` + `"   ⎿  Edit(path=...)"` |
| **TodoWrite** | `"  ● Updated todos"` + 复选框 `[x]` `[*]` `[ ]` |
| **默认工具** | `"  ● ToolName(摘要)"` |
| **工具错误** | 用 `error` 样式（红色加粗） |

### 第三层：用户消息的特殊样式

用户消息块渲染时，整行铺上**深色背景** `USER_MESSAGE_BACKGROUND` (`Rgb(31, 35, 42)`)：

| 内容类型 | 显示方式 |
|---|---|
| **普通提示词** | 深色背景后显示文本 |
| **命令行输出** | `"$ command"` 前缀 + `tool_result` 样式 |
| **文件编辑结果** | diff 预览（绿色新增 / 红色删除 / 上下文行） |
| **中断消息** | `"Interrupted by user"` 用 `warning` 样式 |
| **非 shell 工具结果** | 委托 `render_user_tool_result_message` 加上下文标签 |

### 第四层：系统消息的区分

系统消息根据 `SystemSubtype` 渲染不同前缀 + 样式：

| 子类型 | 显示 | 样式 |
|---|---|---|
| **CompactBoundary** | `✻ Conversation compacted (ctrl+o for history)` | `dim`（暗灰） |
| **MicrocompactBoundary** | **完全隐藏**（返回空 Vec） | — |
| **ApiError** | `Error occurred: 详情` + 重试细节 | `error`（红加粗） |
| **Informational::Info** | `Info: 消息` | `unselected`（浅灰） |
| **Informational::Warning** | `Warning: 消息` | `warning`（琥珀色） |
| **Informational::Error** | `Error: 消息` | `error`（红加粗） |
| **LocalCommand** | `$ 命令` | `system_name`（灰色斜体） |
| **Warning** | `Warning: 消息` | `warning`（琥珀色） |

### 第五层：进度与附件消息

| 消息类型 | 显示 | 样式 |
|---|---|---|
| **ProgressMessage** | `"  ... {message}"` | `dim`（暗灰） |
| **AttachmentMessage::EditedTextFile** | `"[edited: path]"` | `dim` |
| **AttachmentMessage::QueuedCommand** | JSON 序列化后的提示 | `dim` |
| **AttachmentMessage::MaxTurnsReached** | `"[max turns reached: N/M]"` | `dim` |
| **AttachmentMessage::StructuredOutput** | `"[structured output]"` | `dim` |
| **AttachmentMessage::HookStoppedContinuation** | `"[hook stopped continuation]"` | `dim` |
| **AttachmentMessage::NestedMemory** | 自定义格式 | `dim` |
| **AttachmentMessage::SkillDiscovery** | JSON 序列化 | `dim` |

### 第六层：工具执行状态着色

每种工具调用都有独立的 **`ToolState`** 状态标签 + 颜色：

| 状态 | 颜色映射 | 标签显示 |
|---|---|---|
| `Queued` | `dim`（暗灰 `100,100,100`） | `[queued]` |
| `Running` | `info`（天蓝 `130,200,255`） | `[running]` |
| `Succeeded` | `diff_add`（绿色） | `[succeeded]` |
| `Failed` | `error`（红色 `255,100,100`） | `[failed]` |
| `Cancelled` | `warning`（琥珀色 `255,200,80`） | `[cancelled]` |

工具执行时的旋转动画：Spinner 使用 **Braille 点阵字符**（`⡋⡙⡹⢹⢸` 循环），以 `info` 颜色显示，后跟 `dim` 样式的描述文本。

---

## 三、主题系统

定义在 `crates/allthecodes/src/ui/theme/mod.rs`。

### 3.1 六套内置主题

| 主题名 | 说明 |
|---|---|
| `Dark` | 深色主题（默认） |
| `Light` | 浅色主题 |
| `LightDaltonized` | 浅色色盲友好版 |
| `DarkDaltonized` | 深色色盲友好版 |
| `LightAnsi` | 浅色 ANSI 兼容版 |
| `DarkAnsi` | 深色 ANSI 兼容版 |

### 3.2 `ThemeColors` 配色表分组

`ThemeColors` 结构体定义了完整配色表，按功能分组：

| 分组 | 字段 | 用途 |
|---|---|---|
| **核心调色板** | `accent`, `accentDim`, `accentText`, `inverted`, `invertedText` | 品牌主色 |
| **语义状态** | `success`, `error`, `warning`, `suggestion`, `info` | 状态指示器 |
| **UI 表面/铬** | `surface`, `surfaceText`, `muted`, `mutedText`, `inactive`, `inactiveText`, `border`, `borderFocus`, `borderError`, `permission`, `permissionText` | 界面边框与表面 |
| **排版** | `dim`, `bold`, `link`, `code`, `codeBg`, `heading`, `blockquote`, `blockquoteBorder`, `hr` | 文本格式化 |
| **输入/选择** | `selection`, `selectionText`, `cursor`, `cursorText`, `searchHighlight`, `searchHighlightText` | 交互元素 |
| **差异对比** | `diffAdd`, `diffAddBg`, `diffRemove`, `diffRemoveBg`, `diffHeader`, `diffContext` | Git 风格差异 |
| **语法高亮** | `syntaxKeyword` ~ `syntaxLabel` 共 13 个 | 代码块高亮 |
| **状态图标** | `iconSuccess`, `iconError`, `iconWarning`, `iconInfo`, `iconPending`, `iconLoading` | 状态符号颜色 |
| **Agent 颜色** | `agentRed` ~ `agentPink` 共 8 个 | 多 agent 场景节点颜色 |

---

## 四、聚合显示优化

### 4.1 同类工具聚合 (`GroupedToolUse`)

连续相同类型的工具调用被聚合为一条，显示为：

```
  ● 5 Read calls · completed   ← tool_name（金色加粗）
  ● 3 Task calls · 2 completed, 1 failed
```

### 4.2 搜索/读取折叠 (`CollapsedReadSearch`)

连续的 Read / Grep / Glob / LS 调用被折叠为：

```
Searched for 3 patterns, Read 5 files, Listed 2 directories   ← dim（暗灰）
Ctrl+O to expand                                               ← 提示快捷键展开
```

### 4.3 流式消息光标

正在流式输出的 Assistant 消息末尾显示：

```diff
- 这是正在输出的内容 ▌    ← ` ` 闪烁光标
+ 这是已完成的输出        ← 完成后光标消失
```

---

## 五、渲染管线

```
┌──────────────────────────────────────────────────────────┐
│  [数据模型层]                                            │
│  Message::User / Assistant / System / Progress / Attachment│
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [Context 预处理层] messages/render/context.rs            │
│                                                          │
│  1. normalize_messages_for_render()                       │
│     → 拆分多块消息（每个 ContentBlock 独立）                │
│  2. filter_compact_boundary()                             │
│     → 过滤微压缩边界消息                                   │
│  3. should_show_renderable_message()                      │
│     → 按可见性规则过滤                                     │
│  4. reorder_messages_in_ui()                              │
│     → 应用重排序                                          │
│  5. filter_brief_messages()                               │
│     → 在 transcript 模式下隐藏简短消息                      │
│  6. truncate_transcript_messages()                        │
│     → 限制历史记录长度                                     │
│  7. apply_grouping()                                      │
│     → 聚合同类工具调用 → GroupedToolUse                    │
│  8. collapse_read_search_groups()                         │
│     → 折叠连续 Read/Grep/Glob → CollapsedReadSearch       │
│  9. build_message_lookups()                               │
│     → 构建工具 ID 查找表 / 状态 / 进度映射                  │
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [布局计算层] app/render.rs                               │
│                                                          │
│  App::render() 计算垂直布局：                              │
│  ┌─ 内容区 ──────────────────────────────────────────┐   │
│  │  消息列表（VirtualScroll 虚拟滚动）                  │   │
│  ├─ 底部面板 ─────────────────────────────────────────┤   │
│  │  spinner / suggestions / paste notice               │   │
│  │  输入框 / 补全弹窗 / 命令面板 / 命令参数帮助          │   │
│  │  通知栏 / agent 页脚 / 状态行                       │   │
│  └────────────────────────────────────────────────────┘   │
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [消息分发层] messages/render/mod.rs                      │
│                                                          │
│  render_renderable_message_for_layout()                   │
│  ┌─ RenderableMessage::Message ─────────────────────┐    │
│  │  → render_single_message_with_context()            │    │
│  │    ├ Message::User      → render_user_message()    │    │
│  │    ├ Message::Assistant → render_assistant_message()│   │
│  │    ├ Message::System    → render_system_message()  │    │
│  │    ├ Message::Progress  → render_progress_message()│    │
│  │    └ Message::Attachment→ render_attachment_message│    │
│  ├─ GroupedToolUse ──────────────────────────────────┤    │
│  │  → render_grouped_tool_use_lines()                 │    │
│  └─ CollapsedReadSearch ─────────────────────────────┤    │
│     → render_collapsed_read_search_lines()             │    │
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [类型专用渲染器]                                         │
│                                                          │
│  render_assistant_message()                               │
│  ├ 文本块 → markdown_to_lines()（完整 Markdown 渲染）      │
│  ├ ToolUse → 工具名 + 参数 + 状态标签                     │
│  ├ Thinking → "∴ Thinking..."                            │
│  ├ RedactedThinking → 省略标记                            │
│  ├ API Error → "Error occurred: 详情"                    │
│  └ Cost 显示 → "($0.xxxx)" 在 dim 样式中                  │
│                                                          │
│  render_user_message()                                    │
│  ├ 提示词 → 深色背景 + 文本                               │
│  ├ 命令输出 → "$ command" + 工具结果样式                   │
│  ├ 文件编辑 → diff 预览（绿/红行）                        │
│  └ 中断 → "Interrupted by user"                          │
│                                                          │
│  render_system_message() / render_progress_message()      │
│  / render_attachment_message() / ...                       │
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [富文本渲染层]                                           │
│                                                          │
│  markdown_to_lines() (rendering/markdown.rs)              │
│  ├ 标题：H1 → Bold + Underlined，H2+ → Bold              │
│  ├ 粗体/斜体/行内代码                                    │
│  ├ 代码块：syntect 语法高亮（可选）                       │
│  ├ 有序/无序列表                                         │
│  ├ 链接：Underlined + link 颜色                           │
│  ├ 引用块：左侧边框线                                    │
│  ├ 表格：对齐列                                          │
│  └ 删除线                                                │
│                                                          │
│  syntax_highlight() (rendering/syntax_highlight.rs)       │
│  └ 根据 token 类型映射到语法高亮颜色                      │
│                                                          │
│  wrap.rs → 文本换行到终端宽度                              │
└────────────────┬─────────────────────────────────────────┘
                 ↓
┌──────────────────────────────────────────────────────────┐
│  [缓冲区渲染] ratatui Frame → 终端 Buffer → 屏幕          │
│                                                          │
│  各 overlay/对话框（权限请求、命令确认等）叠加渲染          │
└──────────────────────────────────────────────────────────┘
```

---

## 六、关键技术文件索引

### 核心框架入口

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/Cargo.toml` | 声明 ratatui、crossterm、pulldown-cmark、syntect 依赖 |
| `crates/allthecodes/src/ui/mod.rs` | TUI 模块声明 |
| `crates/allthecodes/src/ui/app/render.rs` | App::render() 顶层渲染与布局 |

### 数据模型

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes-types/src/message.rs` | Message、ContentBlock、UserMessage、AssistantMessage 等 |
| `crates/allthecodes-types/src/status_line.rs` | StatusLinePayload 与子类型 |
| `crates/allthecodes-types/src/lib.rs` | 类型 crate 模块声明 |

### 主题系统

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/theme/mod.rs` | ThemeName、ThemeColors、ThemeProvider、6 套内置主题 |
| `crates/allthecodes/src/ui/theme/color.rs` | 颜色解析工具 |
| `crates/allthecodes/src/ui/rendering/theme.rs` | 语义角色 → Style 映射 |

### 消息渲染管线

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/messages/render/mod.rs` | 渲染入口、消息分发 |
| `crates/allthecodes/src/ui/messages/render/context.rs` | MessageRenderContext、RenderableMessage、预处理 |
| `crates/allthecodes/src/ui/messages/render/render_assistant.rs` | Assistant/System/Progress/Attachment 渲染 |
| `crates/allthecodes/src/ui/messages/render/render_user.rs` | User 消息渲染 |
| `crates/allthecodes/src/ui/messages/render/preprocessing.rs` | 规范化、过滤、空消息检测 |
| `crates/allthecodes/src/ui/messages/render/grouping.rs` | 工具聚合、搜索折叠 |

### 消息类型专用渲染

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs` | 工具调用各状态渲染 |
| `crates/allthecodes/src/ui/messages/assistant_thinking_message.rs` | 思维块渲染 |
| `crates/allthecodes/src/ui/messages/assistant_text_message.rs` | 文本分类与 API 错误渲染 |
| `crates/allthecodes/src/ui/messages/user_text_message.rs` | 用户文本路由（bash/cmd/memory/MCP 标签检测） |
| `crates/allthecodes/src/ui/messages/user_bash_output_message.rs` | Bash 输出格式化与截断 |
| `crates/allthecodes/src/ui/messages/file_edit_tool_updated_message.rs` | 文件编辑 diff 预览 |
| `crates/allthecodes/src/ui/messages/system_api_error_message.rs` | API 错误渲染 |
| `crates/allthecodes/src/ui/messages/attachment_message.rs` | 附件渲染 |
| `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs` | 聚合工具渲染 |
| `crates/allthecodes/src/ui/messages/collapsed_read_search_content.rs` | 折叠搜索渲染 |
| `crates/allthecodes/src/ui/messages/compact_boundary_message.rs` | 压缩边界渲染 |
| `crates/allthecodes/src/ui/messages/user_tool_result_message/` | 工具结果渲染（成功/错误/拒绝/取消） |

### 共享渲染工具

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/rendering/markdown.rs` | Markdown → 带样式 Line/Span |
| `crates/allthecodes/src/ui/rendering/syntax_highlight.rs` | 代码语法高亮 |
| `crates/allthecodes/src/ui/rendering/tool_activity.rs` | ToolActivity、ToolState、工具名映射 |
| `crates/allthecodes/src/ui/rendering/spinner.rs` | Braille 点阵 Spinner 动画 |
| `crates/allthecodes/src/ui/rendering/virtual_scroll.rs` | VirtualScroll 虚拟滚动 |
| `crates/allthecodes/src/ui/messages/wrap.rs` | 文本换行 |
| `crates/allthecodes/src/ui/divider.rs` | 视觉分割线 |

### 交互组件

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/components/` | 20+ 可复用组件（bottom_pane、fuzzy_match、fuzzy_picker、history_search、keyboard_shortcuts、list_item、tabs、tooltips、welcome 等） |
| `crates/allthecodes/src/ui/overlays/` | 模态叠加层基础设施 |
| `crates/allthecodes/src/ui/permissions/` | 15+ 权限对话框类型 |

### 功能模块 UI

| 文件路径 | 职责 |
|---|---|
| `crates/allthecodes/src/ui/agents/` | Agent 管理界面 |
| `crates/allthecodes/src/ui/mcp/` | MCP 服务器管理界面 |
| `crates/allthecodes/src/ui/memory/` | 记忆管理界面 |
| `crates/allthecodes/src/ui/skills/` | Skill 界面 |
| `crates/allthecodes/src/ui/tasks/` | Task 界面 |
| `crates/allthecodes/src/ui/teams/` | Team 界面 |
| `crates/allthecodes/src/ui/diff/` | Diff 渲染 |
| `crates/allthecodes/src/ui/notifications/` | 应用内通知 |
| `crates/allthecodes/src/ui/command_palette/` | 命令面板 |
| `crates/allthecodes/src/ui/command_surface/` | 命令表面 |

---

## 七、区分策略总结

项目采用 **四维区分（Color-by-Type）** 策略：

```
维度 1：消息角色
  User / Assistant / System / Progress / Attachment
  → 不同颜色 + 加粗/斜体特效

维度 2：内容块类型
  Text / ToolUse / Thinking / Error
  → 不同视觉图案和标记字符（● / ∴ / ⎿ / [x]）

维度 3：执行状态
  Queued / Running / Succeeded / Failed / Cancelled
  → 不同颜色 + 状态标签文字

维度 4：系统子类型
  Info / Warning / Error / Boundary / APIError
  → 不同前缀标签（Info: / Warning: / Error: / ✻） + 样式
```

每个维度的度绑定唯一的颜色和样式，用户无需阅读文字内容即可通过视觉特征判断数据类型。
