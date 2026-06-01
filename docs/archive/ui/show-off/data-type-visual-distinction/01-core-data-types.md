# 第一章：核心数据类型枚举索引

本文索引 `data-type-visual-distinction.md` 第一章涉及的核心消息类型，重点说明它们在 allthecodes TUI 中承担的职责、关键字段、源码位置、渲染入口，以及当前是直接可见、默认隐藏，还是通过聚合/上下文间接展示。

## 1. 总览：从数据到 TUI 行

| 阶段 | 入口 | 关键类型 | 作用 | 备注 |
|---|---|---|---|---|
| 数据定义 | `crates/allthecodes-types/src/message.rs:7` | `ContentBlock` | API 内容块联合枚举 | 助手文本、工具调用、工具结果、思考、图片都在这里分型。 |
| 数据定义 | `crates/allthecodes-types/src/message.rs:273` | `Message` | TUI 消息流顶层联合枚举 | 包含 `User`、`Assistant`、`System`、`Progress`、`Attachment`。 |
| 预处理 | `crates/allthecodes/src/ui/messages/render/context.rs:151` | `prepare_renderable_messages` | 生成可渲染消息与查找表 | 会归一化、过滤、重排工具结果、聚合工具调用、折叠读/搜/列目录。 |
| 归一化 | `crates/allthecodes/src/ui/messages/render/preprocessing.rs:12` | `normalize_messages_for_render` | 将多块消息拆成单块 `RenderableMessage::Message` | 后续聚合和折叠主要基于“第一块内容”。 |
| 可见性过滤 | `crates/allthecodes/src/ui/messages/render/preprocessing.rs:175` | `should_show_renderable_message` | 决定消息是否进入主 TUI 列表 | `Progress` 默认隐藏，部分 `Attachment` 默认隐藏，纯 meta 用户消息默认隐藏。 |
| 主渲染 | `crates/allthecodes/src/ui/messages/render/mod.rs:53` | `render_messages` | 将可见消息写入 ratatui `Buffer` | 处理虚拟滚动、用户消息背景、流式光标。 |
| 单条分派 | `crates/allthecodes/src/ui/messages/render/mod.rs:214` | `render_single_message_with_context` | 按 `Message` 变体分派到专用 renderer | `User`/`Assistant`/`System`/`Progress`/`Attachment` 的直接入口。 |
| 聚合分派 | `crates/allthecodes/src/ui/messages/render/mod.rs:234` | `render_renderable_message_with_context` | 渲染真实消息或派生聚合记录 | 额外支持 `GroupedToolUse` 和 `CollapsedReadSearch`。 |

可见性术语：

| 状态 | 含义 |
|---|---|
| 直接可见 | 该类型自身会被渲染成 TUI 行。 |
| 默认隐藏 | 默认 TUI 主列表不展示；可能在 verbose/transcript 或内部查找表中保留。 |
| 间接展示 | 原始类型不单独展示，信息被用于状态、聚合、折叠、复制文本或详情。 |

## 2. `Message` 顶层枚举

定义：`crates/allthecodes-types/src/message.rs:273`。公共辅助方法 `Message::uuid` 和 `Message::timestamp` 位于 `crates/allthecodes-types/src/message.rs:325`，TUI 用它们做滚动、选择、缓存和派生聚合 UUID。

| 变体 | 职责 | 关键字段 | TUI 渲染入口 | 当前展示状态 | 说明 |
|---|---|---|---|---|---|
| `Message::User(UserMessage)` | 用户输入、用户回放、工具结果承载消息 | `uuid`、`timestamp`、`content`、`is_meta`、`tool_use_result`、`source_tool_assistant_uuid`，见 `message.rs:82` | `render_single_message_with_context` -> `render_user_message`，见 `render/mod.rs:221`、`render_user.rs:28` | 直接可见；部分 meta 隐藏 | 普通文本走 `render_user_text_message`；`ToolResult` 会转入工具结果 renderer；`is_meta && !transcript && !tool_result` 默认隐藏。 |
| `Message::Assistant(AssistantMessage)` | 模型响应与模型发起的工具调用 | `content: Vec<ContentBlock>`、`usage`、`stop_reason`、`is_api_error_message`、`api_error`、`cost_usd`，见 `message.rs:97` | `render_single_message_with_context` -> `render_assistant_message`，见 `render/mod.rs:225`、`render_assistant.rs:22` | 直接可见；纯思考块可能隐藏 | 文本 Markdown 渲染；工具调用转 `render_assistant_tool_use_message`；非零 `cost_usd` 追加灰色成本行。 |
| `Message::System(SystemMessage)` | 系统事件、边界、错误、提示 | `subtype`、`content`，见 `message.rs:194` | `render_single_message_with_context` -> `render_system_message`，见 `render/mod.rs:228`、`render_assistant.rs:287` | 直接可见；`MicrocompactBoundary` 隐藏 | `CompactBoundary` 使用专用压缩边界组件；API retry 错误使用系统文本错误组件。 |
| `Message::Progress(ProgressMessage)` | 工具或 hook 的进度事件 | `tool_use_id`、`data`，见 `message.rs:203` | 有 renderer：`render_progress_message`，见 `render_assistant.rs:366`；但预处理默认过滤 | 默认隐藏；间接保存 | `should_show_renderable_message` 对 `Message::Progress` 返回 `false`，见 `preprocessing.rs:179`；同时 `build_message_lookups` 仍收集到 `progress_messages_by_tool_use_id`，见 `context.rs:231`。 |
| `Message::Attachment(AttachmentMessage)` | 文件变更、排队命令、结构化输出、技能发现等附加事件 | `attachment: Attachment`，见 `message.rs:212` | `render_single_message_with_context` -> `render_attachment_message`，见 `render/mod.rs:230`、`render_assistant.rs:389` | 部分直接可见，部分默认隐藏 | `EditedTextFile`、`MaxTurnsReached`、`StructuredOutput` 属于 null rendering attachment，默认过滤；其他附件可见。 |

### `MessageContent`

| 类型 | 源码 | 用途 | TUI 行为 |
|---|---|---|---|
| `MessageContent::Text(String)` | `crates/allthecodes-types/src/message.rs:250` | 用户消息的纯文本形态 | `render_user_message` 直接读取文本；用户背景是否铺色由 `user_message_uses_background` 判断。 |
| `MessageContent::Blocks(Vec<ContentBlock>)` | `crates/allthecodes-types/src/message.rs:252` | 用户消息携带多模态内容或工具结果 | 归一化时拆成单块用户消息；含 `ToolResult` 时优先走工具结果渲染。 |

## 3. `ContentBlock` 内容块索引

定义：`crates/allthecodes-types/src/message.rs:7`。TUI 中 `AssistantMessage.content` 直接是 `Vec<ContentBlock>`；`UserMessage.content` 通过 `MessageContent::Blocks` 间接持有内容块。

| 变体 | 关键字段 | 职责 | Assistant 渲染 | User 渲染 | 可见性 |
|---|---|---|---|---|---|
| `Text` | `text`，见 `message.rs:8` | 普通文本/Markdown | `render_assistant_message` 使用 `markdown_to_lines`；API 错误合成消息改走 `api_error_display_text`，见 `render_assistant.rs:34` | 普通用户文本走 `render_user_text_message`，见 `render_user.rs:80` | 直接可见；空文本会被过滤。 |
| `ToolUse` | `id`、`name`、`input`，见 `message.rs:11` | 模型请求执行本地工具 | `tool_state_for_id` 计算状态，再调 `render_assistant_tool_use_message`，见 `render_assistant.rs:83` | 作为用户内容时只在 copy/reference 中摘要，不是常规用户渲染路径 | 直接可见；可被聚合/折叠替代。 |
| `ServerToolUse` | `id`、`name`、`input`，见 `message.rs:18` | 服务端/MCP 工具调用 | 与 `ToolUse` 同样渲染，但每行加 `server: ` 前缀，见 `render_assistant.rs:101` | 同 `ToolUse` | 直接可见；名称匹配时同样可被聚合/折叠。 |
| `ToolResult` | `tool_use_id`、`content`、`is_error`，见 `message.rs:25` | 工具执行结果 | 如果出现在 assistant 内容中，展示最多 5 行 `Result`/`Error` 预览，见 `render_assistant.rs:119` | 用户工具结果的主路径；shell、file edit、通用工具结果分流，见 `render_user.rs:154` | 通常直接可见；也会间接影响工具调用状态。 |
| `Thinking` | `thinking`、`signature`，见 `message.rs:33` | 模型思考内容 | `render_assistant_thinking_lines`，见 `render_assistant.rs:175` | copy 文本可提取 thinking；用户渲染没有专门路径 | 默认通常隐藏或折叠；verbose/transcript 可见。 |
| `RedactedThinking` | `data`，见 `message.rs:39` | 被隐藏/截断的思考块 | 仅在 verbose 或 transcript 中调用 `render_assistant_redacted_thinking_lines`，见 `render_assistant.rs:207` | copy 文本显示 `[redacted thinking]` | 默认隐藏；verbose/transcript 可见。 |
| `ConnectorText` | `connector_text`、`signature`，见 `message.rs:42` | 连接器文本内容 | 与普通文本相同，走 Markdown 渲染，见 `render_assistant.rs:65` | copy 文本可提取；用户无专门样式 | 直接可见。 |
| `Image` | `source: ImageSource`，见 `message.rs:48` | base64 图片引用 | 渲染为 `[image: <media_type>, <chars> chars]`，见 `render_assistant.rs:226`、`copy_text.rs:185` | `render_user_image_blocks` 渲染同样的图片引用，见 `render_user.rs:106` | 直接可见，但不是终端内联图片。 |

### `ToolResultContent`

| 变体 | 源码 | 职责 | TUI 行为 |
|---|---|---|---|
| `Text(String)` | `crates/allthecodes-types/src/message.rs:55` | 纯文本工具输出 | assistant 结果预览取前 5 行；user 工具结果可进入 shell/file edit/通用工具结果 renderer。 |
| `Blocks(Vec<ContentBlock>)` | `crates/allthecodes-types/src/message.rs:56` | 嵌套内容块输出 | assistant 路径只保留 `Text`、`ConnectorText`、`Image`，其他块显示 `[...]`；user 路径只保留 `Text` 和 `Image`，见 `render_user.rs:276`。 |

## 4. `SystemSubtype` 系统消息索引

定义：`crates/allthecodes-types/src/message.rs:116`。渲染入口是 `render_system_message`，见 `crates/allthecodes/src/ui/messages/render/render_assistant.rs:287`。

| 变体 | 关键字段 | 职责 | TUI 展示 | 可见性 |
|---|---|---|---|---|
| `CompactBoundary` | `compact_metadata: Option<CompactMetadata>` | 标记一次上下文压缩边界 | 直接返回 `render_compact_boundary_lines(theme)`，见 `render_assistant.rs:334` | 直接可见；非 verbose 模式会只保留最后一个边界后的消息，见 `preprocessing.rs:148`。 |
| `MicrocompactBoundary` | `microcompact_metadata: Option<MicrocompactMetadata>` | 标记工具结果微压缩 | `render_system_message` 直接返回空 `Vec`，见 `render_assistant.rs:330` | 默认隐藏。 |
| `ApiError` | `retry_attempt`、`max_retries`、`retry_in_ms`、`error` | API 错误与重试状态 | 走 `render_system_text_message("api_error", ...)` 后按 `theme.error` 输出，见 `render_assistant.rs:306` | 直接可见；重排时只保留连续/历史 API error 的最后一条，见 `preprocessing.rs:215`。 |
| `Informational { level: Info }` | `level` | 普通系统提示 | 前缀 `Info: `，样式 `theme.unselected`，见 `render_assistant.rs:297` | 直接可见。 |
| `Informational { level: Warning }` | `level` | 系统警告提示 | 前缀 `Warning: `，样式 `theme.warning` | 直接可见。 |
| `Informational { level: Error }` | `level` | 系统错误提示 | 前缀 `Error: `，样式 `theme.error` | 直接可见。 |
| `LocalCommand` | `content` | 本地命令输出/记录 | 前缀 `$ `，样式 `theme.system_name`，见 `render_assistant.rs:302` | 直接可见；当前 match 使用 `msg.content` 输出，变体内 `content` 字段不单独读取。 |
| `Warning` | 无额外字段 | 通用警告 | 前缀 `Warning: `，样式 `theme.warning`，见 `render_assistant.rs:303` | 直接可见。 |

系统元数据补充：

| 类型 | 源码 | 用途 | 展示情况 |
|---|---|---|---|
| `CompactMetadata` | `crates/allthecodes-types/src/message.rs:147` | 记录压缩前后 token、保留段、压缩前发现的工具 | `render_compact_boundary_lines` 当前只负责边界展示；`compact_boundary_summary` 可生成 token 摘要，但现有 `CompactBoundary` 分支先返回专用组件。 |
| `MicrocompactMetadata` | `crates/allthecodes-types/src/message.rs:172` | 记录触发原因、节省 token、被压缩工具结果 ID、清理附件 UUID | 主 TUI 隐藏；可通过 transcript/调试路径保留数据。 |
| `ApiErrorInfo` | `crates/allthecodes-types/src/message.rs:188` | API 状态码和错误消息 | `ApiError` 消息内容为空时使用 `error.message`。 |

## 5. 工具调用状态：`ToolUseState` 与 `ToolState`

当前代码中有两套相关状态，职责不同：

| 状态类型 | 源码 | 作用范围 | 入口 |
|---|---|---|---|
| `ToolUseState` | `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs:15` | 单条 assistant `ToolUse`/`ServerToolUse` 在消息列表中的生命周期显示 | `render_assistant_tool_use_message`，见 `assistant_tool_use_message.rs:270` |
| `ToolState` | `crates/allthecodes/src/ui/rendering/tool_activity.rs:8` | 工具活动/任务聚合行的状态标签和颜色 | `ToolActivity::compact_styled_line`、`render_grouped_styled_activity`，见 `tool_activity.rs:105`、`tool_activity.rs:251` |

### `ToolUseState`：消息列表中的工具调用状态

| 变体 | 来源/计算 | 显示方式 | 当前可见性 |
|---|---|---|---|
| `Queued` | 枚举存在，但 `tool_state_for_id` 当前不产生此状态 | 默认格式 `● <tool>`；特殊工具有 `Queued`/`Queued edit`/`Queued todo update` | 间接支持，当前主路径通常不可达。 |
| `InProgress` | `tool_use_id` 未出现在 resolved/errored 集合，见 `render_assistant.rs:264` | `Running`、`Editing`、`Updating todos` 或默认 `● Tool(args)` | 直接可见。 |
| `Resolved` | `build_message_lookups` 收到用户 `ToolResult` 后加入 `resolved_tool_use_ids`，见 `context.rs:207` | shell 显示 `Ran`，编辑显示 `Edited`，Todo 显示 `Updated todos`，默认显示工具摘要 | 直接可见；也可能被聚合/折叠替代。 |
| `Error` | 用户 `ToolResult.is_error = true` 时加入 `errored_tool_use_ids`，见 `context.rs:223` | shell `Failed [error]`，编辑 `Edit failed [error]`，默认 `● Tool(args) [error]` | 直接可见；错误计数也进入聚合行。 |
| `WaitingForPermission` | 枚举存在，当前 `tool_state_for_id` 不产生 | `waiting for permission...` 或特殊工具的 permission 文案 | 间接支持，当前主路径通常不可达。 |
| `ClassifierChecking` | 枚举存在，当前 `tool_state_for_id` 不产生 | `classifier checking...` 或特殊工具 checking 文案 | 间接支持，当前主路径通常不可达。 |

特殊工具显示索引：

| 工具名 | 特殊渲染函数 | 支持的输入字段 | 显示特点 |
|---|---|---|---|
| `Bash`、`PowerShell` | `render_shell_tool_use_message`，见 `assistant_tool_use_message.rs:100` | `description`、`command` | 两行显示：状态标题加 `⎿  Bash(...)`/`PowerShell(...)` 调用摘要。 |
| `Edit`、`Write`、`FileEdit`、`FileWrite`、`NotebookEdit`、`MultiEdit` | `render_file_edit_tool_use_message`，见 `assistant_tool_use_message.rs:127` | `file_path`、`path`、`notebook_path` | 两行显示：编辑状态加 `⎿  Edit(path=...)`。 |
| `TodoWrite`、`todo_write` | `render_todo_write_tool_use_message`，见 `assistant_tool_use_message.rs:164` | `todos[].status`、`todos[].content`、`todos[].activeForm` | 显示最多 12 条 checklist，状态为 `[x]`、`[*]`、`[ ]`。 |
| 其他工具 | `ToolActivity::from_tool_use` + 默认分支，见 `assistant_tool_use_message.rs:293` | 优先 `path`、`file_path`、`command`、`pattern`、`url`、`query`、`description` | 单行 `● DisplayName(args)`。 |

### `ToolState`：工具活动/任务行状态

| 变体 | 标签 | 颜色/样式映射 | 典型输出 |
|---|---|---|---|
| `Queued` | `[queued]` | `theme.dim` | `● Tool(args) | [queued] | worked for 0ms` |
| `Running` | `[running]` | `theme.info` | 可附带 `progress` 和输出行数。 |
| `Succeeded` | `[succeeded]` | 状态和工具名用 `theme.diff_add` | 成功态工具名变绿。 |
| `Failed` | `[failed]` | `theme.error` | `error_summary` 存在时追加 `error: ...`。 |
| `Cancelled` | `[cancelled]` | `theme.warning` | 用于取消/中断的任务活动。 |

`ToolActivity` 的关键字段定义在 `crates/allthecodes/src/ui/rendering/tool_activity.rs:29`：`name`、`user_facing_name`、`arguments_summary`、`state`、`summary`、`error_summary`、`elapsed_ms`、`progress`、`output_lines`、`output_preview`。工具名归一化由 `user_facing_tool_name` 完成，见 `tool_activity.rs:288`。

## 6. 同类工具聚合与折叠支持矩阵

TUI 里有两类派生显示，不是 `message.rs` 中的持久数据类型，而是渲染上下文中的 `RenderableMessage` 变体：

| 派生类型 | 源码 | 生成入口 | 渲染入口 | 作用 |
|---|---|---|---|---|
| `RenderableMessage::GroupedToolUse` | `crates/allthecodes/src/ui/messages/render/context.rs:43` | `apply_grouping`，见 `grouping.rs:12` | `render_grouped_tool_use_lines`，见 `render/mod.rs:245` | 将同一原始消息内连续/拆分出的同名工具调用聚成一行。 |
| `RenderableMessage::CollapsedReadSearch` | `crates/allthecodes/src/ui/messages/render/context.rs:44` | `collapse_read_search_groups`，见 `grouping.rs:87` | `render_collapsed_read_search_lines`，见 `render/mod.rs:266` | 将读文件、搜索、列目录类工具折叠成摘要行。 |

### `GroupedToolUse` 支持哪些工具

`is_groupable_tool` 当前只支持以下名称，见 `crates/allthecodes/src/ui/messages/render/grouping.rs:293`：

| 工具名 | 是否支持同类聚合 | 显示名称 | 说明 |
|---|---:|---|---|
| `Task` | 是 | `Agent` | `user_facing_tool_name` 将 `Task` 映射为 `Agent`，见 `tool_activity.rs:299`。 |
| `Agent` | 是 | `Agent` | 多个 agent/subagent 调用会聚合为 `N Agent calls`。 |
| `Read` | 是 | `Read` | 也可继续进入读/搜折叠。 |
| `Grep` | 是 | `Search` | 也可继续进入读/搜折叠。 |
| `Glob` | 是 | `Glob` | 也可继续进入读/搜折叠。 |
| `ServerToolUse` 中同名工具 | 是 | 取决于名称映射 | `grouping_tool_use_key` 同时匹配 `ToolUse` 和 `ServerToolUse`，见 `grouping.rs:281`。 |

聚合行的内容由 `GroupedToolUseView` 决定：`tool_name`、`count`、`resolved_count`、`error_count`，见 `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs:8`。渲染规则是 `N <DisplayName> calls`，失败时追加 `· N failed`，全部完成追加 `· completed`，部分完成追加 `· R/N completed`。

### 当前不支持 `GroupedToolUse` 的常见工具

| 工具名/类别 | 聚合状态 | 原因/替代展示 |
|---|---|---|
| `Bash`、`PowerShell` | 不支持 | 不在 `is_groupable_tool` 白名单；单条工具调用有 shell 专用两行显示，工具结果走 shell output renderer。 |
| `Edit`、`Write`、`FileEdit`、`FileWrite`、`MultiEdit`、`NotebookEdit` | 不支持 | 不在白名单；单条调用使用编辑专用渲染，结果可走 file edit diff 预览。 |
| `TodoWrite`、`todo_write` | 不支持 | 不在白名单；自身有 checklist 专用显示。 |
| `WebFetch` | 不支持 | 不在白名单；默认工具摘要或工具结果展示。 |
| `WebSearch` | 不支持同类聚合 | 不在 `is_groupable_tool`；但支持 `CollapsedReadSearch` 搜索折叠。 |
| `LS`、`List` | 不支持同类聚合 | 不在 `is_groupable_tool`；但支持 `CollapsedReadSearch` 列目录折叠。 |
| 未知工具名 | 不支持 | 默认单条 `ToolActivity` 摘要显示。 |

### `CollapsedReadSearch` 支持哪些工具

`collapsible_kind_for_tool` 当前支持以下名称，见 `crates/allthecodes/src/ui/messages/render/grouping.rs:256`：

| 工具名 | 折叠类别 | 摘要字段 | 显示效果 |
|---|---|---|---|
| `Read` | Read | `read_count` | `Read N files` 或 active 时 `Reading N files...` |
| `Grep` | Search | `search_count` | `Searched for N patterns` 或 active 时 `Searching for N patterns...` |
| `Glob` | Search | `search_count` | 同搜索类。 |
| `WebSearch` | Search | `search_count` | 同搜索类。 |
| `LS` | List | `list_count` | `Listed N directories` 或 active 时 `Listing N directories...` |
| `List` | List | `list_count` | 同列目录类。 |

折叠至少需要两个读/搜/列目录类工具，见 `grouping.rs:151`。active 状态来自 `in_progress_tool_use_ids`，见 `render/mod.rs:267`；active 时会显示 `latest_hint`，该提示来自 `tool_primary_input`，见 `grouping.rs:245`。

## 7. 可见、隐藏、间接展示清单

| 类型/变体 | 默认主列表 | verbose/transcript 影响 | 间接用途 |
|---|---|---|---|
| `Message::User` 普通文本 | 可见 | transcript 可显示 meta/历史文本 | 复制文本、主引用、选择详情。 |
| `Message::User` 且 `is_meta` 无工具结果 | 隐藏 | transcript 可见 | 保留系统注入上下文。 |
| `Message::User` 携带 `ToolResult` | 可见 | verbose 会影响工具结果详细程度 | 更新 `resolved_tool_use_ids`、`errored_tool_use_ids`、shell 展开状态。 |
| `Message::Assistant` 文本 | 可见 | transcript 可完整导出 | Markdown 渲染、复制文本、主引用。 |
| `Message::Assistant` 工具调用 | 可见或被聚合/折叠 | verbose 禁用聚合/折叠 | 建立 `tool_uses` 查找表，驱动后续工具结果渲染。 |
| `Thinking` | 默认通常不出行 | verbose/transcript 可见 | `last_thinking_block_id` 进入 render cache key。 |
| `RedactedThinking` | 默认隐藏 | verbose/transcript 可见 | copy 文本可显示 `[redacted thinking]`。 |
| `SystemSubtype::CompactBoundary` | 可见 | verbose/transcript 不裁剪边界前消息 | 非 verbose 下作为历史裁剪点。 |
| `SystemSubtype::MicrocompactBoundary` | 隐藏 | 当前 renderer 仍返回空行 | 保留微压缩元数据。 |
| `SystemSubtype::ApiError` | 可见 | 无特殊展开 | 历史 API error 会被重排过滤，只保留末尾有效错误。 |
| `Message::Progress` | 隐藏 | 当前过滤逻辑仍隐藏 | 收集到 `progress_messages_by_tool_use_id`，为工具进度/未来 hook 展示保留。 |
| `Attachment::EditedTextFile` | 隐藏 | 当前过滤逻辑隐藏 | copy/reference 可保留 path；变更详情通常由工具结果 diff 展示。 |
| `Attachment::MaxTurnsReached` | 隐藏 | 当前过滤逻辑隐藏 | copy 文本可表达轮次上限。 |
| `Attachment::StructuredOutput` | 隐藏 | 当前过滤逻辑隐藏 | copy 文本为 JSON 数据。 |
| `Attachment::QueuedCommand` | 可见 | 无特殊展开 | 使用 `attachment_message::render_attachment_message` 生成提示。 |
| `Attachment::HookStoppedContinuation` | 可见 | 无特殊展开 | 显示 `[hook stopped continuation]`。 |
| `Attachment::NestedMemory` | 可见 | 无特殊展开 | 使用附件 helper 渲染，reference 为 `path=...`。 |
| `Attachment::SkillDiscovery` | 可见 | 无特殊展开 | 渲染技能列表 JSON。 |

## 8. 路径速查

| 主题 | 文件/类型/函数 |
|---|---|
| 核心消息类型 | `crates/allthecodes-types/src/message.rs`：`ContentBlock`、`ToolResultContent`、`UserMessage`、`AssistantMessage`、`SystemSubtype`、`ProgressMessage`、`Attachment`、`MessageContent`、`Message` |
| 主消息渲染 | `crates/allthecodes/src/ui/messages/render/mod.rs`：`render_messages`、`render_single_message_with_context`、`render_renderable_message_with_context` |
| 渲染上下文 | `crates/allthecodes/src/ui/messages/render/context.rs`：`MessageRenderContext`、`RenderableMessage`、`MessageLookups`、`prepare_renderable_messages`、`build_message_lookups` |
| 预处理/过滤 | `crates/allthecodes/src/ui/messages/render/preprocessing.rs`：`normalize_messages_for_render`、`should_show_renderable_message`、`filter_compact_boundary`、`reorder_messages_in_ui` |
| Assistant/System/Progress/Attachment 渲染 | `crates/allthecodes/src/ui/messages/render/render_assistant.rs`：`render_assistant_message`、`tool_state_for_id`、`render_system_message`、`render_progress_message`、`render_attachment_message` |
| User/ToolResult 渲染 | `crates/allthecodes/src/ui/messages/render/render_user.rs`：`render_user_message`、`render_tool_result_user_message`、`render_file_edit_preview`、`tool_result_content_text` |
| 工具调用消息 | `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs`：`ToolUseState`、`render_assistant_tool_use_message`、各特殊工具 renderer |
| 工具聚合/折叠 | `crates/allthecodes/src/ui/messages/render/grouping.rs`：`apply_grouping`、`collapse_read_search_groups`、`is_groupable_tool`、`collapsible_kind_for_tool` |
| 工具活动状态 | `crates/allthecodes/src/ui/rendering/tool_activity.rs`：`ToolState`、`ToolActivity`、`render_grouped_styled_activity`、`user_facing_tool_name` |
