# UI 差异分析：权限系统、对话框/叠加层 UI 与输入控件

> **目的**：比较 TypeScript（claude-code-bun）和 Rust 代码库在权限/审批系统、对话框/叠加层 UI 和输入控件方面的差异。针对每个领域，本文档评估 Rust 的完成度并列出了具体的缺失功能及其对用户的影响。

---

## 汇总表

| 领域 | Rust 完成度 | 关键差距 |
|------|:-:|---------|
| 权限系统（模块覆盖） | 4/5 | 所有模块都存在，但大部分为死代码 |
| 权限对话框集成 | 2/5 | 单一基础叠加层，无按工具区分的变体 |
| 对话框 / 叠加层系统 | 1/5 | 仅有权限叠加层；无通用对话框基础设施 |
| 输入控件（PromptInput） | 2/5 | 仅单行，不支持多行，无高亮 |
| Vim 模式 | 3/5 | 状态机较好，但未与 UI 集成 |
| 聊天编辑器 | 3/5 | 仅有基础状态；无渲染集成 |
| 命令面板 | 4/5 | 实现扎实，支持模糊过滤、编辑目标 |
| 选择界面 | 3/5 | 通用选择器，无标签页支持，结构有限 |
| 事件路由 | 2/5 | 简单枚举匹配，无优先级/和弦处理 |
| 按键绑定系统 | 3/5 | 支持上下文感知，但无和弦绑定，无用户配置 |
| **平均** | **2.7/5** | |

---

## 领域：权限系统 — 模块覆盖

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/permissions/` — 子目录中 60+ 个文件
- 所有子模块：`ask_user_question_permission_request/`、`bash_permission_request/`、
  `computer_use_approval/`、`enter_plan_mode_permission_request/`、
  `exit_plan_mode_permission_request/`、`fallback_permission_request.rs`、
  `file_edit_permission_request/`、`file_permission_dialog/`、
  `file_write_permission_request/`、`filesystem_permission_request/`、
  `monitor_permission_request/`、`notebook_edit_permission_request/`、
  `power_shell_permission_request/`、`review_artifact_permission_request/`、
  `sandbox_permission_request.rs`、`sed_edit_permission_request/`、
  `skill_permission_request/`、`web_fetch_permission_request/`
- `permissions/dialog_overlay.rs` — `PermissionDialog` 结构体，包含 `PermissionChoice`
- `permissions/utils.rs` — `PermissionRequestView`、`PermissionOption`、`PermissionDecision`、`PermissionScope`
- `permissions/permission_request.rs` — `basic_permission_request`、`render_permission_request_surface`
- `permissions/permission_prompt.rs` — `PermissionPromptState`
- `permissions/permission_dialog.rs` — `render_permission_dialog_summary`
- `permissions/permission_explanation.rs`、`permission_request_title.rs`、`permission_rule_explanation.rs`、
  `permission_decision_debug_info.rs`
- `permissions/hooks.rs` — `PermissionHookEvent`
- `permissions/worker_badge.rs`、`worker_pending_permission.rs`
- `permissions/shell_permission_helpers.rs`、`use_shell_permission_feedback.rs`
- `permissions/rules/` — `AddPermissionRules`、`PermissionRuleList`、`PermissionRuleInput`、
  `PermissionRuleDescription`、`add_workspace_directory`、`remove_workspace_directory`、
  `recent_denials_tab`、`workspace_tab`
- `components/approval_overlay.rs` — `ApprovalOverlay`，包含 `ApprovalKind` 和 `ApprovalChoice`

### TS 文件
- `claude-code-bun/src/components/permissions/` — 20+ 个组件：
  - `PermissionDialog.tsx` — 对话框框架包装器
  - `PermissionRequest.tsx` — 路由到按工具区分的组件
  - `PermissionPrompt.tsx` — 共享提示，带可选的反馈输入
  - `PermissionRequestTitle.tsx`、`PermissionExplanation.tsx`
  - `PermissionDecisionDebugInfo.tsx`、`PermissionRuleExplanation.tsx`
  - `BypassPermissionsModeDialog.tsx`
  - 按工具区分：`BashPermissionRequest/`、`PowerShellPermissionRequest/`、
    `FileEditPermissionRequest/`、`FileWritePermissionRequest/`、
    `FilesystemPermissionRequest/`、`NotebookEditPermissionRequest/`、
    `SedEditPermissionRequest/`、`SkillPermissionRequest/`、
    `WebFetchPermissionRequest/`、`MonitorPermissionRequest/`、
    `SandboxPermissionRequest.rs`、`EnterPlanModePermissionRequest/`、
    `ExitPlanModePermissionRequest/`、`AskUserQuestionPermissionRequest/`、
    `ReviewArtifactPermissionRequest/`、`FallbackPermissionRequest.tsx`
  - `useShellPermissionFeedback.ts`、`shellPermissionHelpers.tsx`
  - `WorkerBadge.tsx`、`WorkerPendingPermission.tsx`
  - 规则：`AddPermissionRules/`、`PermissionRuleList/`、`PermissionRuleInput/`、
    `PermissionRuleDescription.tsx`
  - `FilePermissionDialog/` — 包含 `ideDiffConfig`、`permissionOptions`、
    `useFilePermissionDialog`、`usePermissionHandler`
- `claude-code-bun/src/utils/permissions/` — 类型、schema、模式、管道中继
- `claude-code-bun/src/hooks/toolPermission/` — `PermissionContext.ts`
- `claude-code-bun/src/bridge/bridgePermissionCallbacks.ts`
- `src/components/permissions/` — 额外的非 bun 权限组件
- `src/utils/permissions/` — `permissionExplainer.ts`、`permissions.ts`、`permissionSetup.ts`

### Rust 完成度：4/5

所有 TS 权限模块类型都有对应的 Rust 模块。镜像结构完整：每个按工具区分的权限请求、每个规则面板、每个工具函数都有对应的 Rust 实现。`permissions.rs` 模块索引列出了 27 个子模块，全部标记为 `#[allow(dead_code)]`。

### 当前状态

Rust 权限模块结构几乎完全同步了 TS 权限组件树。每个 TS 工具特定的权限对话框在测试套件中都有对应的 Rust 渲染函数（带 `insta` 快照）。`dialog_overlay.rs` 提供了一个可用的交互式叠加层。`approval_overlay.rs` 提供了一个基于 `BetterViewPanel` 的 ApprovalOverlay。

然而，几乎每个模块都标记了 `#[allow(dead_code)]`，这意味着这些渲染函数存在但可能仅在测试中被调用，或者未连接到事件循环。

### 相对于 TS 的缺失功能
- 权限反馈输入（Tab 展开反馈文本字段）
- IDE diff 集成配置（`ideDiffConfig`）
- 绕过权限模式对话框（`BypassPermissionsModeDialog`）
- 完整的交互式集成 — 大多数模块仅通过快照测试，未连接到实时的权限流程
- 权限决策的分析追踪
- 管道/桥接权限中继

### 影响
权限提示会在叠加层中渲染，但缺少 TS 的精细度和交互深度（反馈、配置、分析）。对于一般使用场景，基本的允许/拒绝可以工作，但高级用户会失去向 Claude 提供反馈的能力以及 IDE 集成流程。

---

## 领域：权限对话框 — 交互集成

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs` — 单一统一的
  `PermissionDialog` 控件，支持允许/拒绝/始终允许、左/右箭头导航、
  键盘快捷键（y/n/a）、居中的叠加层渲染（使用 ratatui `Clear` 控件）
- `rust/crates/claude-code-rs/src/ui/components/approval_overlay.rs` — `ApprovalOverlay`，
  使用 `BetterViewPanel` 渲染，包含类型化的 `ApprovalKind` 变体

### TS 文件
- `claude-code-bun/src/components/permissions/PermissionDialog.tsx` — 对话框框架，带
  颜色/边框/工作线程徽章
- `claude-code-bun/src/components/permissions/PermissionPrompt.tsx` — 共享提示，支持
  Tab 展开反馈、`Select` 集成、分析钩子
- `claude-code-bun/src/components/permissions/PermissionRequest.tsx` — 将工具特定的
  请求路由到专用组件
- 15+ 个按工具区分的权限请求组件（如上所列）
- `claude-code-bun/src/components/permissions/FilePermissionDialog/` — 复杂的文件
  权限对话框，带 diff 预览、IDE 配置、保存规则集成
- `claude-code-bun/src/hooks/toolPermission/PermissionContext.ts` — 权限流程的
  React 上下文

### Rust 完成度：2/5

### 当前状态

Rust 有两个权限对话框实现：
1. `PermissionDialog` — 独立的 ratatui 控件，内联渲染，3 个固定按钮，
   以及基本的左/右键盘导航。使用 `Clear` 渲染为居中叠加层。
   支持 `ApprovalKind` 分类（Bash、FileEdit、WebFetch、MCP、UserInput、
   Fallback）。
2. `ApprovalOverlay` — 使用 `BetterViewPanel` 布局，渲染为带样式的行。
   支持 `ApprovalChoice`（AllowOnce、AllowAlways、Deny、EditRequest）。

两者都可以工作但都是整体式的 — 没有像 TS 的 `BashPermissionRequest`（显示命令选项）
或 `FileEditPermissionRequest`（显示内联 diff）那样的按工具定制。

### 相对于 TS 的缺失功能
- 按工具区分的权限请求组件（单个统一对话框处理所有工具）
- 文件编辑权限中的内联 diff/补丁预览
- 反馈输入（Tab 展开文本字段，用于"告诉 Claude 如何做得不同"）
- 权限决策的分析集成
- 权限提示提示系统（Tab 提示、特定选项的按键绑定）
- IDE diff 配置显示
- 带提交/导航的多选题系统（AskUserQuestion）
- 带风险指示器的 Shell 特定权限详情
- Plan 模式进入/退出专用视图

### 影响
权限用户体验可工作但单一。用户在 bash、文件编辑和 web 抓取中看到相同的对话框，
失去了每个工具所需的专门上下文。缺少内联 diff 使文件编辑审批更难评估。

---

## 领域：对话框 / 叠加层系统

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs` — 仅有权限叠加层
  （Clear + 块居中在区域内）
- 不存在通用对话框系统

### TS 文件
- `claude-code-bun/src/components/design-system/Dialog.tsx` — 从 `@anthropic/ink`
  重新导出 `Dialog`
- `claude-code-bun/src/components/design-system/SetupDialog.tsx` — 通用 `SetupDialog`
  组件（标题、主体、关闭、z 顺序，内容通过 `children` 传入），包含
  `SetupDialogLauncher` 和 `useSetupDialog` 钩子
- `claude-code-bun/src/dialogLaunchers.tsx` — 7 个异步对话框启动器，使用
  `showSetupDialog` + 动态导入：`launchSnapshotUpdateDialog`、
  `launchInvalidSettingsDialog`、`launchAssistantSessionChooser`、
  `launchAssistantInstallWizard`、`launchTeleportResumeWrapper`、
  `launchTeleportRepoMismatchDialog`、`launchResumeChooser`
- 40+ 个对话框组件位于 `claude-code-bun/src/components/`：
  - 系统：`BypassPermissionsModeDialog`、`InvalidSettingsDialog`、
    `InvalidConfigDialog`、`AutoModeOptInDialog`
  - MCP：`MCPServerApprovalDialog`、`MCPServerDesktopImportDialog`、
    `MCPServerDialogCopy`、`MCPServerMultiselectDialog`
  - 搜索：`QuickOpenDialog`、`GlobalSearchDialog`、`HistorySearchDialog`
  - 任务：`BackgroundTasksDialog`、`AsyncAgentDetailDialog`、
    `InProcessTeammateDetailDialog`、`ShellDetailDialog`、`WorkflowDetailDialog`、
    `DreamDetailDialog`、`MonitorMcpDetailDialog`、`RemoteSessionDetailDialog`
  - 其他：`TrustDialog`、`BridgeDialog`、`ExportDialog`、`TeamsDialog`、
    `DiffDialog`、`PromptDialog`、`ElicitationDialog`、
    `IdleReturnDialog`、`CostThresholdDialog`、`WorktreeExitDialog`、
    `ChannelDowngradeDialog`、`TeleportRepoMismatchDialog`、
    `RemoteEnvironmentDialog`、`WorkflowMultiselectDialog`
  - 向导：`WizardDialogLayout`
  - 代理：`SnapshotUpdateDialog`
  - 计划：`UltraplanChoiceDialog`、`UltraplanLaunchDialog`
- `claude-code-bun/src/context/overlayContext.ts` — 模态叠加层状态管理

### Rust 完成度：1/5

### 当前状态

Rust 只有一个叠加层：`dialog_overlay.rs` 中的权限对话框。它使用 ratatui `Clear` + `Block` + `Paragraph` 渲染一个居中的带边框方框。没有通用的对话框框架，没有对话框生命周期管理，没有堆叠叠加层，没有模态状态追踪。`components/approval_overlay.rs` 中的 `ApprovalOverlay` 是一个基于行的渲染器（用于 `BetterViewPanel`），不是交互式叠加层。

TS 有 40+ 个对话框组件，涵盖系统配置、任务管理、MCP 设置、搜索、入门引导、向导和传送。`dialogLaunchers.tsx` 展示了一个一致的模式：`showSetupDialog(root, done => <Component onComplete={done}/>)`。

### 相对于 TS 的缺失功能
- 通用对话框基础设施（标题、主体、关闭、z 顺序）
- 对话框生命周期管理（打开/关闭/取消、结果回调）
- 堆叠/模态叠加层支持
- 向导对话框布局模式
- QuickOpen、GlobalSearch、HistorySearch 对话框
- MCP 服务器审批/导入/配置对话框
- 任务详情对话框（后台任务、代理详情、shell 详情）
- 信任对话框、桥接对话框、导出对话框
- 成本阈值、空闲返回、渠道降级、环境选择
- 工作树退出确认对话框
- `dialogLaunchers.tsx` 中的所有对话框启动器

### 影响
重大的用户体验差距。用户无法通过对话框访问系统配置（信任、MCP、设置、任务）。整个非权限对话框表面都不存在。这是 Rust UI 中最大的单一差距。

---

## 领域：输入控件（PromptInput）

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/prompt_input.rs` — 单行输入控件，
  支持光标、粘贴、占位符、提示、模式指示器
- `rust/crates/claude-code-rs/src/ui/components/chat_composer.rs` — 编辑器状态
  （String 输入、模式枚举、队列、状态提示、基础渲染）

### TS 文件
- `claude-code-bun/src/components/PromptInput/PromptInput.tsx` — 约 3000 行，主要输入组件
- `claude-code-bun/src/components/PromptInput/PromptInputFooter.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputFooterSuggestions.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputFooterLeftSide.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputModeIndicator.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputQueuedCommands.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputStashNotice.tsx`
- `claude-code-bun/src/components/PromptInput/PromptInputHelpMenu.tsx`
- `claude-code-bun/src/components/PromptInput/usePromptInputPlaceholder.ts`
- `claude-code-bun/src/components/BaseTextInput.tsx` — 基础多行输入组件
- `claude-code-bun/src/components/VimTextInput.tsx`
- `claude-code-bun/src/components/TextInput.tsx`

### Rust 完成度：2/5

> **补齐计划**: 见 `docs/ui/great/plans/plan-03-input-editor.md`（6 项缺失功能，共 ~2000 行估算工作量）
>
> 计划分 3 阶段：
> - Phase 1（独立、无阻塞）：撤销/重做（#3）+ 图片粘贴接线（#4）
> - Phase 2（渲染核心改造）：多行输入（#1）+ 文本高亮（#2）
> - Phase 3（集成功能）：输入建议（#5）+ ChatComposer Widget（#6）

### 当前状态

Rust 的 `PromptInput` 是一个简洁的单行文本输入（397 行），支持：
- 在光标处插入/删除字符，正确处理 UTF-8 边界
- 箭头键导航，Home/End、Backspace、Delete
- Ctrl 快捷键：U（清除）、A（行首）、E（行尾）、W（删除单词）、K（删除到行尾）
- 粘贴处理，大粘贴内容截断提示
- 提示文本和模式指示器渲染
- 光标移出屏幕时水平滚动

`ChatComposerState`（126 行）是一个纯状态结构体，包含：
- 输入文本、模式（Insert/VimNormal/Slash/Busy）、队列
- 提交或排队行为（忙时排队）
- 基本状态提示字符串
- 简单的 `render_lines()` — 用 `\n` 显示替换换行符

TS PromptInput 约 3000 行，支持：
- 多行输入，带文本换行视口
- 图片粘贴（[Image #N] 药丸引用，自动存储）
- 文本粘贴（大粘贴引用 vs 内联）
- 撤销缓冲区（50 条记录，1 秒去抖）
- 文本高亮（ultrathink/ultraplan 的彩虹色、@提及、/命令、token 预算）
- 内联虚影文本（输入建议）
- 提示建议集成（推测接受）
- 历史搜索（Ctrl+R）
- 暂存/取消暂存（Ctrl+S）
- 外部编辑器（$EDITOR 集成）
- 模式循环（权限模式）
- 底部药丸导航（任务、团队、桥接、tmux）
- 坐标感知点击定位光标
- 语音听写临时范围显示
- 输入过滤器（药丸后延迟空格）
- 伴生精灵集成
- 更多

### 相对于 TS 的缺失功能
- 多行输入（Rust 仅支持单行）
- 文本高亮（彩虹色、@提及、命令、token 预算）
- 图片粘贴，带 [Image #N] 药丸引用
- 文本粘贴，带大粘贴引用
- 撤销缓冲区（编辑历史）
- 提示建议集成
- 历史搜索（Ctrl+R）
- 暂存/取消暂存（Ctrl+S）
- 外部编辑器集成
- 内联虚影文本 / 输入建议
- 底部导航（任务、团队、桥接）
- 点击定位光标
- 语音听写显示
- 协调器/任务集成

### 影响
关键的用户体验差距。Rust 输入对于单行提示可以工作，但无法处理多行编辑、图片附件或用户依赖的丰富建议/撤销/高亮系统。

---

## 领域：Vim 模式

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/input/vim.rs` — 916 行，完整的 vim 状态机，
  包含 Normal/Insert/Visual 模式、操作符待定、重复计数、寄存器、单词移动

### TS 文件
- `claude-code-bun/src/components/VimTextInput.tsx` — 独立的 vim 输入组件
- `claude-code-bun/src/types/textInputTypes.ts` — `VimMode` 类型
- `claude-code-bun/src/components/PromptInput/utils.ts` — `isVimModeEnabled()`

### Rust 完成度：3/5

### 当前状态

Rust vim 实现结构良好，包含：
- `VimMode`（Normal、Insert、Visual）带模式指示器
- `EditorModeSetting`（Normal/Vim）带配置解析
- `VimAction` 枚举涵盖所有操作（None、InsertChar、Delete、MoveCursor、Yank、
  Paste、DeleteLine、YankLine、Submit、SwitchMode、Undo、Passthrough）
- `VimState` 包含待定操作符、重复计数、寄存器、可视锚点
- 普通模式：hjkl、w/b/e 单词移动、0/$、dd/yy/cc、x/X、p、u、I、A、D、C
- 可视模式：在选区上进行导航、d/y/c 操作
- 插入模式：Esc 返回普通模式，其余透传
- 重复计数（例如 3w = 向前 3 个单词）
- 全面的测试套件（40+ 个测试）

### 相对于 TS 的缺失功能
- 未连接到实际的 PromptInput（Rust 的 PromptInput 没有与 VimState 整合 —
  vim 作为独立状态机存在）
- PromptInput 渲染中无模式指示器（Rust PromptInput 有 mode_indicator 槽位，
  但 vim 未填充它）
- 渲染输出中无可视选区高亮
- 待定操作符视觉反馈（显示等待移动的 "d"）
- 重复计数显示

### 影响
Vim 模式作为状态机实现得不错，但实际未连接到输入控件。启用 vim 模式的用户将看不到任何视觉反馈，并且很可能会遇到行为异常，因为状态机的输出没有被 PromptInput 消费。

---

## 领域：聊天编辑器

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/components/chat_composer.rs` — 126 行，
  `ChatComposerState` 结构体

### TS 文件
- 没有独立的 chat_composer 文件 — 编辑器集成在 PromptInput.tsx（3000 行）和 REPL.tsx 中

### Rust 完成度：3/5

### 当前状态
`ChatComposerState` 是一个简洁的状态结构体，包含输入、模式、队列和基本的状态提示。它正确处理提交或排队行为（忙时排队）。

### 相对于 TS 的缺失功能
- 不是渲染组件 — `render_lines()` 是一个简单的字符串方法，并非 ratatui 控件
- 无底部集成（排队命令状态、模式指示器）
- 未与实际事件循环或按键处理集成
- 无粘贴/图片处理（委托给 PromptInput）
- 无命令面板或建议集成

### 影响
编辑器状态结构正确，但未连接到 UI。Rust TUI 中没有可见的编辑器区域。

---

## 领域：命令面板 / 命令界面

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/command_palette/mod.rs` — CommandPalette 结构体，
  包含 sync_from_input、handle_key、edit_target_picker 集成
- `rust/crates/claude-code-rs/src/ui/command_palette/render.rs` — 完整渲染
  实现：列表视图、详情面板、编辑目标选择器、参数帮助
- `rust/crates/claude-code-rs/src/ui/command_palette/filter.rs` — 对命令名称、
  别名和描述的模糊匹配
- `rust/crates/claude-code-rs/src/ui/command_palette/metadata.rs` — 命令元数据
  （用法、示例、编辑目标）
- `rust/crates/claude-code-rs/src/ui/command_palette/edit_targets.rs` — EditTarget 类型
  和 has_edit_target_picker
- `rust/crates/claude-code-rs/src/ui/command_palette/tests.rs` — 快照测试
- `rust/crates/claude-code-rs/src/ui/selection_surface.rs` — 通用 SelectionSurface，
  用作编辑目标选择器后端

### TS 文件
- 在 claude-code-bun 中未找到等效的命令面板。QuickOpen 对话框
  （`claude-code-bun/src/components/QuickOpenDialog.tsx`）提供文件/命令搜索。
- PromptInput 中的内联输入建议（useTypeahead 钩子）在用户输入时提供斜杠命令建议。
- `claude-code-bun/src/components/PromptInput/PromptInputFooterSuggestions.tsx` —
  建议下拉列表

### Rust 完成度：4/5

### 当前状态
Rust 命令面板是最完整的 UI 子系统。它具有：
- 通过 `/` 输入前缀检测激活
- 对命令名称、别名、描述的模糊过滤
- 专用的 handle_key，支持 Up/Down/PageUp/PageDown/Esc
- 显示用法、示例、行为、编辑目标的详情面板
- 编辑目标选择器（辅助 SelectionSurface），由 Ctrl+E 触发
- 带参数的命令的参数帮助渲染
- 首选高度计算
- 根视图、过滤后视图和编辑目标的快照测试
- 清晰的分离：mod.rs（状态）、render.rs（控件）、filter.rs（搜索）、
  metadata.rs（命令丰富）、edit_targets.rs（编辑目标类型）

### 相对于 TS 的缺失功能
- 未与实际输入控件集成（它是独立的状态机 — sync_from_input 被调用，
  但输出未被 PromptInput 消费）
- 无内联建议下拉列表（命令面板是单独的叠加层，不是用户输入时的内联建议）
- 无建议底部项（TS 的底部建议列表）
- 无历史搜索集成（Ctrl+R）

### 影响
实现扎实，反映了 TS 命令 UI。差距在于集成：面板以单独的叠加层而非内联方式渲染，且参数帮助不会反馈到输入控件的自动补全中。功能表面相当，但用户体验感觉不同。

---

## 领域：选择界面（通用选择器）

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/selection_surface.rs` — 约 320 行，通用
  `SelectionSurface`，包含：
  - `SelectionItem`（id、label、description、enabled、disabled_reason、preview_lines、
    actions、search_terms）
  - `SelectionAction`（id、label、enabled、disabled_reason）
  - `SelectionSurfaceEvent`（None、Selected、Closed）
  - 通过 `best_fuzzy_match` 进行模糊过滤
  - 导航（上/下、Enter、Esc、Backspace、Ctrl+U）
  - `render_lines(height)` 输出
  - `visible_indices()` 基于模糊分数排序计算
- `rust/crates/claude-code-rs/src/ui/selection_surface_details.rs` — 详情渲染
  辅助函数（预览行、操作行、搜索词）

### TS 文件
- `claude-code-bun/src/components/design-system/FuzzyPicker.tsx` — 通用选择器
  组件
- 各种选择器组件：ModelPicker、FastModePicker、EffortPicker、ThemePicker

### Rust 完成度：3/5

### 当前状态
通用选择界面，支持模糊过滤、项目操作、启用/禁用状态、预览行。被命令面板编辑目标选择器使用。

### 相对于 TS 的缺失功能
- 基于标签页的导航（来源标签如"全部 / 内置 / 用户 / 项目"）
- 详情/预览面板（左导航、右详情布局）
- 直接执行操作 vs 填充提示操作的区别
- 键盘快捷键提示（Enter、Esc、Tab 提示）
- 多选支持（批量操作的复选框）
- 章节标题 / 分组分隔符

### 影响
对简单选择器可用，但缺少 TS 用于命令、配置、代理和技能的结构化左/右标签页+详情布局。

---

## 领域：事件路由

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/runtime/event_router.rs` — 82 行，简单的
  基于枚举的路由

### TS 文件
- 无独立的事件路由器 — Ink 的 `useInput` + `useKeybinding` 在每个组件中处理路由。
  App 组件和屏幕组件管理事件流。

### Rust 完成度：2/5

### 当前状态
一个最小的基于枚举的事件路由器：
- `TerminalEvent`（Key、Paste、Resize、Tick）
- `EngineEvent`（AssistantDelta、ToolStarted、PermissionRequested、Completed）
- `UiEvent`（包装 Terminal/Engine 事件 + CommandResult、OverlayResponse）
- `RouteAction`（SubmitInput、QueueInput、AppendTranscript、UpdateToolActivity、
  ShowApproval、CloseOverlay、Redraw、Noop）
- `route_event()` 函数：将事件匹配到一个或多个操作
- `RouteContext`（busy、overlay_active）用于上下文感知路由

### 相对于 TS 的缺失功能
- 无按组件处理的输入（事件全局路由，而非路由到焦点组件）
- 无优先级系统
- 无和弦/按键序列路由
- 无按键绑定上下文解析（全局 vs 输入 vs 底部）
- 无事件传播 / stopPropagation 模式
- 无叠加层感知的事件过滤（overlay_active 作为布尔值处理，而非堆叠）

### 影响
事件路由器是一个可用的起点，但缺乏多上下文输入处理的复杂性。事件无法优先路由到焦点组件，
这导致对话框、命令面板或底部导航中的键盘快捷键无法正确工作。

---

## 领域：按键绑定系统

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/input/keybindings.rs` — 422 行，`KeybindingRegistry`
  包含：
  - `Action` 枚举（27 个操作）：Quit、ForceQuit、Cancel、Help、ToggleVerbose、
    ToggleFastMode、ScrollUp/Down/PageUp/PageDown/ToTop/ToBottom、Submit、ClearInput、
    CursorLeft/Right/Home/End、DeleteBack/Forward/Word、KillToEnd、HistoryPrev/Next、
    CompactHistory、ClearMessages、ShowCost、EnterVimNormal
  - `KeyBind` 结构体（修饰符 + 键码、显示方法）
  - `BindingContext`（Global、Input、Scroll）
  - `KeybindingRegistry` 带默认注册、bind/unbind、resolve（上下文带 Global 回退）、
    bindings_for_action、list_all
  - 10 个单元测试

### TS 文件
- `claude-code-bun/packages/@ant/ink/src/keybindings/` — 5 个文件：
  - `types.ts` — 操作类型、上下文类型
  - `match.ts` — 按键事件匹配
  - `parser.ts` — 按键绑定字符串解析
  - `resolver.ts` — 和弦和优先级解析
  - `useKeybinding.ts` — React 钩子
- `claude-code-bun/src/keybindings/` — 应用级别：
  - `KeybindingContext.ts` — React 上下文提供者
  - `KeybindingProviderSetup.tsx` — 设置组件
  - `shortcutFormat.ts` — `getShortcutDisplay`
  - `useKeybinding.ts` — 应用钩子
  - `confirmation-keybindings.test.ts` — 测试
  - `keybindings.ts` — 命令定义
- `claude-code-bun/src/components/ConfigurableShortcutHint.tsx` — 快捷键提示组件
- 用户可通过 `keybindings.json` 配置
- 跨上下文的 50+ 个操作名称：Chat、Footer、Help、Global、History 等
- 和弦绑定（例如 Ctrl+E + S 的两键序列）
- 每个操作的 `isActive` 守卫

### Rust 完成度：3/5

### 当前状态
扎实的按键绑定注册表，支持上下文感知解析（Input、Scroll、Global）、
27 个预定义操作和完整的测试覆盖。API 简洁清晰：`reg.bind(context, key, action)`、
`reg.resolve(context, event)` 带 Global 回退。

### 相对于 TS 的缺失功能
- 和弦绑定（两键序列如 Ctrl+E + S）
- 用户可配置的按键绑定（无 JSON 配置加载）
- 无 React 上下文提供者（钩子模式）
- 无每个操作的 `isActive` 守卫
- 仅 27 个操作（TS 有 50+ 个）
- 无快捷键显示工具函数（`getShortcutDisplay`）
- 无 `ConfigurableShortcutHint` 组件
- 无按组件处理程序注册（所有按键绑定都是全局的）
- 有限的上下文类型（仅 Global/Input/Scroll — 无 Chat/Footer/Help/History）

### 影响
基本的按键绑定系统适用于导航和文本编辑，但无法支持 TS 完整范围的键盘快捷键。
缺少和弦绑定和用户配置是最明显的差距 — 用户无法自定义快捷键或使用多键组合来执行命令。

---

## 现有文档

三份现有文档描述了目标状态：

1. **`rust/docs/ui/commands/command-surfaces.md`** — 命令面板的预期快照，涵盖
   /commands、/agents、/config、/diff、/hooks、/login、/mcp、/memory、/sandbox、/skills、
   /tasks、/team/teams。每个指定了触发方式、布局、键盘交互。

2. **`rust/docs/ui/better-view/selector-surfaces.md`** — 选择器界面的预期快照
   （命令面板、编辑目标选择器、代理、diff、技能、任务、团队、历史搜索）。
   指定了左导航/右详情布局，带固定底部。

3. **`rust/docs/ui/better-view/approval-panels.md`** — 审批面板的预期快照
   （工具权限、文件编辑、询问用户问题、MCP 服务器审批、工作区信任、
   直接执行状态）。每个指定了信息层次：上下文摘要、风险、决策、
   固定底部。

Rust 代码库部分实现了这些：命令面板约完成 80%（缺少左/右标签页布局），
审批面板约完成 40%（基本的 PermissionDialog 和 ApprovalOverlay 存在但缺少按工具变体），
选择器界面约完成 30%（SelectionSurface 存在但缺少标签页+详情结构）。

---

## 执行计划

### 权限系统接线计划（plan-07）

已创建：`docs/ui/great/plans/plan-07-permissions-wiring.md`

**目标**：将 20+ 工具专用权限变体从 `#[cfg(test)]` 提升为主渲染管道的一部分。

| Phase | 描述 | 工作量 |
|-------|------|:------:|
| 1 | PermissionRequestRouter + app.rs 集成 | 2-3 天 |
| 2 | 权限反馈输入（Tab 展开） | 3-4 天 |
| 3 | IDE diff 配置交互 | 1-2 天 |
| 4 | BypassPermissionsModeDialog | 0.5-1 天 |
| 5 | 移除所有 `#[allow(dead_code)]` | 0.5 天 |
| 6 | 分析集成 | 1-2 天 |

### 死代码清理计划（plan-08）

已创建：`docs/ui/great/plans/plan-08-dead-code-cleanup.md`

**目标**：移除 50+ 处 `#[allow(dead_code)]`，删除 `metadata.rs` 中 175 行完全冗余的代码。

| Phase | 描述 | 工作量 | 并行性 |
|-------|------|:------:|:------:|
| 1 | 权限系统死代码（与 plan-07 同步） | — | 阻塞于 plan-07 |
| 2 | agent_navigation.rs 接线 | 0.5-1 天 | 独立 |
| 3 | messages.rs 子模块死代码 | 1-2 天 | 独立 |
| 4 | metadata.rs 去重（13 个冗余函数，175 行） | 0.5 天 | 独立 |
| 5 | mod.rs 模块级清理（35 处） | 2-3 天 | 依赖 Phase 2/3/4 |

### 依赖关系

```
死代码清理 Phase 2/3/4 (并行)
       │
       ▼
死代码清理 Phase 5 (mod.rs)
       │
       ▼
权限接线 Phase 1-6
```

---

## 结论

### 优势（Rust 最接近对等的方面）
- **权限模块覆盖**：所有 TS 权限类型都有对应的 Rust 实现
- **命令面板**：完整的模糊过滤、详情渲染、编辑目标选择器
- **按键绑定注册表**：简洁的上下文感知设计，测试覆盖良好
- **Vim 状态机**：全面的 Normal/Insert/Visual 状态机
- **选择界面**：功能完整的通用选择器，支持模糊过滤

### 弱点（最大差距）
- **对话框/叠加层系统**：仅一个权限叠加层，而 TS 有 40+ 个对话框
- **输入控件**：仅单行，而 TS 支持多行带高亮/撤销/图片
- **集成**：大多数子系统作为独立状态机存在，未连接到实时事件循环或渲染管线
- **按工具区分的权限对话框**：单个统一对话框 vs 15+ 个专用组件
- **用户可配置的按键绑定**：无和弦支持，无 JSON 配置加载
