# ratatui UI 功能对标清单

> 对比 `ui/src/components/` (TypeScript/OpenTUI 前端, ~301 文件) 与
> `crates/claude-code-rs/src/ui/` (Rust/ratatui 后端, ~164 源文件)
>
> 生成日期: 2026-05-08 | 上游参考: `F:\AIclassmanager\cc\src\**`
>
> 最近复核范围：2026-05-04 P0 milestone 之后的 ratatui/direct TUI 更新，重点包括
> server-side streaming block 保留、direct TUI tool progress、replayed file-edit
> result 渲染，以及 UI snapshot 审阅导出工具。

---

## 对比方法说明

- **TS 端** = OpenTUI/React 终端 UI 组件（前端渲染层）
- **Rust 端** = ratatui 终端 UI 组件（后端直接渲染）
- "✅ 完整" = ratatui 侧有对应的完整实现
- "⚠️ 部分" = 有基本实现但缺少关键子功能
- "❌ 缺失" = ratatui 侧无对应实现
- "➖ 不适用" = 纯前端抽象（React hooks、设计系统基类等），ratatui 不需要等价物

---

## 1. 核心 App Shell

| 功能 | TS 端 | Rust 端 | 状态 | 说明 |
|------|-------|---------|------|------|
| App 顶层状态机 | `App.tsx` | `app.rs` + `tui.rs` | ✅ | Rust 有完整的 App+Action 架构 |
| 全屏布局 | `FullscreenLayout.tsx` | `app/render.rs` | ✅ | |
| 终端 resize 回流 | `resize-sync.ts` | `terminal_env.rs` | ⚠️ | Rust TUI 端已修 (#12)，TS 端待修 |
| 消息区滚动 | `ScrollKeybindingHandler.tsx` | `virtual_scroll.rs` | ✅ | |
| 转录模式 | — | `app/transcript_mode.rs` | ✅ | Ctrl+O 切换 |
| 事件路由 | — | `runtime/event_router.rs` | ✅ | |
| 流式控制 | — | `runtime/streaming_controller.rs` | ✅ | |
| 帧请求调度 | — | `runtime/frame_requester.rs` | ✅ | |
| 会话日志 | — | `runtime/session_log.rs` | ✅ | |
| 语音控制 | — | `app/voice.rs` | ✅ | |
| 工作区信任 | — | `app/workspace_trust.rs` | ✅ | |
| 回退导航 | — | `cc-commands/src/rewind.rs` | ✅ | UI-only stub removed; slash command owns rewind |
| 已加载线程 | — | `cc-teams/src/loaded_threads.rs` | ✅ | |
| 服务端适配 | — | `cc-ipc-adapters/src/lib.rs` + `cc-ipc-client/src/requests.rs` | ✅ | |
| 可视化回归 | — | `runtime/visual_regression.rs` | ✅ | 含 insta snapshot |
| Snapshot 审阅导出 | — | `runtime/snapshot_export.rs` + `scripts/export-ui-snapshots.ps1` | ⚠️ | 本地已补导出入口，可汇总 accepted UI snapshots 到 `target/ui-snapshots/`；脚本/导出模块需确认纳入版本控制 |

---

## 2. 消息渲染 (Message Rendering)

### 2.1 消息类型覆盖

| 消息类型 | TS 端 | Rust 端 | 状态 |
|----------|-------|---------|------|
| assistant_text | `messages/AssistantTextMessage.tsx` | `messages/assistant_text_message.rs` | ✅ |
| assistant_thinking | `messages/ThinkingPreview.tsx` | `messages/assistant_thinking_message.rs` | ✅ |
| assistant_redacted_thinking | — | `messages/assistant_redacted_thinking_message.rs` | ✅ |
| assistant_tool_use | `messages/ToolActivityMessage.tsx` | `messages/assistant_tool_use_message.rs` | ✅ |
| user_text | `messages/UserTextMessage.tsx` | `messages/user_text_message.rs` | ✅ |
| user_prompt | — | `messages/user_prompt_message.rs` | ✅ |
| user_command | — | `messages/user_command_message.rs` | ✅ |
| user_bash_input | — | `messages/user_bash_input_message.rs` | ✅ |
| user_bash_output | — | `messages/user_bash_output_message.rs` | ✅ |
| user_image | — | `messages/user_image_message.rs` | ✅ |
| user_memory_input | — | `messages/user_memory_input_message.rs` | ✅ |
| user_plan | — | `messages/user_plan_message.rs` | ✅ |
| user_channel | — | `messages/user_channel_message.rs` | ✅ |
| user_teammate | — | `messages/user_teammate_message.rs` | ✅ |
| user_agent_notification | — | `messages/user_agent_notification_message.rs` | ✅ |
| user_local_command_output | — | `messages/user_local_command_output_message.rs` | ✅ |
| user_resource_update | — | `messages/user_resource_update_message.rs` | ✅ |
| user_tool_result | — | `messages/user_tool_result_message.rs` | ✅ |
| system_text | `messages/SystemMessage.tsx` | `messages/system_text_message.rs` | ✅ |
| system_api_error | — | `messages/system_api_error_message.rs` | ✅ |
| compact_boundary | `messages/CompactBoundaryMessage.tsx` | `messages/compact_boundary_message.rs` | ✅ |
| tool_group | `messages/ToolGroupMessage.tsx` | `messages/grouped_tool_use_content.rs` | ✅ |
| tool_result_orphan | `messages/ToolResultOrphanMessage.tsx` | (handled by render pipeline) | ✅ |
| streaming | `messages/StreamingMessage.tsx` | `rendering/markdown_stream.rs` + `tui/engine_events.rs` | ✅ direct TUI 已保留 server-side content blocks，覆盖 text/thinking/tool_use streaming 接线 |
| advisor | — | `messages/advisor_message.rs` | ✅ |
| attachment | — | `messages/attachment_message.rs` | ✅ |
| collapsed_read_search | — | `messages/collapsed_read_search_content.rs` | ✅ |
| highlighted_thinking | — | `messages/highlighted_thinking_text.rs` | ✅ |
| hook_progress | — | `messages/hook_progress_message.rs` | ✅ |
| null_rendering_attachments | — | `messages/null_rendering_attachments.rs` | ✅ |
| plan_approval | — | `messages/plan_approval_message.rs` | ✅ |
| rate_limit | — | `messages/rate_limit_message.rs` | ✅ |
| shutdown | — | `messages/shutdown_message.rs` | ✅ |
| task_assignment | — | `messages/task_assignment_message.rs` | ✅ |
| team_mem_collapsed | — | `messages/team_mem_collapsed.rs` | ✅ |
| team_mem_saved | — | `messages/team_mem_saved.rs` | ✅ |
| file_edit_preview | `messages/FileEditToolPreview.tsx` | `messages/render.rs` + `messages/file_edit_tool_updated_message.rs` | ⚠️ replayed `tool_use_result kind=file_edit` 已可渲染 hunk preview；非 replay/live 结构化事件仍依赖后端数据流 |

> Rust 消息渲染 **非常完整**——32 种消息类型的 snapshot 测试齐全。TS 端 11 个消息文件，Rust 端 34 个（含 render/wrap 框架层）。

### 2.2 消息交互功能

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 消息列表虚拟滚动 | `MessageList.tsx` | `virtual_scroll.rs` | ✅ |
| Markdown 渲染 | `Markdown.tsx` | `rendering/markdown.rs` + `markdown_render.rs` | ✅ |
| Markdown 表格 | `MarkdownTable.tsx` | (无独立模块) | ⚠️ 合并到 markdown_render |
| 代码高亮 | `HighlightedCode.tsx` + `highlighted-code/` | `rendering/markdown_render.rs` | ⚠️ |
| Diff 内联渲染 | `FileEditToolDiff.tsx` | `rendering/get_git_diff.rs` | ✅ |
| 文件路径可点击链接 | `FilePathLink.tsx` | — | ❌ |
| 消息时间戳 | `MessageTimestamp.tsx` | — | ❌ |
| 消息选择器 | `MessageSelector.tsx` | — | ❌ |
| 消息操作 (复制等) | `messageActions.tsx` | — | ❌ |
| 结构化 diff 展示 | `StructuredDiff.tsx` + `StructuredDiffList.tsx` | `diff/structured_diff.rs` + `diff/` | ✅ 已有 hunk 解析、旧/新 gutter、multi-hunk 分隔与截断覆盖 |
| 折叠展开提示 | `CtrlOToExpand.tsx` | (inline in transcript) | ✅ |
| 压缩摘要 | `CompactSummary.tsx` | — | ❌ |
| 用户中断展示 | `InterruptedByUser.tsx` | — | ❌ |
| 工具使用加载动画 | `ToolUseLoader.tsx` | `rendering/spinner.rs` | ✅ |
| 微光加载效果 | — | `rendering/shimmer.rs` | ✅ |
| Shell 输出展开 | `shell/ExpandShellOutputContext.tsx` | `messages/user_bash_output_message.rs` | ⚠️ 已有 expanded option，待自动展开上下文接线 |
| Shell 输出格式化 | `shell/OutputLine.tsx` | `messages/user_bash_output_message.rs` | ⚠️ 已有 ANSI 清理 / 小 JSON 格式化 / 宽度截断 |
| Shell 时间显示 | `shell/ShellTimeDisplay.tsx` | `messages/user_bash_output_message.rs` + `tasks/shell_progress.rs` | ⚠️ 已有 elapsed/timeout 文本显示 |

---

## 3. 输入系统 (Input / Composer)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 主输入组件 | `InputPrompt.tsx` | `components/prompt_input.rs` | ✅ |
| 聊天编辑器 | `PromptInput/ComposerBuffer.tsx` | `components/chat_composer.rs` | ✅ |
| 模式指示器 | `PromptInput/ModeIndicator.tsx` | — | ❌ |
| 斜杠命令提示 | `PromptInput/SlashCommandHints.tsx` | `input/slash_command.rs` | ✅ |
| 命令提示 | `CommandHint.tsx` | — | ⚠️ 合并到 slash_command |
| 快捷键提示 | `ConfigurableShortcutHint.tsx` | — | ❌ |
| 队列提交 | `PromptInput/QueuedSubmissions.tsx` | (backend-managed) | ➖ |
| 输入占位符 | `PromptInput/usePromptInputPlaceholder.ts` | — | ❌ |
| 输入截断 | `PromptInput/useMaybeTruncateInput.ts` | — | ❌ |
| 提交逻辑 | `PromptInput/useComposerSubmit.ts` | `app/input.rs` | ✅ |
| 键盘处理 | `PromptInput/keys.ts` | `input/keybindings.rs` | ✅ |
| 输入历史插入 | — | `input/insert_history.rs` | ✅ |
| Vim 模式 | `VimTextInput.tsx` | `input/vim.rs` | ✅ |
| 基础文本输入 | `BaseTextInput.tsx` | (ratatui 原生) | ➖ |
| 搜索框 | `SearchBox.tsx` | `components/search_box.rs` | ✅ 文本渲染 primitive 已补齐，含 focused/cursor/borderless snapshot |
| 文件搜索 (Ctrl+F) | — | `input/file_search.rs` | ✅ |
| 提及编解码 | — | `input/mention_codec.rs` | ✅ |
| 表单导航 | — | `input/form_navigation.rs` | ✅ |
| 剪贴板粘贴 | (built-in) | `input/clipboard_paste.rs` | ✅ |
| 剪贴板文本 | (built-in) | `input/clipboard_text.rs` | ✅ |
| 粘贴显示 (大文本折叠) | `__tests__/paste-display.test.ts` | — | ❌ |
| CWD 提示 | — | `components/cwd_prompt.rs` | ✅ |
| 底部状态栏 | `PromptInput/PromptInputFooter.tsx` | `components/bottom_pane.rs` | ✅ |

---

## 4. 权限系统 (Permissions)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Bash 权限请求 | `permissions/BashPermissionRequest.tsx` | `permissions/bash_permission_request.rs` | ✅ |
| 文件写入权限 | `permissions/FileWritePermissionRequest.tsx` | `permissions/file_write_permission_request.rs` | ✅ |
| 文件编辑权限 | `permissions/FileEditPermissionRequest.tsx` | `permissions/file_edit_permission_request.rs` | ✅ |
| WebFetch 权限 | `permissions/WebFetchPermissionRequest.tsx` | (in fallback) | ⚠️ |
| 通用权限回退 | `permissions/FallbackPermissionRequest.tsx` | `permissions/fallback_permission_request.rs` | ✅ |
| 权限对话框框架 | `permissions/PermissionDialogFrame.tsx` | `permissions/permission_dialog.rs` | ✅ |
| 权限选项渲染 | `permissions/PermissionPromptOptions.tsx` | `permissions/permission_prompt.rs` | ✅ |
| 权限请求入口 | `permissions/PermissionRequestDialog.tsx` | `permissions/permission_request.rs` | ✅ |
| 审批覆盖 | — | `components/approval_overlay.rs` | ✅ |
| AskUserQuestion | — | `permissions/ask_user_question_permission_request.rs` | ✅ |
| Computer Use 审批 | — | `permissions/computer_use_approval.rs` | ✅ |
| Plan 模式进入/退出 | — | `permissions/enter_plan_mode_permission_request.rs` + `exit_plan_mode_permission_request.rs` | ✅ |
| 文件权限对话框 | — | `permissions/file_permission_dialog.rs` | ✅ |
| 文件系统权限 | — | `permissions/filesystem_permission_request.rs` | ✅ |
| Hook 权限 | — | `permissions/hooks.rs` | ✅ |
| Monitor 权限 | — | `permissions/monitor_permission_request.rs` | ✅ |
| Notebook 编辑权限 | — | `permissions/notebook_edit_permission_request.rs` | ✅ |
| PowerShell 权限 | — | `permissions/power_shell_permission_request.rs` | ✅ |
| Sandbox 权限 | — | `permissions/sandbox_permission_request.rs` | ✅ |
| 决策调试信息 | — | `permissions/permission_decision_debug_info.rs` | ✅ |
| 权限说明 | — | `permissions/permission_explanation.rs` | ✅ |
| 权限规则说明 | — | `permissions/permission_rule_explanation.rs` | ✅ |
| Review Artifact 权限 | — | `permissions/review_artifact_permission_request.rs` | ✅ |
| 规则引擎 | — | `permissions/rules.rs` | ✅ |
| 权限请求标题 | — | `permissions/permission_request_title.rs` | ✅ |
| 自动模式选择加入 | `AutoModeOptInDialog.tsx` | — | ❌ |
| 绕过权限模式 | `BypassPermissionsModeDialog.tsx` | — | ❌ |

> Rust 权限系统 **25 个子模块**，比 TS 端 (9 文件) 更完整。"generated upstream" 标注说明这些是从上游 React 组件直接映射的。

---

## 5. Agent / 团队 (Agents & Teams)

### 5.1 Agent 管理

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Agent 列表 | `agents/AgentsList.tsx` | `agents/agents_list.rs` | ✅ |
| Agent 详情 | `agents/AgentDetail.tsx` | `agents/agent_detail.rs` | ✅ |
| Agent 编辑器 | `agents/AgentEditor.tsx` | `agents/agent_editor.rs` | ✅ |
| Agent 菜单 | `agents/AgentsMenu.tsx` | `agents/agents_menu.rs` | ✅ |
| 导航页脚 | `agents/AgentNavigationFooter.tsx` | `agents/agent_navigation_footer.rs` | ✅ |
| 颜色选择器 | `agents/ColorPicker.tsx` | `agents/color_picker.rs` | ✅ |
| 工具选择器 | `agents/ToolSelector.tsx` | `agents/tool_selector.rs` | ✅ |
| 模型选择器 | `agents/ModelSelector.tsx` | `agents/model_selector.rs` | ✅ |
| Agent 生成 | `agents/generateAgent.ts` | `agents/generate_agent.rs` | ✅ |
| Agent 验证 | `agents/validateAgent.ts` | `agents/validate_agent.rs` | ✅ |
| 文件工具 | `agents/agentFileUtils.ts` | `agents/agent_file_utils.rs` | ✅ |
| 类型定义 | `agents/types.ts` | `agents/types.rs` | ✅ |
| 工具函数 | `agents/utils.ts` | `agents/utils.rs` | ✅ |
| 创建向导 | `agent-settings/CreateAgentWizard.tsx` | `agents/new_agent_creation/create_agent_wizard.rs` | ✅ |
| 向导步骤 (Type) | `wizard-steps/TypeStep.tsx` | `wizard_steps/type_step.rs` | ✅ |
| 向导步骤 (Method) | `wizard-steps/MethodStep.tsx` | `wizard_steps/method_step.rs` | ✅ |
| 向导步骤 (Generate) | `wizard-steps/GenerateStep.tsx` | `wizard_steps/generate_step.rs` | ✅ |
| 向导步骤 (Description) | `wizard-steps/DescriptionStep.tsx` | `wizard_steps/description_step.rs` | ✅ |
| 向导步骤 (Prompt) | `wizard-steps/PromptStep.tsx` | `wizard_steps/prompt_step.rs` | ✅ |
| 向导步骤 (Model) | `wizard-steps/ModelStep.tsx` | `wizard_steps/model_step.rs` | ✅ |
| 向导步骤 (Color) | `wizard-steps/ColorStep.tsx` | `wizard_steps/color_step.rs` | ✅ |
| 向导步骤 (Confirm) | `wizard-steps/ConfirmStep.tsx` | `wizard_steps/confirm_step.rs` | ✅ |
| 向导步骤 (Memory) | `wizard-steps/MemoryStep.tsx` | `wizard_steps/memory_step.rs` | ✅ |
| 向导步骤 (Location) | `wizard-steps/LocationStep.tsx` | `wizard_steps/location_step.rs` | ✅ |
| 向导步骤 (Tools) | `wizard-steps/ToolsStep.tsx` | `wizard_steps/tools_step.rs` | ✅ |
| 向导框架 | `wizard/WizardProvider.tsx` | (内联到 create_agent_wizard) | ⚠️ |
| Agent 设置对话框 | `agent-settings/AgentsDialog.tsx` | (command_surface) | ✅ |
| Agent 树面板 | `AgentTreePanel.tsx` | — | ❌ |
| Coordinator 状态 | `CoordinatorAgentStatus.tsx` | — | ❌ |
| Agent 进度行 | `AgentProgressLine.tsx` | — | ❌ |
| Teammate 视图头 | `TeammateViewHeader.tsx` | — | ❌ |

### 5.2 Team 面板

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Team 面板 | `TeamPanel.tsx` | `teams/teams_dialog.rs` | ✅ |
| Team 状态 | — | `teams/team_status.rs` | ✅ |
| Team 成员卡片 | `panels/TeamMemberCard.tsx` | — | ❌ |
| Team 摘要 | `panels/team-summary.ts` | — | ❌ |

---

## 6. MCP 服务器管理

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| MCP 列表面板 | `mcp/MCPListPanel.tsx` | `mcp/mcp_list_panel.rs` | ✅ |
| MCP 工具列表 | `mcp/MCPToolListView.tsx` | `mcp/mcp_tool_list_view.rs` | ✅ |
| MCP 工具详情 | `mcp/MCPToolDetailView.tsx` | `mcp/mcp_tool_detail_view.rs` | ✅ |
| MCP Stdio 菜单 | `mcp/MCPStdioServerMenu.tsx` | `mcp/mcp_stdio_server_menu.rs` | ✅ |
| MCP Remote 菜单 | `mcp/MCPRemoteServerMenu.tsx` | `mcp/mcp_remote_server_menu.rs` | ✅ |
| MCP 能力列表 | `mcp/CapabilitiesSection.tsx` | `mcp/capabilities_section.rs` | ✅ |
| MCP 重连 | `mcp/MCPReconnect.tsx` | `mcp/mcp_reconnect.rs` | ✅ |
| MCP 重连工具 | `mcp/reconnectHelpers.ts` | `mcp/utils/reconnect_helpers.rs` | ✅ |
| MCP 类型 | `mcp/types.ts` | (内联在各模块) | ✅ |
| MCP 工具函数 | `mcp/utils.ts` | `mcp/utils/mod.rs` | ✅ |
| MCP 对话框 | `mcp/McpDialog.tsx` | `mcp/mod.rs` | ✅ |
| MCP 解析警告 | — | `mcp/mcp_parsing_warnings.rs` | ✅ |
| MCP 设置 | — | `mcp/mcp_settings.rs` | ✅ |
| MCP Elicitation | — | `mcp/elicitation_dialog.rs` | ✅ |
| MCP Agent Server | — | `mcp/mcp_agent_server_menu.rs` | ✅ |
| MCP 服务器审批 | `MCPServerApprovalDialog.tsx` | `mcp/mcp_server_approval_dialog.rs` + `/mcp approve|reject` | ✅ |
| MCP Desktop 导入 | `MCPServerDesktopImportDialog.tsx` | `mcp/mcp_server_desktop_import_dialog.rs` | ⚠️ UI surface 完成；等待后端提供 Claude Desktop 数据源 |
| MCP 服务器拷贝 | `MCPServerDialogCopy.tsx` | `mcp/mcp_server_dialog_copy.rs` | ✅ |
| MCP 多选 | `MCPServerMultiselectDialog.tsx` | `mcp/mcp_server_multiselect_dialog.rs` | ✅ |
| MCP 服务器卡片 | `panels/McpServerCard.tsx` | `mcp/mcp_server_card.rs` | ✅ |

---

## 7. Diff / 变更展示

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Diff 对话框 | `diff/DiffDialog.tsx` | `diff/diff_dialog.rs` | ✅ |
| Diff 文件列表 | `diff/DiffFileList.tsx` | `diff/diff_file_list.rs` | ✅ |
| Diff 详情视图 | `diff/DiffDetailView.tsx` | `diff/diff_detail_view.rs` | ✅ |
| 结构化 Diff hunks | `StructuredDiff/hunks.ts` | `diff/structured_diff.rs` | ✅ |
| 文件编辑 Diff | `FileEditToolDiff.tsx` | `diff/file_edit_diff.rs` + `diff/structured_diff.rs` + `permissions/file_edit_permission_request/file_edit_tool_diff.rs` | ✅ |
| 文件编辑更新消息 | `FileEditToolUpdatedMessage.tsx` | `messages/file_edit_tool_updated_message.rs` + `messages/render.rs` | ⚠️ renderer/snapshot 与 replayed tool-result path 已补齐；非 replay/live transcript 接线仍依赖 backend file-edit event data |
| Diff 内联视图 | `DiffView.tsx` | `diff/file_edit_diff.rs` + `diff/diff_detail_view.rs` | ⚠️ 共享 renderer 已补齐；未单独引入 React 式 `DiffView` 组件 |

---

## 8. 设置 / 配置 (Settings)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 设置主页 | `Settings/Settings.tsx` | `command_surface/surfaces/config.rs` | ⚠️ 简化版 |
| 设置配置 | `Settings/Config.tsx` | (同上) | ⚠️ |
| 设置状态 | `Settings/Status.tsx` | (同上) | ⚠️ |
| 设置用量 | `Settings/Usage.tsx` | — | ❌ |
| 模型选择器 | `ModelPicker.tsx` | `components/command_surface/surfaces/config.rs` | ⚠️ 已补 `/config` Model picker，支持 configured/built-in/current model、filter 与 `/config set model` |
| 主题选择器 | `ThemePicker.tsx` | `components/command_surface/surfaces/config.rs` + `rendering/theme.rs` | ⚠️ 已补 `/config` Theme picker，支持 known/custom theme 与 `/config set theme`；live preview / syntax toggle 仍待 renderer 数据流 |
| 输出风格选择 | `OutputStylePicker.tsx` | — | ❌ |
| 语言选择器 | `LanguagePicker.tsx` | — | ❌ |
| Thinking 开关 | `ThinkingToggle.tsx` | — | ❌ |
| 无效配置对话框 | `InvalidConfigDialog.tsx` | — | ❌ |
| 无效设置对话框 | `InvalidSettingsDialog.tsx` | — | ❌ |
| 托管设置安全对话框 | `ManagedSettingsSecurityDialog/` (2 files) | — | ❌ |
| Sandbox 设置 | `sandbox/SandboxSettings.tsx` | `command_surface/surfaces/sandbox.rs` | ⚠️ 简化版 |
| Sandbox 配置标签 | `sandbox/SandboxConfigTab.tsx` | — | ❌ |
| Sandbox 依赖标签 | `sandbox/SandboxDependenciesTab.tsx` | — | ❌ |
| Sandbox 覆盖标签 | `sandbox/SandboxOverridesTab.tsx` | — | ❌ |
| Sandbox Doctor | `sandbox/SandboxDoctorSection.tsx` | — | ❌ |
| Sandbox 适配器 | `sandbox/sandbox-adapter.ts` | — | ❌ |
| 成本阈值对话框 | `CostThresholdDialog.tsx` | — | ❌ |
| Token 警告 | `TokenWarning.tsx` | — | ❌ |

---

## 9. Hook 管理

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Hook 配置菜单 | `hooks/HooksConfigMenu.tsx` | `hooks/hooks_config_menu.rs` | ✅ |
| Hook 提示对话框 | `hooks/PromptDialog.tsx` | `hooks/prompt_dialog.rs` | ✅ |
| 选择事件模式 | `hooks/SelectEventMode.tsx` | `hooks/select_event_mode.rs` | ✅ |
| 选择 Hook 模式 | `hooks/SelectHookMode.tsx` | `hooks/select_hook_mode.rs` | ✅ |
| 选择匹配器模式 | `hooks/SelectMatcherMode.tsx` | `hooks/select_matcher_mode.rs` | ✅ |
| 查看 Hook 模式 | `hooks/ViewHookMode.tsx` | `hooks/view_hook_mode.rs` | ✅ |
| Hook 类型定义 | `hooks/types.ts` | (内联) | ✅ |

---

## 10. 内存 / Memory

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| Memory 文件选择器 | `memory/MemoryFileSelector.tsx` | `memory/memory_file_selector.rs` | ✅ |
| Memory 更新通知 | `memory/MemoryUpdateNotification.tsx` | `memory/memory_update_notification.rs` | ✅ |

---

## 11. 技能 (Skills)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 技能菜单 | `skills/` (空目录) | `skills/skills_menu.rs` | ✅ (Rust 更完整) |
| 技能辅助 | — | `helpers/skills_helpers.rs` | ✅ |

---

## 12. LSP 推荐

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| LSP 推荐对话框 | `LspRecommendationDialog.tsx` | `lsp_recommendation/lsp_recommendation_menu.rs` | ✅ |
| LSP 推荐菜单 | `LspRecommendation/LspRecommendationMenu.tsx` | (同上) | ✅ |
| LSP 服务器卡片 | `panels/LspServerCard.tsx` | — | ❌ |

---

## 13. 任务 (Tasks)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 任务列表面板 | `TaskListV2.tsx` | `command_surface/surfaces/tasks.rs` + `tasks/background_tasks_dialog.rs` | ✅ 已接 command surface，覆盖 tool/team 混合列表、details/stop/delete/refresh action 与 snapshot |
| 后台任务状态 | `tasks/BackgroundTaskStatus.tsx` | `tasks/background_task_status.rs` | ✅ 已有 running/failed 聚合状态与 snapshot |
| 后台任务卡片 | `tasks/BackgroundTask.tsx` | `tasks/background_task.rs` | ✅ 已显示 kind/state/elapsed/progress/summary，覆盖 snapshot |
| Shell 进度 | `tasks/ShellProgress.tsx` | `tasks/shell_progress.rs` + `rendering/progress_bar.rs` | ⚠️ 已接共享 ProgressBar，待 shell 输出细节补齐 |
| 工具活动渲染 | `tasks/renderToolActivity.tsx` | `tasks/render_tool_activity.rs` + `rendering/tool_activity.rs` | ✅ 已复用统一 `ToolActivity` 模型，覆盖状态、参数、progress、错误和输出预览 |
| 任务状态工具 | `tasks/taskStatusUtils.ts` | `tasks/task_status_utils.rs` | ✅ 已接共享 ProgressBar、kind/state label 与 elapsed formatting |
| 恢复任务选择器 | `ResumeTask.tsx` | `components/resume_picker.rs` | ✅ |

---

## 14. 欢迎页 (Welcome)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 欢迎屏幕 | `WelcomeScreen.tsx` | `components/welcome.rs` | ✅ |
| Logo V2 动画 | `LogoV2/LogoV2.tsx` + 17 files | — | ❌ (有意省略) |
| 入门引导 | `Onboarding.tsx` | — | ❌ |
| 实验登记通知 | `LogoV2/ExperimentEnrollmentNotice.tsx` | — | ❌ |
| Gate 覆盖警告 | `LogoV2/GateOverridesWarning.tsx` | — | ❌ |
| 通道通知 | `LogoV2/ChannelsNotice.tsx` | — | ❌ |

---

## 15. 设计系统 (Design System)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 对话框 | `design-system/Dialog.tsx` | (分散在各模块) | ➖ |
| 标签页 | `design-system/Tabs.tsx` | (内联 render_tabs) | ⚠️ |
| 模糊选择器 | `design-system/FuzzyPicker.tsx` | `components/fuzzy_match.rs` + `selection_surface.rs` | ⚠️ fuzzy 排序基础已补齐，完整 preview/action picker 待消费者接入 |
| 列表项 | `design-system/ListItem.tsx` | (ratatui List) | ➖ |
| 加载状态 | `design-system/LoadingState.tsx` | `rendering/spinner.rs` | ✅ |
| 进度条 | `design-system/ProgressBar.tsx` | `rendering/progress_bar.rs` | ✅ 纯文本 1/8 block 渲染 |
| 窗格 | `design-system/Pane.tsx` | (ratatui Block) | ➖ |
| 分割线 | `design-system/Divider.tsx` | (ratatui 原生) | ➖ |
| 状态图标 | `design-system/StatusIcon.tsx` | — | ❌ |
| 快捷键提示 | `design-system/KeyboardShortcutHint.tsx` | — | ❌ |
| 主题盒子 | `design-system/ThemedBox.tsx` | (ratatui 原生) | ➖ |
| 主题文本 | `design-system/ThemedText.tsx` | (ratatui Span) | ➖ |
| 主题提供者 | `design-system/ThemeProvider.tsx` | `rendering/theme.rs` | ✅ |
| Ratchet | `design-system/Ratchet.tsx` | — | ❌ |
| Byline | `design-system/Byline.tsx` | — | ❌ |
| 颜色工具 | `design-system/color.ts` | `rendering/theme.rs` | ✅ |
| 自定义选择组件 | `customselect/` (10 files) | `components/selection_surface.rs` | ⚠️ 简化版 |
| 标签页组件 | `TagTabs.tsx` | (内联 render_tabs) | ⚠️ |

---

## 16. 状态行 (Status Line)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 状态行渲染 | `StatusLine.tsx` | `status/status_line_resolver.rs` | ✅ |
| 内置状态行 | `BuiltinStatusLine.tsx` | (cc_engine::status_line) | ✅ |
| 自定义状态行 | `StatusLine.tsx` custom command flow | `cc_engine::status_line::{payload,runner}` + `commands/statusline_cmd.rs` + `app/render.rs` | ✅ `/statusline` command/payload/runner 已接入 |
| 状态行状态机 | `StatusLine.tsx` debounce/cancel flow | `cc_engine::status_line::runner.rs` | ✅ throttled refresh、payload fingerprint、in-flight abort、fallback error marker |
| 状态通知 | `StatusNotices.tsx` | `components/status_widget.rs` | ✅ |
| 子系统状态 | `SubsystemStatus.tsx` (current upstream file absent; legacy row) | `components/status_widget.rs` + `runtime/capability_contract.rs` | ✅ status widget 可显示 subsystem ok/attention 与可选 indicator |
| IDE 状态指示器 | `IdeStatusIndicator.tsx` | `components/status_widget.rs` optional indicator | ✅ 有数据时渲染，不在本步引入 IDE 后端 |
| 内存用量指示器 | `MemoryUsageIndicator.tsx` | `components/status_widget.rs` optional warning/error indicator | ✅ 有数据时渲染，absent 状态有 snapshot |
| PR 徽章 | `PrBadge.tsx` | `components/status_widget.rs` optional indicator | ⚠️ 可渲染 PR indicator，live PR 数据流未接 |
| 精力指示器 | `EffortIndicator.ts` + `EffortCallout.tsx` | `app/render.rs` + `components/status_widget.rs` | ✅ fallback footer 同步 `effortLevel`/runtime effort |
| 开发栏 | `DevBar.tsx` | — | ⚠️ dev-only chrome，暂不作为 ratatui P1 阻塞 |

---

## 17. 对话框 / 模态窗口 (未归类)

### 17.1 远程 / Bridge

| TS 组件 | 状态 | 说明 |
|---------|------|------|
| `BridgeDialog.tsx` | ❌ | 远程桥接对话框 |
| `RemoteCallout.tsx` | ❌ | 远程功能提示 |
| `RemoteEnvironmentDialog.tsx` | ❌ | 远程环境选择 |
| `TeleportError.tsx` | ❌ | Teleport 错误 |
| `TeleportProgress.tsx` | ❌ | Teleport 进度 |
| `TeleportRepoMismatchDialog.tsx` | ❌ | Teleport 仓库不匹配 |
| `TeleportResumeWrapper.tsx` | ❌ | Teleport 恢复包装 |
| `TeleportStash.tsx` | ❌ | Teleport 暂存 |
| `ClaudeInChromeOnboarding.tsx` | ❌ | Chrome 集成入门 |

### 17.2 IDE 集成

| TS 组件 | 状态 | 说明 |
|---------|------|------|
| `IdeAutoConnectDialog.tsx` | ❌ | IDE 自动连接 |
| `IdeOnboardingDialog.tsx` | ❌ | IDE 入门引导 |
| `ShowInIDEPrompt.tsx` | ❌ | 在 IDE 中展示 |

### 17.3 会话管理

| TS 组件 | 状态 | 说明 |
|---------|------|------|
| `ExportDialog.tsx` | ❌ | 会话导出 |
| `ExitFlow.tsx` | ❌ | 退出确认流程 |
| `SessionBackgroundHint.tsx` | ❌ | 后台会话提示 |
| `SessionPreview.tsx` | ❌ | 会话预览 |
| `IdleReturnDialog.tsx` | ❌ | 空闲返回 |
| `ResumeTask.tsx` | ✅ | 已有 `resume_picker.rs` |

### 17.4 搜索 / 导航

| TS 组件 | 状态 | 说明 |
|---------|------|------|
| `GlobalSearchDialog.tsx` | ❌ | 全局搜索 |
| `HistorySearchDialog.tsx` | ⚠️ | `components/history_search_dialog.rs` 已补齐 Ctrl+R in-session 历史搜索；持久历史读取 API 仍缺 |
| `QuickOpenDialog.tsx` | ❌ | 快速打开 (Ctrl+P) |
| `LogSelector.tsx` | ❌ | 日志选择器 |
| `ContextVisualization.tsx` | ❌ | 上下文可视化 |

### 17.5 其他对话框

| TS 组件 | 状态 | 说明 |
|---------|------|------|
| `ApproveApiKey.tsx` | ❌ | API Key 审批 |
| `ConsoleOAuthFlow.tsx` | ❌ | 控制台 OAuth 流程 |
| `ContextSuggestions.tsx` | ❌ | 上下文建议 |
| `DevChannelsDialog.tsx` | ❌ | 开发通道 |
| `ChannelDowngradeDialog.tsx` | ❌ | 通道降级 |
| `ClaudeMdExternalIncludesDialog.tsx` | ❌ | CLAUDE.md 外部包含 |
| `KeybindingWarnings.tsx` | ❌ | 快捷键冲突警告 |
| `NativeAutoUpdater.tsx` | ❌ | 原生自动更新 |
| `PackageManagerAutoUpdater.tsx` | ❌ | 包管理器自动更新 |
| `SandboxViolationExpandedView.tsx` | ❌ | Sandbox 违规详情 |
| `SkillImprovementSurvey.tsx` | ❌ | 技能改进调查 |
| `Stats.tsx` | ❌ | 统计信息 |
| `UndercoverAutoCallout.tsx` | ❌ | Undercover 模式提示 |
| `BashModeProgress.tsx` | ❌ | Bash 模式进度 |
| `DiagnosticsDisplay.tsx` | ❌ | 诊断显示 |
| `ClickableImageRef.tsx` | ❌ | 可点击图片引用 |
| `OffscreenFreeze.tsx` | ❌ | 离屏冻结 (性能) |
| `ToolUseLoader.tsx` | ❌ | 工具加载动画 |
| `FallbackToolUseErrorMessage.tsx` | ❌ | 工具错误回退 |
| `FallbackToolUseRejectedMessage.tsx` | ❌ | 工具拒绝回退 |
| `NotebookEditToolUseRejectedMessage.tsx` | ❌ | Notebook 编辑拒绝 |
| `FileEditToolUseRejectedMessage.tsx` | ❌ | 文件编辑拒绝 |
| `CompactSummary.tsx` | ❌ | 压缩摘要 |
| `OrderedList.tsx` | ❌ | 有序列表渲染 |
| `PressEnterToContinue.tsx` | ❌ | 按回车继续 |
| `ValidationErrorsList.tsx` | ❌ | 验证错误列表 |
| `ServerListEditor.tsx` | ❌ | 服务器列表编辑器 |
| `SentryErrorBoundary.ts` | ➖ | 纯 React 概念 |
| `opentui-syntax.ts` | ➖ | OpenTUI 特定语法 |

---

## 18. 平台 / 终端 (Platform)

| 功能 | Rust 端 | 状态 |
|------|---------|------|
| 终端环境检测 | `platform/terminal_env.rs` | ✅ |
| 终端集成 | `platform/terminal_integration.rs` | ✅ |
| 自定义终端 | `platform/custom_terminal.rs` | ✅ |
| 浏览器启动 | `platform/browser.rs` | ✅ |
| 音频设备 | `platform/audio_device.rs` | ✅ |
| 调试配置 | `platform/debug_config.rs` | ✅ |
| 通知 (BEL) | `notifications/bel.rs` | ✅ |
| 通知 (OSC 9) | `notifications/osc9.rs` | ✅ |

---

## 19. 帮助系统 (Help)

| 功能 | TS 端 | Rust 端 | 状态 |
|------|-------|---------|------|
| 帮助 V2 主页 | `helpv2/HelpV2.tsx` | (command_palette) | ⚠️ |
| 命令帮助 | `helpv2/Commands.tsx` | `command_palette/` | ⚠️ |
| 通用帮助 | `helpv2/General.tsx` | — | ❌ |

---

## 20. 渲染层 (Rendering)

| 功能 | Rust 端 | 状态 |
|------|---------|------|
| Markdown 渲染 | `rendering/markdown.rs` | ✅ |
| Markdown 流式渲染 | `rendering/markdown_stream.rs` + `tui/engine_events.rs` | ✅ direct TUI 已保留 text/thinking/tool_use content blocks |
| Markdown 渲染核心 | `rendering/markdown_render.rs` | ✅ |
| 虚拟滚动 | `rendering/virtual_scroll.rs` | ✅ |
| 主题系统 | `rendering/theme.rs` | ✅ |
| 动画微光 | `rendering/shimmer.rs` | ✅ |
| 旋转器 | `rendering/spinner.rs` | ✅ |
| Git Diff 获取 | `rendering/get_git_diff.rs` | ✅ |
| 工具活动渲染 | `rendering/tool_activity.rs` | ✅ 已补齐 user-facing 名称、参数摘要、状态、elapsed、progress、错误摘要与输出预览 |
| 历史单元格 | `rendering/history_cell.rs` | ✅ |
| Snapshot 审阅导出 | `runtime/snapshot_export.rs` + `scripts/export-ui-snapshots.ps1` | ⚠️ 可导出 accepted insta snapshots 到平铺目录；当前脚本/导出模块需确认是否纳入 git 跟踪 |

---

## 汇总统计

### 按状态分类

| 状态 | 数量 | 说明 |
|------|:----:|------|
| ✅ 完整 | ~140 | 功能对齐或 Rust 侧更完整 |
| ⚠️ 部分 | ~25 | 有基础实现但细节不足 |
| ❌ 缺失 | ~80 | Rust 侧无对应实现 |
| ➖ 不适用 | ~15 | 纯前端抽象/有意省略 |

### 按优先级分类的缺失项

#### P0 — 影响核心体验（2026-05-04 milestone gate）

| 项目 | 里程碑状态 |
|------|------------|
| Shell 输出展开/格式化 | 基础完成：已补齐 expanded option、ANSI/JSON/宽度截断、elapsed/timeout footer。残余：最新 shell 输出自动展开上下文仍待 runtime/event 接线，见 `development/archive/KNOWN_ISSUES.md` `UI-002` |
| 结构化 Diff (hunks) | 完成：已补齐 `diff/structured_diff.rs` 与 `diff/file_edit_diff.rs`，支持 unified diff hunk 解析、old/new gutter、multi-hunk 分隔、no-newline/large/truncated snapshot 覆盖，并复用于文件编辑 permission/update surfaces |
| 搜索框 (`SearchBox`) | 完成：已补齐共享文本渲染 primitive，并接入 `SelectionSurface` 头部 |
| 历史搜索 (`HistorySearchDialog`) | 基础完成：已补齐 Ctrl+R in-session 历史搜索、SearchBox、exact-first/fuzzy-second 过滤、窄/宽预览、空态与 key handling。残余：Rust 端暂无持久 timestamped history reader，当前从本次会话 `push_history` 条目生成时间戳，见 `development/archive/KNOWN_ISSUES.md` `UI-003` |
| 进度条 (`ProgressBar`) | 完成：已补齐共享 1/8 block 渲染，并接入任务/shell surface |
| Tool 活动渲染完善 | 完成：已补齐统一 `ToolActivity` 模型与 grouped/task/message 复用，覆盖 queued/running/succeeded/failed/cancelled、参数摘要、progress、错误和输出预览 snapshot；direct TUI 已通过 tool progress callback 将长任务进度写入 `Message::Progress`，同一 `tool_use_id` 会更新上一条进度消息 |

P0 milestone residual risks:

- 最新 shell 输出尚未根据实时 shell 上下文自动展开；当前 renderer 已支持展开/折叠和完整 detail view，但自动策略等待事件接线。
- Ctrl+R 历史搜索当前只覆盖本次 TUI 会话内提交的 prompt；跨会话持久历史需要后续 reader/API。
- 文件编辑成功/拒绝/取消 render surface 已补齐；direct TUI replay path 已可从 `tool_use_result kind=file_edit` 渲染 hunk preview；非 replay/live transcript 结构化事件仍依赖后端提供 file-edit event 数据流，不阻塞 P0 hunk renderer 基础。

#### P1 — 影响功能完整性

| 缺失项 | 说明 |
|--------|------|
| 设置页完善 (ModelPicker, ThemePicker 等) | 已补 `/config` Model/Theme/Effort picker 基础，复用 `SelectionSurface` 与 `/config set` 持久化；standalone picker、live theme preview、syntax toggle 仍待后续增强 |
| 任务面板完善 (BackgroundTask, ShellProgress) | 已完成集成复核：`/tasks` command surface 汇总 tool/team task，列表显示 kind/state/elapsed/progress/summary，并覆盖 tool/team action snapshot；shell 最新输出自动展开仍按 P0 residual #21 跟踪 |
| 状态行增强 | 已接 `/statusline` custom command runner/payload；fallback footer 同步 permission/sandbox/effort；`StatusSnapshot` 支持 subsystem/IDE/memory/PR 等 optional indicators 并覆盖 present/absent snapshot；live IDE/PR 后端数据不在本步强行引入 |
| 文件编辑 diff 完善 | 已补齐 shared file-edit diff preview、permission hunk 展开、updated/rejected/canceled render snapshots，并补齐 direct TUI replayed tool-result preview；非 replay/live transcript 接线取决于 backend file-edit event data |
| 模糊选择器 (`FuzzyPicker`) | fuzzy scorer 与 SelectionSurface/command palette 排序已补齐；完整 preview/action picker 仍待后续步骤 |
| MCP 审批/导入对话框 | 审批、拷贝、多选、服务器卡片已补齐；Desktop 导入 UI surface 已补齐，自动发现 Claude Desktop 配置仍待后端数据源 |

#### P2 — 平台/集成功能

| 缺失项 | 说明 |
|--------|------|
| IDE 集成组件 (3 files) | 视 IDE 集成路线决定 |
| 远程/Bridge (9 files) | 视多端集成路线决定 |
| Teleport (5 files) | 视远程功能路线决定 |
| 自动更新 (2 files) | Windows 更新策略待定 |

#### P3 — 辅助/装饰功能

| 缺失项 | 说明 |
|--------|------|
| LogoV2/欢迎动画 (18 files) | 纯装饰，ratatui 无动画 |
| 入门引导 (`Onboarding`) | 首次体验优化 |
| 设计系统基类 (Dialog, Pane 等) | ratatui 有原生替代 |
| 自定义选择组件 (10 files) | `selection_surface.rs` 部分覆盖 |
| 其他对话框 (~20 files) | 按需补齐 |

### Rust 侧更强的地方

- **消息类型覆盖**: 32 种 vs TS 11 种，含完整 snapshot 测试
- **权限系统**: 25 个子模块，从上游 React 组件映射更彻底
- **平台层**: 独立抽象 (audio, browser, terminal, notifications)
- **渲染层**: markdown/markdown_stream/markdown_render 三层分离
- **Command Surface**: 12 个模态面，统一的路由架构
- **Agent 向导**: 与 TS 端 1:1 对齐，包含所有 11 个步骤

---

## 下一步建议

1. **立即**: 按 [`development/archive/plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md`](plan/ratatui-ui-parity-untracked-gap-plan-2026-05-08.md) 给所有 `⚠️ 部分` / `❌ 缺失` 条目补 tracking owner，并统一 `WORK_STATUS` / `IMPLEMENTATION_GAPS` / archive 记录
2. **短期**: 将 fuzzy/search foundation 继续复用到 Agent 选择面
3. **中期**: Claude Desktop MCP 配置自动发现、live file-edit transcript event 接线
4. **长期**: IDE 集成、远程功能 (视路线图)
5. **不追**: LogoV2 动画、纯 React 抽象 (SentryErrorBoundary)、设计系统基类

---

## Ratatui UI parity OMX closeout (2026-05-10)

Final verification for `target/codex-runs/ratatui-ui-parity-omx` read all available batch summary files and task last-message files. The run confirms these parity effects are now represented in the working tree:

- Shared UI primitives: reusable tab, status-icon, and keyboard-shortcut hint renderers; `SelectionSurface` preview/action/disabled metadata support.
- Settings/safety surfaces: richer `/config` tabs, `/sandbox` diagnostics, and command-surface coverage for permission safety modes.
- Message/composer surfaces: selected-message affordances, clearer tool-result fallback/reject/error rendering, prompt mode/placeholder/large-paste/truncation preview, and command hint updates.
- Agent/team/task/search/integration surfaces: `/agents` list/detail mode, team/task summary/detail refinements, persistent-history reader wiring where backend data exists, LSP/IDE/Chrome/channel command-surface status, and updated snapshot coverage.

Verification status:

- PASS: `git diff --check`.
- PASS: focused UI snapshot/filter tests for `command_surface`, `selection_surface`, `status_widget`, `config_cmd`, `sandbox`, `permissions`, `messages`, `prompt_input`, `history_search`, `command_palette`, `agents`, `teams`, `resume`, `session_export`, `ide`, `lsp_recommendation`, `channels`, `hooks`, and `visual_regression` after accepting intentional snapshot updates.
- WARNING: package-wide `cargo test -p claude-code-rs -- --nocapture` is not green in this workspace; see `KNOWN_ISSUES.md` TEST-002 for the current failure list and follow-up.
