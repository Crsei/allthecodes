# Ratatui UI parity 未跟踪缺口实现计划

日期: 2026-05-08

模式: `$plan` direct

目标: 将 [`docs/RATATUI_UI_PARITY.md`](../RATATUI_UI_PARITY.md) 中标记为
`⚠️ 部分` 和 `❌ 缺失` 的 ratatui UI 功能收敛成正式实现计划，尤其补上目前没有
进入 `docs/plan/` 的功能跟踪。本文档不实现代码，只把缺口、归属、顺序、验收和
验证路径固定下来。

## 0. 当前跟踪状态

已有正式跟踪，不在本计划重复展开：

| 范围 | 现有跟踪 |
| --- | --- |
| `/plugin` TUI | [`plugin-ui-port-to-rust-plan-2026-05-08.md`](plugin-ui-port-to-rust-plan-2026-05-08.md) |
| 通用长列表选择组件 | [`generic-selectable-command-surface-plan-2026-05-08.md`](generic-selectable-command-surface-plan-2026-05-08.md) |
| 命令设置类 UI 缺口 | [`command-settings-ui-coverage-audit-2026-05-08.md`](command-settings-ui-coverage-audit-2026-05-08.md) |
| Session export 后端 | [`session-export-implementation-guide.md`](session-export-implementation-guide.md) |
| Remote/control/channel | [`remote-control-gateway-execution-plan-2026-05-08.md`](remote-control-gateway-execution-plan-2026-05-08.md), [`remote-channel-phase1-telegram-lark-plan-2026-05-08.md`](remote-channel-phase1-telegram-lark-plan-2026-05-08.md) |
| IPC subsystem 拆分 | [`ipc-refactor-plan.md`](ipc-refactor-plan.md) |
| shell 自动展开、持久历史 | [`../KNOWN_ISSUES.md`](../KNOWN_ISSUES.md) `UI-002`, `UI-003` |

需要本计划补跟踪的主干：

- `RATATUI_UI_PARITY.md` §2 消息交互缺失项。
- §3 输入/composer 缺失项。
- §4 权限设置/安全模式 UI 缺失项。
- §5 Agent/Team 状态可视化缺失项。
- §8 Settings/Sandbox/Usage/Token 警告缺失项。
- §12 LSP 卡片。
- §14 Welcome/Onboarding 非动画能力。
- §15 design-system 可复用 primitive 缺失项。
- §17 大量未归类对话框。
- §19 Help V2 缺口。

## 1. 原则和裁剪边界

1. 只改 Rust ratatui 路径：`crates/claude-code-rs/src/ui/**`，必要时补
   `crates/claude-code-rs/src/commands/**` 或后端状态查询 API。
2. 不修改 `ui/src/components/**`，它只作为行为参考。
3. 不为纯 React 框架抽象补 1:1 文件。`Dialog`、`Pane`、`ThemedBox` 等已有
   ratatui 原生替代时，只补共享 helper 或 surface。
4. 不追装饰动画。`LogoV2` 动画继续作为 intentional crop，除非产品路线明确要求
   TUI 视觉动效。
5. 每个新增 surface 都必须有 snapshot 或 command-surface 测试；涉及真实 runtime
   接线时再补 PTY/e2e。
6. 新增设置/权限相关持久化必须保持 `.cc-rust` 路径隔离，不能写回原版 Claude 路径。
7. 先补共享 primitive，再补多个消费者，避免每个缺口复制自己的 tab/list/key hint
   实现。

## 2. 缺口分组和首要落地文件

### 2.1 Shared UI primitives

覆盖的 parity 条目：

- `design-system/Tabs.tsx`、`TagTabs.tsx`：当前分散 `render_tabs`。
- `design-system/FuzzyPicker.tsx`、`customselect/`：当前只有简化
  `SelectionSurface`。
- `design-system/StatusIcon.tsx`。
- `design-system/KeyboardShortcutHint.tsx`、`ConfigurableShortcutHint.tsx`。
- `Ratchet.tsx`、`Byline.tsx`：仅在有实际消费者时补。

首要文件：

- `crates/claude-code-rs/src/ui/components/tabs.rs`
- `crates/claude-code-rs/src/ui/components/status_icon.rs`
- `crates/claude-code-rs/src/ui/components/keyboard_shortcut_hint.rs`
- `crates/claude-code-rs/src/ui/components/selection_surface.rs`
- `crates/claude-code-rs/src/ui/components/fuzzy_match.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/mod.rs`

交付：

1. 新增共享 `TabsState` / `TabsRender`，替代 command surface 内重复 tab string helper。
2. 扩展 `SelectionSurface` 支持 preview pane、action row、disabled reason、multi-line row。
3. 新增统一 `KeyboardShortcutHint` renderer，供 prompt footer、help、surface footer 使用。
4. 新增 `StatusIcon`，统一 ok/warn/error/running/disabled/unknown 文案和颜色映射。

验收：

- 现有 `/config`、`/mcp`、`/memory`、`/tasks` snapshot 不回退。
- 新 primitive 有独立 snapshot。
- 不强行迁移所有消费者；第一批只迁 `/config` 或一个小 surface 验证 API。

### 2.2 Settings / Sandbox / Usage

覆盖的 parity 条目：

- `Settings/Settings.tsx`、`Settings/Config.tsx`、`Settings/Status.tsx`：当前
  `ConfigSurface` 是简化版。
- `Settings/Usage.tsx`。
- `OutputStylePicker.tsx`、`LanguagePicker.tsx`、`ThinkingToggle.tsx`。
- `InvalidConfigDialog.tsx`、`InvalidSettingsDialog.tsx`。
- `ManagedSettingsSecurityDialog/`。
- `sandbox/SandboxConfigTab.tsx`、`SandboxDependenciesTab.tsx`、
  `SandboxOverridesTab.tsx`、`SandboxDoctorSection.tsx`、`sandbox-adapter.ts`。
- `CostThresholdDialog.tsx`、`TokenWarning.tsx`。

首要文件：

- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/config.rs`
- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/sandbox.rs`
- `crates/claude-code-rs/src/commands/config_cmd.rs`
- `crates/claude-code-rs/src/commands/cost.rs`
- `crates/claude-code-rs/src/commands/extra_usage.rs`
- `crates/claude-code-rs/src/commands/sandbox_cmd.rs`
- `crates/claude-code-rs/src/ui/messages/rate_limit_message.rs`

交付：

1. 将 `/config` 扩展为 Settings dashboard：
   - Status：effective config、sources、managed/user/project/local 层级。
   - Model/Theme/Effort：保留现有 picker。
   - Output：output style、language、thinking display。
   - Usage：当前 token/cost、rate-limit、extra usage 摘要。
   - Safety：managed settings、invalid settings/config diagnostics。
2. 将 `/sandbox` 扩展为 Config / Dependencies / Overrides / Doctor tabs。
3. 新增 token/cost warning surface：
   - 可由 status widget 显示 compact/cost 阈值；
   - 可由 `/config` Usage tab 打开详情；
   - 不阻塞现有 `/cost`、`/extra-usage` 文本命令。

验收：

- `/config` 无参数能展示 usage/output/language/thinking/safety tabs。
- invalid settings/config 能产生可见 diagnostics，而不是静默 fallback。
- `/sandbox` 能解释当前 sandbox 依赖、override 来源和 doctor 结果。
- `cargo test -p claude-code-rs config_cmd sandbox command_surface` 通过。

### 2.3 Permissions safety UI

覆盖的 parity 条目：

- `WebFetchPermissionRequest.tsx`：当前曾记录为 fallback，实际已有
  `permissions/web_fetch_permission_request` 模块，需要确认 runtime 路由。
- `AutoModeOptInDialog.tsx`。
- `BypassPermissionsModeDialog.tsx`。
- `/permissions` top-level surface 缺口由 command settings 审计指出，本计划负责
  parity 子功能细化。

首要文件：

- `crates/claude-code-rs/src/ui/permissions.rs`
- `crates/claude-code-rs/src/ui/permissions/web_fetch_permission_request/web_fetch_permission_request.rs`
- `crates/claude-code-rs/src/ui/permissions/rules/**`
- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/permissions.rs`（新增）
- `crates/claude-code-rs/src/commands/permissions_cmd.rs`
- `crates/claude-code-rs/src/permissions/**`

交付：

1. 确认 WebFetch runtime 能路由到专门 renderer；如果仍走 fallback，补映射和测试。
2. 新增 auto-mode opt-in surface，显示将改变的 permission mode、风险和可撤销路径。
3. 新增 bypass permissions mode dialog，明确 managed setting 禁用状态、危险范围和确认。
4. `/permissions` surface 中纳入 Mode / Rules / Workspace / Session grants /
   Recent denials tabs。

验收：

- WebFetch 权限请求 snapshot 不再显示 generic fallback。
- Auto/Bypass 模式切换需要显式确认；managed 禁用时不可绕过。
- SAFETY-001/SAFETY-002/SAFETY-003 相关 UI 至少能展示原因和阻断状态。

### 2.4 Message interaction and rendering gaps

覆盖的 parity 条目：

- `FilePathLink.tsx`、`ClickableImageRef.tsx`。
- `MessageTimestamp.tsx`。
- `MessageSelector.tsx`。
- `messageActions.tsx`：复制、展开、跳转、查看详情。
- `CompactSummary.tsx`、`InterruptedByUser.tsx`。
- `FallbackToolUseErrorMessage.tsx`、`FallbackToolUseRejectedMessage.tsx`。
- `NotebookEditToolUseRejectedMessage.tsx`、`FileEditToolUseRejectedMessage.tsx`。
- `OrderedList.tsx`。
- `PressEnterToContinue.tsx`、`ValidationErrorsList.tsx`。
- `MarkdownTable.tsx`、`HighlightedCode.tsx` 剩余 fidelity。
- `FileEditToolPreview.tsx`、`FileEditToolUpdatedMessage.tsx` live transcript 接线 residual。

首要文件：

- `crates/claude-code-rs/src/ui/messages/render.rs`
- `crates/claude-code-rs/src/ui/messages/user_tool_result_message/**`
- `crates/claude-code-rs/src/ui/messages/file_edit_tool_updated_message.rs`
- `crates/claude-code-rs/src/ui/messages/compact_boundary_message.rs`
- `crates/claude-code-rs/src/ui/messages/user_image_message.rs`
- `crates/claude-code-rs/src/ui/rendering/markdown_render.rs`
- `crates/claude-code-rs/src/ui/rendering/history_cell.rs`
- `crates/claude-code-rs/src/ui/runtime/transcript.rs`
- `crates/claude-code-rs/src/ui/app/transcript_mode.rs`

交付：

1. 先建立消息 action 模型：
   - selected message id；
   - copy selected message；
   - open file/path/image reference；
   - expand/collapse compact/tool output；
   - show raw/detail view。
2. 增加 timestamps：
   - session/replay 有 timestamp 时显示；
   - 当前 live message 缺 timestamp 时不伪造，显示可省略状态。
3. 增强 rejected/error/canceled tool result renderers：
   - file edit、notebook edit、fallback tool error/rejected 分开文案；
   - snapshot 覆盖成功、拒绝、取消、错误。
4. markdown renderer 补 ordered list、table/code highlight fidelity，避免独立组件泛滥。
5. live file-edit event data 完整后，接入非 replay path 的 structured preview。

验收：

- transcript 模式能选择消息并执行至少 copy/detail 两个 action。
- compact summary 和 interrupted-by-user 有专门 renderer。
- ordered list/table/code block snapshot 覆盖窄宽 viewport。
- file edit success/rejected/canceled/error 在 live 和 replay 两条路径都能渲染。

### 2.5 Prompt / Composer UX

覆盖的 parity 条目：

- `PromptInput/ModeIndicator.tsx`。
- `CommandHint.tsx` 剩余独立提示。
- `ConfigurableShortcutHint.tsx`。
- `usePromptInputPlaceholder.ts`。
- `useMaybeTruncateInput.ts`。
- `paste-display.test.ts` 大文本折叠。

首要文件：

- `crates/claude-code-rs/src/ui/components/prompt_input.rs`
- `crates/claude-code-rs/src/ui/components/chat_composer.rs`
- `crates/claude-code-rs/src/ui/components/bottom_pane.rs`
- `crates/claude-code-rs/src/ui/app/input.rs`
- `crates/claude-code-rs/src/ui/input/clipboard_paste.rs`
- `crates/claude-code-rs/src/ui/input/slash_command.rs`
- `crates/claude-code-rs/src/ui/input/keybindings.rs`

交付：

1. 增加 mode indicator：normal/vim/transcript/focus/permission/command palette 状态。
2. 统一 shortcut hint 和 command hint，避免 footer 文案散落。
3. 增加 placeholder resolver：
   - 空 prompt；
   - busy/queued；
   - plan mode；
   - shell/bash mode；
   - disabled input。
4. 增加 large paste display：
   - 粘贴过长时折叠预览；
   - 明确可展开/清除；
   - 不阻断正常提交。
5. 增加 input truncation preview，只截 UI 显示，不截真实 buffer。

验收：

- prompt snapshot 覆盖空态、busy、vim、plan mode、大粘贴、长输入。
- keybinding reload 后 footer hints 和实际行为一致。

### 2.6 Agents / Teams / Tasks status

覆盖的 parity 条目：

- `AgentTreePanel.tsx`。
- `CoordinatorAgentStatus.tsx`。
- `AgentProgressLine.tsx`。
- `TeammateViewHeader.tsx`。
- `TeamMemberCard.tsx`。
- `team-summary.ts`。
- `ShellProgress.tsx` 剩余 shell 输出细节。
- `BashModeProgress.tsx`。

首要文件：

- `crates/claude-code-rs/src/ui/components/command_surface/surfaces/agents.rs`
- `crates/claude-code-rs/src/ui/app/agent_navigation.rs`
- `crates/claude-code-rs/src/ui/teams/teams_dialog.rs`
- `crates/claude-code-rs/src/ui/teams/team_status.rs`
- `crates/claude-code-rs/src/ui/tasks/background_task.rs`
- `crates/claude-code-rs/src/ui/tasks/background_tasks_dialog.rs`
- `crates/claude-code-rs/src/ui/tasks/shell_progress.rs`
- `crates/claude-code-rs/src/ui/tasks/remote_session_progress.rs`
- `crates/claude-code-rs/src/commands/coordinator.rs`
- `crates/claude-code-rs/src/commands/team_cmd.rs`

交付：

1. `/agents` list/detail/create wizard 接线按 command settings 审计执行。
2. Agent tree panel 显示 active threads、loaded subthreads、agent source、pending status。
3. Coordinator status 显示 active coordinator、queue、pending approval、blocked reason。
4. Team member card 显示 member id、model、state、last activity、mailbox pending。
5. Team summary 聚合 running/blocked/done/failed。
6. Shell/Bash progress 显示 command、elapsed、last output tail、exit code、expand action。

验收：

- `/agents` 不再只是提交 `/agents show`；能在 surface 内查看 detail。
- `/team` surface 能解释每个 member 为什么 pending/blocked。
- `/tasks` shell detail 能展示足够输出上下文，并和 UI-002 自动展开策略兼容。

### 2.7 Search / Navigation / Session dialogs

覆盖的 parity 条目：

- `GlobalSearchDialog.tsx`。
- `HistorySearchDialog.tsx` persistent history residual。
- `QuickOpenDialog.tsx`。
- `LogSelector.tsx`。
- `ContextVisualization.tsx`。
- `ExportDialog.tsx`。
- `ExitFlow.tsx`。
- `SessionBackgroundHint.tsx`。
- `SessionPreview.tsx`。
- `IdleReturnDialog.tsx`。

首要文件：

- `crates/claude-code-rs/src/ui/components/history_search_dialog.rs`
- `crates/claude-code-rs/src/ui/components/resume_picker.rs`
- `crates/claude-code-rs/src/ui/components/pager_overlay.rs`
- `crates/claude-code-rs/src/ui/tui/export.rs`
- `crates/claude-code-rs/src/commands/session.rs`
- `crates/claude-code-rs/src/commands/resume.rs`
- `crates/claude-code-rs/src/commands/export.rs`
- `crates/claude-code-rs/src/commands/session_export.rs`
- `crates/claude-code-rs/src/commands/context.rs`
- `crates/claude-code-rs/src/session/**`

交付：

1. 将 Ctrl+R 从 in-session history 扩展为跨会话 history reader。
2. QuickOpen 先覆盖文件/路径/最近文件；不做 IDE 全局 workspace index。
3. GlobalSearch 覆盖当前 transcript、session metadata、known files。
4. LogSelector 读取 session/log index，复用 `ResumePicker` 列表模型。
5. SessionPreview 显示 title、cwd、message count、last modified、summary snippet。
6. ExportDialog 调用已有 `/session-export` 与 `/audit-export` 命令能力，不重写导出后端。
7. ExitFlow 显示 running tasks、unsaved approvals、background session hint。
8. ContextVisualization 先显示 token/message/tool distribution，不做复杂图形。

验收：

- `/resume` 无参数 picker 和 SessionPreview 可达。
- ExportDialog 能完成一次当前 session 导出并显示路径。
- ExitFlow 在有后台任务时不静默退出。

### 2.8 IDE / Remote / Chrome / Teleport

覆盖的 parity 条目：

- `IdeAutoConnectDialog.tsx`、`IdeOnboardingDialog.tsx`、`ShowInIDEPrompt.tsx`。
- `BridgeDialog.tsx`、`RemoteCallout.tsx`、`RemoteEnvironmentDialog.tsx`。
- `TeleportError.tsx`、`TeleportProgress.tsx`、`TeleportRepoMismatchDialog.tsx`、
  `TeleportResumeWrapper.tsx`、`TeleportStash.tsx`。
- `ClaudeInChromeOnboarding.tsx`。
- LSP server card。

首要文件：

- `crates/claude-code-rs/src/commands/ide_cmd.rs`
- `crates/claude-code-rs/src/commands/chrome_cmd.rs`
- `crates/claude-code-rs/src/commands/channels.rs`
- `crates/claude-code-rs/src/ui/components/status_widget.rs`
- `crates/claude-code-rs/src/ui/lsp_recommendation/lsp_recommendation_menu.rs`
- `crates/claude-code-rs/src/lsp_service/**`
- remote/channel 文件按 remote-control/channel 计划执行。

交付：

1. 先做 IDE picker/status surface：
   - detected IDEs；
   - auto-connect/auto-install 状态；
   - reconnect/clear；
   - show-in-IDE prompt。
2. LSP server card 显示 server name、language、status、diagnostics count、plugin source。
3. Chrome onboarding 只做当前支持能力说明、native host/MCP 状态、打开配置提示。
4. Remote/Teleport 先做裁剪决策：
   - 如果远程计划本阶段不交付，则在 `RATATUI_UI_PARITY.md` 改成 roadmap/crop；
   - 如果交付，则只实现 remote-control/channel 计划已经定义的数据流，不单独造 UI 假状态。

验收：

- `/ide` 无参数 surface 可达，且和 command handler 状态一致。
- LSP recommendation 之外有 server status card。
- Remote/Teleport 相关条目要么有真实接线，要么明确移到后续计划，不能长期保持“缺失但无归属”。

### 2.9 Help / Onboarding / Diagnostics

覆盖的 parity 条目：

- `helpv2/HelpV2.tsx`、`helpv2/Commands.tsx`、`helpv2/General.tsx`。
- `Onboarding.tsx`。
- `ExperimentEnrollmentNotice.tsx`、`GateOverridesWarning.tsx`、`ChannelsNotice.tsx`。
- `DiagnosticsDisplay.tsx`。
- `KeybindingWarnings.tsx`。
- `Stats.tsx`。
- `SkillImprovementSurvey.tsx`。
- `DevChannelsDialog.tsx`、`ChannelDowngradeDialog.tsx`。
- `NativeAutoUpdater.tsx`、`PackageManagerAutoUpdater.tsx`。

首要文件：

- `crates/claude-code-rs/src/ui/components/command_palette/**`
- `crates/claude-code-rs/src/ui/components/welcome.rs`
- `crates/claude-code-rs/src/commands/help.rs`
- `crates/claude-code-rs/src/commands/doctor.rs`
- `crates/claude-code-rs/src/commands/keybindings_cmd.rs`
- `crates/claude-code-rs/src/commands/insights.rs`
- `crates/claude-code-rs/src/commands/channels.rs`
- `crates/claude-code-rs/src/commands/terminal_setup.rs`

交付：

1. Help V2：
   - home；
   - commands；
   - general usage；
   - keybindings；
   - troubleshooting。
2. Onboarding：
   - first-run checklist；
   - auth/config/workspace trust；
   - optional IDE/Chrome/MCP hints；
   - no decorative LogoV2 animation.
3. DiagnosticsDisplay 复用 `/doctor` 输出模型，做结构化 panel。
4. KeybindingWarnings 接入 keybindings parser/reload diagnostics。
5. Stats 复用 `/insights` 或 cost/session stats。
6. AutoUpdater 先做平台裁剪决策；Windows 发布策略未定前不造假 updater UI。

验收：

- `/help` 或 command palette 能打开 Help V2 surface。
- first-run welcome 能进入 onboarding checklist。
- keybinding 配置错误能在 UI 中看到，不只在文本命令输出里。

## 3. 实施顺序

### Phase 0. 跟踪归档和裁剪决策

1. 把本计划加入 `WORK_STATUS.md` 的 ratatui parity 入口。
2. 修正 `WORK_STATUS.md` 中不存在的 `ui-parity-update-plan.md` 引用，指向本计划或把
   `docs/tmp/ui-parity-update-plan.md` 迁入 archive。
3. 在 `RATATUI_UI_PARITY.md` 给 intentional crop 加明确标记：
   - LogoV2 动画；
   - 纯 React 抽象；
   - 远程/Teleport 如果本阶段不交付。
4. 为每个 `⚠️/❌` 条目增加 tracking owner：
   - 本计划；
   - 既有计划；
   - intentional crop；
   - roadmap deferred。

### Phase 1. Shared primitives

实现 tabs、shortcut hint、status icon、selection preview/actions。先只迁一个小
consumer，避免一次性重构所有 surface。

验证：

```powershell
cargo test -p claude-code-rs selection_surface command_surface
```

### Phase 2. Settings / Sandbox / Permissions

先补用户最常触达、风险最高的设置面：

1. `/config` dashboard 扩展。
2. `/sandbox` tabs 扩展。
3. `/permissions` surface 和 Auto/Bypass safety dialogs。
4. token/cost warnings。

验证：

```powershell
cargo test -p claude-code-rs config_cmd sandbox permissions command_surface
```

### Phase 3. Messages / Composer

补消息选择、操作、timestamp、path/image references、compact/interrupted renderers、
large paste、placeholder、input truncation。

验证：

```powershell
cargo test -p claude-code-rs messages prompt_input history_search
```

### Phase 4. Agents / Teams / Tasks

补 `/agents` detail/create 接线、agent tree、coordinator status、team member card、
team summary、shell/bash progress detail。

验证：

```powershell
cargo test -p claude-code-rs agents teams tasks command_surface
```

### Phase 5. Navigation / Session / Help

补 persistent history、quick open、global search、log selector、session preview/export、
exit flow、Help V2、diagnostics/keybinding warnings。

验证：

```powershell
cargo test -p claude-code-rs resume session_export command_palette
```

### Phase 6. IDE / LSP / Chrome / Remote decision

补 IDE picker/status、LSP server card、Chrome onboarding。Remote/Teleport 按已有
remote 计划决定实现或裁剪。

验证：

```powershell
cargo test -p claude-code-rs ide lsp_recommendation channels
```

### Phase 7. 文档回写

每个 phase 完成后同步：

1. `docs/RATATUI_UI_PARITY.md`：状态从 `❌`/`⚠️` 改为 `✅` 或明确 crop/deferred。
2. `docs/WORK_STATUS.md`：当前 release gate 更新。
3. `docs/IMPLEMENTATION_GAPS.md`：只保留仍未完成的全量构建 TODO。
4. `docs/archive/COMPLETED_FULL.md`：记录完成项和验证命令。

## 4. 验收标准

整体完成标准：

1. `RATATUI_UI_PARITY.md` 中所有 `⚠️/❌` 条目都有明确归属：
   - 已实现；
   - 本计划阶段；
   - 既有计划；
   - intentional crop；
   - roadmap deferred。
2. P1/P2 发布支持面内的 `❌` 不再停留在“无对应实现且无计划”。
3. 每个新增 UI 模块至少有 snapshot/unit test。
4. 每个 runtime 接线模块至少有 command-surface、app、或 PTY/e2e 覆盖之一。
5. 文档不能提前把计划写成事实；只有实现和验证完成后才改 parity 状态。

## 5. 风险和约束

| 风险 | 处理 |
| --- | --- |
| 80+ 缺口一次性展开导致失控 | 按 phase 交付；每个 phase 只做一个用户可见领域和对应 tests |
| 复制 TS React 架构造成 Rust UI 复杂化 | 只移植行为，不移植 React hooks/Provider 结构 |
| 远程/Teleport/Updater 无后端支持 | 先裁剪或 roadmap deferred，不做假 UI |
| 设置/权限 UI 误导用户以为已持久化 | mutation 统一复用现有 command/settings API，并展示写入 scope |
| snapshot 更新掩盖回归 | 每个 snapshot 更新必须说明行为变化，并保留关键空态/窄宽状态 |

## 6. 后续第一步

推荐先执行 Phase 0 + Phase 1：

1. 将本计划加入 `WORK_STATUS.md`。
2. 给 `RATATUI_UI_PARITY.md` 增加 tracking owner/crop/deferred 说明。
3. 实现 shared `Tabs`、`KeyboardShortcutHint`、`StatusIcon` 的最小版本和 snapshots。
4. 再启动 `/config` dashboard 扩展，作为第一个真实消费者。
