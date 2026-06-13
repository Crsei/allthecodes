# Computer Use — 桌面原生控制工具集

> 功能门控：`--computer-use` 命令行参数
> 实现状态：原生 Rust 实现，跨平台（macOS / Windows / Linux）
> 工具名前缀：`mcp__computer-use__*`

## 一、功能概述

Computer Use 让模型可以直接操控用户的桌面——移动鼠标、点击、键盘输入、截取屏幕截图。与 TypeScript 参考实现（仅支持 macOS）不同，allthecodes 的 Rust 移植使用原生平台 API 实现了三平台支持，无需依赖 Swift 辅助进程或 osascript。

## 二、实现架构

### 2.1 Crate 结构

`allthecodes-computer-use` crate 包含以下模块：

| 模块 | 文件 | 职责 |
|------|------|------|
| `tools` | `tools.rs` | 10 个 Tool trait 实现（截图/点击/键盘/滚动/鼠标移动/光标位置） |
| `executor` | `executor.rs` | 工作流编排：加锁 -> 预执行 -> 操作 -> 后执行 -> 解锁 |
| `input` | `input/` | 平台原生输入模拟（鼠标、键盘） |
| `screenshot` | `screenshot/` | 跨平台截图捕获 |
| `lock` | `lock.rs` | 并发锁，防止同时执行 CU 操作 |
| `drain_run_loop` | `drain_run_loop.rs` | 事件循环排空（macOS CGEvent） |
| `esc_hotkey` | `esc_hotkey.rs` | 紧急退出热键（三连击 Escape 中止） |
| `host_adapter` | `host_adapter.rs` | 宿主机能力探测与权限检查 |
| `app_names` | `app_names.rs` | 应用名称映射与启动/聚焦 |
| `input_loader` | `input_loader.rs` | 按平台选择最佳输入方法 |
| `swift_loader` | `swift_loader.rs` | macOS Swift 辅助脚本加载器 |
| `detection` | `detection.rs` | CU 工具风险等级分类与动作提取 |
| `setup` | `setup.rs` | 动态工具注册入口 |
| `win32/` | `win32/` | Windows 特有模块（COM 自动化、UI Automation、虚拟光标） |

### 2.2 工具清单

10 个工具通过 `register_cu_tools()` 注册：

| 工具名 | 类型 | 只读 | 输入参数 |
|--------|------|------|----------|
| `mcp__computer-use__screenshot` | 截图 | 是 | 无 |
| `mcp__computer-use__cursor_position` | 光标位置 | 是 | 无 |
| `mcp__computer-use__left_click` | 左键点击 | 否 | x, y |
| `mcp__computer-use__right_click` | 右键点击 | 否 | x, y |
| `mcp__computer-use__middle_click` | 中键点击 | 否 | x, y |
| `mcp__computer-use__double_click` | 双击 | 否 | x, y |
| `mcp__computer-use__type_text` | 键盘输入 | 否 | text |
| `mcp__computer-use__key` | 按键组合 | 否 | key |
| `mcp__computer-use__scroll` | 滚轮 | 否 | x, y, amount |
| `mcp__computer-use__mouse_move` | 鼠标移动 | 否 | x, y |

### 2.3 数据流

```
模型调用 CU 工具
       │
       ▼
  cu_check_permissions()  ← 三级权限检查
       │                   1. Session-level grant
       │                   2. Persistent allow rules
       │                   3. Ask user（含风险等级标注）
       ▼
  Executor 实例
       │
       ├── acquire_lock() — 获取全局并发锁
       ├── pre_drain()    — 排空事件循环
       ├── execute()      — 执行具体操作
       │     ├── input::execute_input()
       │     └── screenshot::capture_screenshot()
       ├── post_drain()   — 再次排空事件循环
       └── 释放锁
       │
       ▼
  ToolResult 返回给模型
```

### 2.4 权限模型

三级权限检查机制：

1. **会话级授权（Session Grant）**：同一会话内首次批准后不再重复询问
2. **持久化允许规则（Always Allow Rules）**：用户预先配置的永久允许列表
3. **用户询问**：根据动作风险等级标注 `[medium risk]` 或 `[HIGH RISK]`

风险等级由 `classify_risk()` 函数判定：截图、光标位置为低风险；点击、键盘输入、鼠标移动为高风险。

## 三、跨平台支持

### 3.1 输入模拟（input 模块）

| 功能 | macOS | Windows | Linux |
|------|-------|---------|-------|
| 鼠标移动 | CGEvent | SetCursorPos | xdotool |
| 鼠标点击 | CGEvent | SendInput | xdotool click |
| 键盘按键 | CGEvent | keybd_event | xdotool key |
| 文本输入 | CGEvent | SendKeys | xdotool type |

### 3.2 截图（screenshot 模块）

| 平台 | 方式 |
|------|------|
| macOS | screencapture CLI / CGDisplay |
| Windows | 平台 API |
| Linux | xdotool + xrandr |

### 3.3 事件循环处理

- **macOS**：CGEvent pump 确保事件被处理
- **非 macOS**：无需 pump，直接执行操作
- **ESC 热键**：macOS 使用 CGEventTap；其他平台跳过，使用 Ctrl+C fallback

## 四、关键设计决策

1. **原生 Rust 实现**：不依赖 Swift/Node.js 辅助进程，直接调用平台 API
2. **并发锁**：通过全局 `ComputerUseLock` 防止同时操作冲突
3. **配置化 Executor**：`ExecutorConfig` 控制锁、drain、超时等行为
4. **Win32 增强**：Windows 平台额外支持 COM 自动化（Word/Excel）、UI Automation 树导航、虚拟光标

## 五、使用方式

```bash
# 启动时启用 Computer Use
allthecodes --computer-use

# 或通过配置文件启用
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-computer-use/src/tools.rs` | 10 个 Tool 实现 |
| `crates/allthecodes-computer-use/src/executor.rs` | 工作流编排与锁管理 |
| `crates/allthecodes-computer-use/src/input/` | 跨平台输入模拟 |
| `crates/allthecodes-computer-use/src/screenshot/` | 跨平台截图 |
| `crates/allthecodes-computer-use/src/input_loader.rs` | 输入方法选型 |
| `crates/allthecodes-computer-use/src/detection.rs` | 风险分类 |
| `crates/allthecodes-computer-use/src/host_adapter.rs` | 权限探测 |
| `crates/allthecodes-computer-use/src/setup.rs` | 工具注册入口 |
| `crates/allthecodes-computer-use/src/win32/` | Windows 特有模块集 |
