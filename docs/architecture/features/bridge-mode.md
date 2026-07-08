# Bridge Mode — IPC 桥接模式

> 实现状态：完整实现
> 传输协议：JSONL stdio / 版本化 Envelope
> 组件：IPC Protocol DTOs + IPC Transport 层

## 一、功能概述

Bridge Mode 是 allthecodes 进程与前端进程（IDE 插件、Web UI、TUI）之间的通信桥梁。它定义了一套标准化的进程间通信协议，支持前后端消息的双向传递、事件流、会话管理，以及前端发送的命令控制。

## 二、实现架构

### 2.1 Crate 结构

通信层分为两个 crate：

**allthecodes-ipc-protocol** — 数据模型层

| 模块 | 文件 | 职责 |
|------|------|------|
| `envelope` | `envelope.rs` | 版本化 IPC 事件信封 |
| `protocol` | `protocol.rs` | 前后端消息枚举定义 |
| `normalized` | `normalized.rs` | 跨传输标准化负载（生命周期、会话、工具、权限事件） |
| `subsystem_events` | `subsystem_events.rs` | 子系统事件定义 |
| `subsystem_types` | `subsystem_types.rs` | 子系统状态快照类型 |
| `lsp` | `lsp.rs` | LSP 相关类型（补全、文档变更） |

**allthecodes-ipc::transport** — 传输层

| 模块 | 文件 | 职责 |
|------|------|------|
| `jsonl` | `jsonl.rs` | JSONL stdio 传输实现 |
| `frame` | `frame.rs` | IPC 帧定义与读写 trait |
| `sink` | `sink.rs` | 前端输出 sink（stdout / 内存） |
| `event_class` | `event_class.rs` | 事件分类与队列压力诊断 |

### 2.2 协议栈

```
前端进程 (IDE / Web UI / TUI)
       │
       ▼
  ┌─────────────────────────┐
  │   传输层 (Transport)     │  ← Frame / JSONL / Memory
  ├─────────────────────────┤
  │   信封层 (Envelope)      │  ← 版本化、关联 ID、会话/轮次/运行
  ├─────────────────────────┤
  │   负载层 (Payload)       │  ← FrontendMessage / BackendMessage
  ├─────────────────────────┤
  │   标准化事件层 (Normalized)│  ← Lifecycle / Conversation / Tool / Permission
  └─────────────────────────┘
       │
       ▼
 allthecodes 后端进程
```

### 2.3 消息类型

**前端消息（前端 -> 后端）：**

| 类型 | 说明 |
|------|------|
| `Quit` | 退出请求 |
| `AbortQuery` | 中止当前查询 |
| `SubmitMessage` | 提交用户消息 |
| `PermissionResponse` | 权限授权响应 |
| `ScheduleCommand` | 调度命令 |
| `LspCompletions` | LSP 补全请求 |
| `SetAutoReply` | 设置自动回复 |
| `Restart` | 重启请求 |

**后端消息（后端 -> 前端）：**

| 类型 | 说明 |
|------|------|
| `ConversationEvent` | 对话事件（StreamStart/Delta/End） |
| `ToolEvent` | 工具事件（ToolUse/ToolResult/Progress） |
| `LifecycleEvent` | 生命周期（Ready/Shutdown） |
| `SystemInfo` | 系统信息 |
| `Error` | 错误通知 |
| `ResolvedToolResult` | 工具结果输出 |
| `LspUpdate` | LSP 更新通知 |

### 2.4 IPC 信封

`IpcEnvelope<T>` 提供传输无关的通用包装：

| 字段 | 说明 |
|------|------|
| `version` | 信封版本（当前 v1） |
| `id` | 事件唯一 ID |
| `seq` | 序列号 |
| `timestamp` | 时间戳 |
| `session_id` | 关联的会话 ID |
| `turn_id` | 关联的轮次 ID |
| `run_id` | 关联的运行 ID |
| `correlation_id` | 关联 ID（请求-响应匹配） |

版本兼容性：当前版本 1，最低兼容版本 1。未来版本升级通过 `is_supported_envelope_version()` 检测。

### 2.5 传输实现

**JSONL Stdio Transport：**

```
后端进程                   前端进程
  │                        │
  ├─ stdout ──────────────►│  BackendMessage (JSON line)
  │                        │
  │◄─ stdin ────────────── ├  FrontendMessage (JSON line)
  │                        │
```

每条消息为一行 JSON（JSONL 格式），使用 `\n` 分隔。

**Memory Transport（测试用）：**

`MemoryTransport` 提供内存中的帧缓冲区，`FrontendSink` 提供 `memory()` 和 `stdout()` 两种输出模式。

### 2.6 前端 Sink

`FrontendSink` 是 `BackendMessage` 的统一出口：

```rust
pub struct FrontendSink {
    writer: Arc<dyn SinkWriter>,
    memory: Option<Arc<MemoryWriter>>,
}

// 用法
let sink = FrontendSink::stdout();    // 写入 stdout
let sink = FrontendSink::memory();    // 内存缓冲区（测试）
sink.send(&msg);                      // 单条发送
sink.send_many(msgs);                 // 批量发送
```

## 三、标准化事件模型

### 3.1 生命周期事件

| 事件 | 说明 |
|------|------|
| `Ready` | 后端就绪（含 session_id / model / cwd / permission_mode） |
| `Shutdown` | 关闭（含原因） |

### 3.2 对话事件

| 事件 | 说明 |
|------|------|
| `StreamStart` | 流式开始 |
| `StreamDelta` | 文本增量 |
| `ThinkingDelta` | 思考过程增量 |
| `StreamEnd` | 流式结束 |
| `Tombstone` | 消息作废标记 |
| `AssistantMessage` | 完整助手消息 |
| `ConversationReplaced` | 会话被替换（历史回滚） |

### 3.3 工具事件

| 事件 | 说明 |
|------|------|
| `ToolUse` | 模型发起工具调用 |
| `ToolResult` | 工具执行结果（含内容块） |
| `ToolProgress` | 工具执行进度 |

### 3.4 权限与流控事件

| 事件 | 说明 |
|------|------|
| `PermissionRequest` | 权限请求 |
| `PermissionDecision` | 权限决定 |
| `FlowControlEvent` | 流控制（节流/队列/错误） |

## 四、事件分类与队列管理

`EventClass` 枚举对事件进行分类，用于队列压力诊断：

| 类 | 包含事件 |
|----|---------|
| `ConversationStream` | StreamStart/Delta/End/Thinking/Tombstone |
| `ToolResult` | ToolResult/ResolvedToolResult |
| `SystemInfo` | SystemInfo/Error |
| `Lifecycle` | Ready/Shutdown |
| `Lsp` | LspUpdate/LspDiagnostics |

`ClientEventQueue` 提供自适应队列管理，监控累积量和吞吐量，输出 `QueuePressureDiagnostic`。

## 五、关键设计决策

1. **协议与传输分离**：`ipc-protocol` 定义数据模型，`ipc-transport` 实现传输，可独立演进
2. **版本化信封**：`IpcEnvelope` 提供版本协商机制，向前兼容
3. **JSONL 为主**：开发调试友好，后端通用
4. **标准化事件模型**：在原始消息之上提供结构化事件层，降低前端解析成本

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-ipc-protocol/src/envelope.rs` | 版本化信封定义 |
| `crates/allthecodes-ipc-protocol/src/protocol.rs` | 前后端消息枚举 |
| `crates/allthecodes-ipc-protocol/src/normalized.rs` | 标准化事件模型 |
| `crates/allthecodes-ipc-protocol/src/subsystem_events.rs` | 子系统事件 |
| `crates/allthecodes-ipc/src/transport/jsonl.rs` | JSONL stdio 传输 |
| `crates/allthecodes-ipc/src/transport/frame.rs` | 帧与 IO trait |
| `crates/allthecodes-ipc/src/transport/sink.rs` | 前端输出 sink |
| `crates/allthecodes-ipc/src/transport/event_class.rs` | 事件分类与队列管理 |
