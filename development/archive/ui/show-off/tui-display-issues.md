# TUI 展示问题记录

本文记录基于 `development/archive/ui/show-off/data-type-visual-distinction*` 系列文档观察到的 TUI 展示风险。范围只覆盖 Rust TUI 的消息展示、工具状态、聚合折叠、主题和附件呈现。

## 高优先级

### 1. 工具状态会误导用户

`ToolUseState::Queued`、`WaitingForPermission`、`ClassifierChecking` 已有渲染分支，但主链路 `tool_state_for_id` 当前不会推导这些状态。实际 UI 容易把排队、等待权限、分类检查中的工具显示成运行中。

影响：

- 用户可能误以为工具已经开始执行。
- 权限等待态不够明显，容易造成“卡住”的感知。

依据：

- `data-type-visual-distinction/01-core-data-types.md`：`ToolUseState` 表中说明这些状态“当前主路径通常不可达”。
- `data-type-visual-distinction/02-six-layer-distinction.md`：6.1 节说明 queued/permission/classifier 状态 helper 支持但主链路未推导。

建议：

- 在 `MessageRenderContext.lookups` 或工具活动状态中接入 queued、permission、classifier 语义。
- 状态文案必须明确区分 `Running`、`Queued`、`Needs permission`、`Checking`。

### 2. 折叠后的 Read/Search/List 不显示失败

`CollapsedReadSearch` 只展示 active/非 active，不展示失败数量。文档明确说明错误结果不会单独显示为 failed，只会让 active 状态消失。

影响：

- 搜索、读取、列目录失败后可能看起来像正常完成。
- 原始 tool result 被吸收后，用户很难在主列表里发现失败。

依据：

- `data-type-visual-distinction/04-aggregation-optimizations.md`：4.3 节“状态统计与 active 判定”。

建议：

- `CollapsedReadSearchView` 增加 `error_count` 或 `failed_count`。
- 失败时在摘要行中显示 `N failed`，并对失败片段使用 `theme.error`。

### 3. `Ctrl+O to expand` 可能是假承诺

`CollapsedReadSearch` 主文案固定包含 `Ctrl+O to expand`，但 `GroupedToolUse` 和 `CollapsedReadSearch` 虽然能通过 `source_indices` 命中选中态，却不会走原始消息的详情装饰分支。

影响：

- 用户看到“可展开”，但展开后可能拿不到对应原始工具调用和结果详情。
- 聚合节点的交互语义与普通消息不一致。

依据：

- `data-type-visual-distinction/04-aggregation-optimizations.md`：4.4 节说明 `decorate_selected_message` 目前只对 `RenderableMessage::Message` 插入详情行。

建议：

- 为 `GroupedToolUse` 和 `CollapsedReadSearch` 增加专用展开视图。
- 如果暂时不能展开，应移除或改写 `Ctrl+O to expand` 文案。

### 4. Progress 默认隐藏，运行中反馈弱

`Message::Progress` 默认不进入主消息列表，只进入 lookup；文档也说明 progress 不按 `tool_use_id` 直接附着到工具行。

影响：

- 长任务中间阶段不可见，用户只能看到粗粒度 running。
- hook 或工具进度信息存在但没有被有效呈现。

依据：

- `data-type-visual-distinction/01-core-data-types.md`：`Message::Progress` 默认隐藏但进入 lookup。
- `data-type-visual-distinction/02-six-layer-distinction.md`：Progress 弱支持，不按 `tool_use_id` 直接附着到工具行。

建议：

- 将最新 progress 摘要附着到对应工具调用行或折叠行。
- 对长时间运行工具显示最近一次 progress message。

## 中优先级

### 5. 用户消息背景硬编码

普通用户文本使用固定深色背景 `Rgb(31,35,42)`，不是主题字段。

影响：

- 浅色主题、ANSI 主题、自定义终端配色下可能突兀或低对比。
- 主题系统无法统一控制用户消息背景。

依据：

- `data-type-visual-distinction/02-six-layer-distinction.md`：第三层用户消息说明使用 `USER_MESSAGE_BACKGROUND`。

建议：

- 将用户消息背景迁移到 `ThemeColors` / legacy `Theme` 字段。
- 为 dark/light/ANSI/daltonized 分别定义可读背景。

### 6. 附件和结构化输出展示太弱

`Attachment::StructuredOutput` 只显示 `[structured output]`；部分附件 helper 能力没有接入顶层；`diagnostics`、`command_permissions`、部分 hook success 等路径可能空渲染。

影响：

- 排错信息和结构化结果被隐藏。
- 用户知道“有输出”，但不知道输出内容和重要字段。

依据：

- `data-type-visual-distinction/02-six-layer-distinction.md`：第五层进度与附件。

建议：

- 对结构化输出提供摘要，例如 key 列表、状态字段、错误字段或首屏 JSON 预览。
- 补齐顶层 `Attachment` 到 helper label/detail 的接线。

### 7. ServerToolUse 聚合后丢失来源语义

单条 `ServerToolUse` 会加 `server: ` 前缀，但进入聚合后不会保留该前缀。

影响：

- MCP/服务端工具和本地工具在聚合摘要中不易区分。
- 安全感知和来源判断变弱。

依据：

- `data-type-visual-distinction/02-six-layer-distinction.md`：ServerToolUse 单条有 `server:` 前缀，聚合后不会保留 `server:`。

建议：

- `GroupedToolUseRenderRecord` 保留来源类型。
- 聚合显示中区分 `server Search calls`、`local Search calls`，或在混合来源时显示来源统计。

### 8. 聚合工具失败状态不够醒目

`GroupedToolUse` 整行使用 `theme.tool_name`，失败只靠 `N failed` 文案表达；被聚合的工具结果会从主列表移除。

影响：

- 失败状态不够突出。
- 失败原因不在主列表中暴露，用户需要额外定位。

依据：

- `data-type-visual-distinction/04-aggregation-optimizations.md`：GroupedToolUse 渲染格式和 tool result 跳过规则。

建议：

- 对 `failed` 片段使用 `theme.error`。
- 展开聚合节点时显示失败工具的 error summary。

### 9. 复杂 ToolResult 过度摘要

嵌套 `ToolResultContent::Blocks` 在 assistant 路径只保留文本、连接器文本和图片引用，其它块显示 `[...]`；user 路径只保留文本和图片。

影响：

- MCP、结构化工具、多模态结果可能丢失关键展示信息。
- 用户看到占位符但无法判断被省略内容的重要性。

依据：

- `data-type-visual-distinction/01-core-data-types.md`：`ToolResultContent::Blocks` 行为。

建议：

- 为常见嵌套 block 增加语义摘要，而不是统一显示 `[...]`。
- 对不可展示 block 至少显示类型名和数量。

## 主题与可访问性问题

### 10. 主题系统没有完全闭环

ANSI 主题只替换主语义色，`diff*`、`syntax*`、`icon*`、`agent*` 仍可能继承 RGB；语法高亮使用固定 `base16-ocean.dark`，不完全适配浅色主题。

影响：

- 浅色主题下代码块可能低对比或刺眼。
- ANSI 主题无法保证全局 ANSI 兼容。
- Daltonized 主题只覆盖局部红绿语义，不是完整无障碍调色板。

依据：

- `data-type-visual-distinction/03-theme-system.md`：ANSI、语法高亮、diff 和 Daltonized 限制。

建议：

- 将 syntect token scope 映射到 `ThemeColors.syntax*`。
- 让 ANSI 主题决定是否全量使用 `Color::Indexed`。
- 补齐 daltonized 完整 palette。

## 建议修复顺序

1. 接线 queued / permission / classifier 状态，避免工具状态误导。
2. 为 `CollapsedReadSearch` 和 `GroupedToolUse` 增加失败可见性。
3. 修正 `Ctrl+O to expand` 的真实展开行为或文案。
4. 将用户消息背景纳入主题系统。
5. 把 progress 附着到对应工具行。
6. 补齐附件、结构化输出和复杂 ToolResult 摘要。
7. 修主题闭环，重点验证 light、ANSI、daltonized 下的代码块和 diff。
