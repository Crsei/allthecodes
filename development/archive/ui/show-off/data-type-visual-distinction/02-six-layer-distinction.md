# 第二章：六层区分机制详细索引

本文索引 `data-type-visual-distinction.md` 第二章「六层区分机制」对应的 Rust TUI 实现。当前渲染链路不是单一 switch 完成，而是先把原始 `Message` 预处理成 `RenderableMessage`，再按消息角色、内容块、系统/进度/附件、工具生命周期、聚合记录逐层添加视觉信号。

核心入口：

| 层级 | 主要判断对象 | 入口函数 | 关键文件 |
|---|---|---|---|
| 角色样式 | `Message::{User, Assistant, System, Progress, Attachment}` | `render_single_message_with_context` | `crates/allthecodes/src/ui/messages/render/mod.rs` |
| ContentBlock 内细分 | `ContentBlock::*` | `render_assistant_message`、`render_user_message` | `crates/allthecodes/src/ui/messages/render/render_assistant.rs`、`render_user.rs` |
| 用户消息 | `UserMessage.content`、工具结果块、bash XMLish 标签 | `render_user_message` | `crates/allthecodes/src/ui/messages/render/render_user.rs` |
| 系统消息 | `SystemSubtype::*` | `render_system_message` | `crates/allthecodes/src/ui/messages/render/render_assistant.rs` |
| 进度/附件 | `ProgressMessage`、`AttachmentMessage` | `render_progress_message`、`render_attachment_message` | `crates/allthecodes/src/ui/messages/render/render_assistant.rs`、`attachment_message.rs` |
| 工具状态 | tool use id lookup、聚合记录 | `tool_state_for_id`、`render_assistant_tool_use_message`、`render_grouped_tool_use_lines`、`render_collapsed_read_search_lines` | `render_assistant.rs`、`assistant_tool_use_message.rs`、`grouped_tool_use_content.rs`、`collapsed_read_search_content.rs` |

## 1. 第一层：角色样式

第一层决定一条消息进入哪条渲染路径。`render_single_message_with_context` 负责把顶层 `Message` 分发到具体 renderer；`render_renderable_message_with_context` 额外处理预处理阶段产生的 `GroupedToolUse` 与 `CollapsedReadSearch`。

| 消息角色/记录 | 渲染函数 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|---|
| `Message::Assistant` | `render_assistant_message` | Markdown 正文、工具块、思考块、图片引用、成本行 | `tool_name`、`tool_result`、`error`、`thinking`、`dim`、Markdown 相关字段 | 支持；`assistant_name` 是 legacy 字段，当前消息正文不固定显示助理名称标签 |
| `Message::User` | `render_user_message` | 普通用户文本使用深色背景；工具结果、图片、bash 输出不铺背景 | `USER_MESSAGE_BACKGROUND`、`warning`、`tool_result`、`error` | 支持；背景判断在 `renderable_message_uses_user_background` 与 `user_message_uses_background` 中二次确认 |
| `Message::System` | `render_system_message` | 前缀、隐藏微压缩、压缩边界专用行、API 错误行 | `dim`、`error`、`warning`、`unselected`、`system_name` | 支持；`MicrocompactBoundary` 明确返回空行集 |
| `Message::Progress` | `render_progress_message` | `  ... ` 前缀 + 进度摘要 | `dim` | 弱支持；只读取 JSON object 的 `message` 字段，否则 `to_string()` |
| `Message::Attachment` | `render_attachment_message` | 方括号摘要或委托附件 helper 的自然语言摘要 | `dim` | 部分支持；顶层 `Attachment` 枚举只接入少数分支，helper 支持更多 label/detail 形态 |
| `RenderableMessage::GroupedToolUse` | `render_grouped_tool_use_lines` | `  ● N Tool calls` + 完成/失败摘要 | `tool_name` | 支持；仅非 verbose 且同一 `source_index` 下同类工具数量大于等于 2 |
| `RenderableMessage::CollapsedReadSearch` | `render_collapsed_read_search_lines` | `Read/Searched/Listed ... Ctrl+O to expand`，活跃时带省略号和 hint | `dim` | 支持；只覆盖读/搜索/列目录类工具 |

主题字段来源有两层：`crates/allthecodes/src/ui/rendering/theme.rs` 定义 `Theme` 的实际 ratatui `Style` 字段；`crates/allthecodes/src/ui/theme/mod.rs` 的 `Theme::from_design_colors` 把设计系统 `ThemeColors` 映射回这些字段。也就是说渲染代码消费的是 `Theme`，不是直接消费 `ThemeColors`。

| `Theme` 字段 | 默认视觉 | 主要消费者 | 备注 |
|---|---|---|---|
| `assistant_name` | 紫色加粗 | legacy label 场景 | 当前消息渲染主体弱使用 |
| `user_name` | 浅蓝加粗 | legacy label 场景、prompt 同色系 | 当前用户消息主要靠背景区分 |
| `system_name` | 灰色斜体 | `LocalCommand` 系统消息 | 角色前缀 `$ ` 使用该样式 |
| `tool_name` | 金色加粗 | 工具调用、文件编辑预览首行、聚合工具 | 工具可视识别的主色 |
| `tool_result` | 中灰 | 工具输出、bash 输出、文件编辑预览正文 | 错误时切到 `error` |
| `error` | 红色加粗 | API 错误、工具错误、失败状态 | 错误语义主通道 |
| `warning` | 琥珀色 | 中断、警告、取消语义 | 用户中断也走该字段 |
| `info` | 天蓝色 | 工具活动 `Running` 状态 | 普通 progress message 没有使用 `info` |
| `thinking` | 暗灰斜体 | assistant thinking renderer | 仅在 verbose/transcript 可见时产生内容 |
| `dim` | 暗灰 | 压缩、进度、附件、成本、图片引用 | 次要信息主通道 |

## 2. 第二层：ContentBlock 内细分

`AssistantMessage` 的一条消息内部可以包含多个 `ContentBlock`。`render_assistant_message` 逐块处理，并通过空行与缩进保持块边界：工具块后跟文本/结果时会插入空行；非首块通常以 8 个空格缩进。

| `ContentBlock` 分支 | 渲染函数/辅助函数 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|---|
| `Text { text }` | `markdown_to_lines`；API 错误时 `api_error_display_text` | Markdown 标题、粗体、斜体、代码、表格、链接；API 错误整行红色 | Markdown 使用 `heading`、`bold`、`italic`、`code`、`link`、语法高亮字段；错误使用 `error` | 支持；`is_api_error_message` 会绕过 Markdown |
| `ConnectorText { connector_text, .. }` | `markdown_to_lines` | 同普通 Markdown 文本 | 同上 | 支持；连接器元数据没有额外视觉标签 |
| `ToolUse { id, name, input }` | `tool_state_for_id` + `render_assistant_tool_use_message` | `●` 起始的工具调用摘要，必要时多行 `⎿` 续行 | 外层统一 `tool_name` | 支持；状态来自 lookup，仅能区分进行中/已解决/错误 |
| `ServerToolUse { id, name, input }` | 同 `ToolUse` | 在每行前加 `server: ` | `tool_name` | 支持；服务端工具和普通工具共享视觉主色，只有文本前缀区别 |
| `ToolResult { content, is_error, .. }` | 内联分支处理 | `Result:` 或 `Error:`，最多显示前 5 行，超出显示 `... N more lines` | 成功 `tool_result`，失败 `error`，截断提示 `dim` | 支持；block 型结果只展开文本、连接器文本和图片引用，其它块显示 `[...]` |
| `Thinking { thinking, .. }` | `render_assistant_thinking_lines` | 思考块专用格式；非首块缩进 | `thinking` 等由 thinking renderer 决定 | 弱支持；仅 verbose 或 transcript 模式会把非空 thinking 计入可见内容 |
| `RedactedThinking { .. }` | `render_assistant_redacted_thinking_lines` | 被隐藏/截断思考提示 | thinking/redacted renderer 决定 | 弱支持；仅 verbose 或 transcript 模式显示 |
| `Image { source }` | `image_reference` | 图片引用文本 | `dim` | 支持文本占位；不在 TUI 内直接渲染位图 |

Markdown 子层由 `crates/allthecodes/src/ui/rendering/markdown.rs` 负责。它启用表格和删除线扩展，维护 256 项 thread-local LRU cache，并把主题指纹纳入 cache key，避免换主题后复用旧样式。

| Markdown 元素 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|
| 标题 | H1 加粗 + 下划线，H2 加粗，其它用 bold | `heading`、`bold` | 支持 |
| 粗体/斜体 | modifier 叠加 | `bold`、`italic` | 支持 |
| 行内代码 | 前景 + 背景 | `code` | 支持 |
| fenced/indented code block | 代码块背景与可选语法高亮 | `code`、`code_bg`、`syntax_*` | 支持；高亮依赖 `highlight_code_block` |
| 列表 | `  - ` 或 `  N. ` 前缀 | 当前样式栈 | 支持 |
| 链接 | 下划线 | `link` | 支持；终端内不是可点击链接协议层 |
| 表格 | 对齐后的文本表格 | 当前样式栈 | 支持 |

## 3. 第三层：用户消息

用户消息的关键视觉差异不是名称颜色，而是背景与特殊内容路由。`render_user_message` 先把 `MessageContent` 规整为文本；如果发现工具结果或图片块，会提前进入专用渲染路径并跳过背景。

| 用户消息形态 | 判断位置 | 渲染函数 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|---|---|
| 普通文本 | `render_user_message` + `render_user_text_message` | `render_user_text_message` 后逐行加前导空格 | 整行深色背景 `Rgb(31,35,42)`；布局层前后再插入空行 | `USER_MESSAGE_BACKGROUND` | 支持；隐藏类用户文本会返回空 |
| 中断文本 | 精确匹配 `[Request interrupted by user]` 或 `CONVERSATION_INTERRUPTED_MESSAGE` | 直接返回 warning line | `Interrupted by user` 或原始中断文案 | `warning` | 支持；不铺用户背景 |
| `<bash-stdout>`/`<bash-stderr>` | `render_tagged_user_text` | `render_user_bash_output_message_with_options` | shell 输出摘要，可按最新/选中展开 | `tool_result` | 支持；宽度最小 20，统计行数和字节数 |
| shell 工具结果 | `render_tool_result_user_message` 查询 `render_context.tool_use` 并判断 `is_shell()` | `render_user_bash_output_message_with_options` | 命令输出块，最新 shell 结果自动展开 | 成功 `tool_result`，错误 `error` | 支持 |
| 文件编辑结果预览 | `render_file_edit_preview` 解析 `tool_use_result` JSON | `render_file_edit_tool_updated_message` | 首行工具名色，后续 diff/正文结果色 | 首行 `tool_name`，后续 `tool_result` | 支持但依赖 `kind=file_edit`、`path`、`hunk_lines` |
| 非 shell 工具结果 | `render_tool_result_user_message` fallback | `render_user_tool_result_message` | 根据工具 lookup 生成上下文标签和结果文本 | helper 内部使用传入 `Theme` | 支持；当前传入的 `Tools` 是空 Vec，工具元数据增强能力弱 |
| 图片块 | `render_user_image_blocks` | `image_reference` | 图片引用文本列表 | `dim` | 支持文本占位；不铺用户背景 |

用户背景有两处控制。`render_user_message` 给普通文本每个 Span 加背景；`render_renderable_message_for_layout` 还会在满足 `renderable_message_uses_user_background` 时给消息上下插入空行，并由 `render_messages` 对整行 buffer 设置背景。工具结果、图片、bash 标签、中断文本都被排除在整行背景之外。

## 4. 第四层：系统消息

系统消息按 `SystemSubtype` 选择前缀和样式。特殊分支优先返回：API 错误委托 `render_system_text_message`，微压缩隐藏，普通压缩边界委托 `render_compact_boundary_lines`。

| `SystemSubtype` | 渲染函数/路径 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|---|
| `CompactBoundary { .. }` | `render_compact_boundary_lines` | 压缩边界专用提示 | `dim` | 支持；不会走普通 `context compacted` 文本 |
| `MicrocompactBoundary { .. }` | 直接 `Vec::new()` | 完全隐藏 | 无 | 支持隐藏 |
| `ApiError { retry_attempt, retry_in_ms, error, .. }` | `render_system_text_message("api_error", ...)` + `plain_text_to_lines` | API 错误摘要；有重试延迟时带 `retry_attempt=` | `error` | 支持；优先使用 `msg.content`，为空则使用 `error.message` |
| `Informational { level: Info }` | 普通多行渲染 | `Info: ` 前缀 | `unselected` | 支持 |
| `Informational { level: Warning }` | 普通多行渲染 | `Warning: ` 前缀 | `warning` | 支持 |
| `Informational { level: Error }` | 普通多行渲染 | `Error: ` 前缀 | `error` | 支持 |
| `LocalCommand { .. }` | 普通多行渲染 | `$ ` 前缀 | `system_name` | 支持；目前主要靠 content 展示命令 |
| `Warning` | 普通多行渲染 | `Warning: ` 前缀 | `warning` | 支持 |

普通多行系统消息第一行由 `prefix + content_lines[0]` 组成，后续行添加两个空格缩进。空内容时只显示前缀。

## 5. 第五层：进度与附件

进度消息是轻量摘要，附件消息则分成两个层次：顶层 `Attachment` 枚举在 `render_assistant.rs` 中先做一次转换；部分分支再委托 `crates/allthecodes/src/ui/messages/attachment_message.rs` 的 label/detail helper。helper 的覆盖面明显大于顶层枚举当前接入面。

| 类型 | 顶层渲染路径 | 视觉信号 | 主题字段 | 当前支持状态 |
|---|---|---|---|---|
| `ProgressMessage` | `render_progress_message` | `  ... {message}` | `dim` | 弱支持；不按 `tool_use_id` 直接附着到工具行，只独立渲染 |
| `Attachment::EditedTextFile` | 直接 format | `[edited: path]` | `dim` | 支持 |
| `Attachment::QueuedCommand` | helper label `queued_command` | `[queued: prompt]`，长 prompt 截断 | `dim` | 支持 |
| `Attachment::MaxTurnsReached` | 直接 format | `[max turns reached: turn/max]` | `dim` | 支持 |
| `Attachment::StructuredOutput` | 直接 format | `[structured output]` | `dim` | 弱支持；不展开结构化内容 |
| `Attachment::HookStoppedContinuation` | 直接 format | `[hook stopped continuation]` | `dim` | 弱支持；不带 hook 名称/事件/消息 |
| `Attachment::NestedMemory` | helper label `nested_memory` | `Loaded path` | `dim` | 支持 |
| `Attachment::SkillDiscovery` | helper label `skill_discovery` | `N relevant skills found` | `dim` | 支持；传入 skills JSON 数组时按数组长度估算 |

附件 helper 支持的 label/detail 类型索引：

| 分类 | 支持 label/类型 | 视觉信号 | 未支持或弱支持点 |
|---|---|---|---|
| 文件与引用 | `file`、`already_read_file`、`compact_file_reference`、`pdf_reference`/`pdf`、`directory`、`selected_lines_in_ide`、`nested_memory`、`relevant_memories`/`collapsed_read_search_group` | `Read ...`、`Referenced ...`、`Listed directory ...`、多行 recalled memories | 顶层 `Attachment` 当前没有把所有这些 label 都接进来 |
| Skills/Tools | `dynamic_skill`、`skill_listing`、`agent_listing_delta`、`invoked_skills`、`skill_discovery`、`tool_discovery`、`diagnostics` | 可用数量、恢复技能、发现工具 | `diagnostics` 返回空；initial listing 返回空 |
| MCP/Commands | `queued_command`、`plan_file_reference`、`mcp_resource`、`command_permissions` | 队列命令、计划文件、MCP 资源读取 | `command_permissions` 返回空 |
| Hooks | `async_hook_response`、`hook_blocking_error`、`hook_non_blocking_error`、`hook_error_during_execution`、`hook_success`、`hook_stopped_continuation`、`hook_system_message`、`hook_permission_decision` | hook 完成/错误/系统消息/权限决策 | `Stop`/`SubagentStop` 的部分 hook 在非 verbose 语义下返回空；`hook_success` 返回空 |
| Task/Teammate | `task_status`、`teammate_shutdown_batch`、`teammate_mailbox` | task 更新、队友关闭、邮箱未读 | 顶层 `Attachment` 当前未直接接入 |
| fallback | 任意未知 label | `Attachment: label -> detail` | 支持兜底，但没有语义样式差异 |

## 6. 第六层：工具状态

工具状态在当前 UI 里有两套相关模型。第一套是 assistant tool-use 消息使用的 `ToolUseState`；第二套是 `ToolActivity` 使用的 `ToolState`，主要服务 compact activity 行、聚合活动测试和任务渲染。第二章旧文档把两者合并描述，实际源码需要分开看。

### 6.1 Assistant tool-use 状态

`render_assistant_message` 根据 tool use id 查询 `MessageRenderContext.lookups`：

| lookup 条件 | 转换结果 | 来源 | 视觉结果 |
|---|---|---|---|
| `errored_tool_use_ids.contains(id)` | `ToolUseState::Error` | 用户 `ToolResult.is_error = true` | 工具标题带失败/错误文案，外层仍用 `tool_name` |
| `resolved_tool_use_ids.contains(id)` | `ToolUseState::Resolved` | 用户 `ToolResult` 已出现 | 工具标题变为完成态文案 |
| 否则 | `ToolUseState::InProgress` | 有 tool use 但无结果 | 工具标题显示执行中语义 |

`ToolUseState::Queued`、`WaitingForPermission`、`ClassifierChecking` 在 `assistant_tool_use_message.rs` 中已有渲染分支，但当前 `tool_state_for_id` 不会从 `MessageRenderContext` 推导出这些状态。因此它们属于 helper 支持、主消息链路弱支持或未接线状态。

| `ToolUseState` | 专用文案 | 主题字段 | 当前支持状态 |
|---|---|---|---|
| `Queued` | `Queued`、`Queued edit`、`Queued todo update` 或 `  ● Tool` | 外层 `tool_name` | helper 支持；主链路未推导 |
| `InProgress` | `Running`、`Editing`、`Updating todos` 或 `Tool(args)` | 外层 `tool_name` | 支持 |
| `Resolved` | `Ran`、`Edited`、`Updated todos` 或透明 wrapper 的 `Tool` | 外层 `tool_name` | 支持 |
| `Error` | `Failed [error]`、`Edit failed [error]`、`Todo update failed [error]` 或 `[error]` | 外层 `tool_name`；结果块错误用 `error` | 支持 |
| `WaitingForPermission` | `Needs permission`、`Edit needs permission` 等 | 外层 `tool_name` | helper 支持；主链路未推导 |
| `ClassifierChecking` | `Checking`、`Checking edit`、`classifier checking...` | 外层 `tool_name` | helper 支持；主链路未推导 |

### 6.2 工具类型专用格式

`render_assistant_tool_use_message` 按工具名优先匹配专用 renderer，再 fallback 到 `ToolActivity::from_tool_use(..., ToolState::Running)` 的摘要能力。

| 工具/工具族 | 匹配函数 | 支持工具名 | 显示格式 | 不支持或弱支持点 |
|---|---|---|---|---|
| Todo | `render_todo_write_tool_use_message` | `TodoWrite`、`todo_write` | `  ● Updated todos`，后续 `⎿  [x] / [*] / [ ] item`，最多 12 条 | 超过 12 条只显示 `... N more`；状态只影响标题 |
| Shell | `render_shell_tool_use_message` | `Bash`、`PowerShell` | `  ● Ran/Running/Failed...` + `⎿  Bash(description, command)` | 小写 `bash` 不走专用 shell 分支，但 fallback 会映射为 `Bash` |
| 文件编辑 | `render_file_edit_tool_use_message` | `Edit`、`Write`、`FileEdit`、`FileWrite`、`NotebookEdit`、`MultiEdit` | `  ● Edited/Editing...` + `⎿  Edit(path=...)` | 需要 JSON object 中存在 `file_path`/`path`/`notebook_path` |
| 透明 wrapper | `is_transparent_wrapper_tool` | `Bash`、`PowerShell`、`Write`、`Edit`、`NotebookEdit`、`Read` | resolved fallback 保留简短完成 marker | `FileEdit`、`FileWrite`、`MultiEdit` 不在透明 wrapper 列表，但已由文件编辑专用分支覆盖 |
| 通用工具 | fallback `ToolActivity::from_tool_use` | 任意其它工具名 | `  ● DisplayName(args)`，错误时追加 `[error]` | 只提取 JSON 标量参数；复杂数组/对象不会详细展开 |

fallback 的用户可见工具名由 `user_facing_tool_name` 规范化：

| 输入工具名 | 用户可见名 |
|---|---|
| `read_file`、`Read` | `Read` |
| `edit_file`、`file_edit`、`FileEdit`、`Edit`、`MultiEdit`、`NotebookEdit`、`file_write`、`FileWrite`、`Write` | `Edit` |
| `bash`、`Bash` | `Bash` |
| `powershell`、`PowerShell` | `PowerShell` |
| `grep`、`Grep` | `Search` |
| `glob`、`Glob` | `Glob` |
| `web_fetch`、`WebFetch` | `Fetch` |
| `todo_write`、`TodoWrite` | `Todo` |
| `Task`、`Agent` | `Agent` |
| 其它 | 原名 |

### 6.3 同类工具聚合与读/搜/列目录折叠

聚合发生在 `prepare_renderable_messages` 的末段：先 `apply_grouping`，再 `collapse_read_search_groups`。verbose 模式会跳过两类聚合。

| 聚合类型 | 入口函数 | 支持工具 | 触发条件 | 显示函数 | 视觉信号 |
|---|---|---|---|---|---|
| 同类工具聚合 | `apply_grouping`、`is_groupable_tool` | `Task`、`Agent`、`Read`、`Grep`、`Glob` | 非 verbose；同一 `source_index` + 同一工具名数量大于等于 2 | `render_grouped_tool_use_lines` | `  ● N Tool calls · completed/failed/partially completed`，`tool_name` |
| 读/搜/列目录折叠 | `collapse_read_search_groups`、`collapsible_kind_for_tool` | `Read`、`Grep`、`Glob`、`WebSearch`、`LS`、`List` | 非 verbose；连续可折叠工具总数大于等于 2；也可吸收前一步的 grouped record | `render_collapsed_read_search_lines` | `Searched for N patterns, Read N files Ctrl+O to expand`，`dim` |

不聚合或弱支持的工具：

| 工具/类别 | 当前行为 | 原因 |
|---|---|---|
| `Bash`、`PowerShell` | 不进入同类工具聚合，也不进入读/搜折叠 | `is_groupable_tool` 与 `collapsible_kind_for_tool` 均未包含 |
| `Edit`、`Write`、`MultiEdit`、`NotebookEdit`、`FileEdit`、`FileWrite` | 不聚合；逐条显示专用文件编辑行 | 避免隐藏写入语义，当前没有聚合规则 |
| `TodoWrite` | 不聚合；每条最多展示 12 个 todo | 专用 renderer 已经承担列表摘要 |
| `WebFetch` | 不聚合也不折叠 | fallback 可显示 `Fetch(url=...)`，但折叠列表只含 `WebSearch` |
| `ServerToolUse` | 可参与同类聚合/折叠，前提是名字命中规则 | grouping 对 `ToolUse` 和 `ServerToolUse` 共用匹配 | 聚合后不会保留 `server:` 前缀 |
| 任意未知工具 | 走 fallback 单条显示 | 没有聚合规则，只做参数摘要 |

### 6.4 ToolActivity 五态

`crates/allthecodes/src/ui/rendering/tool_activity.rs` 的 `ToolState` 用于 compact activity 行，不是 assistant 主链路的直接状态源。它有明确的状态颜色映射和进度条字段。

| `ToolState` | label | 状态样式 | 名称样式 | 当前使用位置 |
|---|---|---|---|---|
| `Queued` | `[queued]` | `dim` | `tool_name` | `compact_styled_line`、测试 helper |
| `Running` | `[running]` | `info` | `tool_name` | `compact_styled_line`、fallback 摘要构造 |
| `Succeeded` | `[succeeded]` | `diff_add` | `diff_add` | `compact_styled_line` |
| `Failed` | `[failed]` | `error` | `tool_name` | `compact_styled_line` |
| `Cancelled` | `[cancelled]` | `warning` | `tool_name` | `compact_styled_line` |

`ToolActivity::compact_styled_line` 还会展示 `worked for ...`、`done/total [progress bar]`、`error: ...`、summary、输出行数。进度条使用 `progress_fill` 和 `progress_empty`，不是第二章旧表中描述的 spinner。spinner 模块存在于 `crates/allthecodes/src/ui/rendering/spinner.rs`，但当前这里的工具状态 compact 行没有直接使用 spinner。

## 7. 支持边界总览

| 机制 | 已完整支持 | 弱支持/未接线 | 主要改进入口 |
|---|---|---|---|
| 角色样式 | 顶层消息分发、用户背景、系统前缀、工具主色 | `assistant_name`/`user_name` 主要是 legacy 字段 | `render/mod.rs`、`theme.rs` |
| ContentBlock | 文本、连接器文本、工具调用、工具结果、图片引用 | thinking/redacted thinking 依赖 verbose/transcript；复杂 ToolResult blocks 只显示 `[...]` | `render_assistant.rs` |
| 用户消息 | 普通文本、bash 输出、shell 结果、文件编辑预览、图片、中断 | 非 shell 工具结果传入空 `Tools`，工具元数据弱 | `render_user.rs`、`user_tool_result_message` |
| 系统消息 | 压缩边界、微压缩隐藏、API 错误、info/warning/error/local command | 普通系统消息没有更细图标层 | `render_assistant.rs`、`system_text_message.rs` |
| 进度/附件 | progress 摘要、核心 Attachment 枚举、丰富 label/detail helper | 顶层 Attachment 未接入 helper 的全部类型；StructuredOutput 不展开 | `render_assistant.rs`、`attachment_message.rs` |
| 工具状态 | in-progress/resolved/error 主链路、shell/edit/todo 专用格式、聚合/折叠 | queued/permission/classifier 状态 helper 有但主链路未推导；Bash/Edit/Todo/WebFetch 不聚合 | `context.rs`、`grouping.rs`、`assistant_tool_use_message.rs` |
