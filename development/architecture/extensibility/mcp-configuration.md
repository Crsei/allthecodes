---
title: "MCP 配置 - 协议实现与服务器管理"
description: "从源码角度解析 allthecodes MCP 实现：JSON-RPC 2.0 协议、stdio/SSE/Streamable HTTP 三种传输、McpManager 连接生命周期、工具发现和配置管理。"
keywords: ["MCP", "Model Context Protocol", "JSON-RPC", "McpClient", "McpManager", "传输协议", "stdio", "SSE"]
---

## MCP 协议概述

allthecodes 实现了 **Model Context Protocol (MCP)**，基于 JSON-RPC 2.0 的标准化协议，用于与外部工具服务器通信。

协议规范遵循：https://modelcontextprotocol.io/specification/2025-03-26/

源码位置：`crates/allthecodes-mcp/src/`

## 传输层：三种连接方式

### stdio（默认）

启动一个子进程，通过 stdin/stdout 交换换行分隔的 JSON-RPC 消息：

```json
{
  "my-server": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem"],
    "env": { "API_KEY": "..." }
  }
}
```

实现文件：`client/stdio.rs`

**通信流程**：
1. `spawn()` 创建子进程（piped stdin/stdout/stderr）
2. `reader_loop()` 后台任务读取 stdout → 解析 JSON-RPC 响应 → 分发给 pending 请求
3. `write_line()` 写入 JSON-RPC 请求到 stdin
4. 通过 `oneshot` channel 通知等待中的请求

**关键设计**：后台 reader 线程在子进程退出时自动失败所有 pending 请求，防止请求永远挂起。

### SSE（Server-Sent Events）

HTTP SSE 传输，用于远程 MCP 服务器：

```json
{
  "my-remote": {
    "type": "sse",
    "url": "http://localhost:8080/sse",
    "headers": { "Authorization": "Bearer ..." },
    "oauth": {
      "clientId": "...",
      "authServerMetadataUrl": "https://auth.example.com/.well-known/oauth-authorization-server"
    }
  }
}
```

实现文件：`client/sse.rs`

**连接流程**：
1. 建立 SSE 连接（接收 `endpoint` 事件 + `message` 事件）
2. `endpoint` 事件提供 POST URL，用于客户端→服务器的 JSON-RPC 消息
3. `message` 事件携带 JSON-RPC 响应/通知
4. SSE reader 后台任务解析事件流

**安全性**：
- Loopback HTTP 使用原始 TCP 连接（轻量）
- 远程端点必须使用 HTTPS
- 重定向被禁用，防止凭证泄漏到未验证的地址

### Streamable HTTP

MCP 的最新 HTTP 传输方式，定义于协议 `2025-11-25`：

```json
{
  "my-http": {
    "type": "streamable-http",
    "url": "https://mcp.example.com/mcp",
    "headers": { "X-API-Key": "..." }
  }
}
```

实现文件：`client/streamable_http.rs`

**关键差异**：
- 初始化使用协议版本 `2025-11-25`（而非 stdio/SSE 的 `2024-11-05`）
- 请求→响应是 POST 驱动的（可选的 SSE GET 流用于服务器推送）
- GET SSE 流退出时**不会**失败 pending 请求（不同于 stdio/SSE）

## McpManager：连接管理

`manager.rs` 中的 `McpManager` 管理多个 MCP 服务器连接：

### 生命周期

```
connect_all(configs)
  ├── 遍历每个 McpServerConfig
  │   ├── connect_server(config)
  │   │   ├── 断开旧连接（同名）
  │   │   ├── 检查 disabled 标志 → 跳过
  │   │   ├── 重试 3 次（指数退避 50ms → 100ms → 200ms）
  │   │   ├── client.connect() → 建立传输层
  │   │   ├── client.initialize() → JSON-RPC 握手
  │   │   ├── 支持 tools? → client.list_tools()
  │   │   └── 支持 resources? → client.list_resources()
  │   └── 插入 clients HashMap
  └── 失败不阻断：单个服务器失败不影响其他服务器
```

### 重试策略

```rust
fn connect_retry_delay_ms(attempt: usize) -> u64 {
    let factor = 1_u64 << attempt.min(8);  // 指数增长，最大 256
    CONNECT_RETRY_BASE_DELAY_MS(50) * factor  // 最大 250ms
}
```

最大重试 3 次，延迟指数增长：50ms → 100ms → 200ms。

### 事件发射

```rust
pub enum McpSubsystemEvent {
    ServerStateChanged { server_name, state, error },
    ToolsDiscovered { server_name, tools: Vec<McpToolInfo> },
    ResourcesDiscovered { server_name, resources: Vec<McpResourceInfo> },
    ChannelNotification { server_name, content, meta },
}
```

通过 `McpEventSink` trait 发送事件，允许宿主（engine/daemon）消费这些事件。

## JSON-RPC 2.0 协议

`lib.rs` 定义了完整的 JSON-RPC 类型：

### 请求格式

```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,  // "2.0"
    pub id: Value,         // 请求 ID
    pub method: String,    // 方法名
    pub params: Option<Value>,  // 参数
}
```

### 通知格式（无 ID）

```rust
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}
```

### 响应格式

```rust
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}
```

### MCP 特定方法

| 方法 | 方向 | 说明 |
|------|------|------|
| `initialize` | 客户端→服务器 | 握手，交换 capabilities |
| `notifications/initialized` | 客户端→服务器 | 通知初始化完成 |
| `tools/list` | 客户端→服务器 | 获取工具列表 |
| `tools/call` | 客户端→服务器 | 调用工具 |
| `resources/list` | 客户端→服务器 | 获取资源列表 |
| `resources/read` | 客户端→服务器 | 读取资源 |
| `notifications/claude/channel` | 服务器→客户端 | 通道通知 |

## 工具发现与适配

### McpToolDef

```rust
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub server_name: String,  // 客户端设置，用于追踪来源
}
```

MCP 工具发现后通过 `server_name` + `tool_name` 标记来源。每个 MCP 工具在注册时附加 `mcp_server_name()` 信息。

### 工具调用

```rust
pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<CallToolResult> {
    let params = json!({"name": tool_name, "arguments": arguments});
    let response = self.send_request_with_timeout("tools/call", Some(params), TOOL_CALL_TIMEOUT_SECS).await?;
    // 解析为 CallToolResult { content: Vec<ToolCallContent>, is_error: bool }
}
```

## 配置合并

MCP 配置来自多个来源（settings.json 的 `mcpServers` 键、企业策略、插件等），由 `allthecodes-config` crate 负责加载和合并。配置格式在 `McpServerConfig` 中定义，支持传输类型、命令、参数、URL、环境变量、OAuth 认证等字段。

### 禁用标志

```rust
pub struct McpServerConfig {
    // ...
    pub disabled: Option<bool>,  // 通过 settings-edit UX 临时禁用
}
```

禁用的服务器会被 `McpManager::connect_server()` 跳过（不在 `clients` 中注册），但配置保留在磁盘上，便于稍后重新启用。

## 通道通知

MCP 服务器可以通过 `notifications/claude/channel` 方法发送通道通知：

```rust
fn notification_event(server_name: &str, value: &Value) -> Option<McpSubsystemEvent> {
    // 检查 method == "notifications/claude/channel"
    // 解析 params → ChannelNotification
    // 回调通知宿主
}
```

这允许 MCP 服务器向用户推送状态更新（如"构建完成"），而不需要轮询。
