# Web Browser Tool — 浏览器自动化工具

> 功能门控：Chrome 扩展 + Native Messaging Host
> 实现状态：完整实现（MCP stdio 桥接模式）
> 通信链路：allthecodes -> MCP Bridge -> Native Host -> Chrome 扩展

## 一、功能概述

Web Browser Tool 让模型可以控制 Chromium 浏览器——导航网页、点击元素、填写表单、执行 JavaScript、读取控制台和网络请求。与 TypeScript 参考实现使用 Bun WebView API 不同，Rust 移植通过 Chrome 扩展 + Native Messaging Host 的方式实现，支持更丰富的浏览器交互能力。

## 二、实现架构

### 2.1 Crate 结构

`allthecodes-browser` crate 包含以下模块：

| 模块 | 文件 | 职责 |
|------|------|------|
| `common` | `common.rs` | 浏览器发现：跨平台路径、注册表、扩展 ID |
| `detection` | `detection.rs` | 浏览器安装检测与工具分类 |
| `mcp_bridge` | `mcp_bridge.rs` | MCP stdio 桥接服务（核心） |
| `native_host` | `native_host.rs` | Native Messaging Host 安装与管理 |
| `permissions` | `permissions.rs` | 浏览器权限管理 |
| `session` | `session.rs` | 浏览器会话管理 |
| `setup` | `setup.rs` | 安装向导与配置 |
| `state` | `state.rs` | 浏览器连接状态管理 |
| `tool_rendering` | `tool_rendering.rs` | 工具结果渲染 |
| `transport` | `transport.rs` | Unix Socket / Named Pipe 传输层 |

### 2.2 通信链路

```
allthecodes (MCP 客户端)
       │  MCP stdio 协议
       ▼
--claude-in-chrome-mcp (mcp_bridge.rs)
       │  带帧的 JSON over socket
       ▼
--chrome-native-host (native_host.rs)
       │  Native Messaging (4字节帧)
       ▼
Chrome 扩展 (公开版 / 内部版)
```

### 2.3 工具目录

MCP 桥接器暴露以下 9 个工具（定义在 `mcp_bridge.rs` 的 `tool_catalogue()` 中）：

| 工具名 | 描述 | 主要参数 |
|--------|------|----------|
| `navigate` | 导航活动标签页到 URL | url, tab_id |
| `tabs_context_mcp` | 获取当前标签页信息 | 无 |
| `tabs_create_mcp` | 创建新标签页 | url |
| `get_page_text` | 读取页面可见文本 | tab_id |
| `click` | 点击页面元素 | selector, tab_id |
| `form_input` | 填写表单字段 | selector, value, tab_id |
| `javascript_tool` | 在页面中执行 JavaScript | script, tab_id |
| `read_console_messages` | 读取控制台消息 | tab_id, pattern |
| `read_network_requests` | 读取网络请求 | tab_id, pattern |

### 2.4 浏览器发现

支持 7 种 Chromium 浏览器，按优先级排列：

| 浏览器 | macOS | Linux | Windows |
|--------|-------|-------|---------|
| Google Chrome | App Bundle | google-chrome 二进制 | 注册表 + Local AppData |
| Brave | App Bundle | brave-browser 二进制 | 注册表 + Local AppData |
| Arc | App Bundle | 不支持 | 注册表 + Local AppData |
| Microsoft Edge | App Bundle | microsoft-edge 二进制 | 注册表 + Local AppData |
| Chromium | App Bundle | chromium 二进制 | 注册表 + Local AppData |
| Vivaldi | App Bundle | vivaldi 二进制 | 注册表 + Local AppData |
| Opera | App Bundle | opera 二进制 | 注册表 + Roaming AppData |

### 2.5 MCP 桥接协议

`mcp_bridge.rs` 运行一个独立的 stdio MCP 服务器进程（`--claude-in-chrome-mcp`），通过以下步骤工作：

1. **初始化**：接收 MCP `initialize` 请求，返回协议版本和能力声明
2. **工具列表**：响应 `tools/list`，返回 9 个浏览器工具定义
3. **工具调用**：接收 `tools/call`，通过 Unix Socket（Unix）/ Named Pipe（Windows）转发到 Chrome 原生宿主
4. **响应路由**：使用 `request_id` 将异步响应匹配到对应的等待调用方

关键实现细节：

- 使用手写 JSON-RPC 2.0 实现（非 SDK 依赖）
- 工具调用超时：120 秒
- 连接延迟：原生宿主在首次调用时按需连接
- 请求追踪：`Pending` 表使用 `oneshot` channel 匹配请求与响应

### 2.6 Native Messaging Host

| 特性 | 实现 |
|------|------|
| manifest 写入 | `setup.rs` 按平台写入正确的 manifest 路径 |
| Windows 注册表 | 使用 HKCU 注册表键发现 native messaging hosts |
| 扩展 ID | 公开版 + 内部开发版（`USER_TYPE=ant`） |
| 传输 | Unix: Unix Domain Socket / Windows: Named Pipe |

## 三、关键设计决策

1. **Chrome 扩展优先**：选择 Chrome 扩展 + Native Messaging 模式而非内置 WebView，支持更丰富的浏览器能力（DOM 交互、JS 执行、网络监控）
2. **松开耦合的桥接架构**：MCP 桥接器作为独立子进程运行，与主进程故障隔离
3. **JSON-RPC 2.0**：手写协议实现，避免第三方 SDK 依赖
4. **跨平台传输**：Unix 使用 Domain Socket，Windows 使用 Named Pipe

## 四、使用方式

```bash
# 安装浏览器扩展
allthecodes chrome install

# 启动（自动启用浏览器工具）
allthecodes

# 查看浏览器状态
/chrome status
```

## 五、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-browser/src/common.rs` | 浏览器配置表、路径发现、扩展 ID |
| `crates/allthecodes-browser/src/mcp_bridge.rs` | MCP stdio 桥接服务、工具目录、JSON-RPC 处理 |
| `crates/allthecodes-browser/src/native_host.rs` | 原生宿主协议（读/写帧） |
| `crates/allthecodes-browser/src/transport.rs` | Socket 路径管理与安全 |
| `crates/allthecodes-browser/src/setup.rs` | 安装向导、manifest 管理 |
| `crates/allthecodes-browser/src/detection.rs` | 浏览器检测 |
| `crates/allthecodes-browser/src/permissions.rs` | 权限管理 |
| `crates/allthecodes-browser/src/session.rs` | 浏览器会话 |
