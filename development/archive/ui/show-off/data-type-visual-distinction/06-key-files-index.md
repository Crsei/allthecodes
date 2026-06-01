# 第六章：关键技术文件索引

本文是 `data-type-visual-distinction.md` 第六章的展开索引，用于快速定位 TUI 中“不同数据类型如何被区分、聚合、着色并显示”的实现位置。

标记说明：

- 【渲染核心】：直接参与消息到 `ratatui::Line` / `Buffer` 的转换，或决定布局、滚动、主题样式。
- 【组件/Overlay】：可复用控件、命令面板、弹层、权限对话框等 UI 基础设施。
- 【功能模块 UI】：Agent、MCP、Memory、Skills、Tasks 等业务域界面。
- 【数据/运行时】：数据模型、事件桥接、状态同步、输入输出适配，不直接负责最终视觉样式但会影响渲染内容。

## 1. 总入口与运行时骨架

### `crates/allthecodes/Cargo.toml`

- 类型：数据/运行时。
- 职责：声明 TUI 所需依赖，包括 `ratatui`、`crossterm`、`pulldown-cmark`、`syntect` 等；判断渲染能力、终端事件、Markdown 解析、语法高亮是否可用时先查这里。
- 典型关键词：`ratatui`、`crossterm`、`pulldown-cmark`、`syntect`、`features`。
- 关联：被 `crates/allthecodes/src/ui/*` 依赖；Markdown 和高亮能力最终落到 `rendering/markdown.rs` 与 `rendering/syntax_highlight.rs`。

### `crates/allthecodes/src/ui/mod.rs`

- 类型：数据/运行时。
- 职责：Rust TUI 模块总门面，声明 `agents`、`app`、`messages`、`theme`、`overlays` 等模块，并用 `#[path = "..."]` 保持兼容导出路径。
- 典型关键词：`pub mod messages`、`#[path = "components/`、`#[path = "rendering/`、`pub mod theme`。
- 关联：许多源文件实际位于 `components/`、`rendering/`、`runtime/` 等目录，但通过这里导出为 `crate::ui::<name>`。查文件路径时看 `#[path]`，查调用路径时看 `pub mod` 名称。

### `crates/allthecodes/src/ui/tui.rs`

- 类型：数据/运行时。
- 职责：TUI runner，连接 `App`、终端事件、QueryEngine、后台消息、权限回调和通知系统；负责 raw mode、alternate screen、输入事件线程和 async engine 通道。
- 典型关键词：`run_tui`、`TerminalGuard`、`EngineEvent`、`handle_sdk_message`、`spawn_engine_query`、`AppAction`。
- 关联：调用 `App` 处理用户动作；将 SDK / backend 事件转成 `Message` 后进入 `app.rs` 状态，再由 `app/render.rs` 渲染。

### `crates/allthecodes/src/ui/app.rs`

- 类型：数据/运行时。
- 职责：主 UI 状态容器，保存消息列表、选中消息、滚动、输入框、权限对话框、命令面板、通知、主题、Agent 页脚等状态；定义 `AppAction` 作为 UI 对外动作。
- 典型关键词：`pub struct App`、`messages: Vec<Message>`、`selected_message`、`is_streaming`、`PermissionDialog`、`CommandSurface`。
- 关联：状态由 `tui.rs` 推动；渲染入口在子模块 `app/render.rs`；输入处理在 `app/input.rs`；状态行在 `app/status.rs`。

### `crates/allthecodes/src/ui/app/render.rs`

- 类型：渲染核心。
- 职责：顶层布局和帧渲染入口 `App::render()`。计算消息区、底部面板、输入框、补全、命令面板、通知、Agent 页脚、状态行等区域；调用 `render_messages()` 绘制会话历史。
- 典型关键词：`App::render`、`BottomPaneHeights`、`render_messages`、`VirtualScroll`、`workspace_trust_pending`、`view_mode`、`split_session_scrollbar_area`。
- 关联：上游接 `app.rs` 状态，下游接 `messages/render/mod.rs`、`virtual_scroll.rs`、`prompt_input.rs`、`command_palette/`、`command_surface/`、`notifications/`、`overlays/`。

### `crates/allthecodes/src/ui/app/input.rs`

- 类型：数据/运行时。
- 职责：键盘和鼠标输入到 `AppAction` 的转换层；决定滚动、展开选中消息、提交 prompt、打开命令面板或处理 overlay。
- 典型关键词：`handle_key`、`handle_mouse`、`AppAction`、`selected_message_expanded`、`scroll_offset`。
- 关联：输入结果改变 `app.rs` 状态，下一帧由 `app/render.rs` 和消息渲染管线体现出来。

### `crates/allthecodes/src/ui/app/transcript_mode.rs`

- 类型：渲染核心。
- 职责：Transcript / Focus 类视图的替代渲染路径，不走普通 prompt-mode chrome；排查“历史导出、复制、Transcript 模式下为什么隐藏部分消息”时看这里。
- 典型关键词：`render_transcript`、`TranscriptState`、`ViewMode`、`show_all_in_transcript`。
- 关联：与 `runtime/transcript.rs`、`messages/render/context.rs` 的 `MessageRenderOptions` 共同决定 transcript 模式下的可见消息。

## 2. 数据模型与渲染输入

### `crates/allthecodes-types/src/message.rs`

- 类型：数据/运行时。
- 职责：定义进入 TUI 的核心消息模型，包括 `Message`、`ContentBlock`、`UserMessage`、`AssistantMessage`、`SystemSubtype`、`ProgressMessage`、`AttachmentMessage`。所有“数据类型可视化区分”都从这些枚举分支开始。
- 典型关键词：`enum ContentBlock`、`ToolUse`、`ToolResult`、`Thinking`、`SystemSubtype`、`Attachment`、`ProgressMessage`。
- 关联：被 `messages/render/context.rs` 预处理，被 `render_assistant.rs` / `render_user.rs` 分发；工具状态还会由 `ToolResult`、`ProgressMessage` 和 `ToolUse` ID 共同推导。

### `crates/allthecodes-types/src/status_line.rs`

- 类型：数据/运行时。
- 职责：状态行 payload 与子类型定义；影响底部状态区域而不是消息正文。
- 典型关键词：`StatusLinePayload`、`StatusLine`、`usage`、`model`。
- 关联：由 `ui/status/status_line_resolver.rs` 和 engine status-line runner 消费，最终在 `app/render.rs` 的底部布局中显示。

### `crates/allthecodes-types/src/lib.rs`

- 类型：数据/运行时。
- 职责：类型 crate 模块声明入口；当某个消息、agent、callback 或 status 类型路径不清楚时从这里顺藤摸瓜。
- 典型关键词：`pub mod message`、`pub mod callbacks`、`pub mod agent_events`。
- 关联：UI、engine、IPC 都会引用该 crate；消息渲染相关优先跳到 `message.rs`。

## 3. 消息渲染管线

### `crates/allthecodes/src/ui/messages.rs`

- 类型：渲染核心。
- 职责：消息 UI 模块门面，集中声明所有消息专用渲染器，并 re-export `render_messages`、`MessageRenderContext`、`MessageRenderOptions` 等入口。
- 典型关键词：`render_messages`、`build_message_render_context_with_options`、`assistant_tool_use_message`、`user_tool_result_message`。
- 关联：`app/render.rs` 通过这里调用消息渲染；具体逻辑分散在 `messages/render/*` 与 `messages/*_message.rs`。

### `crates/allthecodes/src/ui/messages/render/mod.rs`

- 类型：渲染核心。
- 职责：消息列表渲染入口。使用 `VirtualScroll` 决定可见范围，给用户消息铺背景，追加流式输出光标，并把 `RenderableMessage` 分发到单条消息、工具聚合、搜索折叠渲染器。
- 典型关键词：`render_messages`、`render_renderable_message_for_layout`、`USER_MESSAGE_BACKGROUND`、`GroupedToolUse`、`CollapsedReadSearch`、` ▌`。
- 关联：上游由 `app/render.rs` 调用；下游使用 `render_assistant.rs`、`render_user.rs`、`grouped_tool_use_content.rs`、`collapsed_read_search_content.rs`；高度计算由 `virtual_scroll.rs` 复用同一渲染入口。

### `crates/allthecodes/src/ui/messages/render/context.rs`

- 类型：渲染核心。
- 职责：构建 `MessageRenderContext`。执行规范化、过滤、重排、transcript 裁剪、工具聚合、搜索折叠，并建立 `tool_use_id` 到工具调用、结果、错误、进度的查找表。
- 典型关键词：`prepare_renderable_messages`、`MessageLookups`、`RenderableMessage`、`tool_uses`、`resolved_tool_use_ids`、`errored_tool_use_ids`、`in_progress_tool_use_ids`。
- 关联：调用 `preprocessing.rs` 和 `grouping.rs`；输出被 `render/mod.rs`、`render_assistant.rs`、`render_user.rs` 用于判断工具状态、shell 展开、选中消息、transcript 模式。

### `crates/allthecodes/src/ui/messages/render/preprocessing.rs`

- 类型：渲染核心。
- 职责：消息进入 UI 前的基础整理：拆分多 content block、过滤 microcompact、隐藏空消息、重排 UI 显示顺序、transcript 模式下过滤 brief 消息和限制历史长度。
- 典型关键词：`normalize_messages_for_render`、`filter_compact_boundary`、`should_show_renderable_message`、`reorder_messages_in_ui`、`filter_brief_messages`、`truncate_transcript_messages`、`find_last_thinking_block_id`。
- 关联：被 `context.rs` 串联调用；如果某条原始消息没显示，优先查这里和 `MessageRenderOptions`。

### `crates/allthecodes/src/ui/messages/render/grouping.rs`

- 类型：渲染核心。
- 职责：同类工具聚合和 Read/Search/List 折叠。非 verbose 模式下，把连续/同源工具调用转换为 `GroupedToolUse` 或 `CollapsedReadSearch`。
- 典型关键词：`apply_grouping`、`collapse_read_search_groups`、`collapsible_kind_for_tool`、`Read`、`Grep`、`Glob`、`WebSearch`、`LS`、`List`。
- 关联：`context.rs` 调用它生成虚拟渲染记录；最终显示由 `grouped_tool_use_content.rs` 与 `collapsed_read_search_content.rs` 负责。若要判断“哪些工具支持搜索/读取折叠”，看 `collapsible_kind_for_tool`。

### `crates/allthecodes/src/ui/messages/render/render_assistant.rs`

- 类型：渲染核心。
- 职责：Assistant、System、Progress、Attachment 消息分发。Assistant 内部按 `ContentBlock` 区分 Text、ConnectorText、ToolUse、ServerToolUse、ToolResult、Thinking、RedactedThinking、Image；同时处理 API error 和 cost 显示。
- 典型关键词：`render_assistant_message`、`ContentBlock::Text`、`ServerToolUse`、`tool_state_for_id`、`render_system_message`、`render_progress_message`、`render_attachment_message`。
- 关联：文本走 `markdown_to_lines`；工具调用走 `assistant_tool_use_message.rs`；思维块走 `assistant_thinking_message.rs`；系统提示走 `system_text_message.rs` / `system_api_error_message.rs` / `compact_boundary_message.rs`。

### `crates/allthecodes/src/ui/messages/render/render_user.rs`

- 类型：渲染核心。
- 职责：User 消息分发。处理普通 prompt 背景、`<bash-stdout>` / `<bash-stderr>` 标签、shell 工具结果、文件编辑预览、图片块、非 shell 工具结果、用户中断提示。
- 典型关键词：`render_user_message`、`render_tool_result_user_message`、`render_tagged_user_text`、`USER_MESSAGE_BACKGROUND`、`render_file_edit_preview`、`render_user_tool_result_message`。
- 关联：shell 输出走 `user_bash_output_message.rs`；文件编辑 diff 走 `file_edit_tool_updated_message.rs`；工具结果走 `messages/user_tool_result_message/`。

### `crates/allthecodes/src/ui/messages/render/copy_text.rs`

- 类型：渲染核心。
- 职责：为消息复制、主引用、折叠摘要提供纯文本提取逻辑；不直接画 UI，但会影响复制内容和某些摘要文本。
- 典型关键词：`message_copy_text`、`content_block_copy_text`、`message_primary_reference`、`tool_primary_input`、`compact_boundary_summary`。
- 关联：`context.rs`、`render_user.rs`、`grouping.rs` 都会借用这里的内容提取；查“显示和复制文本不一致”时需要同时看渲染器和这里。

### `crates/allthecodes/src/ui/messages/wrap.rs`

- 类型：渲染核心。
- 职责：把 `Line` 包装到终端宽度，保持 span 样式并避免超宽内容破坏布局。
- 典型关键词：`wrap_line_to_width`、`unicode_width`、`Span`。
- 关联：`render/mod.rs` 在布局前调用；如果样式正确但终端换行异常，查这里和上游传入的 `width`。

## 4. 消息类型专用渲染器

### `crates/allthecodes/src/ui/messages/assistant_tool_use_message.rs`

- 类型：渲染核心。
- 职责：Assistant 工具调用的状态机和可视格式。区分 shell、文件编辑、TodoWrite、默认工具，支持 queued、in-progress、resolved、error、waiting-for-permission、classifier-checking。
- 典型关键词：`ToolUseState`、`TRANSPARENT_WRAPPER_TOOLS`、`render_shell_tool_use_message`、`render_file_edit_tool_use_message`、`render_todo_write_tool_use_message`、`TodoWrite`、`PowerShell`。
- 关联：由 `render_assistant.rs` 调用；结构化状态样式依赖 `rendering/tool_activity.rs`；工具结果显示在 `render_user.rs` 和 `user_tool_result_message/`。

### `crates/allthecodes/src/ui/messages/assistant_text_message.rs`

- 类型：渲染核心。
- 职责：Assistant 文本分类、API 错误文本渲染辅助；用于区分普通 markdown 与错误样式。
- 典型关键词：`classify_assistant_text`、`render_api_error`、`api_error`。
- 关联：`render_assistant.rs` 遇到 `is_api_error_message` 时会走错误路径；用户工具结果也会复用其中的错误展示辅助。

### `crates/allthecodes/src/ui/messages/assistant_thinking_message.rs`

- 类型：渲染核心。
- 职责：Thinking block 的折叠/展开、verbose/transcript 模式差异与思维样式。
- 典型关键词：`render_assistant_thinking_lines`、`AssistantThinkingView`、`verbose`、`is_transcript_mode`、`∴`。
- 关联：`render_assistant.rs` 根据 `MessageRenderOptions` 决定是否显示完整思维；高亮片段另见 `highlighted_thinking_text.rs`。

### `crates/allthecodes/src/ui/messages/assistant_redacted_thinking_message.rs`

- 类型：渲染核心。
- 职责：被截断或 redacted thinking 的占位显示。
- 典型关键词：`render_assistant_redacted_thinking_lines`、`redacted`。
- 关联：只在 verbose 或 transcript 模式下由 `render_assistant.rs` 显示。

### `crates/allthecodes/src/ui/messages/user_text_message.rs`

- 类型：渲染核心。
- 职责：用户文本路由。识别普通输入、隐藏消息、中断消息、命令/记忆/MCP 等带标签文本。
- 典型关键词：`route_user_text`、`render_user_text_message`、`CONVERSATION_INTERRUPTED_MESSAGE`、`Hidden`。
- 关联：由 `render_user.rs` 调用；影响用户消息是否铺背景，以及某些 meta 文本是否完全隐藏。

### `crates/allthecodes/src/ui/messages/user_bash_output_message.rs`

- 类型：渲染核心。
- 职责：shell 输出格式化、折叠、展开、截断和行数/字节数提示。
- 典型关键词：`render_user_bash_output_message_with_options`、`ShellOutputRenderOptions`、`expanded`、`total_lines`、`total_bytes`。
- 关联：`render_user.rs` 处理 shell `ToolResult` 或 `<bash-stdout>` 标签时调用；是否展开由 `MessageRenderContext::shell_expanded` 决定。

### `crates/allthecodes/src/ui/messages/file_edit_tool_updated_message.rs`

- 类型：渲染核心。
- 职责：文件编辑结果的 diff 预览、取消、拒绝等展示。
- 典型关键词：`render_file_edit_tool_updated_message`、`FileEditToolUpdatedView`、`FileEditMessageStyle`、`hunk_lines`、`render_file_edit_tool_rejected_message`。
- 关联：`render_user.rs` 从工具结果 JSON 中识别 `kind=file_edit` 后调用；diff 行样式依赖 `theme.diff_add` / `theme.diff_remove`。

### `crates/allthecodes/src/ui/messages/system_api_error_message.rs`

- 类型：渲染核心。
- 职责：系统级 API 错误、重试次数、等待时间等信息的可视化。
- 典型关键词：`render_system_api_error_message`、`retry_attempt`、`max_retries`、`retry_in_ms`。
- 关联：由 `render_assistant.rs` 的 `render_system_message` 路径调用；消息数据来自 `SystemSubtype::ApiError`。

### `crates/allthecodes/src/ui/messages/system_text_message.rs`

- 类型：渲染核心。
- 职责：系统 info/warning/error/local command 等普通系统文本展示。
- 典型关键词：`render_system_text_message`、`Info`、`Warning`、`Error`、`LocalCommand`。
- 关联：由 `render_assistant.rs` 分发；样式来自 `Theme` 的 `info`、`warning`、`error`、`system_name`。

### `crates/allthecodes/src/ui/messages/attachment_message.rs`

- 类型：渲染核心。
- 职责：附件消息的统一展示，包括编辑文件、排队命令、结构化输出、hook 停止、nested memory、skill discovery 等。
- 典型关键词：`render_attachment_message`、`Attachment`、`QueuedCommand`、`StructuredOutput`、`SkillDiscovery`。
- 关联：由 `render_assistant.rs` 的 attachment 分发路径调用；具体附件类型定义在 `allthecodes-types/src/message.rs`。

### `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs`

- 类型：渲染核心。
- 职责：同类工具聚合后的单行/多行渲染，例如多次 `Read` 或多次 `Task` 调用的汇总。
- 典型关键词：`render_grouped_tool_use_lines`、`GroupedToolUseView`、`resolved_count`、`error_count`。
- 关联：聚合记录由 `messages/render/grouping.rs` 生成；状态计数来自 `MessageLookups`。

### `crates/allthecodes/src/ui/messages/collapsed_read_search_content.rs`

- 类型：渲染核心。
- 职责：Read/Grep/Glob/WebSearch/LS/List 等读取和搜索类工具的折叠摘要展示。
- 典型关键词：`render_collapsed_read_search_lines`、`CollapsedReadSearchView`、`read_count`、`search_count`、`list_count`、`latest_hint`。
- 关联：支持哪些工具由 `grouping.rs` 的 `collapsible_kind_for_tool` 决定；verbose 模式通常绕过折叠。

### `crates/allthecodes/src/ui/messages/compact_boundary_message.rs`

- 类型：渲染核心。
- 职责：上下文压缩边界提示的显示；microcompact 通常在预处理阶段隐藏。
- 典型关键词：`render_compact_boundary_lines`、`compact_boundary`、`pre_compact_token_count`、`post_compact_token_count`。
- 关联：`preprocessing.rs` 决定哪些 boundary 可见；copy 文本摘要在 `copy_text.rs`。

### `crates/allthecodes/src/ui/messages/user_tool_result_message/`

- 类型：渲染核心。
- 职责：非 shell 工具结果的成功、错误、拒绝、取消、计划拒绝等专用展示。
- 典型关键词：`user_tool_result_message.rs`、`user_tool_success_message.rs`、`user_tool_error_message.rs`、`user_tool_reject_message.rs`、`rejected_tool_use_message.rs`、`user_tool_canceled_message.rs`、`utils.rs`。
- 关联：由 `render_user.rs` 在非 shell `ToolResult` 分支调用；需要工具调用元数据时从 `MessageRenderContext.lookups.tool_uses` 转成该目录自己的 lookup 类型。

## 5. 共享渲染工具与样式

### `crates/allthecodes/src/ui/rendering/theme.rs`

- 类型：渲染核心。
- 职责：消息渲染器实际使用的语义样式结构 `Theme`，把 assistant/user/system/tool/error/warning/info/thinking/diff/syntax 等语义角色映射为 `ratatui::Style`。
- 典型关键词：`pub struct Theme`、`assistant_name`、`tool_name`、`error`、`diff_add`、`syntax_keyword`、`selected`。
- 关联：被 `messages/*`、`rendering/markdown.rs`、`rendering/tool_activity.rs` 直接消费；设计系统主题在 `theme/mod.rs`，两者容易混淆。

### `crates/allthecodes/src/ui/theme/mod.rs`

- 类型：渲染核心。
- 职责：设计系统主题提供者，定义 `ThemeName`、`ThemeSetting`、`ThemeColors`、`ThemeProvider` 和六套内置主题。
- 典型关键词：`ThemeName`、`ThemeColors`、`ThemeProvider`、`DarkDaltonized`、`LightAnsi`、`resolve`。
- 关联：`app.rs` 持有 `design_theme_provider` 和渲染 `Theme`；颜色解析辅助在 `theme/color.rs`；消息渲染最终仍读 `rendering/theme.rs` 的 `Theme` 字段。

### `crates/allthecodes/src/ui/theme/color.rs`

- 类型：渲染核心。
- 职责：颜色解析和转换工具。
- 典型关键词：`Color`、`parse`、`rgb`、`ansi`。
- 关联：被 `theme/mod.rs` 使用；排查主题配置字符串或颜色 fallback 时看这里。

### `crates/allthecodes/src/ui/rendering/markdown.rs`

- 类型：渲染核心。
- 职责：Markdown 到 `Vec<Line>` 的转换，支持标题、粗体、斜体、行内代码、代码块、列表、链接、引用、表格、删除线，并带 LRU 缓存。
- 典型关键词：`markdown_to_lines`、`pulldown_cmark`、`ENABLE_TABLES`、`ENABLE_STRIKETHROUGH`、`MD_CACHE`、`highlight_code_block`。
- 关联：Assistant 文本和 ConnectorText 由 `render_assistant.rs` 调用这里；代码块高亮下沉到 `syntax_highlight.rs`。

### `crates/allthecodes/src/ui/rendering/syntax_highlight.rs`

- 类型：渲染核心。
- 职责：代码块 token 高亮，把 syntect scope 或 fallback 分类映射到 `Theme` 的 syntax 样式。
- 典型关键词：`highlight_code_block`、`syntect`、`syntax_keyword`、`syntax_string`、`syntax_comment`。
- 关联：只处理代码块内部 span；块级 Markdown 结构仍由 `markdown.rs` 控制。

### `crates/allthecodes/src/ui/rendering/virtual_scroll.rs`

- 类型：渲染核心。
- 职责：消息列表虚拟滚动和高度缓存，避免每帧渲染全量历史；计算可见消息范围和每条消息的 visual offset。
- 典型关键词：`VirtualScroll`、`ensure_up_to_date`、`total_visual_lines`、`visual_range`、`visual_offset_of`。
- 关联：`app/render.rs` 调用它计算内容高度和滚动条；`messages/render/mod.rs` 根据其范围实际绘制。

### `crates/allthecodes/src/ui/rendering/tool_activity.rs`

- 类型：渲染核心。
- 职责：结构化工具活动与 `ToolState` 样式化显示，包含 queued/running/succeeded/failed/cancelled 标签、耗时、进度、输出摘要。
- 典型关键词：`ToolState`、`ToolActivity`、`compact_styled_line`、`tool_label_and_args`、`format_elapsed`。
- 关联：`assistant_tool_use_message.rs` 可用它生成工具状态行；颜色来自 `Theme` 的 `dim`、`info`、`diff_add`、`error`、`warning`。

### `crates/allthecodes/src/ui/rendering/spinner.rs`

- 类型：渲染核心。
- 职责：spinner 状态和 Braille 点阵动画帧。
- 典型关键词：`SpinnerState`、`tick`、`frame`、`⡋`。
- 关联：`app.rs` 保存 spinner 状态；`app/render.rs` 在流式输出或后台执行时分配 spinner 高度并显示。

### `crates/allthecodes/src/ui/rendering/progress_bar.rs`

- 类型：渲染核心。
- 职责：进度条渲染辅助，多用于测试或进度类展示。
- 典型关键词：`progress`、`ratio`、`progress_fill`、`progress_empty`。
- 关联：`tool_activity.rs` 内部也有状态行进度展示；具体 UI 是否使用需看调用方。

### `crates/allthecodes/src/ui/rendering/markdown_render.rs` 与 `markdown_stream.rs`

- 类型：渲染核心。
- 职责：Markdown 渲染和流式 Markdown 的测试/辅助路径，部分导出带 `#[cfg(test)]`。
- 典型关键词：`markdown_render`、`markdown_stream`、`stream`、`snapshot`。
- 关联：生产路径主要看 `markdown.rs`；排查快照或流式渲染回归时再查这两个文件。

## 6. 布局、输入和可复用组件

### `crates/allthecodes/src/ui/panel_layout.rs`

- 类型：组件/Overlay。
- 职责：面板尺寸预设和布局辅助。
- 典型关键词：`PanelSizePreset`、`layout`、`height`、`width`。
- 关联：被 `app/render.rs` 和部分弹层/面板用于统一尺寸。

### `crates/allthecodes/src/ui/prompt_input.rs`

- 类型：组件/Overlay。
- 职责：输入框状态和渲染，包含 prompt 文本、多行输入、paste notice、光标和输入上下文。
- 典型关键词：`PromptInput`、`PromptInputRenderContext`、`large_paste_notice`、`render`。
- 关联：`app.rs` 持有状态；`app/render.rs` 把底部输入区域交给它；补全和路径提示由 `input/*` 模块提供。

### `crates/allthecodes/src/ui/components/bottom_pane.rs`

- 类型：组件/Overlay。
- 职责：底部面板高度聚合，统一计算 spinner、suggestions、paste notice、input、completion popup、command palette、notification、agent footer、status 的高度。
- 典型关键词：`BottomPaneHeights`、`total`、`spinner`、`completion_popup`、`status`。
- 关联：`app/render.rs` 依赖它决定消息区剩余高度。

### `crates/allthecodes/src/ui/command_palette/`

- 类型：组件/Overlay。
- 职责：命令面板的过滤、编辑目标、元数据和渲染。
- 典型关键词：`CommandPalette`、`filter.rs`、`render.rs`、`metadata.rs`、`edit_targets.rs`、`preferred_height`。
- 关联：由 `app.rs` 持有、`app/render.rs` 绘制；命令是否可用还受 `tui/command_availability.rs` 影响。

### `crates/allthecodes/src/ui/command_surface/`

- 类型：组件/Overlay。
- 职责：斜杠命令打开的交互 surface 容器，管理 `/agents`、`/config`、`/mcp`、`/memory`、`/skills`、`/tasks` 等面板的枚举、渲染和键盘事件。
- 典型关键词：`CommandSurface`、`CommandSurfaceOutcome`、`for_slash_command`、`surfaces/`、`adapters/`。
- 关联：`command_surface/mod.rs` 是总入口；具体业务面板在 `command_surface/surfaces/*.rs`；部分 surface 通过 `adapters/*.rs` 接功能模块状态。

### `crates/allthecodes/src/ui/components/`

- 类型：组件/Overlay。
- 职责：可复用 UI 组件目录，包括搜索框、tabs、pane、divider、loading state、pager overlay、fuzzy picker、history search、welcome、status widget 等。注意部分文件在 `ui/mod.rs` 中标记为 `#[cfg(test)]`，生产路径并不一定导出。
- 典型关键词：`search_box.rs`、`tabs.rs`、`pane.rs`、`divider.rs`、`history_search_dialog.rs`、`welcome.rs`、`fuzzy_match.rs`。
- 关联：组件一般不理解 `Message` 数据模型，只提供可复用视觉和交互单元；业务面板或 `app/render.rs` 组合它们。

### `crates/allthecodes/src/ui/overlays/`

- 类型：组件/Overlay。
- 职责：modal / overlay 基础设施。`overlay_stack.rs` 管理 z-index 栈，`dialog.rs` 提供通用对话框和退出保护，`mod.rs` 导出渲染辅助。
- 典型关键词：`OverlayStack`、`OverlayKind`、`OverlayEntry`、`render_centered_dialog_lines`、`CenteredOverlayFrame`、`ExitGuard`。
- 关联：权限弹窗、pager、fuzzy picker、退出确认等都应走 overlay 概念；最终由 `app/render.rs` 在主界面之上叠加。

### `crates/allthecodes/src/ui/permissions/`

- 类型：组件/Overlay。
- 职责：权限请求、问题确认、sandbox 权限、skill 权限、hook 权限、worker badge 等对话框和提示。
- 典型关键词：`PermissionDialog`、`PermissionChoice`、`QuestionDialog`、`permission_request.rs`、`sandbox_permission_request.rs`、`skill_permission_request/`、`hooks.rs`。
- 关联：事件来源在 `tui/engine_events.rs` 的权限回调；状态保存在 `app.rs`；视觉叠加由 `app/render.rs` 和 overlay/dialog 组件完成。

### `crates/allthecodes/src/ui/input/`

- 类型：组件/Overlay。
- 职责：输入编辑与补全辅助，包括 keybindings、vim、clipboard、path completion、shell history completion、slack channel completion、form navigation 等。
- 典型关键词：`keybindings.rs`、`vim.rs`、`completions.rs`、`path_completion.rs`、`clipboard_paste.rs`、`form_navigation.rs`。
- 关联：改变的是输入区行为和候选内容；最终渲染仍由 `prompt_input.rs`、`command_palette/` 或对应 surface 完成。

## 7. 功能模块 UI

### `crates/allthecodes/src/ui/agents/`

- 类型：功能模块 UI。
- 职责：Agent 管理、列表、详情、编辑器、模型选择、工具选择、颜色选择、新建 wizard、导航页脚。
- 典型关键词：`agents_menu.rs`、`agent_editor.rs`、`agent_detail.rs`、`tool_selector.rs`、`model_selector.rs`、`color_picker.rs`、`agent_navigation_footer.rs`。
- 关联：`command_surface/surfaces/agents.rs` 提供 slash command surface；`app/agent_navigation.rs` 和 `app/agent_tree_dialog.rs` 处理会话内 Agent 导航。

### `crates/allthecodes/src/ui/mcp/`

- 类型：功能模块 UI。
- 职责：MCP 服务器列表、卡片、工具列表、工具详情、server 菜单、导入、审批、重连、capabilities、settings、elicitation dialog。
- 典型关键词：`mcp_list_panel.rs`、`mcp_server_card.rs`、`mcp_tool_list_view.rs`、`mcp_tool_detail_view.rs`、`mcp_reconnect.rs`、`elicitation_dialog.rs`。
- 关联：`command_surface/surfaces/mcp.rs` 打开 MCP surface；MCP 工具调用最终仍以 `ContentBlock::ToolUse` / `ServerToolUse` 进入消息渲染管线。

### `crates/allthecodes/src/ui/memory/`

- 类型：功能模块 UI。
- 职责：记忆文件选择和更新通知。
- 典型关键词：`memory_file_selector.rs`、`memory_update_notification.rs`。
- 关联：`command_surface/surfaces/memory.rs` 提供入口；用户侧 memory 输入可能被 `user_memory_input_message.rs` 或 `user_text_message.rs` 显示。

### `crates/allthecodes/src/ui/skills/`

- 类型：功能模块 UI。
- 职责：Skill 菜单和 skill UI 状态。
- 典型关键词：`skills_menu.rs`、`SkillsSurface`、`SkillDiscovery`。
- 关联：`command_surface/surfaces/skills.rs` 提供入口；skill discovery 附件消息由 `attachment_message.rs` 显示；skill 权限在 `permissions/skill_permission_request/`。

### `crates/allthecodes/src/ui/tasks/`

- 类型：功能模块 UI。
- 职责：后台任务、shell 进度、远程会话进度、async agent、workflow、dream、monitor MCP 等任务详情和状态渲染。
- 典型关键词：`background_tasks_dialog.rs`、`background_task_status.rs`、`shell_progress.rs`、`render_tool_activity.rs`、`*_detail_dialog.rs`。
- 关联：`command_surface/surfaces/tasks.rs` 提供入口；工具活动状态可与 `rendering/tool_activity.rs` 的状态表达保持一致。

### `crates/allthecodes/src/ui/teams/`

- 类型：功能模块 UI。
- 职责：Team 状态、team dialog 等多 agent / team 相关界面。
- 典型关键词：`team_status.rs`、`teams_dialog.rs`。
- 关联：`command_surface/surfaces/team.rs` 提供入口；team 记忆消息由 `team_mem_collapsed.rs`、`team_mem_saved.rs` 显示。

### `crates/allthecodes/src/ui/diff/`

- 类型：功能模块 UI。
- 职责：结构化 diff、文件列表、详情视图、diff dialog、file edit diff 数据转换。
- 典型关键词：`structured_diff.rs`、`diff_file_list.rs`、`diff_detail_view.rs`、`diff_dialog.rs`、`file_edit_diff.rs`、`unified_hunk_lines_from_edit`。
- 关联：文件编辑消息 `file_edit_tool_updated_message.rs` 会用 hunk lines 展示 diff；`command_surface/surfaces/diff.rs` 提供 diff 面板入口。

### `crates/allthecodes/src/ui/notifications/`

- 类型：功能模块 UI。
- 职责：应用内通知、OSC 9、BEL、桌面通知 backend 检测和发送。
- 典型关键词：`in_app.rs`、`NotificationPriority`、`NotificationTone`、`osc9.rs`、`bel.rs`、`detect_backend`。
- 关联：backend 通知在 `tui.rs` 转成 `AppEvent` 或桌面通知；当前 in-app notification 在 `app/render.rs` 底部区域显示。

### `crates/allthecodes/src/ui/hooks/`

- 类型：功能模块 UI。
- 职责：hook 配置菜单、事件模式、matcher 模式、hook 模式选择、prompt dialog。
- 典型关键词：`hooks_config_menu.rs`、`select_event_mode.rs`、`select_matcher_mode.rs`、`select_hook_mode.rs`、`prompt_dialog.rs`。
- 关联：`command_surface/surfaces/hooks.rs` 提供入口；hook 进度消息由 `hook_progress_message.rs` 显示，权限事件由 `permissions/hooks.rs` 显示。

### `crates/allthecodes/src/ui/lsp_recommendation/`

- 类型：功能模块 UI。
- 职责：LSP 插件推荐菜单。
- 典型关键词：`lsp_recommendation_menu.rs`、`LspRecommendationSurface`。
- 关联：`command_surface/surfaces/lsp_recommendation.rs` 处理推荐响应；LSP 事件桥接在 `tui.rs`。

## 8. 常查问题索引

### Q1：某条消息为什么没有显示？

- 先查 `messages/render/context.rs` 的 `prepare_renderable_messages`。
- 再查 `messages/render/preprocessing.rs` 的 `filter_compact_boundary`、`should_show_renderable_message`、`filter_brief_messages`、`truncate_transcript_messages`。
- 如果是用户文本，继续查 `messages/user_text_message.rs` 的 `route_user_text` 是否返回隐藏。
- 如果是 thinking/redacted thinking，查 `render_assistant.rs` 和 `MessageRenderOptions.verbose` / `is_transcript_mode`。

### Q2：工具调用为什么被合成一条？

- 查 `messages/render/grouping.rs` 的 `apply_grouping`。
- 支持“同类工具聚合”的判断入口是 `grouping_tool_use_key` 和同源 source index。
- 最终展示查 `messages/grouped_tool_use_content.rs`。
- verbose 模式会绕过聚合。

### Q3：Read/Grep/Glob/LS 这类工具为什么折叠？

- 查 `messages/render/grouping.rs` 的 `collapse_read_search_groups` 和 `collapsible_kind_for_tool`。
- 当前折叠分类包括：Read 类 `Read`；Search 类 `Grep`、`Glob`、`WebSearch`；List 类 `LS`、`List`。
- 最终展示查 `messages/collapsed_read_search_content.rs`。
- 不在 `collapsible_kind_for_tool` 中的工具不会进入该折叠逻辑。

### Q4：工具状态颜色从哪里来？

- 工具生命周期状态先在 `messages/render/context.rs` 通过 `MessageLookups` 推导出 resolved、errored、in-progress。
- Assistant 工具调用状态映射在 `messages/render/render_assistant.rs` 的 `tool_state_for_id`。
- 工具调用文案在 `messages/assistant_tool_use_message.rs`。
- 状态标签颜色在 `rendering/tool_activity.rs` 的 `compact_styled_line`，语义颜色来自 `rendering/theme.rs` 的 `Theme`。

### Q5：用户消息深色背景在哪里设置？

- 背景色常量在 `messages/render/mod.rs`：`USER_MESSAGE_BACKGROUND`。
- 是否铺背景由 `renderable_message_uses_user_background` 和 `user_message_uses_background` 判断。
- 用户消息内容渲染在 `messages/render/render_user.rs`，普通文本会额外包一层背景样式。

### Q6：Assistant 文本 Markdown、代码块和语法高亮在哪里改？

- Assistant `ContentBlock::Text` 分发在 `messages/render/render_assistant.rs`。
- Markdown 解析和 block/inline 样式在 `rendering/markdown.rs`。
- 代码块 token 高亮在 `rendering/syntax_highlight.rs`。
- 样式颜色在 `rendering/theme.rs`，设计系统颜色源在 `theme/mod.rs`。

### Q7：系统消息和 API 错误在哪里区分？

- 数据类型在 `allthecodes-types/src/message.rs` 的 `SystemSubtype`。
- 分发在 `messages/render/render_assistant.rs` 的 `render_system_message`。
- API retry 错误查 `messages/system_api_error_message.rs`。
- 普通 info/warning/error/local command 查 `messages/system_text_message.rs`。
- Compact boundary 查 `messages/compact_boundary_message.rs` 与 `messages/render/preprocessing.rs`。

### Q8：shell 输出为什么有时展开、有时折叠？

- shell 工具调用识别和命令摘要在 `messages/assistant_tool_use_message.rs`。
- shell 工具结果展示在 `messages/render/render_user.rs`。
- 展开逻辑在 `MessageRenderContext::shell_expanded`：最新 shell 结果或当前选中且 expanded 的消息会展开。
- 具体截断和行数提示在 `messages/user_bash_output_message.rs`。

### Q9：文件编辑 diff 预览在哪里生成？

- 工具结果 JSON 识别在 `messages/render/render_user.rs` 的 `render_file_edit_preview`。
- diff 视觉渲染在 `messages/file_edit_tool_updated_message.rs`。
- hunk 数据辅助在 `ui/diff/file_edit_diff.rs`。
- diff 颜色在 `rendering/theme.rs` 的 `diff_add`、`diff_remove`、`diff_context`、`diff_header`。

### Q10：底部输入区、命令面板、通知栏的高度在哪里算？

- 总布局在 `app/render.rs`。
- 高度结构在 `components/bottom_pane.rs` 的 `BottomPaneHeights`。
- 输入框查 `prompt_input.rs`。
- 命令面板查 `command_palette/`。
- slash command surface 查 `command_surface/`。
- 通知查 `notifications/in_app.rs`。

### Q11：Overlay 或权限弹窗为什么盖住其它 UI？

- Overlay 栈在 `overlays/overlay_stack.rs`。
- 通用居中弹窗和退出保护在 `overlays/dialog.rs`。
- 权限对话框状态保存在 `app.rs`，事件来自 `tui/engine_events.rs`，渲染由 `permissions/` 与 `app/render.rs` 协作。

### Q12：某个 slash command 打开的页面在哪里？

- 总入口：`command_surface/mod.rs` 的 `CommandSurface::for_slash_command`。
- 具体页面：`command_surface/surfaces/<name>.rs`。
- 数据适配：`command_surface/adapters/<name>.rs`。
- 业务模块：通常在 `ui/agents/`、`ui/mcp/`、`ui/memory/`、`ui/skills/`、`ui/tasks/`、`ui/teams/`。

## 9. 推荐查阅路径

### 查“消息怎么显示”

1. `allthecodes-types/src/message.rs`
2. `ui/messages/render/context.rs`
3. `ui/messages/render/mod.rs`
4. `ui/messages/render/render_assistant.rs` 或 `render_user.rs`
5. 对应 `ui/messages/*_message.rs`
6. `ui/rendering/theme.rs`

### 查“布局为什么这样”

1. `ui/app.rs`
2. `ui/app/render.rs`
3. `ui/components/bottom_pane.rs`
4. `ui/rendering/virtual_scroll.rs`
5. `ui/prompt_input.rs`
6. `ui/command_palette/` 或 `ui/command_surface/`

### 查“主题和颜色”

1. `ui/theme/mod.rs`
2. `ui/theme/color.rs`
3. `ui/rendering/theme.rs`
4. `ui/rendering/markdown.rs`
5. `ui/rendering/syntax_highlight.rs`

### 查“组件、弹窗和功能面板”

1. `ui/components/`
2. `ui/overlays/`
3. `ui/permissions/`
4. `ui/command_surface/mod.rs`
5. `ui/command_surface/surfaces/`
6. 对应功能目录：`ui/agents/`、`ui/mcp/`、`ui/tasks/` 等
