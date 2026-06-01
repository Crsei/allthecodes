# 应用外壳对比：TypeScript vs Rust

> `claude-code-bun`（TypeScript）与 `cc-rust`（Rust/ratatui）之间应用外壳（App 组件、状态栏、事件处理、滚动、会话 UI）的全面对比。

生成日期：2026-05-17
上游参考：`F:\AIclassmanager\cc\claude-code-bun\src\**`、`F:\AIclassmanager\cc\src\**`

---

## 汇总表

| 领域 | Rust 完成度 | 关键差距严重程度 |
|------|:-----------:|:--------------:|
| 应用外壳 / 主循环 | 4/5 | 轻微 |
| 状态栏 | 4/5 | 轻微 |
| 语音模式 | 4/5 | 轻微 |
| 代理导航 | 2/5 | 严重 |
| 通知（桌面） | 4/5 | 轻微 |
| 通知（终端内） | 1/5 | 严重 |
| 对话记录 / 视图模式 | 4/5 | 轻微 |
| 虚拟滚动 | 5/5 | 无 |
| 历史 / 搜索 | 3/5 | 中等 |
| 欢迎屏幕 | 3/5 | 中等 |
| 事件处理 / 按键绑定 | 5/5 | 无 |
| 帧调度 | 5/5 | 无 |

---

## 1. 应用外壳 / 主循环

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app.rs` — `App` 结构体，包含所有状态字段、`AppAction` 枚举
- `rust/crates/claude-code-rs/src/ui/tui.rs` — `run_tui()` 主事件循环：终端初始化、通道、事件读取线程、tokio::select! 分发
- `rust/crates/claude-code-rs/src/ui/app/render.rs` — 完整布局管线：欢迎/消息、底部面板（旋转指示器/建议/粘贴提示/输入/状态）、对话记录/聚焦装饰、工作区信任提示、覆盖层
- `rust/crates/claude-code-rs/src/ui/app/app_event.rs` — 类型化 `AppEvent` 枚举（Backend、LocalNotice、Tick、Shutdown）
- `rust/crates/claude-code-rs/src/ui/app/app_event_sender.rs` — 类型化发送器封装，提供 `channel()` 工厂
- `rust/crates/claude-code-rs/src/ui/app/input.rs` — 完整的按键/鼠标/粘贴事件处理、按键绑定注册表解析、和弦支持、vim 集成、消息操作（复制/选择/导航）、历史上下翻动
- `rust/crates/cc-teams/src/loaded_threads.rs` — 多代理恢复的线程树过滤
- `rust/crates/cc-commands/src/rewind.rs` — `/rewind` 回退命令逻辑
- `rust/crates/cc-ipc-protocol/src/protocol/` — 命令协议（`FrontendMessage`）
- `rust/crates/cc-ipc-adapters/src/lib.rs` — 服务端/后端消息适配器
- `rust/crates/cc-ipc-client/src/requests.rs` — 服务端请求类型
- `rust/crates/claude-code-rs/src/ui/app/workspace_trust.rs` — 工作区信任门控
- `rust/crates/claude-code-rs/src/ui/app/transcript_mode.rs` — 对话记录模式的按键处理
- `rust/crates/claude-code-rs/src/ui/runtime/frame_requester.rs` — 带原因的合帧请求调度器（Input、Stream、Resize、Timer、Overlay）
- `rust/crates/claude-code-rs/src/ui/runtime/streaming_controller.rs` — 流式 delta 状态机（Assistant、Thinking、ToolCall、Final）

### TS 文件
- `claude-code-bun/src/components/App.tsx` — 轻量 Provider 包装器（约 55 行）：将 FpsMetricsProvider + StatsProvider + AppStateProvider 包裹在子元素外
- `claude-code-bun/src/screens/REPL.tsx` — 268KB 主交互屏幕。包含所有 REPL 逻辑：输入处理、消息展示、权限提示、键盘快捷键、语音、恢复、桥接、协调器、代理任务状态、队友消息、群组集成、滚动处理、历史、通知、成本阈值、空闲返回、自动更新器、FPS 跟踪、可脚本化状态栏、提示、IDE 指示器等
- `claude-code-bun/src/screens/ResumeConversation.tsx` — 会话恢复逻辑，包裹 REPL
- `src/components/App.tsx` — 原始上游 App（Ink 的 Provider 包装器）

### Rust 完成度：4/5

### 当前状态
Rust 的 `App` 结构体是一个单体状态容器，包含约 50 个字段，涵盖 TUI 的各个方面：消息、滚动、流式传输、旋转指示器、权限对话框、主题、模型/后端/cwd/会话元数据、输出样式、权限/沙箱/精力/远程标签、会话成本、建议、历史、命令面板、命令界面、历史搜索、虚拟滚动、脏标记、按键绑定、vim、状态栏、视图模式、对话记录状态、终端环境配置、语音控制器、语音设置和工作区信任。事件处理通过健壮的 `AppAction` 枚举进行，涵盖 Submit、Abort、Quit、ScrollUp、ScrollDown、PermissionResponse、LspRecommendationResponse、ExportTranscript 和 CopyMessage。

`tui.rs` 主循环结构良好：专用 crossterm 事件读取线程向 mpsc 通道提供数据，主异步循环使用 `tokio::select!` 带偏置优先级（shutdown > 按键事件 > 引擎事件 > 子系统事件 > 滴答计时器），所有渲染均由脏标记控制。同步更新转义序列通过 `CLAUDE_CODE_NO_FLICKER` 条件性发出。

### 与 TS 相比缺失的功能
- **空闲返回对话框**（TS：`IdleReturnDialog.tsx`）— 长时间无操作后确认用户意图
- **成本阈值对话框**（TS：`CostThresholdDialog.tsx`）— 接近成本限制时发出警告
- **退出确认流程**（TS：`ExitFlow.tsx`）— 带确认的优雅退出
- **协调器代理状态**（TS：`CoordinatorAgentStatus.tsx`）— 内联显示多代理协调状态
- **会话后台提示**（TS：`SessionBackgroundHint.tsx`）— 显示会话正在后台运行
- **队友视图标题**（TS：`TeammateViewHeader.tsx`）— 多代理模式下显示队友信息
- **工作线程待处理权限**（TS：`WorkerPendingPermission.tsx`）— 沙箱工作线程权限显示
- **Sentry 错误边界**（TS：`SentryErrorBoundary`）— React 错误边界（ratatui 不适用）
- **FPS 指标显示**（TS：`FpsMetricsProvider`）— 渲染性能调试覆盖层
- **隐身模式提示**（TS：`UndercoverAutoCallout.tsx`）— 隐身模式指示器

### 影响
低。核心应用外壳功能（渲染循环、事件分发、消息显示、输入处理）已全部实现。缺失的对话框是辅助性的 UX 优化项，不影响核心功能。

---

## 2. 状态栏

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/status.rs` — `SessionUsageSnapshot`、`update_session_usage()`、`set_status_line_settings()`、`sync_status_context_from_state()`、`build_status_payload()`、`trigger_status_refresh()`
- `rust/crates/claude-code-rs/src/ui/app/render.rs`（第 278-376 行）— `render_status_bar()` 函数：自定义可脚本化状态栏拥有完全优先权，内置后备显示模型名称、消息数、会话成本、vim 模式、权限模式、沙箱标签、精力标签、远程指示器、流式/就绪状态以及键盘快捷键提示
- `rust/crates/claude-code-rs/src/ui/status_line/` — 状态栏运行器、负载、解析器模块

### TS 文件
- `claude-code-bun/src/components/BuiltinStatusLine.tsx` — 显示模型名称、上下文使用百分比及 token 数量、5 小时会话速率限制（含进度条和倒计时）、7 天周速率限制（含进度条和倒计时）、会话成本。使用 memo 优化，60 秒重渲染用于倒计时更新。
- `claude-code-bun/src/components/StatusLine.tsx` — 可配置的自定义状态栏命令运行器。构建丰富的负载（会话名称、模型信息、工作区、速率限制、token、成本、vim 模式、代理类型、版本）。
- `claude-code-bun/src/commands/statusline.tsx` — 斜杠命令，用于从 shell PS1 设置状态栏
- `claude-code-bun/src/commands/status/status.tsx` — 状态命令
- `claude-code-bun/src/components/PromptInput/PromptInputFooter.tsx` — 复合页脚，渲染通知 + 状态栏 + 建议 + 帮助菜单
- `claude-code-bun/src/components/PromptInput/PromptInputFooterLeftSide.tsx` — 页脚左侧（vim 指示器、模式指示器）

### Rust 完成度：4/5

### 当前状态
Rust 同时具备自定义可脚本化状态栏（通过 `StatusLineRunner` 实现，含限流刷新、负载指纹识别、运行中中止和错误回退标记）和内置后备页脚。后备页脚显示模型名称、消息数、会话成本、vim 指示器、权限模式、沙箱标签、精力标签、远程指示器、流式/就绪状态以及键盘快捷键提示。自定义状态栏负载包含 session_id、model_id、backend、cwd、输入/输出/缓存 token、总成本、API 调用次数、输出样式名称、编辑模式、工作树状态、流式标记和消息数。

### 与 TS 相比缺失的功能
- **速率限制进度条**（TS：`BuiltinStatusLine`）— 5 小时和 7 天速率限制使用率以进度条形式显示，含倒计时
- **上下文使用百分比**（TS：`BuiltinStatusLine`）— 显示 `%` 和 `已用 token / 上下文窗口大小`
- **60 秒自动刷新**（TS：`BuiltinStatusLine`）— 倒计时通过 setInterval 更新
- **窄/宽布局自适应**（TS：`BuiltinStatusLine`）— 根据终端宽度在紧凑和详细显示之间切换（`columns < 60`、`columns >= 100`）
- **建议页脚**（TS：`PromptInputFooterSuggestions.tsx`）— 在输入框下方渲染提示建议
- **帮助菜单**（TS：`PromptInputHelpMenu.tsx`）— 键盘快捷键参考
- **开发者栏**（TS：`DevBar.tsx`）— 仅开发模式下的调试信息装饰
- **沙箱提示页脚提示**（TS：`SandboxPromptFooterHint.tsx`）— 沙箱模式指示器
- **回溯状态粘性提示**（TS：`StickyBackStatus.tsx`）— 回溯导航状态

### 影响
低-中等。核心状态栏功能正常，具有良好的对等性。速率限制进度条和上下文使用率显示是 TS 用户依赖的可见 UI 功能。缺少它们会降低 Rust TUI 页脚的信息密度，但不会阻塞核心操作。

---

## 3. 语音模式

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/voice.rs` — `set_voice_controller()`、`set_voice_settings()`、`is_voice_ready()`、`begin_push_to_talk()`、`end_push_to_talk()`、`drain_voice_events()`
- App 结构体字段：`voice: Option<VoiceController>`、`voice_enabled: bool`、`voice_supported: bool`、`voice_language: String`
- 使用 `cc_voice` crate，内含 `NullAudioBackend` / `NullTranscriptionClient` 作为占位实现，真实后端可无侵入式替换
- 按键通话通过 KeybindingRegistry 动作 `voice:pushToTalk` 分发
- 语音状态在内置状态栏页脚中显示（voice:unsupported、recording、transcribing 标签）

### TS 文件
- `claude-code-bun/src/context/voice.tsx` — 语音上下文 Provider，包含 voiceState（idle/recording/processing）和 voiceError
- `claude-code-bun/src/components/PromptInput/VoiceIndicator.tsx` — 语音录制/处理指示器组件
- `claude-code-bun/src/components/PromptInput/Notifications.tsx` — 语音状态显示在通知区域，录制期间替换所有通知
- 由 `VOICE_MODE` 特性标志控制
- 在 `PromptInputFooter.tsx` 和 `REPL.tsx` 中引用

### Rust 完成度：4/5

### 当前状态
Rust 拥有结构良好的语音集成，包含专用控制器连接、通过按键绑定注册表的按键通话处理、状态栏集成，以及用于将转录结果拉入提示输入的干净 `drain_voice_events()` 机制。语音控制器在 TUI 启动时初始化，使用空后端作为占位（等待真实音频捕获接入）。

### 与 TS 相比缺失的功能
- **语音指示器组件**（TS：`VoiceIndicator.tsx`）— 录制/转写状态的专用视觉指示器（Rust 状态栏以文本标签替代）
- **语音错误显示**（TS：`Notifications.tsx`）— 语音错误在通知区域显示
- **真实音频捕获后端** — Rust 使用 NullAudioBackend，TS 使用 capture-napi

### 影响
低。语音在两个代码库中均由特性标志控制。Rust 的集成结构完整，仅缺视觉指示器和真实音频后端。按键通话流程已达到功能对等。

---

## 4. 代理导航

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/agent_navigation.rs` — `AgentNavigationState`，包含 `BTreeMap<String, AgentThreadEntry>`、插入顺序跟踪、`adjacent_thread_id()`、`active_agent_label()`、`render_agent_tree()`
- `rust/crates/cc-teams/src/loaded_threads.rs` — `find_loaded_subagent_threads_for_primary()` 树遍历

### TS 文件
- `claude-code-bun/src/components/agents/AgentNavigationFooter.tsx` — 含操作说明和 Ctrl+C/D 退出的导航页脚
- `claude-code-bun/src/components/agents/AgentsList.tsx` — 代理列表显示
- `claude-code-bun/src/components/agents/AgentDetail.tsx` — 代理详情视图
- `claude-code-bun/src/components/agents/AgentEditor.tsx` — 代理编辑器（向导）
- `claude-code-bun/src/components/agents/AgentsMenu.tsx` — 代理选择菜单
- `claude-code-bun/src/components/agents/ColorPicker.tsx` — 颜色选择组件
- `claude-code-bun/src/components/agents/ToolSelector.tsx` — 工具选择组件
- `claude-code-bun/src/components/agents/ModelSelector.tsx` — 模型选择组件
- `claude-code-bun/src/components/agents/new-agent-creation/` — 11 步创建向导
- `claude-code-bun/src/components/agents/generateAgent.ts` — 代理代码生成
- `claude-code-bun/src/components/agents/validateAgent.ts` — 代理配置验证
- `claude-code-bun/src/components/agents/agentFileUtils.ts` — 文件 I/O 工具函数
- `claude-code-bun/src/components/agents/types.ts` — 类型定义

### Rust 完成度：2/5

### 当前状态
Rust 拥有纯粹的代理线程导航数据模型（`AgentNavigationState`），包含插入顺序跟踪、相邻线程导航和代理树渲染。然而，整个模块被标记为 `#[allow(dead_code)]`——它**未**接入 App 结构体的渲染路径。`App` 结构体没有 `AgentNavigationState` 字段。渲染管线没有代理导航页脚。agent_navigation 模块存在但未被使用。

RATATUI_UI_PARITY.md 文档确认 Rust 已对等实现了 AgentsList、AgentDetail、AgentEditor、AgentsMenu、AgentNavigationFooter、ColorPicker、ToolSelector、ModelSelector、generate_agent、validate_agent、agent_file_utils、types、utils 以及所有 11 个向导步骤——但这些位于代码库的不同部分（可能在 OpenTUI 前端的 `agents/` 目录下，而非 ratatui 后端）。

### 与 TS 相比缺失的功能
- **代理导航页脚** — 完全未在 TUI 中渲染（模块为死代码）
- **代理列表 UI** — 无 ratatui 界面用于列出代理
- **代理详情/编辑器** — 无 ratatui 界面用于查看/编辑代理配置
- **代理创建向导** — 无 ratatui 界面用于创建新代理
- **代理树面板**（TS：`AgentTreePanel.tsx`）
- **协调器代理状态**（TS：`CoordinatorAgentStatus.tsx`）
- **代理进度行**（TS：`AgentProgressLine.tsx`）
- **队友视图标题**（TS：`TeammateViewHeader.tsx`）

### 补齐计划
详见 `docs/ui/great/plans/plan-05-agent-navigation.md`：
- 阶段 1: 将 `AgentNavigationState` 接入 `App` 结构体（移除 `#[allow(dead_code)]`）
- 阶段 2: 代理导航页脚渲染（多代理模式下的状态栏行）
- 阶段 3: 代理树面板覆盖层（↑↓ 选择，Enter 切换线程）
- 阶段 4: 代理列表视图（ratatui 样式化现有纯文本输出）
- 阶段 5: 协调器/队友状态面板
- 阶段 6: 移除死代码 + 测试（`agents/mod.rs` 设备路径清理）
- 估算: ~610 行新代码（主要工作是将已有代码接线和样式化）

### 影响
高。代理导航是多代理会话的核心功能。Rust ratatui 后端拥有数据模型，但尚未将其接入渲染管线。多代理模式下的用户无法通过 Rust TUI 查看代理列表、在代理之间切换或管理代理配置。这是应用外壳中最显著的差距。

---

## 5. 通知（桌面）

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/notifications/mod.rs` — 后端检测：`Osc9Backend` 优先用于 WezTerm/ghostty/iTerm2/kitty，`BelBackend` 作为回退。`for_method()` 工厂支持 `Auto`/`Osc9`/`Bel` 方法选择。
- `rust/crates/claude-code-rs/src/ui/notifications/bel.rs` — 通过 crossterm Command 发送 BEL（ASCII 0x07 铃声）通知
- `rust/crates/claude-code-rs/src/ui/notifications/osc9.rs` — OSC 9 转义序列通知（`\x1b]9;message\x07`）

### TS 文件
- `claude-code-bun/src/services/notifier.ts` — `sendNotification()` 多通道分发：iterm2、kitty、ghostty、terminal_bell、notifications_disabled。基于 `env.terminal` 自动检测。支持分发前的通知钩子。分析日志记录。
- 使用 `@anthropic/ink` 的 `TerminalNotification` 接口写入实际的终端转义序列

### Rust 完成度：4/5

### 当前状态
Rust 同时拥有 BEL（铃声）和 OSC 9 通知后端，并支持自动终端检测。检测逻辑正确识别 WezTerm、ghostty、iTerm2（通过 ITERM_SESSION_ID）和 kitty（通过 TERM=xterm-kitty）以支持 OSC 9。Windows Terminal（WT_SESSION）被排除在 OSC 9 之外。

### 与 TS 相比缺失的功能
- **Kitty 通知协议**（TS：`terminal.notifyKitty()`）— 含标题和唯一 ID 的富通知
- **Ghostty 通知协议**（TS：`terminal.notifyGhostty()`）— 含标题的富通知
- **iTerm2 通知**（TS：`terminal.notifyITerm2()`）— 与通用 OSC 9 分开
- **Apple Terminal 铃声检测**（TS：`isAppleTerminalBellDisabled()`）— 检查 macOS 设置
- **通知钩子**（TS：`executeNotificationHooks()`）— 分发前钩子执行
- **分析跟踪**（TS：`logEvent('tengu_notification_method_used', ...)`）
- **`/notify` 命令** — `/notify status` / `/notify test` / `/notify on` / `/notify off`
- **可配置通道** — 通过 `preferredNotifChannel` 设置

### 影响
低。核心桌面通知功能（BEL + OSC 9）覆盖了最常见的用例。缺失的功能是终端特定的富通知协议和配置部分。使用现代终端（kitty、ghostty）的用户可能无法获得最丰富的通知体验。

---

## 6. 通知（终端内 UI）

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/notifications/mod.rs` — （仅桌面，无终端内通知系统）
- `rust/crates/claude-code-rs/src/ui/app/render.rs` — 状态栏显示基本标签（权限模式、沙箱、精力、远程），但无通知队列

### TS 文件
- `claude-code-bun/src/components/PromptInput/Notifications.tsx` — 完整的终端内通知系统，包含：
  - 基于优先级的队列（当前通知 + 待处理队列）
  - JSX 和文本通知类型
  - 自动超时关闭
  - 分类：IDE 状态、语音状态、API 密钥状态、调试模式、详细 token 计数、token 警告、超量模式、自动更新器、内存使用、沙箱提示、外部编辑器提示
  - 语音录制状态替换所有通知
- `claude-code-bun/src/context/notifications.ts` — 通知上下文，包含 `addNotification()`、`removeNotification()`
- `claude-code-bun/src/hooks/notifs/` 中的多个通知钩子：
  - `useMcpConnectivityStatus.ts`
  - `useLspInitializationNotification.ts`
  - `usePluginInstallationStatus.ts`
  - `usePluginAutoupdateNotification.ts`
  - `useRateLimitWarningNotification.ts`
  - `useDeprecationWarningNotification.ts`
  - `useNpmDeprecationNotification.ts`
  - `useIDEStatusIndicator.ts`
  - `useModelMigrationNotifications.ts`
  - `useTeammateLifecycleNotification.ts`
  - `useFastModeNotification.ts`
- `claude-code-bun/src/components/IdeStatusIndicator.tsx` — IDE 连接指示器
- `claude-code-bun/src/components/MemoryUsageIndicator.tsx` — 内存使用显示
- `claude-code-bun/src/components/TokenWarning.tsx` — Token 阈值警告
- `claude-code-bun/src/components/StatusNotices.tsx` — 状态通知定义与显示

### Rust 完成度：1/5

### 当前状态
Rust **没有**终端内通知系统。没有通知队列，没有优先级系统，没有通知超时机制，也没有布局中专用的通知区域。状态信息（权限模式、沙箱、精力、远程、语音）作为平面标签显示在状态栏页脚中。没有与 TS 的 `Notifications` 组件等效的实现。

RATATUI_UI_PARITY.md 文档指出 `StatusNotices.tsx`、`SubsystemStatus.tsx`、`IdeStatusIndicator.tsx`、`MemoryUsageIndicator.tsx`、`EffortIndicator.ts` 和 `PrBadge.tsx` 已通过 `components/status_widget.rs` 和 `app/render.rs` 实现对等——但这似乎针对 OpenTUI 前端，而非 ratatui 后端。ratatui 后端没有等效的通知部件。

### 与 TS 相比缺失的功能
- **通知队列**（优先级/超时/分类）— 完全缺失
- **IDE 状态指示器** — 无 ratatui 等效实现
- **内存使用指示器** — 无 ratatui 等效实现
- **Token 警告** — 无 ratatui 等效实现
- **API 密钥状态显示** — 无 ratatui 等效实现
- **自动更新器通知** — 无 ratatui 等效实现
- **外部编辑器提示** — 无 ratatui 等效实现
- **超量模式通知** — 无 ratatui 等效实现
- **调试模式指示器** — 无 ratatui 等效实现
- **插件自动更新通知** — 无 ratatui 等效实现
- **LSP 初始化通知** — 无 ratatui 等效实现
- **MCP 连接状态** — 无 ratatui 等效实现
- **速率限制警告** — 无 ratatui 等效实现

### 补齐计划
详见 `docs/ui/great/plans/plan-04-notification.md`：
- 阶段 1: 核心 `NotificationQueue` 数据结构（优先级/超时/折叠/失效）
- 阶段 2: 集成到 `App` 结构体（`notifications: NotificationState` 字段）
- 阶段 3: 通知渲染布局（状态栏上方 1-2 行横幅区域）
- 阶段 4: 6-8 个通知钩子（IDE 状态、Token 警告、API Key、内存使用等）
- 阶段 5: 自动更新通知
- 估算: ~750 行新代码

### 影响
高。终端内通知系统是关键的用户体验元素，用于展示重要的系统状态（IDE 连接、内存压力、token 警告、API 密钥问题等）。没有它，用户会错过关于系统状态的关键反馈。这是 Rust ratatui 后端最大的 UI 差距之一。

---

## 7. 对话记录 / 视图模式

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/runtime/transcript.rs` — `ViewMode` 枚举（Prompt/Transcript/Focus）、`TranscriptState`（scroll_offset、input_mode、query、matches、focused）、`SearchMatch`、`TranscriptEntry`、`FocusView`、`search_messages()`、`build_focus_view()`、`render_markdown_dump()`、`transcript_entries()`
- `rust/crates/claude-code-rs/src/ui/app/render.rs`（第 43-46 行、第 380-534 行）— `render_transcript()`、`render_focus_view()`、`render_transcript_header()`、`render_transcript_footer()`
- 视图模式通过 `Ctrl+O` 或 `app:toggleTranscript` 按键绑定动作循环切换
- 聚焦模式隐藏所有装饰，用于截图/屏幕阅读器
- 对话记录模式支持增量搜索（不区分大小写的子串）、关键字匹配导航（`n`/`N`）、导出到 `$EDITOR`（`e`）、跳转到顶部/底部（`g`/`G`）

### TS 文件
- `claude-code-bun/src/screens/REPL.tsx` — 对话记录逻辑嵌入在庞大的 REPL 组件中
- `claude-code-bun/src/components/ScrollKeybindingHandler.tsx` — 滚动按键处理
- `claude-code-bun/src/components/VirtualMessageList.tsx` — 支持虚拟滚动的消息列表
- TS 使用 Ink 的 `useSearchHighlight` 和类似浏览器的 `TabStatusKind` 进行视图模式跟踪

### Rust 完成度：4/5

### 当前状态
Rust 拥有干净、结构良好的对话记录系统，包含三种专用视图模式（Prompt、Transcript、Focus）。`TranscriptState` 与主 `App` 结构体清晰分离。搜索支持不区分大小写的子串匹配、匹配循环和导出到外部编辑器。聚焦模式移除所有装饰以便干净截图。

### 与 TS 相比缺失的功能
- **丰富的搜索高亮**在消息正文中（TS：`useSearchHighlight`）— Rust 仅在页眉中显示匹配位置
- **按回车继续**用于长输出（TS：`PressEnterToContinue.tsx`）
- **上下文可视化**（TS：`ContextVisualization.tsx`）— 显示上下文中的内容

### 影响
低。对话记录系统功能完整。搜索和导航正常工作。聚焦模式已存在。缺失的功能是增强而非核心差距。

---

## 8. 虚拟滚动

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/rendering/virtual_scroll.rs` — `VirtualScroll` 结构体，包含：
  - 双缓存：逻辑高度（每个渲染行占 1 行）和考虑宽度的自动换行视觉高度
  - 前缀和偏移数组，支持二分查找可见范围（`partition_point`）
  - `OVERSCAN` 常量（视口上方/下方各 40 行）
  - 宽度变化自动失效
  - 基于渲染上下文缓存键的失效
  - `invalidate_from()` / `invalidate_all()` 用于增量更新
  - 全面的测试覆盖：宽度变化重算、视觉范围边界测试、缓存失效

### TS 文件
- `claude-code-bun/src/components/VirtualMessageList.tsx` — 虚拟消息列表组件
- `claude-code-bun/src/hooks/useTerminalSize.ts` — 终端尺寸变化跟踪
- Ink 框架原生处理布局/重排（与虚拟滚动不同）

### Rust 完成度：5/5

### 当前状态
Rust 的虚拟滚动实现非常彻底。双缓存系统（逻辑高度 vs. 自动换行的视觉高度）比 TS 所需的方式（Ink 在 React 协调器中原生处理布局）更为复杂。基于二分查找的可见范围提供了 O(log n) 的性能。宽度变化失效和基于渲染上下文键的缓存防止了不必要的重算。

### 与 TS 相比缺失的功能
- 无显著缺失。Rust 实现可以说比 TS 对应部分更为复杂，因为 ratatui 需要显式滚动管理，而 Ink 隐式处理。

### 影响
无。这是 Rust 实现的优势所在。

---

## 9. 历史 / 搜索

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/input.rs` — `history_up()`、`history_down()`、`open_history_search()`、`seed_persistent_history()`、`push_history()`
- `rust/crates/claude-code-rs/src/ui/rendering/history_cell.rs` — 类型化 `HistoryCell` 枚举（User/Assistant/System/Tool/Diff/Status），含 `HistoryRenderMode`（Prompt/Transcript）
- `rust/crates/claude-code-rs/src/ui/history_search_dialog.rs` — Ctrl+R 历史搜索覆盖层
- 通过 `load_persistent_history_for_workspace()` 从持久化后端存储加载历史
- 命令面板/`command_surface` 用于斜杠命令

### TS 文件
- `claude-code-bun/src/screens/REPL.tsx` — 历史管理嵌入在 REPL 中
- `claude-code-bun/src/history.ts` — 历史抽象，包含 `addToHistory`、`removeLastFromHistory`、`expandPastedTextRefs`、`parseReferences`
- `claude-code-bun/src/components/BaseTextInput.tsx` — 支持历史的文本输入
- `claude-code-bun/src/components/HistorySearchDialog.tsx` — 历史搜索对话框

### Rust 完成度：3/5

### 当前状态
Rust 支持会话内提示历史，包含上下箭头导航和 Ctrl+R 搜索对话框。已实现从工作区存储加载持久化历史。history_cell 类型系统比 TS 更丰富，支持类型化条目（User、Assistant、System、Tool、Diff、Status）和双渲染模式。

### 与 TS 相比缺失的功能
- **持久化历史写入** — Rust 可以**加载**持久化历史，但应用外壳代码中未显示跨会话持久化的保存机制
- **粘贴文本引用展开**（TS：`expandPastedTextRefs`）
- **历史搜索对话框模糊排序**（TS：使用模糊匹配，Rust 使用精确优先/模糊其次）
- **历史条目中的时间戳** — Rust 的 `HistorySearchEntry` 包含 `timestamp` 字段但未在 UI 中显示

### 影响
低-中等。会话内历史导航和搜索正常工作。主要差距在于跨会话持久化历史写入，这可能会影响依赖先前会话历史的用户。

---

## 10. 欢迎屏幕

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/render.rs` — 当 `show_welcome` 为 true 时渲染欢迎屏幕（在第一条用户/助手消息时关闭）
- `rust/crates/claude-code-rs/src/ui/..` — 欢迎组件显示版本、模型名称、会话 ID 和 CWD

### TS 文件
- `claude-code-bun/src/components/WelcomeScreen.tsx` — 完整欢迎屏幕
- `claude-code-bun/src/components/LogoV2/` — 带动画的 Logo，含 17 个支持文件（LogoV2、实验、门控覆盖、渠道通知）
- `claude-code-bun/src/components/Onboarding.tsx` — 首次运行引导流程

### Rust 完成度：3/5

### 当前状态
Rust 有一个基本的欢迎屏幕，显示版本、模型、会话 ID 和 CWD。它功能完整、信息充足但风格简洁。

### 与 TS 相比缺失的功能
- **Logo V2 动画**（TS：18 个文件）— 有意省略（ratatui 不支持动画）
- **引导流程**（TS：`Onboarding.tsx`）— 首次运行体验
- **实验注册通知**（TS：`ExperimentEnrollmentNotice.tsx`）
- **门控覆盖警告**（TS：`GateOverridesWarning.tsx`）
- **渠道通知**（TS：`ChannelsNotice.tsx`）

### 影响
低。欢迎屏幕是一个临时显示。动画 Logo 和引导流程是 ratatui 后端有意省略的内容。

---

## 11. 事件处理 / 按键绑定

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/app/input.rs` — 完整的按键事件处理管线：
  - 按键绑定注册表，支持和弦（`KeybindingRegistry.resolve_chord()`）
  - 上下文感知解析（MessageActions、Transcript、Chat、Scroll、Global 上下文）
  - Vim 模式集成（VimState + VimAction 分发）
  - 消息操作（选择上/下一条、展开、复制、主引用）
  - 滚动处理（PageUp/Down、Ctrl+U/D、Shift+Up/Down、鼠标滚轮）
  - 历史上下翻动/搜索
  - 工作区信任按键处理
  - 权限对话框按键处理
  - 命令界面按键处理
  - 历史搜索对话框按键处理
  - 粘贴事件处理
  - 鼠标滚动事件

- `rust/crates/claude-code-rs/src/ui/app/app_event.rs` — 用于后端到 TUI 路由的 `AppEvent` 枚举
- `rust/crates/claude-code-rs/src/ui/app/app_event_sender.rs` — 类型化事件发送器

### TS 文件
- `claude-code-bun/src/screens/REPL.tsx` — 通过 `useInput` 钩子处理输入
- `claude-code-bun/src/keybindings/useKeybinding.ts` — 按键绑定钩子
- `claude-code-bun/src/hooks/useExitOnCtrlCDWithKeybindings.ts` — 退出处理
- Ink 框架的 `useInput` 用于原始按键事件

### Rust 完成度：5/5

### 当前状态
Rust 的事件处理全面且架构良好。按键绑定注册表支持多键和弦、按上下文解析绑定以及自定义用户定义的按键绑定。所有主要事件类型（键盘、鼠标滚轮、粘贴）均已处理。Vim 模式已完全集成。该系统比 TS 基于钩子的方式更显式、更可测试。

### 与 TS 相比缺失的功能
- 无显著缺失。Rust 按键绑定系统与 TS 对等或更优。

### 影响
无。这是 Rust 实现的优势所在。

---

## 12. 帧调度

### Rust 文件
- `rust/crates/claude-code-rs/src/ui/runtime/frame_requester.rs` — `FrameRequester`，包含待处理标记、请求计数、上次原因跟踪。原因为：Input、Stream、Resize、Timer、Overlay。

### TS 文件
- `claude-code-bun/src/context/fpsMetrics.tsx` — FPS 指标 Provider
- Ink 的内部渲染调度（相当于 requestAnimationFrame）

### Rust 完成度：5/5

### 当前状态
Rust 拥有显式且设计良好的帧调度系统。`FrameRequester` 支持合并多个帧请求并跟踪每个请求的原因。App 中的脏标记模式防止不必要的绘制。TUI 主循环使用 16ms 滴答间隔（60fps），配合 `MissedTickBehavior::Skip` 避免帧堆积。帧请求器快照可用于诊断。

### 与 TS 相比缺失的功能
- 无。Rust 帧调度比 TS/Ink 的内部调度更为显式。

### 影响
无。这是 Rust 实现的优势所在。

---

## Rust 中完全缺失的内容（应用外壳领域）

以下功能/组件存在于 TypeScript 应用外壳中，但在 Rust ratatui 后端中**没有**等效实现：

| 功能 | TS 位置 | 类型 | 优先级 | 补齐计划 |
|------|---------|------|:------:|:--------:|
| 终端内通知队列 | `PromptInput/Notifications.tsx` | 系统状态显示 | P1 | `plans/plan-04-notification.md` |
| IDE 状态指示器 | `IdeStatusIndicator.tsx` | 系统状态 | P1 | `plans/plan-04-notification.md` |
| 代理管理 UI（列表/详情/编辑器） | `components/agents/`（40+ 文件） | 核心交互 | P1 | `plans/plan-05-agent-navigation.md` |
| 代理导航页脚（已接入） | `agents/AgentNavigationFooter.tsx` | 导航 | P1 | `plans/plan-05-agent-navigation.md` |
| 代理树面板 | `AgentTreePanel.tsx` | 导航 | P2 | `plans/plan-05-agent-navigation.md` |
| 协调器代理状态 | `CoordinatorAgentStatus.tsx` | 系统状态 | P2 | `plans/plan-05-agent-navigation.md` |
| 代理进度行 | `AgentProgressLine.tsx` | 系统状态 | P2 | — |
| 队友视图标题 | `TeammateViewHeader.tsx` | 协作 | P2 | — |
| 内存使用指示器 | `MemoryUsageIndicator.tsx` | 系统状态 | P2 | `plans/plan-04-notification.md` |
| Token 警告 | `TokenWarning.tsx` | 系统状态 | P2 | `plans/plan-04-notification.md` |
| 速率限制进度条含倒计时 | `BuiltinStatusLine.tsx` | 信息显示 | P2 | — |
| 上下文使用百分比显示 | `BuiltinStatusLine.tsx` | 信息显示 | P2 | — |
| 成本阈值对话框 | `CostThresholdDialog.tsx` | UX 对话框 | P2 | — |
| 空闲返回对话框 | `IdleReturnDialog.tsx` | UX 对话框 | P2 | — |
| 退出确认流程 | `ExitFlow.tsx` | UX 对话框 | P3 | — |
| 会话后台提示 | `SessionBackgroundHint.tsx` | UX 提示 | P3 | — |
| 会话预览 | `SessionPreview.tsx` | UX 对话框 | P3 | — |
| Logo V2 动画（有意省略） | `LogoV2/`（18 个文件） | 装饰 | P3（不修复）| — |
| 引导流程（有意省略） | `Onboarding.tsx` | 首次运行 UX | P3（不修复）| — |
| 隐身模式提示 | `UndercoverAutoCallout.tsx` | 状态指示器 | P3 | — |
| 全局搜索对话框 | `GlobalSearchDialog.tsx` | 导航 | P3 | — |
| 快速打开对话框（Ctrl+P） | `QuickOpenDialog.tsx` | 导航 | P3 | — |

---

## Rust 相对 TS 的优势

除对等性外，Rust 实现还有一些领域比 TypeScript 版本更强或更显式：

1. **虚拟滚动**：双缓存（逻辑高度 + 视觉高度）配合二分查找可见范围，比 TS 隐式的 Ink 布局更复杂
2. **事件处理**：显式的 `AppAction` 枚举使事件流可测试且可预测，优于 TS 隐式的钩子回调
3. **帧调度**：显式的 `FrameRequester` 配合原因追踪，优于 TS 隐式的 React 重新渲染调度
4. **对话记录状态机**：干净的 `ViewMode` 枚举配合显式的 `TranscriptState` 分离，优于 TS 分散的状态管理
5. **按键绑定注册表**：完整的和弦支持和按上下文解析，优于 TS 更简单的 `useKeybinding` 钩子
6. **状态栏运行器**：基于负载指纹的限流刷新，优于 TS 的 `setInterval` 轮询
7. **通知后端**：干净的终端检测和 BEL/OSC9 自动选择，优于 TS 的单体 `sendToChannel` 分支
8. **历史单元格类型**：类型化的 `HistoryCell` 枚举配合双渲染模式，比 TS 的扁平字符串历史更丰富
9. **代理导航状态**：纯粹的 `render_agent_tree()` 数据模型设计良好（仅需接入）
10. **工作区信任**：欢迎屏幕前的显式门控
