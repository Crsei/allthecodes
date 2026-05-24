# cc-rust Rust TUI 结构梳理

根目录：`F:\AIclassmanager\cc\rust\crates\claude-code-rs\src\ui`

来源：`gpt-5.5` subagent 只读梳理。未修改文件，未运行构建或测试。`rg` 在当前环境被拒绝执行，改用 PowerShell 递归枚举。

## 文件级树

```text
ui/
├── app.rs - TUI 主应用状态 `App`、用户输入动作 `AppAction`、工作区信任提示与状态线用量快照。
├── approval_overlay.rs - 类型化审批/权限覆盖层基础组件与选项渲染。
├── bottom_pane.rs - 底部面板容器与视图栈状态。
├── browser.rs - `/hooks`、`/agents`、`/doctor`、`/tasks`、`/keybindings` 等文本树浏览/打开文件辅助。
├── capability_contract.rs - Rust UI 后端事件、前端命令、UI surface 的能力映射契约。
├── chat_composer.rs - 底部聊天输入 composer 状态、模式和输入效果。
├── clipboard_paste.rs - 剪贴板图片粘贴、临时 PNG 写入、路径规范化和图片格式识别。
├── clipboard_text.rs - 文本复制到系统剪贴板、OSC 52、WSL/平台剪贴板 fallback。
├── command_palette.rs - 命令面板、命令过滤、参数帮助、目标选择和快照测试渲染。
├── diff.rs - 文本 diff 行计算和 ratatui diff 渲染。
├── event_router.rs - 终端事件、引擎事件、UI 事件到路由动作的轻量分发。
├── feature_panels.rs - full-build 功能面板注册表与面板索引渲染。
├── frame_requester.rs - 合并重绘请求的 frame requester。
├── history_cell.rs - 类型化聊天历史单元与历史渲染。
├── keybindings.rs - keybinding 动作、按键组合、上下文和默认/可配置绑定注册表。
├── markdown.rs - Markdown 到 ratatui styled lines 的转换与缓存。
├── messages.rs - 消息列表虚拟滚动渲染和各类消息行渲染。
├── mod.rs - UI 模块导出入口，并重导出 `cc_engine::status_line`。
├── permissions.rs - 工具权限弹窗、审批类型映射、权限选择和渲染辅助。
├── prompt_input.rs - 单行 prompt 输入控件、光标和常见编辑快捷键处理。
├── selection_surface.rs - 可复用选择器/命令 picker surface。
├── spinner.rs - Braille spinner 动画状态和渲染。
├── status_line_resolver.rs - 根 crate 层状态线字段预解析，如 output style 与 worktree 状态。
├── status_widget.rs - 内置 rich status widget 的状态快照。
├── streaming_controller.rs - 流式输出 delta 的确定性控制器。
├── terminal_env.rs - 终端环境变量配置解析、编辑器命令解析和诊断支持。
├── terminal_integration.rs - 终端集成策略、环境能力和策略渲染。
├── theme.rs - 终端 UI 主题样式集合。
├── tool_activity.rs - 工具活动状态、进度和分组文本渲染。
├── transcript.rs - Prompt/Transcript/Focus 三态视图、滚动、搜索和导出状态机。
├── tui.rs - 集成 TUI runner；连接 ratatui/crossterm UI 与异步 `QueryEngine`。
├── vim.rs - 输入框 Vim 模式状态机，含 Normal/Insert/Visual、移动、操作符等。
├── virtual_scroll.rs - 消息虚拟滚动、高度缓存和可见范围计算。
├── visual_regression.rs - 基础/交互 UI surface 的快照测试渲染目标。
├── welcome.rs - 启动欢迎面板渲染和高度计算。
├── notifications/
│   ├── bel.rs - BEL 桌面通知后端与通知命令。
│   ├── mod.rs - 桌面通知后端选择、OSC9 支持检测和通知模块入口。
│   └── osc9.rs - OSC 9 桌面通知后端与通知命令。
└── snapshots/
    ├── claude_code_rs__ui__command_palette__tests__command_argument_help_all_commands_110w.snap - 命令参数帮助快照；推断为 `command_palette` 测试基准。
    ├── claude_code_rs__ui__command_palette__tests__command_palette_after_page_down_100x12.snap - 命令面板 PageDown 后状态快照；推断。
    ├── claude_code_rs__ui__command_palette__tests__command_palette_mcp_filtered_100x12.snap - 命令面板 MCP 过滤快照；推断。
    ├── claude_code_rs__ui__command_palette__tests__command_palette_plugin_filtered_100x12.snap - 命令面板插件过滤快照；推断。
    ├── claude_code_rs__ui__command_palette__tests__command_palette_root_100x12.snap - 命令面板根视图快照；推断。
    ├── claude_code_rs__ui__visual_regression__tests__foundational_ui_surfaces.snap - 基础 UI surface 视觉回归快照；推断。
    └── claude_code_rs__ui__visual_regression__tests__interactive_ui_surfaces.snap - 交互 UI surface 视觉回归快照；推断。
```

## 模块组成摘要

入口文件是 `mod.rs`，它声明并导出整个 `ui` 模块。运行入口在 `tui.rs`，负责接管终端、连接 `QueryEngine`、处理 engine event 和 streaming state。核心应用状态在 `app.rs`。

主要边界：

- 主循环/状态：`tui.rs`、`app.rs`、`event_router.rs`、`frame_requester.rs`
- 输入与交互：`prompt_input.rs`、`chat_composer.rs`、`vim.rs`、`keybindings.rs`、`command_palette.rs`、`selection_surface.rs`
- 渲染组件：`messages.rs`、`markdown.rs`、`diff.rs`、`spinner.rs`、`theme.rs`、`welcome.rs`、`status_widget.rs`、`bottom_pane.rs`
- 权限/审批：`permissions.rs`、`approval_overlay.rs`、`capability_contract.rs`
- 终端/系统集成：`terminal_env.rs`、`terminal_integration.rs`、`clipboard_text.rs`、`clipboard_paste.rs`、`notifications/*`
- 历史/滚动/转录：`history_cell.rs`、`virtual_scroll.rs`、`transcript.rs`
- 测试/视觉回归：内联 `#[cfg(test)]` 分布在多个 `.rs` 文件；快照集中在 `snapshots/`。

## 测试与快照位置

内联测试存在于：

- `app.rs`
- `browser.rs`
- `clipboard_paste.rs`
- `clipboard_text.rs`
- `command_palette.rs`
- `keybindings.rs`
- `terminal_env.rs`
- `transcript.rs`
- `tui.rs`
- `vim.rs`
- `virtual_scroll.rs`
- `visual_regression.rs`
- `welcome.rs`
- `notifications/mod.rs`

快照资源位置：`F:\AIclassmanager\cc\rust\crates\claude-code-rs\src\ui\snapshots`，共 7 个 `.snap` 文件。

## 无法确认的项

- 没有发现独立 UI 配置文件或资源目录，除 `snapshots/` 外也未发现图片/样式等资源文件。
- 部分快照文件职责只能从文件名和命名约定推断；未打开完整快照内容，也未运行测试验证生成关系。
