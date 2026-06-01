# 第五章：渲染管线索引

> 本文索引 `data-type-visual-distinction.md` 第五章“渲染管线”的实现路径，重点说明一条消息从数据模型进入 TUI，到最终写入 ratatui `Buffer` 的完整链路。
>
> 范围只覆盖 Rust TUI：`crates/allthecodes/src/ui/`。

---

## 1. 总览：从 `Message` 到终端像素

渲染管线可以看作八个连续阶段：

```text
App::render / App::render_transcript
  -> build_message_render_context_with_options
  -> prepare_renderable_messages
  -> VirtualScroll::ensure_up_to_date
  -> render_messages
  -> render_renderable_message_for_layout / render_renderable_message_wrapped
  -> 类型专用渲染器 + Markdown / syntax highlight
  -> Buffer::set_style / Buffer::set_line
```

每一层只处理自己负责的形态：

| 层级 | 输入 | 输出 | 主要职责 |
|---|---|---|---|
| 数据模型层 | `Vec<Message>` | 原始消息流 | 保留后端语义：角色、内容块、工具调用、工具结果、附件、系统事件 |
| Context 预处理层 | `&[Message] + MessageRenderOptions` | `MessageRenderContext` | 标准化、过滤、重排、聚合、建立工具查找表和缓存键 |
| 布局计算层 | `MessageRenderContext + width + Theme` | 每条消息高度、总高度、可见范围 | 计算换行后的视觉高度，支持滚动和宽度变化 |
| 消息分发层 | `RenderableMessage` | `Vec<Line>` | 按消息类型和聚合记录选择渲染器 |
| 类型专用渲染层 | 具体消息 / 内容块 | 带样式的 `Line` | 呈现用户、助理、工具、系统、进度、附件等差异 |
| Markdown/高亮层 | Markdown 文本 / fenced code | 带样式的 `Line` / `Span` | 解析标题、列表、表格、代码块和语法高亮 |
| 包装与选择装饰层 | `Vec<Line>` | 包装后的 `Vec<Line>` | 宽度换行、选中态元数据、用户消息背景留白 |
| Buffer 输出层 | 可见消息行 | `ratatui::Buffer` 单元格 | 填充背景、写入行、追加流式光标和滚动条 |

---

## 2. 数据模型层

### 输入

数据源是 `App.messages: Vec<Message>`。顶层类型来自 `allthecodes_types::message`，TUI 目前关心这些变体：

| 变体 | 进入渲染后的主要用途 |
|---|---|
| `Message::User` | 用户输入、工具结果、shell 输出、图片块、文件编辑预览 |
| `Message::Assistant` | Markdown 文本、工具调用、server tool use、thinking、图片、API 错误文本 |
| `Message::System` | compact boundary、microcompact boundary、API error、info/warning/error、本地命令 |
| `Message::Progress` | 工具执行过程中的短进度文本；默认不作为独立消息显示，但进入 lookup |
| `Message::Attachment` | 队列命令、nested memory、skill discovery 等附件提示；部分附件空渲染 |

### 输出

这一层不直接输出 UI 行，只把结构化语义交给 Context 层。重要约束是：`Message` 的原始顺序和 `source_index` 后续会用于选中、复制、工具结果回填和聚合来源追踪。

### 关键入口

- `crates/allthecodes/src/ui/app/render.rs`
  - `App::render`
  - `App::render_transcript`
- `crates/allthecodes/src/ui/messages.rs`
  - re-export `render_messages`
  - re-export `build_message_render_context_with_options`

### 可见行为

数据模型本身不可见，但它决定所有后续分支：

- Assistant 的 `ContentBlock::Text` 会进入 Markdown 渲染。
- Assistant 的 `ContentBlock::ToolUse` / `ServerToolUse` 会显示工具活动行。
- User 的 `ContentBlock::ToolResult` 会被工具结果渲染器解释。
- System 的 `SystemSubtype::MicrocompactBoundary` 最终隐藏。
- Attachment 中 `EditedTextFile`、`MaxTurnsReached`、`StructuredOutput` 会被预处理判定为空渲染并过滤。

### 调试入口

- 先确认 `self.messages` 是否包含目标数据类型。
- 如果消息存在但 UI 不显示，优先查 Context 层的过滤函数，而不是 Buffer 层。

---

## 3. Context 预处理层

Context 层定义在 `crates/allthecodes/src/ui/messages/render/context.rs`，它把原始消息转成可渲染消息和查找表。

### 输入

```rust
build_message_render_context_with_options(
    &self.messages,
    self.selected_message,
    self.selected_message_expanded,
    MessageRenderOptions { ... },
)
```

`MessageRenderOptions` 控制三类行为：

| 字段 | 作用 |
|---|---|
| `verbose` | 禁用工具聚合 / 读写搜索折叠，并展示更多 thinking/redacted thinking 细节 |
| `is_transcript_mode` | transcript/focus 视图使用；保留 compact 前内容和 meta 消息 |
| `show_all_in_transcript` | transcript 模式是否截断最近 30 条 |

### 输出

`MessageRenderContext`：

| 字段 | 用途 |
|---|---|
| `renderable_messages` | 实际参与布局和绘制的 `RenderableMessage` 列表 |
| `lookups` | 工具调用、工具结果、进度、错误、进行中状态等索引 |
| `selected_message` / `selected_expanded` | 控制选中态和详情展开 |
| `options` | 向子渲染器传递 verbose/transcript 语义 |
| `cache_key` | 供 `VirtualScroll` 判断高度缓存是否失效 |

`RenderableMessage` 有三种形态：

| 形态 | 含义 |
|---|---|
| `Message { message, source_index }` | 标准单条消息或拆分后的单个内容块 |
| `GroupedToolUse(GroupedToolUseRenderRecord)` | 同源同类工具调用聚合 |
| `CollapsedReadSearch(CollapsedReadSearchRenderRecord)` | 连续 Read/Search/List 类操作折叠 |

### 关键函数链

`prepare_renderable_messages` 是核心流水线：

```text
normalize_messages_for_render
  -> filter(is_not_empty_renderable_message)
  -> filter_compact_boundary
  -> should_show_renderable_message
  -> reorder_messages_in_ui
  -> filter_brief_messages
  -> truncate_transcript_messages
  -> apply_grouping
  -> collapse_read_search_groups
  -> build_message_lookups
```

#### 3.1 `normalize_messages_for_render`

位置：`messages/render/preprocessing.rs`

| 输入 | 输出 |
|---|---|
| 原始 `&[Message]` | `Vec<RenderableMessage::Message>` |

它把多内容块消息拆成多个单内容块消息：

- Assistant 的 `content: Vec<ContentBlock>` 每个 block 单独成为一条可渲染记录。
- User 的 `MessageContent::Text` 会转成 `ContentBlock::Text`；`Blocks` 则逐块拆分。
- 拆分后保留原始 `source_index`，并通过 `derive_child_uuid` 为子块生成稳定 uuid。

可见行为：

- 一个 Assistant 回复里“文本 + 工具调用 + 文本”会在 UI 上按块分隔、换行和插入空行。
- 选中原始消息时，拆出来的子块仍可通过 `source_index` 关联到同一条源消息。

#### 3.2 空消息与 compact 过滤

位置：`messages/render/preprocessing.rs`

关键函数：

- `is_not_empty_renderable_message`
- `is_empty_message_text`
- `strip_prompt_xml_tags`
- `filter_compact_boundary`
- `should_show_renderable_message`

可见行为：

- 空文本、`[NO_CONTENT]`、部分 prompt XML 标签剥离后为空的消息不会显示。
- 普通模式下只显示最近一次 `CompactBoundary` 之后的内容；`verbose` 或 transcript 模式保留完整消息。
- `Progress` 默认不直接出现在消息列表，但它的内容会进入 lookup，用于工具进度关系。
- `Attachment::EditedTextFile`、`MaxTurnsReached`、`StructuredOutput` 被视为空渲染附件，不进入主消息列表。
- meta user 消息在非 transcript 模式下如果不含工具结果会隐藏。

#### 3.3 工具结果重排

位置：`messages/render/preprocessing.rs`

关键函数：

- `reorder_messages_in_ui`
- `tool_use_id`
- `tool_result_id`
- `is_api_error_message`

可见行为：

- 工具调用后紧跟对应工具结果，即使原始消息流中结果位置不同。
- 同一个 API error 连续出现时只保留最新一个；非最后位置的 API error 会被移除。

#### 3.4 同类工具聚合

位置：`messages/render/grouping.rs`

关键函数：

- `apply_grouping`
- `grouping_tool_use_key`
- `is_groupable_tool`

支持聚合的工具：

| 工具名 | 是否同类聚合 | 说明 |
|---|---:|---|
| `Task` | 是 | 同一 `source_index` 下多个 Task 调用聚成一条 |
| `Agent` | 是 | 同一 `source_index` 下多个 Agent 调用聚成一条 |
| `Read` | 是 | 可先聚合，后续还可能进入 Read/Search/List 折叠 |
| `Grep` | 是 | 可先聚合，后续还可能进入 Read/Search/List 折叠 |
| `Glob` | 是 | 可先聚合，后续还可能进入 Read/Search/List 折叠 |
| `Bash` / `PowerShell` | 否 | 保留单独命令行活动和结果展开语义 |
| `Edit` / `Write` / `MultiEdit` / `NotebookEdit` | 否 | 保留文件编辑摘要和 diff 预览语义 |
| `TodoWrite` | 否 | 保留 todo checkbox 明细 |
| `LS` / `List` | 否 | 不参与同源同类聚合，但可参与 Read/Search/List 折叠 |
| `WebSearch` | 否 | 不参与同源同类聚合，但可参与 Read/Search/List 折叠 |
| 其他工具 | 否 | 走默认工具调用渲染 |

可见行为：

- 普通模式下，两个及以上同源同名工具调用会显示为 `● N Tool calls · completed` 形式。
- `verbose` 模式关闭聚合，逐条显示原始工具调用。
- 被聚合的工具结果消息会从主列表移除，状态通过 lookup 统计到聚合行。

#### 3.5 Read/Search/List 折叠

位置：`messages/render/grouping.rs`

关键函数：

- `collapse_read_search_groups`
- `collapsible_tool_info`
- `collapsible_kind_for_tool`

支持折叠的工具：

| 类别 | 工具名 |
|---|---|
| Read | `Read` |
| Search | `Grep`、`Glob`、`WebSearch` |
| List | `LS`、`List` |

可见行为：

- 连续两个及以上 Read/Search/List 类操作折叠为一条摘要。
- 摘要会显示 `Searched for N patterns`、`Read N files`、`Listed N directories`。
- 若其中任何工具仍在进行中，会使用 `Searching/Reading/Listing...` 的进行态文案，并显示最新 hint。
- `verbose` 模式关闭折叠。

#### 3.6 Lookup 建立

位置：`messages/render/context.rs`

关键函数：

- `build_message_lookups`
- `find_latest_bash_output_uuid`
- `find_last_thinking_block_id`
- `render_context_cache_key`

主要索引：

| lookup 字段 | 来源 | 用途 |
|---|---|---|
| `tool_uses` | Assistant `ToolUse` / `ServerToolUse` | 给 User tool result 找工具名和输入 |
| `tool_results` | User `ToolResult` | 判断工具是否完成、错误、结果内容 |
| `resolved_tool_use_ids` | User `ToolResult` | 工具状态为 resolved |
| `errored_tool_use_ids` | `is_error = true` 的工具结果 | 工具状态为 error |
| `in_progress_tool_use_ids` | 有调用但无结果的工具 | 工具状态为 in progress |
| `progress_messages_by_tool_use_id` | `Message::Progress` | 保留进度关系 |
| `latest_shell_tool_result_id` | 已完成 shell 工具 | 决定最近 shell 输出默认展开 |
| `latest_bash_output_uuid` | `<bash-stdout>` / `<bash-stderr>` 文本 | 决定 tagged bash 输出默认展开 |
| `last_thinking_block_id` | 最后一个 thinking/redacted thinking | 高度缓存失效依据之一 |

可见行为：

- 工具调用行能显示“进行中/成功/失败”。
- shell 结果默认展开最近一次；选中并展开时也会展开对应 shell 输出。
- 聚合工具行能显示 completed/failed 数量。
- 高度缓存会在选中态、展开态、工具结果、thinking 或 renderable 列表变化时失效。

### 调试入口

优先检查：

```bash
rg -n "prepare_renderable_messages|apply_grouping|collapse_read_search_groups|build_message_lookups|render_context_cache_key" crates/allthecodes/src/ui/messages/render
```

如果某条消息不显示：

1. 看是否被 `is_not_empty_renderable_message` 过滤。
2. 看是否被 `filter_compact_boundary` 截掉。
3. 看是否被 `should_show_renderable_message` 隐藏。
4. 看是否被 `apply_grouping` 或 `collapse_read_search_groups` 合并。

---

## 4. 布局计算层

布局计算由 `App::render` 和 `VirtualScroll` 协作完成。

### 输入

普通对话视图：

- `size: Rect`
- `bottom_pane` 各区域高度
- `message_render_context`
- `self.vscroll`
- `self.scroll_offset`

Transcript 视图：

- header 1 行
- footer 1 行
- body 使用剩余高度
- `MessageRenderOptions { is_transcript_mode: true, show_all_in_transcript: true }`

### 输出

| 输出 | 用途 |
|---|---|
| `message_area` | 主消息区域 |
| `message_body_area` | 去掉滚动条后的消息正文区域 |
| `scrollbar_area` | 可选滚动条区域 |
| `total_visual_lines` | 计算最大滚动偏移和滚动条 |
| `visual_range` | 当前 viewport 需要绘制的消息索引范围 |

### 关键函数

位置：`crates/allthecodes/src/ui/app/render.rs`

- `App::render`
- `App::render_transcript`
- `split_session_scrollbar_area`
- `render_session_scrollbar`

位置：`crates/allthecodes/src/ui/rendering/virtual_scroll.rs`

- `VirtualScroll::ensure_up_to_date`
- `VirtualScroll::total_visual_lines`
- `VirtualScroll::visual_range`
- `VirtualScroll::visual_offset_of`
- `VirtualScroll::invalidate_all`
- `VirtualScroll::invalidate_from`

### 计算过程

```text
App::render
  -> 计算 spinner / suggestions / prompt / notification / status 等底部高度
  -> 计算 max_content_height
  -> build_message_render_context_with_options
  -> vscroll.ensure_up_to_date(width)
  -> content_height = min(total_visual_lines, max_content_height)
  -> Layout::vertical([...])
  -> 如有滚动条，缩小 message_body_area 后重新 ensure_up_to_date
  -> clamp scroll_offset
  -> render_messages
```

`VirtualScroll::ensure_up_to_date` 内部会：

- 宽度变化时全量失效。
- `render_context.cache_key()` 变化时全量失效。
- 消息减少时从新长度处失效。
- 对每条可渲染消息调用 `render_renderable_message_for_layout` 计算未 wrap 的行。
- 用 `wrapped_line_height` 基于 `unicode_width` 估算视觉换行高度。
- 为逻辑高度和视觉高度分别维护 prefix sum。

### 可见行为

- 终端宽度变化后，长行换行高度会重新计算。
- 大量历史消息不会每帧全量绘制，只渲染可见范围和 overscan。
- 滚动条出现后正文宽度变窄，会触发一次重新计算，避免高度和实际换行不一致。
- 选中/展开状态变化会改变 cache key，从而重新计算高度。

### 调试入口

```bash
rg -n "ensure_up_to_date|total_visual_lines|visual_range|visual_offset_of|wrapped_line_height" crates/allthecodes/src/ui/rendering/virtual_scroll.rs crates/allthecodes/src/ui/app/render.rs
```

常见问题定位：

| 现象 | 优先检查 |
|---|---|
| 滚动到底部后仍有空白 | `total_visual_lines`、`max_scroll`、`message_area.height` |
| 终端 resize 后错位 | `cached_width` 是否更新，滚动条缩窄后是否二次 `ensure_up_to_date` |
| 选中展开后内容被截断 | `render_context.cache_key()` 是否包含相关状态 |
| 某类消息高度异常 | `render_renderable_message_for_layout` 输出行数和 `wrapped_line_height` |

---

## 5. 消息分发层

消息分发定义在 `crates/allthecodes/src/ui/messages/render/mod.rs`。

### 输入

`render_messages` 接收：

```rust
render_messages(
    &self.messages,
    message_body_area,
    frame.buffer_mut(),
    &self.theme,
    self.is_streaming,
    scroll,
    &self.vscroll,
    &message_render_context,
)
```

实际渲染使用的是 `render_context.renderable_messages()`，`_messages` 参数保留 API 形态但不参与当前绘制逻辑。

### 输出

`render_messages` 不返回值，直接写 `Buffer`。

内部中间输出是：

- 每条消息的 `Vec<Line>`
- 流式 Assistant 最后一行追加的 `Span(" ▌")`
- 用户消息背景样式矩形

### 关键函数

| 函数 | 作用 |
|---|---|
| `render_messages` | 只绘制 viewport 中可见消息 |
| `render_renderable_message_wrapped` | 渲染并按终端宽度换行 |
| `render_renderable_message_for_layout` | 布局测量用，不做最终 viewport 裁剪 |
| `render_renderable_message_with_context` | 按 `RenderableMessage` 变体分发 |
| `render_single_message_with_context` | 按 `Message` 变体分发 |
| `renderable_message_uses_user_background` | 判断是否填充用户消息背景 |
| `decorate_selected_message` | 插入 selected header 和展开详情 |

### 分发矩阵

| 输入形态 | 目标渲染器 |
|---|---|
| `RenderableMessage::Message(Message::User)` | `render_user_message` |
| `RenderableMessage::Message(Message::Assistant)` | `render_assistant_message` |
| `RenderableMessage::Message(Message::System)` | `render_system_message` |
| `RenderableMessage::Message(Message::Progress)` | `render_progress_message` |
| `RenderableMessage::Message(Message::Attachment)` | `render_attachment_message` |
| `RenderableMessage::GroupedToolUse` | `render_grouped_tool_use_lines` |
| `RenderableMessage::CollapsedReadSearch` | `render_collapsed_read_search_lines` |

### 可见行为

- 每条消息之间自动插入一个空行分隔符。
- 正在 streaming 的最后一条 Assistant 消息末尾出现 `▌`。
- 用户普通文本消息会整行填充深色背景；工具结果、图片、bash stdout/stderr 等特殊用户消息不使用该背景。
- 选中消息前会插入 `▶ selected` 或 `▼ selected`，展开时追加 uuid、time、ref、copy 预览。

### 调试入口

```bash
rg -n "render_messages|render_renderable_message_for_layout|render_single_message_with_context|decorate_selected_message" crates/allthecodes/src/ui/messages/render/mod.rs
```

如果 Buffer 上样式正确但高度不对，检查 `render_renderable_message_for_layout`。如果高度正确但屏幕缺行，检查 `render_messages` 的 `skip`、`visual_offset_of` 和 `total_for_msg`。

---

## 6. 类型专用渲染器层

类型专用渲染器把语义数据转成 ratatui `Line` / `Span`。

### 6.1 Assistant 渲染器

位置：`crates/allthecodes/src/ui/messages/render/render_assistant.rs`

关键函数：

- `render_assistant_message`
- `tool_state_for_id`
- `render_system_message`
- `render_progress_message`
- `render_attachment_message`

Assistant 内容块分发：

| `ContentBlock` | 渲染路径 | 可见行为 |
|---|---|---|
| `Text` | `markdown_to_lines` | Markdown 富文本；API error message 改走错误文本 |
| `ConnectorText` | `markdown_to_lines` | 连接器文本按 Markdown 显示 |
| `ToolUse` | `render_assistant_tool_use_message` | 显示工具活动行，状态来自 lookup |
| `ServerToolUse` | `render_assistant_tool_use_message` | 同工具调用，但行内加 `server: ` 前缀 |
| `ToolResult` | 内联结果摘要 | 显示最多 5 行，超出显示 `... N more lines` |
| `Thinking` | `render_assistant_thinking_lines` | 普通模式可能隐藏/压缩；verbose/transcript 显示更多 |
| `RedactedThinking` | `render_assistant_redacted_thinking_lines` | 仅 verbose/transcript 显示 |
| `Image` | `image_reference` | 显示图片引用摘要 |

工具状态计算：

| lookup 状态 | `ToolUseState` |
|---|---|
| `errored_tool_use_ids` 包含 id | `Error` |
| `resolved_tool_use_ids` 包含 id | `Resolved` |
| 其他 | `InProgress` |

### 6.2 User 渲染器

位置：`crates/allthecodes/src/ui/messages/render/render_user.rs`

关键函数：

- `render_user_message`
- `render_tool_result_user_message`
- `render_tagged_user_text`
- `render_file_edit_preview`
- `tool_result_content_text`
- `styled_text_lines`

User 分支：

| 输入 | 渲染路径 | 可见行为 |
|---|---|---|
| `MessageContent::Text` 普通文本 | `render_user_text_message` | 深色背景用户输入 |
| `[Request interrupted by user]` | 直接 warning span | 显示 `Interrupted by user` |
| `<bash-stdout>` / `<bash-stderr>` | `render_user_bash_output_message_with_options` | shell 输出摘要，可默认展开最近输出 |
| `ContentBlock::ToolResult` 且工具是 Bash/PowerShell | shell 输出渲染器 | 命令输出按宽度和展开状态显示 |
| `ToolResult` 且 `tool_use_result.kind=file_edit` | `render_file_edit_tool_updated_message` | 文件 diff 预览 |
| 其他 `ToolResult` | `render_user_tool_result_message` | 按工具名、输入和结果生成通用工具结果视图 |
| `Image` blocks | `image_reference` | 图片引用摘要 |

### 6.3 系统 / 进度 / 附件渲染

位置：`render_assistant.rs`

系统消息：

| `SystemSubtype` | 可见行为 |
|---|---|
| `CompactBoundary` | `render_compact_boundary_lines`，显示 compact 提示 |
| `MicrocompactBoundary` | 返回空 `Vec`，完全隐藏 |
| `ApiError` | `render_system_text_message("api_error", ...)` 后按 error 样式输出 |
| `Informational(Info)` | `Info: ...` |
| `Informational(Warning)` | `Warning: ...` |
| `Informational(Error)` | `Error: ...` |
| `LocalCommand` | `$ ...` |
| `Warning` | `Warning: ...` |

进度消息：

- `render_progress_message` 显示 `  ... {message}`。
- 当前主消息列表默认过滤 `Progress`，但 lookup 会保留进度关系。

附件消息：

- `render_attachment_message` 为非空渲染附件生成 dim 文本。
- 空渲染附件在预处理层已过滤。

### 6.4 聚合渲染器

位置：

- `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs`
- `crates/allthecodes/src/ui/messages/collapsed_read_search_content.rs`

可见行为：

- `GroupedToolUseView` 显示 `N Tool calls`，根据 resolved/error 数量补充 `completed`、`failed` 或 `x/y completed`。
- `CollapsedReadSearchView` 显示 Read/Search/List 计数，进行中时使用现在分词，并可追加 latest hint。

### 调试入口

```bash
rg -n "render_assistant_message|render_user_message|render_system_message|render_grouped_tool_use_lines|render_collapsed_read_search_lines" crates/allthecodes/src/ui/messages
```

如果某类消息的“是否显示”有问题，先查预处理层；如果显示了但文案/样式不对，再查本层。

---

## 7. Markdown 与语法高亮层

Markdown 与代码高亮位于：

- `crates/allthecodes/src/ui/rendering/markdown.rs`
- `crates/allthecodes/src/ui/rendering/syntax_highlight.rs`

`ui/mod.rs` 通过路径映射暴露为：

```rust
#[path = "rendering/markdown.rs"]
pub mod markdown;
#[path = "rendering/syntax_highlight.rs"]
pub mod syntax_highlight;
```

### Markdown 输入 / 输出

| 输入 | 输出 |
|---|---|
| `&str` Markdown 文本 + `Theme` | `Vec<Line<'static>>` |

公开入口：

- `markdown_to_lines(text, theme)`

缓存：

- thread-local `LruCache<u64, Vec<Line<'static>>>`
- 容量 256
- key 由文本和 `theme_fingerprint(theme)` 组成

### 支持的 Markdown 能力

| 语法 | 可见行为 |
|---|---|
| Heading | H1 粗体+下划线；H2 粗体；其他使用 bold |
| Paragraph | 段落结束后空行，最终会移除尾部空白行 |
| Strong | `theme.bold` |
| Emphasis | `theme.italic` |
| Inline code | 以反引号包围并使用 `theme.code` |
| Fenced / indented code block | 缓冲整段代码，交给语法高亮 |
| Unordered list | `  - ` 前缀 |
| Ordered list | `  N. ` 前缀 |
| Link | `theme.link` |
| Block quote | `  | ` 前缀，dim 样式 |
| Horizontal rule | 固定横线，dim 样式 |
| Table | 使用 box drawing 字符画表格边框，并计算列宽 |
| Soft break | 转为空格 |
| Hard break | flush 当前行 |

### 语法高亮输入 / 输出

公开入口：

- `highlight_code_block(code, lang, theme)`

| 输入 | 输出 |
|---|---|
| 代码字符串、语言标识、主题 | `Vec<Span<'static>>`，换行用 raw `"\n"` span 表示 |

语言识别：

- `normalize_lang_token` 清理 fenced code info string。
- `resolve_lang` 将别名映射到 canonical token。
- `preferred_syntect_token` 优先返回支持 syntect 的 token，否则保留规范化 token。

常见别名：

| canonical | aliases |
|---|---|
| `rust` | `rs` |
| `python` | `py`, `python3` |
| `bash` | `shell`, `sh`, `bash`, `zsh` |
| `typescript` | `ts` |
| `javascript` | `js`, `node` |
| `powershell` | `ps1` |
| `yaml` | `yml` |
| `markdown` | `md` |

高亮策略：

| 条件 | 行为 |
|---|---|
| code 为空 | 返回一个 `theme.code` 空 span |
| lang 为空 | 直接 fallback 到 `theme.code` |
| 未启用 `syntect` feature | fallback 到 `theme.code` |
| syntect 找不到语法 | fallback 到 `theme.code` |
| syntect 单行失败 | 当前行 fallback 到 `theme.code`，后续继续 |
| syntect 成功 | 将 syntect foreground/bold/italic/underline 转成 ratatui `Style` |

### 可见行为

- Assistant 文本和 connector text 默认具备 Markdown 样式。
- 代码块在支持 syntect 时有 token 级颜色；否则仍保留代码样式，不影响显示。
- Markdown 表格会被转换为终端 box drawing 表格，而不是原始 pipe 表格。
- 主题变化会改变 Markdown 缓存 key，避免旧颜色复用。

### 调试入口

```bash
rg -n "markdown_to_lines|highlight_code_block|LANG_ALIASES|render_table|theme_fingerprint" crates/allthecodes/src/ui/rendering
```

常见问题定位：

| 现象 | 优先检查 |
|---|---|
| Markdown 文本没有样式 | `render_assistant_message` 是否走到 `markdown_to_lines` |
| 代码块无颜色 | `syntect` feature、语言 token、`highlight_code_block` fallback |
| 表格列宽错 | `compute_column_widths` 和 Unicode 宽度计算 |
| 主题切换后颜色不刷新 | `theme_fingerprint` 是否包含对应样式字段 |

---

## 8. Buffer 渲染层

Buffer 输出发生在 `render_messages`。

### 输入

| 输入 | 说明 |
|---|---|
| `area: Rect` | 消息正文区域 |
| `buf: &mut Buffer` | ratatui 帧缓冲 |
| `theme: &Theme` | 样式来源 |
| `streaming: bool` | 是否给最后 Assistant 行追加光标 |
| `scroll: usize` | 当前视觉滚动偏移 |
| `vscroll: &VirtualScroll` | 可见范围和每条消息视觉 offset |
| `render_context: &MessageRenderContext` | 可渲染消息与 lookup |

### 输出

直接写入 `buf`：

- `buf.set_style(...)` 填充用户消息背景。
- `buf.set_line(...)` 写入每一行 `Line`。
- 空分隔行通过 `Line::default()` 写入。

### 渲染过程

```text
render_messages
  -> render_context.renderable_messages()
  -> viewport_h = area.height
  -> (start, end) = vscroll.visual_range(scroll, viewport_h)
  -> for idx in start..end
       -> render_renderable_message_wrapped
       -> 如果 streaming 且最后 Assistant：最后一行追加 " ▌"
       -> 计算该消息含 separator 的 total_for_msg
       -> message_offset = vscroll.visual_offset_of(idx)
       -> skip = scroll - message_offset
       -> for visible line
            -> 用户背景：buf.set_style(row rect, USER_MESSAGE_BACKGROUND)
            -> buf.set_line(x, y, line, width)
```

### 可见行为

- 只写当前 viewport 需要显示的行。
- 滚动到消息中间时，`skip` 会跳过该消息顶部已经滚出屏幕的行。
- 用户消息背景是按整行矩形填充，不只覆盖文字 span。
- 消息之间的空行也计入滚动高度。
- streaming cursor 只出现在最后一条 Assistant 消息。

### 调试入口

```bash
rg -n "buf.set_style|buf.set_line|visual_range|visual_offset_of|streaming" crates/allthecodes/src/ui/messages/render/mod.rs
```

常见问题定位：

| 现象 | 优先检查 |
|---|---|
| 样式存在但没铺满整行 | `renderable_message_uses_user_background` 和 `buf.set_style` 区域 |
| 最后一行 cursor 不显示 | `streaming`、最后 renderable 是否 Assistant、`msg_lines.last_mut()` |
| 滚动时某条消息上半截重复 | `skip` 和 `message_offset` |
| 屏幕底部残留旧内容 | 调用方是否清理 Frame；`render_messages` 只负责写入消息区域内可见行 |

---

## 9. 端到端数据流示例

### 9.1 Assistant Markdown 文本

```text
Message::Assistant(ContentBlock::Text)
  -> normalize 拆成单 block RenderableMessage
  -> 过滤 / compact / 重排后保留
  -> VirtualScroll 用 render_renderable_message_for_layout 计算高度
  -> render_single_message_with_context
  -> render_assistant_message
  -> markdown_to_lines
  -> wrap_line_to_width
  -> render_messages 写入 Buffer
```

可见结果：标题、列表、表格、代码块等 Markdown 样式在消息区显示；流式最后一条会追加 `▌`。

### 9.2 Bash 工具调用和结果

```text
Assistant ToolUse(Bash)
  -> lookup.tool_uses[id] = { tool_name, input }
User ToolResult(tool_use_id=id)
  -> lookup.resolved_tool_use_ids / errored_tool_use_ids
  -> reorder_messages_in_ui 把结果排在调用后
  -> Assistant 工具调用行通过 tool_state_for_id 显示 Running/Ran/Failed
  -> User 工具结果通过 render_user_bash_output_message_with_options 显示输出
```

可见结果：工具调用行显示 `● Running` / `● Ran` / `● Failed [error]`，shell 输出可按最近结果或选中展开。

### 9.3 多个 Read/Grep/Glob

```text
多个 Assistant ToolUse(Read/Grep/Glob)
  -> apply_grouping 先按同源同类工具聚合
  -> collapse_read_search_groups 再把连续 Read/Search/List 类操作折叠
  -> render_collapsed_read_search_lines 输出摘要
```

可见结果：普通模式下显示 `Searched for ... Read ... Ctrl+O to expand`；verbose 模式逐条显示。

### 9.4 文件编辑结果

```text
User ToolResult + tool_use_result JSON(kind=file_edit)
  -> render_tool_result_user_message
  -> render_file_edit_preview
  -> render_file_edit_tool_updated_message
  -> diff 预览行转成 tool_name/tool_result 样式
```

可见结果：文件路径、增删统计和 hunk 行显示在工具结果位置。

---

## 10. 调试路线图

按“越靠前越可能改变是否显示，越靠后越可能改变怎么显示”的顺序查：

| 问题 | 第一站 | 第二站 | 第三站 |
|---|---|---|---|
| 消息完全不出现 | `prepare_renderable_messages` | `should_show_renderable_message` | `apply_grouping` / `collapse_read_search_groups` |
| 工具状态不对 | `build_message_lookups` | `tool_state_for_id` | `render_assistant_tool_use_message` |
| 工具结果和调用没挨着 | `reorder_messages_in_ui` | `tool_use_id` / `tool_result_id` | 原始 `source_index` |
| Markdown 样式不对 | `render_assistant_message` | `markdown_to_lines` | `highlight_code_block` |
| 高度/滚动不对 | `VirtualScroll::ensure_up_to_date` | `render_context.cache_key` | `wrapped_line_height` |
| 选中态不显示 | `has_source_index` | `decorate_selected_message` | `selected_message` 来源 |
| 用户背景错误 | `user_message_uses_background` | `renderable_message_uses_user_background` | `buf.set_style` |
| transcript 与普通视图不同 | `MessageRenderOptions` | `filter_compact_boundary` | `truncate_transcript_messages` |

最小定位命令：

```bash
rg -n "build_message_render_context_with_options|prepare_renderable_messages|render_messages|render_single_message_with_context|markdown_to_lines|ensure_up_to_date" crates/allthecodes/src/ui
```

---

## 11. 维护注意事项

- 新增消息类型时，至少检查 `render_single_message_with_context`、复制文本路径和 primary reference 路径。
- 新增 `ContentBlock` 时，至少检查 normalize、lookup、Assistant/User 分支和 copy text。
- 新增工具时，先决定是否参与同类聚合、是否参与 Read/Search/List 折叠、是否需要专用工具调用渲染、是否需要专用工具结果渲染。
- 改变可见文本高度的逻辑时，确认 `render_context_cache_key` 能让 `VirtualScroll` 失效。
- 改变 Markdown 样式字段时，确认 `theme_fingerprint` 包含该字段。
- 修改用户消息背景规则时，同时检查 `user_message_uses_background` 和实际 User 渲染分支，避免工具结果被误铺背景。
