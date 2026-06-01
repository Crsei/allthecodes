# 第四章：聚合显示优化索引

本文是 `data-type-visual-distinction.md` 第四章「聚合显示优化」的详细索引，聚焦 TUI 消息渲染链路中的两类聚合节点：

- `GroupedToolUse`：把同一条 assistant 消息中重复出现的同类工具调用压成一行汇总。
- `CollapsedReadSearch`：把连续的读取、搜索、列目录类工具活动压成一段可展开摘要。

相关源码入口：

| 主题 | 源码路径 | 关键函数 / 类型 |
|---|---|---|
| 聚合调度顺序 | `crates/allthecodes/src/ui/messages/render/context.rs` | `prepare_renderable_messages`, `RenderableMessage`, `GroupedToolUseRenderRecord`, `CollapsedReadSearchRenderRecord`, `build_message_lookups` |
| 聚合判定与折叠判定 | `crates/allthecodes/src/ui/messages/render/grouping.rs` | `apply_grouping`, `collapse_read_search_groups`, `grouping_tool_use_key`, `is_groupable_tool`, `collapsible_kind_for_tool`, `collapsible_info_for_tool` |
| 同类工具汇总渲染 | `crates/allthecodes/src/ui/messages/grouped_tool_use_content.rs` | `GroupedToolUseView`, `render_grouped_tool_use_lines` |
| 读/搜/列目录折叠渲染 | `crates/allthecodes/src/ui/messages/collapsed_read_search_content.rs` | `CollapsedReadSearchView`, `render_collapsed_read_search_lines` |
| 顶层消息渲染与流式光标 | `crates/allthecodes/src/ui/messages/render/mod.rs` | `render_messages`, `render_renderable_message_with_context`, `decorate_selected_message` |
| 非聚合文本边界 | `crates/allthecodes/src/ui/messages/assistant_text_message.rs` | `classify_assistant_text`, `render_api_error`, `render_assistant_text_message` |

## 4.1 渲染链路中的位置

聚合发生在 `prepare_renderable_messages` 内，顺序固定为：

1. `normalize_messages_for_render`：把原始 `Message` 规范化为 `RenderableMessage::Message`。
2. `filter_compact_boundary` / `should_show_renderable_message`：过滤压缩边界、隐藏消息等。
3. `reorder_messages_in_ui`：按 UI 需要调整消息顺序。
4. `filter_brief_messages` / `truncate_transcript_messages`：应用简略模式和 transcript 限制。
5. `apply_grouping`：生成 `RenderableMessage::GroupedToolUse`。
6. `collapse_read_search_groups`：生成 `RenderableMessage::CollapsedReadSearch`。
7. `build_message_lookups`：基于规范化消息建立工具调用、工具结果、进度与状态索引。

这个顺序决定了两个行为：

- `GroupedToolUse` 先于 `CollapsedReadSearch`，因此已分组的 `Read` / `Grep` / `Glob` 还可能继续被读搜折叠吸收。
- `build_message_lookups` 使用规范化后的原始消息，而不是只看聚合后的节点，所以即使原始工具调用被隐藏，状态统计仍能通过 `tool_use_ids` 回查。

## 4.2 GroupedToolUse：同类工具聚合

`GroupedToolUse` 的目标是减少同一 assistant 回合内重复工具调用的视觉噪音。它只处理工具调用消息，不处理工具结果正文、普通 assistant 文本或系统消息。

### 分组条件

分组条件由 `grouping.rs` 的 `grouping_tool_use_key` 和 `is_groupable_tool` 决定：

| 条件 | 说明 |
|---|---|
| 消息形态 | 必须是 `RenderableMessage::Message { message: Message::Assistant(..) }` |
| 内容块位置 | 只检查 assistant 的第一个 `ContentBlock` |
| 内容块类型 | 第一个块必须是 `ContentBlock::ToolUse` 或 `ContentBlock::ServerToolUse` |
| 工具白名单 | 工具名必须通过 `is_groupable_tool` |
| 分组键 | `(source_index, tool_name)` |
| 最小数量 | 同一分组键下至少 2 条工具调用 |
| verbose 模式 | `MessageRenderOptions.verbose == true` 时完全跳过聚合 |

`source_index` 是关键约束。它让聚合只发生在同一条源 assistant 消息内部，避免把不同回合、不同上下文里的同名工具合并到一起。

### GroupedToolUse 工具支持矩阵

| 工具名 | 是否支持 GroupedToolUse | 用户可见名称 | 是否还能进入 CollapsedReadSearch | 说明 |
|---|---:|---|---:|---|
| `Task` | 是 | `Agent` | 否 | 用于多个 agent/task 调用的同类聚合 |
| `Agent` | 是 | `Agent` | 否 | 与 `Task` 一样显示为 Agent，但分组键仍按原始工具名区分 |
| `Read` | 是 | `Read` | 是 | 先同类聚合，再可作为 read 类活动被折叠 |
| `Grep` | 是 | `Search` | 是 | 同类聚合后计入 search 数量 |
| `Glob` | 是 | `Glob` | 是 | 同类聚合后计入 search 数量 |
| `WebSearch` | 否 | `WebSearch` | 是 | 不做同类聚合，但连续出现时可进入读搜折叠 |
| `LS` | 否 | `LS` | 是 | 不做同类聚合，只参与 list 类折叠 |
| `List` | 否 | `List` | 是 | 不做同类聚合，只参与 list 类折叠 |
| `Bash` | 否 | `Bash` | 否 | shell 输出有独立展开逻辑，不进入此聚合 |
| `PowerShell` | 否 | `PowerShell` | 否 | 同 Bash |
| `Edit` / `MultiEdit` / `Write` | 否 | `Edit` | 否 | 文件修改类工具保留单独可视化，避免隐藏变更语义 |
| `TodoWrite` | 否 | `Todo` | 否 | todo 列表有专用渲染格式 |
| `WebFetch` | 否 | `Fetch` | 否 | fetch 结果不计入读搜折叠白名单 |
| 其他工具 | 否 | 原始工具名 | 否 | 默认保持原消息渲染 |

### 记录结构

`context.rs` 的 `GroupedToolUseRenderRecord` 保存渲染所需的最小索引：

| 字段 | 来源 | 用途 |
|---|---|---|
| `uuid` | `derive_group_uuid(group[0].uuid(), "grouped")` | 为虚拟聚合节点生成稳定 UUID |
| `timestamp` | 第一条工具调用的 timestamp | 保持聚合节点在时间线上的位置 |
| `source_indices` | 分组内消息的 source index 去重后收集 | 支持选中态命中与来源追踪 |
| `tool_name` | 分组键中的工具名 | 渲染工具类别 |
| `tool_use_ids` | 分组内所有工具调用 id | 回查完成、失败、进行中状态 |

`apply_grouping` 发出聚合节点后会跳过原始工具调用消息，并把这些工具调用 id 放入 `grouped_tool_ids`。后续如果遇到对应的 `ToolResult` 用户消息，`tool_result_id` 命中后也会跳过，从而避免“工具调用已聚合，但结果又逐条显示”的重复。

### 状态统计

`GroupedToolUse` 本身不保存状态。状态在渲染阶段由 `render_renderable_message_with_context` 通过 `MessageLookups` 动态计算：

| 统计项 | 计算来源 | 语义 |
|---|---|---|
| `count` | `group.tool_use_ids.len()` | 聚合内工具调用总数 |
| `resolved_count` | `lookups.resolved_tool_use_ids` 命中的 id 数量 | 已收到工具结果的调用数 |
| `error_count` | `lookups.errored_tool_use_ids` 命中的 id 数量 | 工具结果标记为错误的调用数 |

`build_message_lookups` 在扫描规范化消息时建立这些集合：

- `resolved_tool_use_ids`：用户消息里的 `ContentBlock::ToolResult` 出现即视为 resolved。
- `errored_tool_use_ids`：对应 `ToolResult.is_error == true`。
- `in_progress_tool_use_ids`：有 `ToolUse` 但没有 resolved 的 id。

### 渲染格式

`grouped_tool_use_content.rs` 的 `render_grouped_tool_use_lines` 只输出一行，样式使用 `theme.tool_name`：

| 状态 | 文案格式 |
|---|---|
| 有失败 | `  ● {count} {display_name} calls · {error_count} failed` |
| 全部完成 | `  ● {count} {display_name} calls · completed` |
| 部分完成 | `  ● {count} {display_name} calls · {resolved_count}/{count} completed` |
| 全部未完成 | `  ● {count} {display_name} calls` |

示例：

```text
  ● 4 Search calls · 3/4 completed
  ● 2 Agent calls · completed
  ● 5 Read calls · 1 failed
```

用户可见工具名由 `crates/allthecodes/src/ui/rendering/tool_activity.rs` 的 `user_facing_tool_name` 转换，例如 `Grep` 显示为 `Search`，`Task` / `Agent` 显示为 `Agent`。

## 4.3 CollapsedReadSearch：读/搜/列目录折叠

`CollapsedReadSearch` 面向“连续的信息收集动作”：读文件、搜索、glob、web search、列目录。它比 `GroupedToolUse` 更偏时间线压缩，不要求同一个 `source_index` 或同一个工具名，而是扫描连续可折叠节点。

### 折叠条件

折叠条件由 `collapse_read_search_groups`、`collapsible_tool_info` 和 `collapsible_kind_for_tool` 决定：

| 条件 | 说明 |
|---|---|
| verbose 模式 | `MessageRenderOptions.verbose == true` 时完全跳过折叠 |
| 起点 | 当前消息必须能被 `collapsible_tool_info` 识别 |
| 可识别消息 | 原始 assistant `ToolUse` / `ServerToolUse`，或已经生成的 `GroupedToolUse` |
| 连续性 | 只合并相邻的可折叠工具节点；遇到不可折叠消息即结束 |
| 工具结果 | 如果相邻消息是当前折叠组内某个 `tool_use_id` 的 `ToolResult`，会被吸收并跳过 |
| 最小数量 | `read_count + search_count + list_count >= 2` 才生成折叠节点 |

如果连续片段里只有 1 个可折叠工具，函数会把原始消息放回，不生成 `CollapsedReadSearch`。

### CollapsedReadSearch 工具支持矩阵

| 工具名 | 折叠类别 | 是否支持原始 ToolUse 折叠 | 是否支持从 GroupedToolUse 折叠 | 计数字段 | hint 来源 |
|---|---|---:|---:|---|---|
| `Read` | Read | 是 | 是 | `read_count` | `tool_primary_input(name, input)` 或 grouped 工具名 |
| `Grep` | Search | 是 | 是 | `search_count` | 同上 |
| `Glob` | Search | 是 | 是 | `search_count` | 同上 |
| `WebSearch` | Search | 是 | 否 | `search_count` | `tool_primary_input(name, input)` |
| `LS` | List | 是 | 否 | `list_count` | `tool_primary_input(name, input)` |
| `List` | List | 是 | 否 | `list_count` | `tool_primary_input(name, input)` |
| `Task` | 不支持 | 否 | 否 | - | - |
| `Agent` | 不支持 | 否 | 否 | - | - |
| `Bash` / `PowerShell` | 不支持 | 否 | 否 | - | - |
| `Edit` / `MultiEdit` / `Write` | 不支持 | 否 | 否 | - | - |
| `TodoWrite` | 不支持 | 否 | 否 | - | - |
| `WebFetch` | 不支持 | 否 | 否 | - | - |
| 其他工具 | 不支持 | 否 | 否 | - | - |

“从 `GroupedToolUse` 折叠”只对 `Read` / `Grep` / `Glob` 成立，因为只有这些工具同时在 `is_groupable_tool` 和 `collapsible_kind_for_tool` 的白名单中。

### 记录结构

`context.rs` 的 `CollapsedReadSearchRenderRecord` 保存折叠摘要：

| 字段 | 来源 | 用途 |
|---|---|---|
| `uuid` | `derive_group_uuid(originals[0].uuid(), "collapsed")` | 为折叠节点生成稳定 UUID |
| `timestamp` | 第一条原始消息的 timestamp | 保持折叠节点在时间线上的位置 |
| `source_indices` | 折叠片段里的来源索引 | 支持选中态命中 |
| `tool_use_ids` | 折叠片段里的所有工具调用 id | 判断是否仍在运行 |
| `read_count` | Read 类数量 | 渲染 `Read/Reading ... files` |
| `search_count` | Search 类数量 | 渲染 `Searched/Searching ... patterns` |
| `list_count` | List 类数量 | 渲染 `Listed/Listing ... directories` |
| `latest_hint` | 最近一个非空 hint | 活跃状态下显示当前目标 |

`add_collapsible_info` 每吸收一个工具节点都会累加计数和 `tool_use_ids`，并用最新的非空 `hint` 覆盖 `latest_hint`。这让活跃折叠块显示更接近“当前正在处理什么”，而不是只显示第一项。

### 状态统计与 active 判定

`CollapsedReadSearch` 不展示完成/失败数量，而是展示“是否仍活跃”。在 `render_renderable_message_with_context` 中：

```text
active = any(tool_use_id in lookups.in_progress_tool_use_ids)
```

因此：

- 只要折叠组内还有任意工具没有对应 `ToolResult`，文案使用进行时。
- 所有工具都有结果后，文案切换为过去时。
- 错误结果不会单独显示为 failed；错误只通过 `in_progress_tool_use_ids` 的移除影响 active 状态。

### 渲染格式

`collapsed_read_search_content.rs` 的 `render_collapsed_read_search_lines` 根据三类计数组装英文短语，整段使用 `theme.dim`：

| 类别 | active=true | active=false | 单复数 |
|---|---|---|---|
| Search | `Searching for {n} pattern(s)` | `Searched for {n} pattern(s)` | `pattern` / `patterns` |
| Read | `Reading {n} file(s)` | `Read {n} file(s)` | `file` / `files` |
| List | `Listing {n} directory/directories` | `Listed {n} directory/directories` | `directory` / `directories` |

主行格式：

```text
{parts.join(", ")}{active ? "…" : ""} Ctrl+O to expand
```

示例：

```text
Searching for 3 patterns, Read 2 files Ctrl+O to expand
Searching for 1 pattern, Reading 4 files, Listing 2 directories… Ctrl+O to expand
```

当 `active == true` 且 `latest_hint` 非空时，下一行开始追加当前目标提示：

```text
  ⎿  {latest_hint}
```

如果 `latest_hint` 包含多行，每一行都会加同样的 `⎿` 前缀。

## 4.4 展开 / 折叠提示

`CollapsedReadSearch` 的主文案固定包含 `Ctrl+O to expand`。这里的 “expand” 是 UI 模式层面的提示，不是该函数内部直接展开原始消息。相关交互状态由 `App` 的视图模式和 `MessageRenderContext.selected_expanded` 参与。

`render/mod.rs` 中还有独立的选中消息装饰：

| 函数 | 行为 |
|---|---|
| `decorate_selected_message` | 给选中原始消息增加 `▶ selected` 或 `▼ selected` 标题 |
| `message_detail_lines` | 展开时显示 `uuid`、时间、引用和复制预览 |
| `RenderableMessage::has_source_index` | 聚合节点也可通过 `source_indices` 判断是否命中选中来源 |

需要注意：`decorate_selected_message` 目前只对 `RenderableMessage::Message` 插入详情行；`GroupedToolUse` 和 `CollapsedReadSearch` 可被 `has_source_index` 命中，但不会走原始消息的详情装饰分支。

## 4.5 流式光标

流式光标由 `render/mod.rs` 的 `render_messages` 统一追加：

```text
if streaming
  && idx == renderable_messages.len() - 1
  && renderable_messages[idx].is_assistant_message()
then append " ▌"
```

关键结论：

- 光标只追加到最后一个 `RenderableMessage`。
- 该节点必须满足 `RenderableMessage::is_assistant_message()`。
- `is_assistant_message()` 只对原始 `RenderableMessage::Message { message: Message::Assistant(..) }` 返回 true。
- `GroupedToolUse` 和 `CollapsedReadSearch` 都不会直接显示流式光标。

这避免了聚合摘要行被误认为正在流式生成的 assistant 文本。工具聚合的运行态由 `resolved_count` / `active` 等状态统计表达，而不是使用文本光标表达。

## 4.6 Assistant 文本与聚合边界

`assistant_text_message.rs` 负责 assistant 普通文本的 API 错误分类与渲染，例如 `classify_assistant_text`、`render_api_error`、`render_assistant_text_message`。它不参与 `GroupedToolUse` 或 `CollapsedReadSearch` 的分组判定。

边界规则：

| 内容 | 是否参与工具聚合 | 原因 |
|---|---:|---|
| 普通 assistant 文本 | 否 | 不是 `ToolUse` / `ServerToolUse` |
| API error 文本 | 否 | 由 assistant text 分类逻辑渲染，不是工具调用 |
| assistant 第一个内容块不是工具 | 否 | `grouping_tool_use_key` 和 `collapsible_tool_info` 都只检查第一个工具块 |
| 工具结果文本 | 间接参与 | 可被 `tool_result_id` 用来跳过已聚合结果，或被 `build_message_lookups` 用来统计状态 |

因此，文本错误、工具调用聚合、工具结果状态索引是三条相邻但分离的渲染路径。

## 4.7 实现检查清单

修改或扩展聚合逻辑时，应按下列入口检查：

| 变更目标 | 必查位置 | 检查点 |
|---|---|---|
| 新增可同类聚合工具 | `grouping.rs::is_groupable_tool` | 是否会误合并跨语义工具；是否需要隐藏对应 ToolResult |
| 新增读搜折叠工具 | `grouping.rs::collapsible_kind_for_tool` | 应归入 Read、Search 还是 List |
| 调整工具显示名 | `rendering/tool_activity.rs::user_facing_tool_name` | `GroupedToolUse` 的 `display_name` 是否符合用户预期 |
| 调整 GroupedToolUse 文案 | `grouped_tool_use_content.rs::render_grouped_tool_use_lines` | 失败、全完成、部分完成、未完成四种状态是否完整 |
| 调整 CollapsedReadSearch 文案 | `collapsed_read_search_content.rs::render_collapsed_read_search_lines` | active/非 active、单复数、hint 多行前缀是否正确 |
| 调整状态统计 | `context.rs::build_message_lookups` | resolved、errored、in_progress 集合是否仍从原始消息构建 |
| 调整流式显示 | `render/mod.rs::render_messages` | 聚合节点是否需要显式排除或支持光标 |

