# Computer Use 移植缺口 — 状态：核心路径已补，剩余 parity 缺口待验证

> **更新**: 2026-05-18 — 工具命名、Executor 串行化、真实锁、截图临时文件安全和 Linux Wayland capability 误报已修。Win32 深度自动化模块仍需完整接入工具/executor 可达路径并补真实桌面 smoke 证据。

对应: `claude-code-bun/src/utils/computerUse/`  
Rust 对应: `cc-computer-use/`

## Bun 端规模

| 文件 | 行数 | 说明 |
|------|------|------|
| executor.ts | 728 | macOS ComputerExecutor |
| executorCrossPlatform.ts | 1,154 | Win/Linux 执行器 |
| mcpServer.ts | 107 | MCP 服务器 |
| hostAdapter.ts | 117 | 主机适配器 |
| common.ts | 67 | 共享常量 |
| computerUseLock.ts | 215 | 并发锁 |
| drainRunLoop.ts | 80 | macOS CGEvent drain |
| escHotkey.ts | 55 | 退出热键检测 |
| appNames.ts | 196 | 应用名映射 |
| gates.ts | 69 | 功能开关 |
| inputLoader.ts | 30 | 输入加载器 |
| swiftLoader.ts | 21 | Swift 加载器 |
| toolRendering.tsx | 157 | 工具 UI 渲染 |
| wrapper.tsx | 452 | 包装组件 |
| **platforms/** | | |
| platforms/win32.ts | 984 | Windows 平台 |
| platforms/linux.ts | 517 | Linux 平台 |
| platforms/darwin.ts | 158 | macOS 平台 |
| platforms/types.ts | 153 | 平台类型 |
| **win32/** | | |
| win32/*.ts (13 文件) | ~3,000+ | Win32 特定实现 |
| **合计** | **~14,294** | |

## Rust 已实现

### cc-computer-use/ (完整度 ~95%)
- `input/mod.rs` + `input/darwin.rs` + `input/linux.rs` + `input/win32.rs`: 跨平台输入 ✅
- `screenshot/mod.rs` + `screenshot/darwin.rs` + `screenshot/linux.rs` + `screenshot/win32.rs`: 跨平台截图 ✅
- `detection.rs`: 平台检测 ✅
- `setup.rs`: 设置 ✅
- `tools.rs`: 工具定义 ✅
- `lock.rs`: 并发锁 ✅ (2026-05-18 新增)
- `esc_hotkey.rs`: 退出热键 ✅ (2026-05-18 新增)
- `app_names.rs`: 应用名映射 ✅ (2026-05-18 新增)
- `host_adapter.rs`: 主机适配器 ✅ (2026-05-18 新增)
- `drain_run_loop.rs`: 事件循环 drain ✅ (2026-05-18 新增)
- `executor.rs`: 执行器编排 ✅ (2026-05-18 新增)
- `input_loader.rs`: 输入加载器 ✅ (2026-05-18 新增)
- `swift_loader.rs`: Swift 加载器 ✅ (2026-05-18 新增)
- `win32/mod.rs` + 7 个子模块: Win32 特定实现 ✅ (2026-05-18 新增)

## Rust 已补齐的差异

### 1. Win32 COM 自动化 (Word/Excel) ✅
- `win32/com_word.rs`: Microsoft Word COM 自动化（创建、打开、编辑、格式、保存、PDF导出）
- `win32/com_excel.rs`: Microsoft Excel COM 自动化（创建、打开、单元格读写、公式、范围写入、图表、保存）

### 2. Win32 UI 自动化 ✅
- `win32/ui_automation.rs`: Windows UI Automation 树导航
- 按名称查找元素、属性读取、可访问性快照
- 屏幕坐标点击绑定

### 3. 虚拟光标 (268 行) ✅
- `win32/virtual_cursor.rs`: Windows 虚拟光标管理
- 窗口绑定的光标状态、坐标转换、全局单例

### 4. 窗口边框和管理 ✅
- `win32/window_border.rs`: 窗口边框度量、客户区转换、闪烁高亮
- `win32/shared.rs`: Win32 共享工具（HWND、PowerShell 运行器、窗口枚举）
- `win32/input_indicator.rs`: 输入指示器（鼠标位置可视化叠加层）

### 5. 事件循环 drain ✅
- `drain_run_loop.rs`: macOS CGEvent drain + 跨平台 fallback
- 便捷方法：drain_after_click(), drain_after_keyboard(), drain_after_move()

### 6. 退出热键 ✅
- `esc_hotkey.rs`: 可配置 Escape 检测（长按 / 三击）
- 跨平台实现（macOS/Linux/Windows）

### 7. 并发锁 ✅
- `lock.rs`: 基于 tokio::sync::Mutex 的计算机操作并发控制
- 支持 try_lock_timeout、全局单例

### 8. 应用名映射 ✅
- `app_names.rs`: 26 个应用条目（浏览器、终端、编辑器、Office、通信、媒体、工具）
- 大小写不敏感模糊查找、跨平台启动/聚焦

### 9. 主机适配器 ✅
- `host_adapter.rs`: 桌面应用主机集成 + 权限检测
- 平台能力探测（截图、输入、应用启动、窗口管理）
- 权限状态检查（macOS Accessibility/Screen Recording）

### 10. 编排执行器 ✅
- `executor.rs`: 统一入口，组合 lock + input + screenshot + drain
- click_and_type 等多步操作

## 关键差异总结（更新于 2026-05-18）

| 功能 | Bun | Rust | 差距 |
|------|-----|------|------|
| macOS 输入 | executor.ts (728 行) | input/darwin.rs | ✅ 已移植 |
| Linux 输入 | executorCrossPlatform.ts | input/linux.rs | ✅ 已移植 |
| Win32 输入 | executorCrossPlatform.ts | input/win32.rs | ✅ 已移植 |
| macOS 截图 | (通过 Swift) | screenshot/darwin.rs | ✅ 已移植 |
| Linux 截图 | | screenshot/linux.rs | ✅ 已移植 |
| Win32 截图 | (PrintWindow) | screenshot/win32.rs | ✅ 已移植 |
| 平台检测 | platforms/ | detection.rs | ✅ 已移植 |
| 工具定义 | mcpServer.ts | tools.rs | ✅ 已移植 |
| **Word COM 自动化** | win32/comWord.ts (448 行) | win32/com_word.rs | ✅ 已补齐 |
| **Excel COM 自动化** | win32/comExcel.ts (324 行) | win32/com_excel.rs | ✅ 已补齐 |
| **UI 自动化** | win32/uiAutomation.ts (353 行) | win32/ui_automation.rs | ✅ 已补齐 |
| **虚拟光标** | win32/virtualCursor.ts (268 行) | win32/virtual_cursor.rs | ✅ 已补齐 |
| **并发锁** | computerUseLock.ts (215 行) | lock.rs | ✅ 已补齐 |
| **退出热键** | escHotkey.ts (55 行) | esc_hotkey.rs | ✅ 已补齐 |
| 应用名映射 | appNames.ts (196 行) | app_names.rs | ✅ 已补齐 |
| 事件循环 drain | drainRunLoop.ts (80 行) | drain_run_loop.rs | ✅ 已补齐 |
| 主机适配器 | hostAdapter.ts (117 行) | host_adapter.rs | ✅ 已补齐 |
| 执行器编排 | executor.ts/CrossPlatform | executor.rs | ✅ 已补齐 |
| 输入加载器 | inputLoader.ts (30 行) | input_loader.rs | ✅ 已补齐 |
| Swift 加载器 | swiftLoader.ts (21 行) | swift_loader.rs | ✅ 已补齐 |
| 窗口边框管理 | win32/windowBorder.ts + shared.ts | win32/window_border.rs + shared.rs | ✅ 已补齐 |
| 输入指示器 | win32/inputIndicator.ts | win32/input_indicator.rs | ✅ 已补齐 |
| **工具渲染** | toolRendering.tsx (157 行) | 前端 UI 组件 | 保留为 UI 层职责 |
| **包装器** | wrapper.tsx (452 行) | 前端 UI 组件 | 保留为 UI 层职责 |
| 功能开关 | gates.ts (69 行) | 通过 features.rs 实现 | ✅ 已覆盖 |

## 仍为 UI 层职责的组件

以下为 TypeScript React/TSX 组件，属于前端渲染层而非 Rust 后端：

- **toolRendering.tsx** (157 行): MCP 工具调用结果的 UI 渲染逻辑
- **wrapper.tsx** (452 行): Computer Use 会话包装组件

这些应在 `ui/` (OpenTUI 前端) 中实现，不在 `cc-computer-use` 后端 crate 范围内。

## 当前剩余缺口（2026-05-18 review 后）

- Win32 COM 自动化、UI Automation、虚拟光标、窗口边框和输入指示器已有模块，但还需要接入注册工具或 executor 的实际可达路径，不能仅以模块存在作为 full parity 证据。
- Linux Wayland 输入 backend 仍未实现 `ydotool`；当前只避免误报 capability，输入动作仍依赖 X11 `xdotool`。
- 仍缺真实 macOS / Windows / Linux 桌面 smoke 证据，尤其是权限弹窗、截图回喂、点击/输入串行化和失败恢复。
- 工具渲染和 Computer Use wrapper 仍归 UI 层，需在 Rust TUI / 前端体验中单独验证。
