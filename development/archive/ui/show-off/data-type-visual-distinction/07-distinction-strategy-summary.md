# 区分策略总结详细索引

本文扩展 `data-type-visual-distinction.md` 第七章，用作新增消息类型、内容块、工具类型或主题字段时的检查入口。

## 1. 总体策略

allthecodes TUI 当前采用四维区分策略：消息角色、内容块类型、执行状态、系统子类型。四个维度共同决定一条记录在终端中的可见形态。

| 维度 | 数据来源 | 主要视觉信号 | 渲染入口 | 维护目标 |
|---|---|---|---|---|
| 消息角色 | `Message::{User, Assistant, System, Progress, Attachment}` | 背景、前缀、语义颜色、是否 dim | `render_single_message_with_context()` | 用户先分清“谁发出这条记录” |
| 内容块类型 | `ContentBlock::{Text, ToolUse, ToolResult, Thinking, ...}` | Markdown、工具图案、思维符号、错误样式 | `render_assistant_message()` / `render_user_message()` | 同一条消息内不同 block 不混淆 |
| 执行状态 | `ToolState::{Queued, Running, Succeeded, Failed, Cancelled}` 和 lookup 集合 | 状态标签、进度、成功/错误色 | `assistant_tool_use_message.rs`、`tool_activity.rs` | 工具是否完成、失败或仍在运行一眼可见 |
| 系统子类型 | `SystemSubtype` / `InfoLevel` | `Info:`、`Warning:`、`Error:`、压缩边界标记 | `render_system_message()` | 系统提示不与模型正文混在一起 |

源码入口：

| 文件 | 作用 |
|---|---|
| `crates/allthecodes-types/src/message.rs` | 定义消息、内容块、系统子类型、附件类型 |
| `crates/allthecodes/src/ui/messages/render/context.rs` | 构建 `RenderableMessage`、lookup、分组和折叠后的渲染上下文 |
| `crates/allthecodes/src/ui/messages/render/mod.rs` | 按 `RenderableMessage` 分发到专用渲染器 |
| `crates/allthecodes/src/ui/messages/render/render_assistant.rs` | Assistant/System/Progress/Attachment 的核心渲染 |
| `crates/allthecodes/src/ui/messages/render/render_user.rs` | User 消息、工具结果、shell 输出路由 |
| `crates/allthecodes/src/ui/rendering/theme.rs` | 当前消息渲染使用的 `Theme` 样式表 |
| `crates/allthecodes/src/ui/theme/mod.rs` | 设计系统主题、6 套内置主题和运行时主题解析 |

## 2. 视觉信号优先级

视觉信号应按“结构先于颜色”的顺序设计。终端环境里颜色可能被 ANSI 降级、主题替换或用户配置弱化，所以不能只靠颜色区分类型。

| 优先级 | 信号 | 示例 | 适用场景 |
|---|---|---|---|
| 1 | 独立记录类型 | `RenderableMessage::GroupedToolUse`、`CollapsedReadSearch` | 聚合、折叠、虚拟消息 |
| 2 | 稳定文本前缀 | `Warning:`、`Error:`、`$ command` | 系统提示、本地命令、错误 |
| 3 | 稳定符号 | `●`、`⎿`、`∴`、`[x]` | 工具调用、续行、思维、Todo |
| 4 | 字体修饰 | Bold、Italic、Underline | 名称、思维、链接、标题 |
| 5 | 颜色 | error/warning/info/dim/diff | 状态强化，不应是唯一语义 |
| 6 | 动态效果 | spinner、流式光标 | 运行中状态，完成后必须消失 |

设计要求：

| 场景 | 应做 | 不应做 |
|---|---|---|
| 新增错误分支 | 使用错误前缀和 `theme.error` | 只把普通文本染红 |
| 新增运行中工具 | 给出稳定工具名、状态或 spinner | 只输出一段自由文本 |
| 新增隐藏型内部事件 | 在过滤层显式说明隐藏原因 | 让空消息自然掉落而无文档 |
| 新增聚合虚拟记录 | 建立独立 `RenderableMessage` 变体或专用 record | 在原消息字符串里临时拼接摘要 |

## 3. 新增 `Message` 变体检查清单

当前顶层变体集中在 `crates/allthecodes-types/src/message.rs`。如果新增 `Message` 变体，需要同步处理下面入口。

| 检查点 | 文件/函数 | 需要确认的问题 |
|---|---|---|
| 预处理是否保留 | `preprocessing.rs`、`should_show_renderable_message()` | 默认可见、隐藏还是仅 transcript 可见 |
| 渲染分发是否覆盖 | `render/mod.rs`、`render_single_message_with_context()` | 是否有专用 renderer，未覆盖时是否会空白 |
| lookup 是否需要索引 | `context.rs`、`build_message_lookups()` | 是否影响工具状态、进度、选中展开 |
| copy text 是否合理 | `copy_text.rs` | 复制 transcript 时是否保留关键内容 |
| snapshot 是否更新 | `crates/allthecodes/src/ui/**/snapshots/` | 新视觉形态是否有回归样例 |
| 文档是否更新 | 本目录索引文章 | 变体含义、可见性和文件入口是否记录 |

最低要求：新增顶层变体不能只在类型层存在，必须明确“在 UI 中可见、隐藏、归并到其它记录，还是仅作为内部数据”。

## 4. 新增 `ContentBlock` 检查清单

`ContentBlock` 常见于 Assistant 和 User 消息内部。新增 block 时，应同时考虑 Assistant 正文、User 工具结果和 lookup。

| 检查点 | 文件/函数 | 说明 |
|---|---|---|
| Assistant block 渲染 | `render_assistant_message()` | 是否作为正文、工具、思维、错误或附件类信息展示 |
| User block 提取 | `message_content_blocks()`、`render_user_message()` | User 的 `MessageContent::Blocks` 是否能拿到该 block |
| ToolResult 嵌套 | `ToolResultContent::Blocks` | 嵌套 block 是否应递归展示或摘要化 |
| Markdown 兼容 | `rendering/markdown.rs` | 文本 block 是否走 Markdown 富文本 |
| 查找表影响 | `build_message_lookups()` | 是否需要建立 id、状态、结果映射 |
| 折叠/聚合 | `grouping.rs` | 是否参与 `GroupedToolUse` 或 `CollapsedReadSearch` |

如果 block 携带二进制或大体积内容，应优先做摘要、附件索引或折叠，不应直接把完整内容写入终端 buffer。

## 5. 新增工具类型检查清单

工具在 UI 中至少涉及三类映射：工具名展示、参数摘要、聚合/折叠资格。

| 检查点 | 文件/函数 | 说明 |
|---|---|---|
| 用户可读名称 | `rendering/tool_activity.rs::user_facing_tool_name()` | 将内部名映射成短名称 |
| 参数摘要 | `tool_primary_input()`、`summarize_json_input()` | 优先展示 path/command/pattern/url/query/description |
| 单工具渲染 | `assistant_tool_use_message.rs` | 是否需要专门格式，而不是默认 `ToolName(args)` |
| 结果渲染 | `user_tool_result_message/`、`render_user.rs` | 成功、错误、拒绝、取消是否分支齐全 |
| 同类聚合 | `grouping.rs::is_groupable_tool()` | 只有连续且同 source 的白名单工具会变成 `GroupedToolUse` |
| 搜索读取折叠 | `grouping.rs::collapsible_kind_for_tool()` | 只有读/搜/列目录类工具会变成 `CollapsedReadSearch` |
| 进度消息 | `ProgressMessage` lookup | 工具是否会发 progress，进度应如何展示 |

当前聚合资格：

| 工具 | `GroupedToolUse` | `CollapsedReadSearch` | 说明 |
|---|---:|---:|---|
| `Task` | 是 | 否 | 同 source 内多次 Task 聚合为 Agent calls |
| `Agent` | 是 | 否 | 与 Task 同样按 Agent 展示名处理 |
| `Read` | 是 | 是 | 可先同类聚合，再进入读取折叠 |
| `Grep` | 是 | 是 | 计入搜索数量 |
| `Glob` | 是 | 是 | 计入搜索数量 |
| `WebSearch` | 否 | 是 | 不做同类聚合，但可计入搜索折叠 |
| `LS` / `List` | 否 | 是 | 计入目录列表数量 |
| `Bash` / `PowerShell` | 否 | 否 | 保留单独执行记录和 shell 输出展开 |
| `Edit` / `Write` / `MultiEdit` / `NotebookEdit` | 否 | 否 | 保留 diff/编辑语义 |
| `TodoWrite` | 否 | 否 | 保留复选框列表语义 |
| `WebFetch` | 否 | 否 | 目前仅工具名映射为 Fetch，不参与折叠 |
| MCP / 未知工具 | 否 | 否 | 默认按工具名展示，除非显式加入白名单 |

## 6. 主题与可访问性限制

终端 UI 的可访问性限制主要来自颜色能力、字体能力和宽度约束。

| 限制 | 影响 | 设计策略 |
|---|---|---|
| 终端可能只支持 ANSI 色 | RGB 精细差异失效 | 保留文字前缀和符号 |
| 部分字体不支持特殊字符 | `∴`、`⎿`、Braille spinner 可能显示异常 | 特殊符号只做增强，文本仍可读 |
| 用户可能使用浅色主题 | dim/背景对比变化 | 使用主题字段，不写死颜色 |
| 色盲主题改变 hue | red/green 对比弱化 | 错误和成功同时使用文字标签 |
| 窄终端换行 | 工具参数或路径撑开布局 | 摘要化、截断、走 `wrap.rs` |

主题相关入口：

| 文件 | 关注点 |
|---|---|
| `crates/allthecodes/src/ui/theme/mod.rs` | 运行时主题表、`ThemeName`、`ThemeColors` |
| `crates/allthecodes/src/ui/rendering/theme.rs` | 消息渲染当前实际使用的 `Theme` 样式 |
| `crates/allthecodes/src/ui/rendering/markdown.rs` | Markdown 标题、链接、代码、引用的样式使用 |
| `crates/allthecodes/src/ui/rendering/syntax_highlight.rs` | token 到语法颜色的映射 |

注意：`ui/theme/mod.rs` 是更完整的设计系统主题表；`ui/rendering/theme.rs` 是消息渲染层直接消费的预组合 `Style`。改主题时需要确认两边是否都已接入目标 UI 面。

## 7. 测试建议

文档变化本身不要求编译，但涉及渲染行为的代码改动应按风险选择测试。

| 改动类型 | 最小验证 |
|---|---|
| 新增消息或 block 渲染 | 增加或更新消息渲染 snapshot |
| 改工具状态颜色/标签 | 覆盖 `ToolState` 的 queued/running/succeeded/failed/cancelled |
| 改聚合/折叠规则 | 覆盖 verbose=true、少于 2 条、不支持工具、混合工具、工具结果隐藏 |
| 改 Markdown 样式 | 覆盖标题、链接、代码块、表格、引用、列表 |
| 改主题字段 | 覆盖 dark/light/ANSI/daltonized 的解析和关键颜色映射 |
| 改宽度行为 | 用窄宽度 snapshot 验证换行和截断 |

回归重点：

| 风险 | 常见表现 |
|---|---|
| lookup 漏建 | 工具一直显示 running 或状态不更新 |
| 过滤顺序错误 | 压缩边界、brief 消息或工具结果意外显示/消失 |
| 聚合误伤 | Bash/Edit/TodoWrite 被折叠后丢失关键语义 |
| 颜色硬编码 | 浅色或 ANSI 主题下不可读 |
| 宽度未处理 | 长路径、长命令、JSON 参数撑破布局 |

## 8. 文档维护规则

| 触发条件 | 需要更新的索引 |
|---|---|
| 新增 `Message`、`ContentBlock`、`Attachment`、`SystemSubtype` | `01-core-data-types.md`、本文件 |
| 改角色、block、系统消息、工具状态视觉样式 | `02-six-layer-distinction.md`、本文件 |
| 改主题字段、主题数量、颜色解析 | `03-theme-system.md`、本文件 |
| 改 `GroupedToolUse` 或 `CollapsedReadSearch` 规则 | `04-aggregation-optimizations.md`、本文件 |
| 改 render context、preprocessing、布局入口 | `05-rendering-pipeline.md` |
| 移动或拆分 UI 文件 | `06-key-files-index.md` |

保持原则：总览文档说明“有什么”，章节索引说明“在哪里、为什么、支持范围和不支持范围”。
