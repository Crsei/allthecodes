# UDS_INBOX — Unix Domain Socket 收件箱

> 功能标志：无（基础 IPC 设施）
> 实现状态：完整可用
> 源码文件数：4

## 一、功能概述

UDS Inbox 是 allthecodes 守护进程模式的通信基础设施，基于 Unix Domain Socket 和 JSON Lines 协议，提供后端引擎与前端 UI 进程之间的双向消息通道。守护进程通过文件系统上的命令文件（命令收件箱）和工作事件日志实现异步通信。

### 核心概念

- **命令收件箱（Command Inbox）**：守护进程在 `{daemon_dir}/commands/{worker_id}/` 目录下写入 JSON 命令文件
- **事件日志（Event Log）**：守护进程在 `{daemon_dir}/events/{worker_id}.ndjson` 追加事件行
- **Worker**：每个会话实例是一个 worker，通过轮询命令文件接收指令

## 二、实现架构

### 2.1 守护进程协议

文件：`crates/allthecodes-daemon/src/protocol.rs`

#### 命令类型

```rust
pub enum DaemonCommandKind {
    Submit,             // 提交用户输入
    Abort,              // 中止当前查询
    PermissionResponse, // 权限响应
    AskUserResponse,    // 用户问题响应
    Shutdown,           // 关闭 worker
    ReloadConfig,       // 重载配置
}
```

#### 命令生命周期

```
Pending → Acked → Handled / Failed
```

每个命令 JSON 文件包含完整的元数据：

```rust
pub struct DaemonCommand {
    pub schema_version: u32,
    pub command_id: String,
    pub idempotency_key: Option<String>,
    pub target_worker_id: String,
    pub kind: DaemonCommandKind,
    pub payload: Value,
    pub status: DaemonCommandStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub acked_at: Option<DateTime<Utc>>,
    pub handled_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}
```

#### 事件日志

Worker 事件以 NDJSON（Newline-Delimited JSON）格式写入：

```rust
pub struct DaemonEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub worker_id: String,
    pub command_id: Option<String>,
    pub event_type: String,    // "command_ack", "worker_heartbeat", "abort_ack", etc.
    pub data: Value,
    pub created_at: DateTime<Utc>,
}
```

### 2.2 协议存储（DaemonProtocolStore）

```rust
pub struct DaemonProtocolStore {
    daemon_dir: PathBuf,
}
```

核心操作：

| 方法 | 功能 |
|------|------|
| `enqueue_command()` | 写入命令文件，支持幂等性去重 |
| `read_worker_commands()` | 读取 worker 所有命令 |
| `claim_next_pending_command()` | 原子性地领取下一个待处理命令 |
| `mark_command_handled()` | 标记命令为已处理 |
| `append_event()` | 追加事件到 NDJSON 文件 |

文件结构：
```
{daemon_dir}/
  commands/
    {worker_id}/
      {command_id}.json        # 命令文件
      {command_id}.json.tmp    # 原子写入临时文件
  events/
    {worker_id}.ndjson         # 事件日志
```

### 2.3 IPC 传输层

文件：`crates/allthecodes-ipc/src/transport/`

#### 帧协议

```rust
pub struct IpcFrame {
    pub line: String,
}
```

每个帧是一条 JSON 行，通过 `IpcFrame::json()` 序列化。

#### Transport Trait

```rust
pub trait IpcTransport {
    type Reader: IpcReader;
    type Writer: IpcWriter;
    fn split(self) -> (Self::Reader, Self::Writer);
}
```

#### 标准实现：JsonlStdioTransport

```rust
impl IpcTransport for JsonlStdioTransport {
    type Reader = JsonlStdioReader;  // BufReader<Stdin>
    type Writer = JsonlStdioWriter;  // Stdout
}
```

从 stdin 读取 JSON Lines，写入 stdout。

### 2.4 协议消息

> 文件：`crates/allthecodes-ipc-protocol/src/protocol/mod.rs`

#### Frontend → Backend

```rust
pub enum FrontendMessage {
    SubmitPrompt { text: String, id: String },
    AbortQuery,
    PermissionResponse { tool_use_id: String, decision: String, ... },
    SlashCommand { raw: String },
    Resize { cols: u16, rows: u16 },
    QuestionResponse { id: String, text: String, ... },
    Quit,
    LspCommand { command: LspCommand },
    McpCommand { command: McpCommand },
    PluginCommand { command: PluginCommand },
    SkillCommand { command: SkillCommand },
    IdeCommand { command: IdeCommand },
    AgentSettingsCommand { command: Box<AgentSettingsCommand> },
    QuerySubsystemStatus,
    AgentCommand { command: AgentCommand },
    TeamCommand { command: TeamCommand },
    SearchFiles { request_id: String, pattern: String, ... },
    RequestCompletions { ... },
    AcceptCompletion { ... },
    InstallRecommendedPlugin { ... },
    RefreshPluginTelemetry,
    RequestLspRecommendations { ... },
}
```

#### Backend → Frontend

```rust
pub enum BackendMessage {
    Ready { session_id, model, cwd, ... },
    StreamStart { message_id },
    StreamDelta { message_id, text },
    ThinkingDelta { message_id, thinking },
    StreamEnd { message_id },
    Tombstone { message_id },
    AssistantMessage { id, content, cost_usd },
    ToolUse { id, name, input },
    ToolResult { tool_use_id, output, is_error, ... },
    ToolProgress { tool_use_id, tool, output, ... },
    PermissionRequest { tool_use_id, tool, command, ... },
    QuestionRequest { id, text, choices, ... },
    PlanWorkflowEvent { event, summary, record },
    SystemInfo { text, level },
    ConversationReplaced { messages },
    UsageUpdate { input_tokens, output_tokens, cost_usd },
    StatusLineUpdate { payload, lines, ... },
    Suggestions { items },
    Error { message, recoverable },
    BackgroundAgentComplete { agent_id, description, ... },
    FileSearchResult { request_id, matches, truncated, ... },
    // ...
}
```

### 2.5 Headless 运行时

文件：`crates/allthecodes-ipc/src/headless.rs`

`run_headless()` 函数实现 JSONL 协议的事件循环：

```rust
pub async fn run_headless(config: HeadlessRuntimeConfig) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            // 从 stdin 读取前端消息
            frame = reader.read_frame() => { ... }
            // 接收 Agent IPC 事件
            Some(event) = agent_rx.recv() => { ... }
            // 接收子系统事件
            Ok(event) = event_rx.recv() => { ... }
        }
    }
}
```

## 三、Gateway 远程控制

文件：`crates/allthecodes-daemon/src/gateway_bridge.rs`

Gateway 桥接层将远程控制命令映射为守护进程命令：

```rust
impl GatewayCommandSink for GatewayDaemonBridge {
    fn dispatch(&self, command: GatewayCommand) -> Result<GatewayCommandReceipt, GatewayError> {
        // 将 GatewayCommand 转换为 DaemonCommandKind
        // 通过 protocol_store().enqueue_command() 写入
    }
}
```

## 四、关键设计决策

1. **文件系统作为 IPC**：使用文件系统目录作为命令收件箱，而不是直接 socket 连接，简化可靠性和持久性
2. **原子写入**：先写入 `.json.tmp` 再 rename，防止写入中断导致损坏
3. **幂等性**：`idempotency_key` 确保相同命令不被重复处理
4. **NDJSON 事件日志**：追加写模式，支持实时 tail 和故障恢复
5. **JSON Lines 协议**：每行一个完整 JSON 对象，无状态解析，兼容标准 Unix 管道

## 五、使用方式

```bash
# 守护进程模式
allthecodes daemon --daemon-dir /path/to/daemon

# 写入命令到收件箱
echo '{"type":"submit_prompt","text":"hello","id":"123"}' > /path/to/daemon/commands/worker-1/cmd-123.json
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-daemon/src/protocol.rs` | 守护进程协议（命令/事件 DTO + 存储） |
| `crates/allthecodes-daemon/src/gateway_bridge.rs` | Gateway 桥接 |
| `crates/allthecodes-ipc/src/transport/mod.rs` | IPC 传输 traits |
| `crates/allthecodes-ipc/src/transport/frame.rs` | IPC 帧定义 |
| `crates/allthecodes-ipc/src/transport/jsonl.rs` | JSONL stdio 传输实现 |
| `crates/allthecodes-ipc/src/headless.rs` | Headless 运行时 |
| `crates/allthecodes-ipc-protocol/src/protocol/mod.rs` | 协议消息定义 |
