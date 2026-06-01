# 执行计划：终端内通知系统

> **生成日期**: 2026-05-18
> **来源**: `docs/ui/great/06_missing_features_summary.md` §5
> **参考实现**:
> - TS 通知组件: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/components/PromptInput/Notifications.tsx` (290 行)
> - TS 通知上下文: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/context/notifications.tsx` (291 行)
> - TS 通知钩子: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun/src/hooks/notifs/` (11 个钩子)
> - TS 子组件: `IdeStatusIndicator.tsx`, `MemoryUsageIndicator.tsx`, `TokenWarning.tsx`, `StatusNotices.tsx`
> - Rust 现有: `crates/claude-code-rs/src/ui/notifications/` (桌面通知, BEL/OSC9)
> - Rust 渲染管线: `crates/claude-code-rs/src/ui/app/render.rs` (681 行)
> - Rust 状态 widget: `crates/claude-code-rs/src/ui/components/status_widget.rs` (183 行, 纯文本)

---

## 当前状态

Rust 拥有完整的**桌面**通知系统（BEL 铃声 + OSC 9 转义序列，自动终端检测），但完全没有**终端内**通知系统：

- **无 `NotificationQueue` 数据结构** — 没有带优先级/超时/折叠/失效的队列概念
- **无通知上下文/提供者** — TS `useNotifications()` hook 支持 `addNotification()` / `removeNotification()` / `getNext()` 优先级调度
- **无专用通知布局区域** — 状态栏仅显示平面标签（权限/沙箱/精力/远程/语音），无通知横幅插槽
- **无 Toast/Banner 组件** — 没有 TS 的 `<Box>` 样式通知渲染
- **无通知钩子** — 11 个 TS hook (IDE 状态、内存使用、Token 警告、速率限制、自动更新器、LSP 等) 在 Rust 中均无对应
- 状态信息通过 `status_widget.rs` 中的 `StatusSnapshot` 以纯文本格式拼合到状态栏，而非独立通知

App 结构体 (`app.rs`) 不含任何通知相关字段。

## 目标状态

1. **`NotificationQueue`** — 带 4 级优先级 (`immediate`/`high`/`medium`/`low`) 的队列，支持超时自动清除、同 key 折叠 (`fold`)、key 失效 (`invalidates`)
2. **通知聚合提供者** — `NotificationProvider` 或嵌入 `App` 结构的 `notifications: NotificationState` 字段，包含 `current: Option<Notification>` 和 `queue: Vec<Notification>`，提供 `add_notification()` / `remove_notification()` 方法
3. **通知布局插槽** — 在 `render.rs` 底部区域中状态栏上方插入 1-2 行通知横幅，显示当前通知
4. **通知渲染** — 支持纯文本和富样式两种通知类型（ratatui `Span`/`Line` vs 纯 `String`），带主题感知颜色
5. **6-8 个通知钩子** — IDE 状态、内存使用、Token 警告、API Key 状态、自动更新器、外部编辑器提示、速率限制警告、调试模式指示器
6. **子组件集成** — 将现有 `status_widget.rs` 的部分指示器迁移或对接至通知系统

## TS 参考

### 核心数据结构 (`notifications.tsx`)

```
type Priority = 'low' | 'medium' | 'high' | 'immediate'
type BaseNotification = { key, invalidates?, priority, timeoutMs?, fold? }
type TextNotification = BaseNotification & { text, color? }
type JSXNotification = BaseNotification & { jsx: ReactNode }
```

`getNext(queue)`: 按优先级选择（`immediate` > `high` > `medium` > `low`）
`addNotification()`: 立即处理 `immediate` 优先级，否则入队+折叠检查
`removeNotification()`: 从 current 和 queue 中移除
`processQueue()`: current 超时到期后弹出下一个

### 通知渲染 (`Notifications.tsx`)

`NotificationContent` 组件返回有序列表:
1. `<IdeStatusIndicator>` — IDE 连接状态
2. `notifications.current` — 当前通知（`jsx` 或 `text` 格式）
3. 超量模式提示 (Overage mode)
4. `apiKeyHelper` 慢速提示
5. API Key 无效/缺失提示
6. Debug 模式指示器
7. 详细 Token 计数（verbose 模式）
8. `<TokenWarning>` — Token 阈值警告
9. 语音错误提示（VOICE_MODE）
10. `<MemoryUsageIndicator>` — 内存使用
11. `<SandboxPromptFooterHint>` — 沙箱提示

语音录制中时，**替换所有通知**为 `<VoiceIndicator>`

### 通知钩子 (`hooks/notifs/`)

目录包含 11 个文件: `useMcpConnectivityStatus`, `useLspInitializationNotification`, `usePluginInstallationStatus`, `usePluginAutoupdateNotification`, `useRateLimitWarningNotification`, `useDeprecationWarningNotification`, `useNpmDeprecationNotification`, `useIDEStatusIndicator`, `useModelMigrationNotifications`, `useTeammateLifecycleNotification`, `useFastModeNotification`

### 子组件
- `IdeStatusIndicator.tsx` — IDE 连接/选择状态
- `MemoryUsageIndicator.tsx` — 内存使用 + 阈值着色
- `TokenWarning.tsx` — Token 使用量警告
- `StatusNotices.tsx` — 启动时状态通知（配置警告等）
- `SandboxPromptFooterHint.tsx` — 沙箱模式提示

## Rust 当前代码

### `crates/claude-code-rs/src/ui/notifications/`
- `mod.rs` (157 行): `DesktopNotificationBackend` 枚举 (`Osc9`/`Bel`) + `detect_backend()` 自动检测
- `bel.rs`: BEL 铃声通知实现
- `osc9.rs`: OSC 9 转义序列通知

纯桌面通知，与终端内 UI 完全分离。

### `crates/claude-code-rs/src/ui/components/status_widget.rs`
- `StatusSeverity` 枚举: `Ok`/`Warning`/`Error`
- `StatusIndicator`: 标签+值+严重性，`render_inline()` / `render_detail()` 纯文本
- `StatusSnapshot`: 快照结构, `render_line()` / `render_details()` 纯文本

### `crates/claude-code-rs/src/ui/app/render.rs`
底部区域布局（第 57-218 行）:
```
spinner (1行, 仅流式) → suggestions (1行) → paste_notice (1行) → input (1行)
→ command_palette → command_arg_help → status_bar (1行)
```

当前**无通知区域插槽**。

### `crates/claude-code-rs/src/ui/app.rs` (App 结构体)
无通知相关字段或方法。

## 分步实施

### 阶段 1: 核心数据结构（~150 行）

- [ ] 1.1 新建 `crates/claude-code-rs/src/ui/notifications/in_app.rs`
  - 定义 `NotificationPriority` 枚举 (`Immediate`/`High`/`Medium`/`Low`)
  - 定义 `Notification` 结构体: `{ key: String, priority: NotificationPriority, timeout_ms: Option<u64>, color: Option<ThemeColor>, text: String, rendered: Option<Vec<Span<'static>>> }`
  - 定义 `NotificationState` 结构体: `{ current: Option<Notification>, queue: Vec<Notification> }`
  - 实现 `add_notification()`: 优先级折叠、入队、失效检查
  - 实现 `remove_notification()`: 从 current/queue 移除
  - 实现 `process_queue()`: 当前过期后取下一个（按优先级排序）
  - 实现 `get_next()`: 从队列选最高优先级

### 阶段 2: 集成到 App 结构体（~50 行）

- [ ] 2.1 将 `notifications: NotificationState` 字段添加到 `App` 结构体 (`app.rs`)
- [ ] 2.2 在 `app/app_event.rs` 中添加 `AppEvent::Notification` 变体（用于后端触发通知）
- [ ] 2.3 在 `App::handle_event()` 或新建 `App::handle_notification_event()` 中处理通知事件
- [ ] 2.4 在 `App::new()` 中初始化 `NotificationState::default()`

### 阶段 3: 通知渲染（~100 行）

- [ ] 3.1 在 `render.rs` 底部区域布局中增加 `notification_height` 计算（1-2 行，取决于是否有活跃通知）
- [ ] 3.2 新建 `render_notification()` 方法: 渲染当前通知（主题色 text/span）
- [ ] 3.3 通知区域位于 `status_bar` 上方，spinner/suggestions/input 下方
- [ ] 3.4 支持 `immediate` 优先级通知覆盖其他底部元素

### 阶段 4: 通知钩子（~300 行）

根据 Rust 子系统的实际可用数据，优先实现以下钩子:

- [ ] 4.1 **IDE 状态指示器** — `use_ide_status_indicator()`: 读取 LSP 服务连接状态，在 IDE 连接/断开/选择文件时发出通知。参考 TS `IdeStatusIndicator.tsx` + `useIdeConnectionStatus`
- [ ] 4.2 **Token 警告** — `use_token_warning_notification()`: 从流式控制器读取 token 使用量，在超过阈值时发出高优先级通知。参考 TS `TokenWarning.tsx`
- [ ] 4.3 **Debug 模式指示器** — 当 `debug` 配置启用时显示静态通知
- [ ] 4.4 **API Key 状态** — 从认证模块读取，key 无效/缺失时报错
- [ ] 4.5 **外部编辑器提示** — 输入行数超过阈值时显示编辑提示。参考 TS `getExternalEditor()`
- [ ] 4.6 **速率限制警告** — 从 API 响应头读取剩余配额，接近限制时发出警告
- [ ] 4.7 **内存使用指示器** — 通过系统调用获取 RSS，超过阈值时显示。参考 TS `MemoryUsageIndicator.tsx`
- [ ] 4.8 **MCP 连接状态** — MCP channel 连接状态变化时通知

### 阶段 5: 自动更新通知（~50 行）

- [ ] 5.1 从更新检查模块获取状态，在可用更新时显示通知
- [ ] 5.2 支持通知动作（`/update` 等）

### 阶段 6: 测试与验证（~100 行）

- [ ] 6.1 单元测试: 优先级排序、折叠、失效、过期清除
- [ ] 6.2 集成测试: 通知在渲染管线中的布局
- [ ] 6.3 快照测试: 通知渲染输出样式

## 工作量估算

| 阶段 | 内容 | 估算行数 | 复杂度 |
|:----:|------|:--------:|:------:|
| 1 | 核心数据结构 | ~150 | 中 |
| 2 | App 集成 | ~50 | 低 |
| 3 | 通知渲染 | ~100 | 中 |
| 4 | 通知钩子 (8个) | ~300 | 中高 |
| 5 | 自动更新通知 | ~50 | 低 |
| 6 | 测试 | ~100 | 低 |
| **合计** | | **~750** | |

## 依赖/前提

1. **无外部依赖** — 核心队列可用标准库实现
2. **时间模块** — 需要 `std::time::Instant` 用于超时管理
3. **现有主题** — 通知颜色使用现有 `Theme` 结构体字段（`info`、`warning`、`error`、`dim`）
4. **后续可选** — 通知可通过 `AppEvent::Notification` 桥接至 `tui.rs` 事件循环，使其可与后端交互
5. **与桌面通知并行** — 终端内通知系统应**补充**而非替代桌面通知（BEL/OSC9）；在发出终端内通知的同时可选择也发送桌面通知
