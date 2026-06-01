# 执行计划：plan-07-permissions-wiring

## 当前状态

> 2026-05-20 更新：本文件上半部分保留了接线前的差距分析。当前实现状态以文末“实施后状态与遗留问题”为准。

Rust 权限系统处于一种特有的 "已实现但未接线" 状态：

### 模块结构（镜像 TS）
`permissions/` 目录包含 60+ 文件，覆盖了 TS 参考实现中几乎所有的权限变体。架构对齐如下：

| 类别 | Rust 文件 | TS 对应 |
|------|-----------|---------|
| 请求组件（子模块） | `bash_permission_request/`、`file_edit_permission_request/`、`file_write_permission_request/`、`filesystem_permission_request/`、`web_fetch_permission_request/`、`skill_permission_request/`、`notebook_edit_permission_request/`、`sed_edit_permission_request/`、`computer_use_approval/`、`enter_plan_mode_permission_request/`、`exit_plan_mode_permission_request/`、`ask_user_question_permission_request/`（7 个子文件）、`power_shell_permission_request/`、`monitor_permission_request/`、`review_artifact_permission_request/` | 一一对应 |
| 对话框基础设施 | `dialog_overlay.rs`（`PermissionDialog`）、`permission_dialog.rs`、`permission_request.rs`、`permission_prompt.rs`、`permission_dialog.rs` | `PermissionDialog.tsx`、`PermissionRequest.tsx`、`PermissionPrompt.tsx` |
| 文件权限对话框 | `file_permission_dialog/`（`file_permission_dialog.rs`、`ide_diff_config.rs`、`use_file_permission_dialog.rs`、`use_permission_handler.rs`） | `FilePermissionDialog/`（同名文件） |
| 规则管理 | `rules/`（8 个文件） | `rules/`（8 个文件） |
| 工具函数/钩子 | `utils.rs`、`hooks.rs`、`shell_permission_helpers.rs`、`use_shell_permission_feedback.rs`、`worker_badge.rs`、`worker_pending_permission.rs` | 同名 `.ts`/`.tsx` |
| 回退/沙箱 | `fallback_permission_request.rs`、`sandbox_permission_request.rs` | `FallbackPermissionRequest.tsx`、`SandboxPermissionRequest.tsx` |

### 死代码清单
**文件级 `#[allow(dead_code)]`**（42 处，17 个文件）：
- `permissions.rs` — 29 个模块全部标记（`[allow(dead_code)]` 应用于除 `dialog_overlay` 外的所有子模块）
- `permissions/rules/mod.rs` — 8 个子模块全部标记
- `permissions/ask_user_question_permission_request/mod.rs` — 6 个子模块标记
- `permissions/bash_permission_request/mod.rs` — 2 个子模块标记
- `permissions/file_edit_permission_request/mod.rs` — 2 个子模块标记
- `permissions/file_write_permission_request/mod.rs` — 2 个子模块标记
- `permissions/notebook_edit_permission_request/mod.rs` — 2 个子模块标记
- `permissions/power_shell_permission_request/mod.rs` — 2 个子模块标记
- 其余 9 个文件各 1 处

### 已接线的部分
- **`dialog_overlay.rs` 中的 `PermissionDialog`** — 被 `app.rs:78` 引用为 `Option<PermissionDialog>`（`crate::ui::permissions::PermissionDialog`），在 `app.rs:315` 创建，在 `app/render.rs` 中渲染
- **`approval_overlay.rs` 中的 `ApprovalOverlay`** — 基于 `BetterViewPanel` 的另一种审批面板，未死代码
- **`app.rs` 中的 `WebFetch` 特殊处理** — 手动调用 `render_web_fetch_permission_request`（`app.rs:305`），但仅用于获取字符串，未使用渲染结果

### 使用模式
当前接线方式 (`app.rs`)：
```
let dialog = PermissionDialog::new(tool_name, input, message);
```
单一通用对话框。所有工具使用相同的 3-option 选择（Allow/Deny/Always Allow），没有工具特定的选项、上下文详情、反馈输入或 IDE diff。

### 缺失的关键功能
1. **权限反馈输入** — TS `PermissionPrompt.tsx` 有完整的 Tab 展开反馈文本字段，支持接受/拒绝反馈，带分析追踪。Rust `permission_prompt.rs` 是纯字符串渲染器，包含 `PermissionPromptState` 但不支持反馈输入事件处理
2. **IDE diff 配置显示** — Rust `ide_diff_config.rs` 定义了 `IdeDiffConfig` 结构体和 `render_ide_diff_config()` 纯字符串渲染，仅用于快照测试。TS `useDiffInIDE.ts` + `ShowInIDEPrompt.tsx` 提供完整的 IDE diff 交互流程
3. **BypassPermissionsModeDialog** — TS 有完整实现（~66 行），使用 `Dialog` + `Select` + 分析事件 + 设置持久化。Rust 完全缺失
4. **工具特定的选项/上下文** — 每个 TS 权限请求组件会渲染工具特定的信息（Bash 显示命令和风险指示器、FileEdit 显示内联 diff、WebFetch 显示 URL）。Rust `dialog_overlay.rs` 对所有工具使用同一种布局
5. **分析追踪** — TS 挂钩到 `logEvent` 进行分析（接受/拒绝/反馈/模式进入/转义计数）。Rust 无权限分析

## 目标状态

1. 所有权限渲染函数从 `#[cfg(test)]` 范围提升为主渲染管道的一部分
2. `PermissionRequest` 路由中心根据工具名称分发到专用权限组件
3. 反馈输入（Tab 展开）集成到权限提示循环中
4. IDE diff 配置可显示并可交互
5. BypassPermissionsModeDialog 存在并可触发
6. `permissions.rs` 中的 `#[allow(dead_code)]` 全部移除

## TS 参考

### 关键路由分发：`PermissionRequest.tsx`
```typescript
function permissionComponentForTool(tool: Tool): React.ComponentType {
  switch (tool) {
    case FileEditTool: return FileEditPermissionRequest;
    case FileWriteTool: return FileWritePermissionRequest;
    case BashTool: return BashPermissionRequest;
    // ... 15+ 工具
    default: return FallbackPermissionRequest;
  }
}
```
Rust 对应：无。`dialog_overlay.rs:241` 中的 `approval_kind()` 做了一些启发式分类（bash/file/web/mcp/user/fallback），但不是工具分派。

### 反馈输入：`PermissionPrompt.tsx`（~250 行）
- 状态：`acceptFeedback`、`rejectFeedback`、`acceptInputMode`、`rejectInputMode`
- Tab 键切换：展开/折叠反馈输入
- Select 集成：反馈字段作为 Select 中的 `type: 'input'` 选项
- 分析：模式进入/折叠/提交事件
Rust 对应：`permission_prompt.rs`（37 行）仅有数据结构 + 字符串渲染，无交互状态。

### IDE diff：`FilePermissionDialog/ideDiffConfig.ts` + `useDiffInIDE.ts`
- `IDEDiffSupport<T>` 接口：`getConfig(input): IDEDiffConfig` + `applyChanges(input, edits): T`
- `useDiffInIDE` 钩子：管理打开/关闭 IDE diff 标签页
Rust 对应：`ide_diff_config.rs`（16 行）仅有结构体 + 渲染，无 `useDiffInIDE` 等价物。

### BypassPermissionsModeDialog：`BypassPermissionsModeDialog.tsx`（~66 行）
- 使用 `Dialog` 组件
- 两个选项："No, exit" / "Yes, I accept"
- 接受时写入 `updateSettingsForSource('userSettings', { skipDangerousModePermissionPrompt: true })`
- 分析事件
Rust 对应：完全缺失。

## Rust 当前代码

### `permissions.rs` — 主索引文件
路径：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions.rs`
- 行 1-71：29 个模块标记 `#[allow(dead_code)]`，仅 `dialog_overlay` 未标记
- 行 73-468：`#[cfg(test)]` 模块包含所有子模块的 snapshot 测试

### `dialog_overlay.rs` — 当前唯一接线的组件
路径：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs`
- `PermissionDialog` 结构体（357 行）— 完整的 ratatui widget
- 支持选项：Allow / Deny / Always Allow
- 支持导航：Left/Right/Tab、Enter 确认、Esc 拒绝、y/n/a 快捷键
- 局限性：对所有工具使用同一布局；无反馈输入；无 IDE diff

### `app.rs` — 接线点
路径：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs`
- 行 37：`use super::permissions::{PermissionChoice, PermissionDialog};`
- 行 78：`permission_dialog: Option<PermissionDialog>,`
- 行 300-316：仅对 WebFetch 做了特殊处理（调用 `render_web_fetch_permission_request` 获取字符串，但不使用其渲染结果）；对所有其他工具创建通用 `PermissionDialog::new(tool_name, input, message)`

### 现有工具特定渲染函数（当前仅用于测试）
- `bash_permission_request/bash_permission_request.rs`：`render_bash_permission_request(command, selected_index)` — 可复用，需接入
- `file_edit_permission_request/file_edit_permission_request.rs`：`render_file_edit_permission_request`、`render_file_edit_permission_request_with_diff` — 可复用，需接入
- `web_fetch_permission_request/web_fetch_permission_request.rs`：`render_web_fetch_permission_request(url, method, selected_index)` — app.rs 已引用但不使用其输出
- 等等共 20+ 渲染函数

## 分步实施

### Phase 1：死代码清理（前置条件）
> **注意**：权限接线需要在清理后才能进行以避免引用冲突。

1. **移除 `permissions.rs` 中的 `#[allow(dead_code)]`**
   - 逐步操作：先移除一个模块上的死代码注解，修复由此产生的编译错误，然后移到下一个
   - 依赖关系：这会导致编译错误，因为许多模块当前仅被测试代码引用
   - 文件：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions.rs`

2. **创建 `PermissionRequestRouter`**
   - 在 `permissions/` 中新建 `permission_request_router.rs`
   - 核心函数：`fn render_permission_for_tool(tool_name: &str, input: &str, message: &str, selected: usize) -> ...`
   - 在 `permissions.rs` 中 `pub use`
   - 参考：TS `PermissionRequest.tsx` 中的 `permissionComponentForTool()` 模式
   - 需要修改的文件：
     - 新建：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/permission_request_router.rs`
     - 修改：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions.rs`

3. **在 `app.rs` 中集成路由器**（关键改动点）
   - 替换 `app.rs:300-316` 中的硬编码 `PermissionDialog::new(tool_name, input, message)`
   - 改为根据工具名分发到专用渲染路径
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs`
     - 可能：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app/render.rs`

### Phase 2：权限反馈输入

4. **在 `PermissionPromptState` 中添加反馈状态**
   - 当前 `permission_prompt.rs` 中无交互状态
   - 添加：`accept_feedback`、`reject_feedback`、`accept_input_mode`、`reject_input_mode`、`focused_value`
   - 添加：`handle_tab()` 方法切换输入模式
   - 参考：TS `PermissionPrompt.tsx` 中的反馈模式管理
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/permission_prompt.rs`

5. **将 `PermissionPrompt` 集成到 `PermissionDialog` 中**
   - 在 `dialog_overlay.rs` 中添加反馈输入支持
   - 当反馈模式激活时，在按钮行下方显示文本输入字段
   - 处理 Enter（提交）、Esc（取消反馈）、Tab（切换模式）
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs`

6. **将反馈通过事件系统传递到引擎**
   - `PermissionChoice` 枚举扩展为包含可选的反馈字符串
   - 事件路由更新以携带反馈
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/dialog_overlay.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/permission_request.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/utils.rs`（`PermissionDecision` 枚举）

### Phase 3：IDE diff 配置

7. **将 `ide_diff_config.rs` 从纯数据提升为交互组件**
   - 添加 `IdeDiffConfigState` 结构体（当前 `rendered`、`visible`、`toggle_action`）
   - 在 `FilePermissionDialogState` 中添加 IDE diff 交互支持
   - 参考：TS `useDiffInIDE.ts` 钩子（打开/关闭 IDE diff tab）
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/file_permission_dialog/ide_diff_config.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/file_permission_dialog/use_file_permission_dialog.rs`

8. **在文件权限对话框中显示 IDE diff 信息**
   - 修改 `render_file_permission_dialog` 以在 diff 可用时显示 IDE 配置
   - 添加交互以在 IDE 中打开
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/file_permission_dialog/file_permission_dialog.rs`

### Phase 4：BypassPermissionsModeDialog

9. **创建 `BypassPermissionsModeDialog`**
   - 新建文件，与 TS 语义对应：
     - 标题："WARNING: Claude Code running in Bypass Permissions mode"
     - 两个选项：拒绝（退出）/ 接受（设置 `skipDangerousModePermissionPrompt: true`）
   - 需要一个新模块导出，或者直接添加到 `permissions.rs`
   - 需要修改的文件：
     - 新建：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/bypass_permissions_mode_dialog.rs`
     - 修改：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions.rs`（添加模块导出，移除 `#[allow(dead_code)]`）
     - 修改：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/app.rs`（接入启动/触发逻辑）

### Phase 5：移除 `#[allow(dead_code)]`

10. **从 `permissions.rs` 移除所有 `#[allow(dead_code)]`**
   - 在完成 Phase 1-4 后，所有模块被外部引用
   - 同时移除 `permissions/rules/mod.rs`、`ask_user_question_permission_request/mod.rs` 等子模块中的注解
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions.rs`
     - 17 个子模块中的 `mod.rs` 文件（见死代码清理计划 §8）

### Phase 6：分析集成（可选增强）

11. **添加权限分析事件**
   - 在 `permissions/hooks.rs` 中扩展 `PermissionHookEvent` 以包含分析元数据
   - 添加 `log_permission_decision`、`log_feedback_mode_entered`、`log_escape` 等函数
   - 参考：TS `useShellPermissionFeedback.ts`、`utils.ts` 中的 `logUnaryPermissionEvent`、`tengu_*` 分析事件
   - 需要修改的文件：
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/hooks.rs`
     - `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-rust/crates/claude-code-rs/src/ui/permissions/utils.rs`

## 工作量估算

| 阶段 | 描述 | 预估工作量 | 并行性 |
|------|------|:----------:|:------:|
| Phase 1 | 死代码清理（前置） | 2-3 天 | 单线程——必须逐个解除 |
| Phase 2 | 权限反馈输入 | 3-4 天 | 起始依赖 Phase 1 |
| Phase 3 | IDE diff 配置 | 1-2 天 | 可与 Phase 2 并行 |
| Phase 4 | BypassPermissionsModeDialog | 0.5-1 天 | 可与 Phase 2/3 并行 |
| Phase 5 | 移除 `#[allow(dead_code)]` | 0.5 天 | Phase 1-4 完成后 |
| Phase 6 | 分析集成 | 1-2 天 | 可最后做，不阻塞 |
| **总计** | | **8-12 天** | |

## 风险/注意

1. **现有的 `PermissionDialog` 布局将需要重构**：当前实现（`dialog_overlay.rs`）是通用的 3-option 选择器。要支持按工具定制的布局（如文件 edit 的内联 diff、bash 的风险指示器），需要重构为更灵活的渲染架构。建议逐步过渡：先在通用回调上叠加工具特定的渲染，再完全替换。

2. **ratatui 中缺少 React 的重新渲染循环**：TS 通过 React hooks（`useState`、`useEffect`）管理权限交互状态。Rust 需要显式状态机来管理输入模式切换和反馈文本缓冲。这是架构挑战，不是实现障碍。

3. **反馈输入的键盘冲突**：当反馈文本输入激活时，Tab 键的功能从"切换焦点"变为"提交反馈模式"。需要键盘上下文明确的优先级处理。建议在 `PermissionDialog` 内部使用子状态机（`Selecting` vs `TypingFeedback`）。

4. **IDE diff 集成需要了解外部编辑器**：TS `useDiffInIDE` 钩子通过进程间通信在外部编辑器中打开 diff。Rust 可能需要类似的 IPC 到编辑器进程。如果编辑器不可用，应有优雅的回退（仅内联显示 diff 文本）。

5. **BypassPermissionsModeDialog 的设置持久化**：需要将其链接到设置系统（`updateSettingsForSource` 的 Rust 等价物）。检查 `cc-settings` crate 是否已有 `skipDangerousModePermissionPrompt` 配置键。

6. **测试风险**：当前唯一的测试覆盖率来自 `permissions.rs` 中的 snapshot 测试（`snapshot_permission_component_helpers`）。接入后，这些都需要更新或扩展以测试交互流程。建议为每个交互阶段（渲染 → 选择 → 反馈）分别新建测试。

## 实施后状态与遗留问题（2026-05-20）

本轮已完成生产接线的基础层，而不是继续依赖 description 字符串推断：

1. 权限回调改为传递结构化 `PermissionRequestPayload`，包含 `tool_use_id`、`tool_name`、`tool_input`、`message`、`options`。IPC 继续保留 legacy `command` 字段，同时新增 `input` 字段以保留结构化 tool input。
2. direct TUI 的 `EngineEvent::PermissionRequest`、`App::show_permission_request` 和 `PermissionDialog` 已改为接收完整请求。`PermissionDialog` 现在用结构化 request 驱动 body 渲染和选项数量，不再用空 input 或 description 猜测主要内容。
3. `PermissionRequestRouter` 已进入生产路径，优先按精确 tool name 分发，只有未知工具或缺少必要结构化字段时才走 fallback。当前覆盖 `Bash`、`PowerShell`、`WebFetch`、`Write`、`Edit`、`NotebookEdit`、`SedEdit`、`Filesystem`、`Skill`、`Monitor`、`ReviewArtifact`、`ComputerUse`、`EnterPlanMode`、`ExitPlanMode`、`Sandbox`。
4. `AskUserQuestion` 已接入 direct TUI 的独立 `QuestionDialog` 和 `AskUserCallback` 事件路径，响应仍走 question callback，不混入 permission decision。headless `QuestionRequest` / `QuestionResponse` 语义保持独立。
5. 已生产引用的权限组件解除测试隔离；仍无生产入口的纯快照、规则管理、反馈输入和调试 helper 继续保持 test-only，避免靠 `allow(dead_code)` 掩盖未接线代码。

仍需在后续计划中跟踪：

1. 权限反馈输入、`PermissionChoice` feedback 全链路、IDE diff 交互、`BypassPermissionsModeDialog` 和权限分析事件尚未完成；这些仍对应原计划 Phase 2-4 与 Phase 6。
2. 文件权限目前显示结构化摘要和已有 renderer 可表达的信息；外部 IDE diff 入口需等 editor bridge 能力明确后再启用。
3. Team sandbox 仍是独立事件域，不应塞进普通 tool permission callback；等待 team event handler 具备 request/response UI 入口后再接专用交互。

## 2026-05-20 第二次接线补充

本轮继续把“有真实事件源和真实消费点”的权限模块接入生产路径，并把其余仍缺入口的部分显式保留在 `#[cfg(test)]`：

1. `AskUserQuestion` 已从 text-only callback 升级为结构化 `AskUserRequestPayload { question, choices, allow_free_text }`。headless `BackendMessage::QuestionRequest` 同步新增 `choices` 和 `allow_free_text`，旧客户端继续可以只看 `id/text`。
2. Rust TUI `QuestionDialog` 已消费结构化问题：有 `choices` 时支持上下选择；允许自由输入时仍可直接录入文本；提交优先返回输入文本，否则返回当前选中项。
3. `cc-types::agent_events::AgentEvent` 新增 `PermissionQueued` / `PermissionResolved`。后台 worker 的 child engine permission callback 现在由 supervisor 包装，在真正等待用户响应前后发出 agent permission lifecycle event。
4. Rust TUI `App` 已消费 worker permission lifecycle：收到 `PermissionQueued` 时会把当前 agent 切到对应 worker，并通过 `worker_pending_permission`/`worker_badge` 生成低优先级通知；`PermissionResolved` 会同步当前 agent 焦点。
5. 新增共享权限事件 payload：
   - `HookPermissionDecisionEvent`
   - `PermissionDecisionDebugEvent`
   - callback 载体 `PermissionEventPayload`
6. `QueryEngine` / `cc-ipc-client` / Rust TUI 已新增非阻塞 permission event callback。当前事件来源包括：
   - `PreToolUse` hook 的 allow / deny 覆盖
   - `PermissionRequest` hook 的 allow / deny 决策
   - central permission decision 的 verbose/debug trace
7. headless `BackendMessage` 新增：
   - `HookPermissionDecision`
   - `PermissionDecisionDebug`
   Rust TUI direct path 则通过 `EngineEvent` 同步消费，并在通知区渲染 `permissions::hooks` 与 `permission_decision_debug_info`。
8. 本轮真正解除 test-only 的模块只有：
   - `permissions::hooks`
   - `permissions::permission_decision_debug_info`
   - `permissions::worker_badge`
   - `permissions::worker_pending_permission`
   - `utils::render_key_values`
   - `utils::render_bullets`

仍保持 `#[cfg(test)]` 的模块与原因：

1. `permission_dialog`、`permission_prompt`、`permission_request`、`permission_request_title`、`permission_explanation`、`permission_rule_explanation`、`use_shell_permission_feedback`
原因：当前生产 `PermissionDialog` 已接到结构化 router，但这些 helper 仍只有快照消费点；若强行解禁会重新引入未使用 warning。
2. `file_permission_dialog::{file_permission_dialog, ide_diff_config, use_file_permission_dialog, use_permission_handler}`
原因：`permission_options` 已在生产 file-tool router 中使用，但完整 file dialog state / IDE diff / decision helper 仍缺真实 TUI 入口。
3. `rules::{add_permission_rules, add_workspace_directory, permission_rule_input, recent_denials_tab, remove_workspace_directory, workspace_tab}` 及其 `WorkspaceDirectory` / `RecentDenial`
原因：现有 `/permissions` command surface 仍走表单 tab 和 slash-command 填充，尚未切到这些 renderer；为了保持零 warning，本轮不解除。

因此，本轮完成的是“事件来源补齐并接通实际展示”的那部分；其余未解禁模块已经在文档中明确列为仍缺生产入口，而不是靠 `allow(dead_code)` 假装完成。
4. 缺少必要结构化字段的已知工具会明确 fallback，这是有意行为；后续若某工具需要专用 UI，应先补齐 payload，而不是恢复 description 硬解析。
5. 权限对话框底部操作在极窄宽度下会降级为紧凑文案；宽度非常小时的选中态可读性仍需通过后续快照或 viewport 测试继续覆盖。

已执行的验证：

- `cargo check -p claude-code-rs`
- `cargo test -p claude-code-rs ui:: -- --nocapture --test-threads=1`
- `cargo test -p cc-ipc-client`
- `cargo test -p cc-ipc-protocol`
- `cargo build --workspace --release`
