# IPC Subsystem Extensions Design

**Date:** 2026-04-15
**Status:** Draft
**Scope:** Headless IPC 协议扩展 — LSP / MCP / Plugin / Skills 子系统状态、事件、管理

## 概述

扩展 `src/ipc/protocol.rs` 的 `BackendMessage` / `FrontendMessage` 协议，使前端能够：

1. **状态面板** — 实时显示 LSP / MCP / Plugin / Skills 的连接状态
2. **交互管理** — 触发生命周期操作（start/stop/restart/connect/disconnect）
3. **诊断展示** — 接收 LSP 服务器推送的全量诊断信息
4. **Agent 可观测** — Agent 通过 `SystemStatus` 工具查看子系统状态，系统提示词注入状态摘要

**管理粒度**：混合 — 生命周期操作走 IPC，配置变更走斜杠命令。
**诊断范围**：全量推送 — LSP 服务器发来什么就转发什么。
**IPC 模式**：Headless 优先，Daemon 模式留接口不实现。

---

## 1. 协议类型定义

### 1.1 BackendMessage 新增变体

```rust
// protocol.rs — BackendMessage 新增 5 个变体

/// LSP 子系统事件
LspEvent { event: LspEvent },

/// MCP 子系统事件
McpEvent { event: McpEvent },

/// Plugin 子系统事件
PluginEvent { event: PluginEvent },

/// Skill 子系统事件
SkillEvent { event: SkillEvent },

/// 跨子系统聚合状态快照（响应 QuerySubsystemStatus）
SubsystemStatus { status: SubsystemStatusSnapshot },
```

### 1.2 FrontendMessage 新增变体

```rust
// protocol.rs — FrontendMessage 新增 5 个变体

/// LSP 生命周期命令
LspCommand { command: LspCommand },

/// MCP 生命周期命令
McpCommand { command: McpCommand },

/// Plugin 生命周期命令
PluginCommand { command: PluginCommand },

/// Skill 管理命令
SkillCommand { command: SkillCommand },

/// 查询所有子系统聚合状态
QuerySubsystemStatus,
```

---

## 2. 子枚举定义

### 2.1 LSP

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspEvent {
    /// 服务器状态变更（启动、停止、崩溃、错误）
    ServerStateChanged {
        language_id: String,
        state: String,           // "not_started"|"starting"|"running"|"stopped"|"error"
        error: Option<String>,
    },
    /// LSP 服务器推送的诊断信息（全量替换某 URI 的诊断列表）
    DiagnosticsPublished {
        uri: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    /// 所有 LSP 服务器当前状态（响应 LspCommand::QueryStatus）
    ServerList {
        servers: Vec<LspServerInfo>,
    },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspCommand {
    StartServer { language_id: String },
    StopServer { language_id: String },
    RestartServer { language_id: String },
    QueryStatus,
}
```

### 2.2 MCP

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpEvent {
    /// 服务器连接状态变更
    ServerStateChanged {
        server_name: String,
        state: String,           // "pending"|"connected"|"disconnected"|"error"
        error: Option<String>,
    },
    /// 服务器工具发现完成
    ToolsDiscovered {
        server_name: String,
        tools: Vec<McpToolInfo>,
    },
    /// 服务器资源发现完成
    ResourcesDiscovered {
        server_name: String,
        resources: Vec<McpResourceInfo>,
    },
    /// MCP Channel 通知（实验性）
    ChannelNotification {
        server_name: String,
        content: String,
        meta: serde_json::Value,
    },
    /// 所有 MCP 服务器当前状态（响应 McpCommand::QueryStatus）
    ServerList {
        servers: Vec<McpServerStatusInfo>,
    },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpCommand {
    ConnectServer { server_name: String },
    DisconnectServer { server_name: String },
    ReconnectServer { server_name: String },
    QueryStatus,
}
```

### 2.3 Plugin

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginEvent {
    /// 插件状态变更
    StatusChanged {
        plugin_id: String,
        name: String,
        status: String,          // "not_installed"|"installed"|"disabled"|"error"
        error: Option<String>,
    },
    /// 所有插件列表（响应 PluginCommand::QueryStatus）
    PluginList {
        plugins: Vec<PluginInfo>,
    },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginCommand {
    Enable { plugin_id: String },
    Disable { plugin_id: String },
    QueryStatus,
}
```

### 2.4 Skill

```rust
#[derive(Serialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillEvent {
    /// 技能批量加载/重载完成
    SkillsLoaded { count: usize },
    /// 技能列表（响应 SkillCommand::QueryStatus）
    SkillList {
        skills: Vec<SkillInfo>,
    },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillCommand {
    Reload,
    QueryStatus,
}
```

---

## 3. 共享数据类型

所有类型定义在 `src/ipc/subsystem_types.rs`。

### 3.1 LSP 类型

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspDiagnostic {
    pub range: DiagnosticRange,
    /// "error" | "warning" | "info" | "hint"
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiagnosticRange {
    pub start_line: u32,        // 1-based
    pub start_character: u32,   // 1-based
    pub end_line: u32,
    pub end_character: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspServerInfo {
    pub language_id: String,
    /// "not_started"|"starting"|"running"|"stopped"|"error"
    pub state: String,
    pub extensions: Vec<String>,
    pub open_files_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

### 3.2 MCP 类型

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpResourceInfo {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerStatusInfo {
    pub name: String,
    /// "pending"|"connected"|"disconnected"|"error"
    pub state: String,
    /// "stdio"|"sse"
    pub transport: String,
    pub tools_count: usize,
    pub resources_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<McpServerInfoBrief>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct McpServerInfoBrief {
    pub name: String,
    pub version: String,
}
```

### 3.3 Plugin 类型

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    /// "not_installed"|"installed"|"disabled"|"error"
    pub status: String,
    pub contributed_tools: Vec<String>,
    pub contributed_skills: Vec<String>,
    pub contributed_mcp_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
```

### 3.4 Skill 类型

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    /// "bundled"|"user"|"project"|"plugin"|"mcp"
    pub source: String,
    pub description: String,
    pub user_invocable: bool,
    pub model_invocable: bool,
}
```

### 3.5 聚合快照

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubsystemStatusSnapshot {
    pub lsp: Vec<LspServerInfo>,
    pub mcp: Vec<McpServerStatusInfo>,
    pub plugins: Vec<PluginInfo>,
    pub skills: Vec<SkillInfo>,
    pub timestamp: i64,
}
```

---

## 4. JSON Wire Format 示例

### 4.1 Backend → Frontend

```json
// LSP 诊断推送
{
  "type": "lsp_event",
  "event": {
    "kind": "diagnostics_published",
    "uri": "file:///f/project/src/main.rs",
    "diagnostics": [
      {
        "range": {"start_line": 42, "start_character": 5, "end_line": 42, "end_character": 15},
        "severity": "error",
        "message": "cannot find value `foo` in this scope",
        "source": "rust-analyzer",
        "code": "E0425"
      }
    ]
  }
}

// MCP 服务器连接
{
  "type": "mcp_event",
  "event": {
    "kind": "server_state_changed",
    "server_name": "context7",
    "state": "connected",
    "error": null
  }
}

// MCP 工具发现
{
  "type": "mcp_event",
  "event": {
    "kind": "tools_discovered",
    "server_name": "context7",
    "tools": [
      {"name": "resolve-library-id", "description": "Resolve a library ID"},
      {"name": "query-docs", "description": "Query documentation"}
    ]
  }
}

// Plugin 状态变更
{
  "type": "plugin_event",
  "event": {
    "kind": "status_changed",
    "plugin_id": "claude-mem@official",
    "name": "claude-mem",
    "status": "installed",
    "error": null
  }
}

// 聚合状态快照
{
  "type": "subsystem_status",
  "status": {
    "lsp": [
      {"language_id": "rust", "state": "running", "extensions": [".rs"], "open_files_count": 3, "error": null}
    ],
    "mcp": [
      {"name": "context7", "state": "connected", "transport": "stdio", "tools_count": 2, "resources_count": 0, "server_info": {"name": "context7", "version": "1.0.0"}, "instructions": null, "error": null}
    ],
    "plugins": [
      {"id": "claude-mem@official", "name": "claude-mem", "version": "1.2.0", "status": "installed", "contributed_tools": [], "contributed_skills": ["mem-search","make-plan","do"], "contributed_mcp_servers": [], "error": null}
    ],
    "skills": [
      {"name": "simplify", "source": "bundled", "description": "Review changed code...", "user_invocable": true, "model_invocable": true}
    ],
    "timestamp": 1744675200
  }
}
```

### 4.2 Frontend → Backend

```json
// 启动 LSP 服务器
{"type": "lsp_command", "command": {"kind": "start_server", "language_id": "rust"}}

// 停止 LSP 服务器
{"type": "lsp_command", "command": {"kind": "stop_server", "language_id": "rust"}}

// 重连 MCP 服务器
{"type": "mcp_command", "command": {"kind": "reconnect_server", "server_name": "context7"}}

// 查询所有子系统状态
{"type": "query_subsystem_status"}

// 禁用插件
{"type": "plugin_command", "command": {"kind": "disable", "plugin_id": "claude-mem@official"}}

// 重载技能
{"type": "skill_command", "command": {"kind": "reload"}}
```

---

## 5. 事件传递架构

### 5.1 事件通道

引入一个统一的 `SubsystemEventBus`，各子系统发送事件，`headless.rs` 订阅并转发：

```rust
// src/ipc/event_bus.rs

use tokio::sync::broadcast;

/// 统一的子系统事件类型
#[derive(Debug, Clone)]
pub enum SubsystemEvent {
    Lsp(LspEvent),
    Mcp(McpEvent),
    Plugin(PluginEvent),
    Skill(SkillEvent),
}

pub struct SubsystemEventBus {
    tx: broadcast::Sender<SubsystemEvent>,
}

impl SubsystemEventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx }
    }

    pub fn sender(&self) -> broadcast::Sender<SubsystemEvent> {
        self.tx.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SubsystemEvent> {
        self.tx.subscribe()
    }
}
```

### 5.2 事件来源接入

| 子系统 | 接入点 | 事件触发时机 |
|--------|--------|------------|
| **LSP** | `lsp_service/client.rs` — `request()` 循环 | 收到 `textDocument/publishDiagnostics` 通知时，不再跳过，而是解析为 `LspDiagnostic` 并发送 `DiagnosticsPublished` |
| **LSP** | `lsp_service/mod.rs` — `get_or_start_client()` | 客户端状态变更（启动成功、检测到死亡、重启）时发送 `ServerStateChanged` |
| **LSP** | `lsp_service/client.rs` — `shutdown()` | 关闭时发送 `ServerStateChanged { state: "stopped" }` |
| **MCP** | `mcp/client.rs` — `connect()` | 连接成功/失败时发送 `ServerStateChanged` |
| **MCP** | `mcp/client.rs` — `disconnect()` | 断开时发送 `ServerStateChanged { state: "disconnected" }` |
| **MCP** | `mcp/client.rs` — `list_tools()` | 工具发现完成时发送 `ToolsDiscovered` |
| **MCP** | `mcp/client.rs` — `list_resources()` | 资源发现完成时发送 `ResourcesDiscovered` |
| **MCP** | `mcp/channel.rs` | Channel 通知时发送 `ChannelNotification` |
| **Plugin** | `plugins/mod.rs` — `register_plugin()` | 插件注册时发送 `StatusChanged` |
| **Plugin** | `plugins/mod.rs` — `unregister_plugin()` | 插件移除时发送 `StatusChanged` |
| **Skill** | `skills/mod.rs` — `init_skills()` | 批量加载完成时发送 `SkillsLoaded` |
| **Skill** | `skills/mod.rs` — `register_skill()` | 单个技能注册时（可选，避免过于嘈杂） |

### 5.3 事件总线注入

事件总线作为 `Option<broadcast::Sender<SubsystemEvent>>` 注入各子系统的全局状态：

```rust
// lsp_service/mod.rs
static EVENT_TX: LazyLock<Mutex<Option<broadcast::Sender<SubsystemEvent>>>> =
    LazyLock::new(|| Mutex::new(None));

pub fn set_event_sender(tx: broadcast::Sender<SubsystemEvent>) {
    *EVENT_TX.lock() = Some(tx);
}

// 内部辅助
fn emit(event: SubsystemEvent) {
    if let Some(tx) = EVENT_TX.lock().as_ref() {
        let _ = tx.send(event);   // 无订阅者时静默丢弃
    }
}
```

MCP / Plugin / Skill 模块同样模式。

### 5.4 Headless 事件循环扩展

`headless.rs` 的 `tokio::select!` 新增一个分支：

```rust
// headless.rs — run_headless()

let event_bus = SubsystemEventBus::new();
let mut event_rx = event_bus.subscribe();

// 注入各子系统
lsp_service::set_event_sender(event_bus.sender());
mcp::set_event_sender(event_bus.sender());
plugins::set_event_sender(event_bus.sender());
skills::set_event_sender(event_bus.sender());

loop {
    tokio::select! {
        // 分支 1: Frontend message (existing)
        line = lines.next_line() => { /* ... existing handler ... */ }

        // 分支 2: Background agent (existing)
        Some(completed) = bg_rx.recv() => { /* ... existing handler ... */ }

        // 分支 3 (NEW): Subsystem events
        Ok(event) = event_rx.recv() => {
            let msg = match event {
                SubsystemEvent::Lsp(e) => BackendMessage::LspEvent { event: e },
                SubsystemEvent::Mcp(e) => BackendMessage::McpEvent { event: e },
                SubsystemEvent::Plugin(e) => BackendMessage::PluginEvent { event: e },
                SubsystemEvent::Skill(e) => BackendMessage::SkillEvent { event: e },
            };
            let _ = send_to_frontend(&msg);
        }
    }
}
```

### 5.5 FrontendMessage 命令处理

在 `headless.rs` 的 `FrontendMessage` match 中新增分支：

```rust
FrontendMessage::LspCommand { command } => {
    handle_lsp_command(command, &engine).await;
}
FrontendMessage::McpCommand { command } => {
    handle_mcp_command(command, &engine).await;
}
FrontendMessage::PluginCommand { command } => {
    handle_plugin_command(command).await;
}
FrontendMessage::SkillCommand { command } => {
    handle_skill_command(command).await;
}
FrontendMessage::QuerySubsystemStatus => {
    let status = build_subsystem_status_snapshot(&engine).await;
    let _ = send_to_frontend(&BackendMessage::SubsystemStatus { status });
}
```

命令处理函数定义在 `src/ipc/subsystem_handlers.rs`：

```rust
async fn handle_lsp_command(cmd: LspCommand, engine: &Arc<QueryEngine>) {
    match cmd {
        LspCommand::StartServer { language_id } => {
            // 调用 lsp_service::get_or_start_client()
            // 状态变更通过 event_bus 自动推送
        }
        LspCommand::StopServer { language_id } => {
            // 调用 lsp_service::stop_client()
        }
        LspCommand::RestartServer { language_id } => {
            // stop + start
        }
        LspCommand::QueryStatus => {
            let servers = lsp_service::get_all_server_info();
            let _ = send_to_frontend(&BackendMessage::LspEvent {
                event: LspEvent::ServerList { servers },
            });
        }
    }
}

// McpCommand, PluginCommand, SkillCommand 类似
```

---

## 6. Agent 集成

### 6.1 SystemStatus 工具

新增 `src/tools/system_status.rs`：

```rust
pub struct SystemStatusTool;

// Tool metadata:
//   name: "SystemStatus"
//   description: "Query the current status of subsystems (LSP, MCP, plugins, skills).
//                 Use this to check server connections, diagnostics, available tools, etc."
//   input_schema: {
//     "subsystem": {
//       "type": "string",
//       "enum": ["lsp", "mcp", "plugins", "skills", "all"],
//       "description": "Which subsystem to query. Defaults to 'all'."
//     }
//   }
```

输出格式为人类可读的结构化文本：

```
## LSP Servers
- rust: running (3 files open)
- typescript: not_started

## MCP Servers
- context7: connected (2 tools, 0 resources)
  Tools: resolve-library-id, query-docs
- chrome-devtools: connected (28 tools, 0 resources)

## Plugins
- claude-mem@official: installed (v1.2.0)
  Skills: mem-search, make-plan, do

## Skills (12 total)
- simplify [bundled] — Review changed code...
- remember [bundled] — Save information to memory
- mem-search [plugin:claude-mem] — Search persistent memory
  ...
```

工具注册在 `src/tools.rs` 的 `get_all_base_tools()`。

### 6.2 系统提示词注入

在 `src/engine/system_prompt.rs`（或构建系统提示词的位置）中追加 `<system-reminder>` 块：

```rust
fn build_subsystem_status_reminder() -> Option<String> {
    let lsp_count = lsp_service::running_server_count();
    let mcp_count = mcp_connected_count();
    let plugin_count = plugins::enabled_count();
    let skill_count = skills::get_all_skills().len();

    // 仅在有活跃子系统时注入
    if lsp_count + mcp_count + plugin_count + skill_count == 0 {
        return None;
    }

    Some(format!(
        "<system-reminder>\n\
         # Active Subsystems\n\
         - LSP: {} server(s) running\n\
         - MCP: {} server(s) connected\n\
         - Plugins: {} enabled\n\
         - Skills: {} loaded\n\
         Use the SystemStatus tool for detailed information.\n\
         </system-reminder>",
        lsp_count, mcp_count, plugin_count, skill_count
    ))
}
```

注入时机：每轮查询构建系统消息时调用，追加到现有 system reminders 之后。

---

## 7. LSP 诊断捕获改造

当前 `lsp_service/client.rs` 的 `request()` 方法在等待响应时会跳过所有 server notification。需要改造为：

```rust
// client.rs — request() 内循环

loop {
    let response = self.transport.read_message().await?;

    // 如果是 notification（无 id 字段）
    if response.get("id").is_none() {
        let method = response.get("method").and_then(|m| m.as_str());
        match method {
            Some("textDocument/publishDiagnostics") => {
                if let Some(params) = response.get("params") {
                    let (uri, diagnostics) = parse_publish_diagnostics(params);
                    emit(SubsystemEvent::Lsp(LspEvent::DiagnosticsPublished {
                        uri,
                        diagnostics,
                    }));
                }
            }
            Some(other) => {
                debug!("LSP: skipping notification: {}", other);
            }
            None => {}
        }
        continue;
    }

    // 正常的 response 处理...
}
```

诊断解析函数：

```rust
fn parse_publish_diagnostics(params: &Value) -> (String, Vec<LspDiagnostic>) {
    let uri = params["uri"].as_str().unwrap_or_default().to_string();
    let diagnostics = params["diagnostics"]
        .as_array()
        .map(|arr| arr.iter().filter_map(parse_single_diagnostic).collect())
        .unwrap_or_default();
    (uri, diagnostics)
}

fn parse_single_diagnostic(val: &Value) -> Option<LspDiagnostic> {
    let range = val.get("range")?;
    Some(LspDiagnostic {
        range: DiagnosticRange {
            start_line: range["start"]["line"].as_u64()? as u32 + 1,     // LSP 0-based → 1-based
            start_character: range["start"]["character"].as_u64()? as u32 + 1,
            end_line: range["end"]["line"].as_u64()? as u32 + 1,
            end_character: range["end"]["character"].as_u64()? as u32 + 1,
        },
        severity: match val["severity"].as_u64() {
            Some(1) => "error",
            Some(2) => "warning",
            Some(3) => "info",
            Some(4) => "hint",
            _ => "unknown",
        }.to_string(),
        message: val["message"].as_str()?.to_string(),
        source: val["source"].as_str().map(|s| s.to_string()),
        code: val.get("code").and_then(|c| {
            c.as_str().map(|s| s.to_string())
                .or_else(|| c.as_u64().map(|n| n.to_string()))
        }),
    })
}
```

### 7.1 后台诊断监听

除了 request 循环内捕获 notification 外，还需要一个后台 reader task 来持续监听 LSP 服务器的主动推送（不在 request 期间发来的通知）：

```rust
// client.rs — start() 结束时启动后台 reader

let bg_reader = transport.clone_reader();  // 需要拆分 reader
tokio::spawn(async move {
    loop {
        match bg_reader.read_message().await {
            Ok(msg) if msg.get("id").is_none() => {
                // 处理 notification（同上逻辑）
            }
            Err(_) => break,  // server 断开
            _ => {}           // response 由 request() 处理
        }
    }
});
```

> **注意**：这需要将 `JsonRpcTransport` 的 reader/writer 拆分为独立的 Arc 句柄，或使用 `tokio::sync::mpsc` 做 reader fan-out。具体实现在 Phase 2 中细化。

---

## 8. 文件组织

```
src/ipc/
├── mod.rs                     (扩展: 声明新模块)
├── protocol.rs                (扩展: BackendMessage/FrontendMessage 新变体)
├── headless.rs                (扩展: select 分支 + 命令处理调用)
├── subsystem_types.rs         (NEW: 共享数据类型 — LspDiagnostic, McpToolInfo 等)
├── subsystem_events.rs        (NEW: SubsystemEvent 枚举 + SubsystemEventBus)
└── subsystem_handlers.rs      (NEW: handle_lsp_command, handle_mcp_command 等)

src/tools/
└── system_status.rs           (NEW: SystemStatus 工具)

src/lsp_service/
├── client.rs                  (修改: notification 捕获 + 后台 reader)
└── mod.rs                     (修改: set_event_sender + 状态变更事件)

src/mcp/
├── client.rs                  (修改: 连接/断开/发现事件)
└── mod.rs                     (修改: set_event_sender)

src/plugins/
└── mod.rs                     (修改: set_event_sender + 注册/移除事件)

src/skills/
└── mod.rs                     (修改: set_event_sender + 加载事件)

src/engine/
└── system_prompt.rs           (修改: 注入子系统状态摘要)
```

---

## 9. Daemon 模式接口预留

Headless 优先，但为 Daemon 模式预留扩展点：

- `SubsystemEvent` 枚举可被 `daemon/sse.rs` 订阅，序列化为 SSE 事件
- `SubsystemCommand` 可映射为 `daemon/routes.rs` 中的 REST 端点
- 数据类型 (`subsystem_types.rs`) 在两种模式间共享

预留的 Daemon 端点（不实现）：

```
GET  /api/subsystem/status          → SubsystemStatusSnapshot
POST /api/subsystem/lsp/start       → LspCommand::StartServer
POST /api/subsystem/lsp/stop        → LspCommand::StopServer
POST /api/subsystem/mcp/connect     → McpCommand::ConnectServer
POST /api/subsystem/mcp/disconnect  → McpCommand::DisconnectServer
SSE  /api/subsystem/events          → SubsystemEvent stream
```

---

## 10. 注意事项

1. **诊断 URI 格式**: LSP 使用 `file://` URI，前端可能需要转换为相对路径显示。转换在前端完成，后端原样透传。

2. **诊断量控制**: 全量推送可能在大项目中产生大量数据。如果实际使用中发现性能问题，可在后端添加可选的节流（debounce 100ms 合并同文件诊断）。

3. **事件总线容量**: `broadcast::channel(256)` 的缓冲区大小。慢消费者（前端处理不及时）会丢失旧事件，这是可接受的 — 状态类事件可通过 QueryStatus 重新拉取。

4. **LSP reader 拆分**: 当前 `JsonRpcTransport` 持有 `BufReader<ChildStdout>`，不能共享。需要引入 mpsc channel 做 reader fan-out，或改为 `Arc<Mutex<>>` 方式（后者会阻塞 request 循环）。推荐 mpsc fan-out。

5. **线程安全**: 事件总线使用 `broadcast` channel，`set_event_sender` 使用 `Mutex<Option<Sender>>`，均为 Send + Sync。

6. **Agent 工具权限**: `SystemStatus` 工具为只读查询，应设为自动允许（无需用户确认）。
