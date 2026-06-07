# IPC 模板未来应用计划

> 基于 Codex app-server/client 实现对照，以及本项目当前 `/api/ipc/ws`、JSONL stdio、IPC protocol/envelope/event-class 的现状。
> 目标：把 IPC 从“某个 WebSocket handler 的前后端消息枚举”升级为“协议级 request/event/server-request envelope”，同时保留 legacy frontend contract。

状态：规划草案，尚未实现。除“当前资产”中列出的现有文件和 legacy `/api/ipc/ws`、JSONL stdio 外，本文提到的 `IpcPayload`、`IpcRuntime`、`ServerRequestEnvelope`、`/api/v2/ipc/ws`、envelope opt-in、dispatcher 接入均为未来目标。

---

## 结论

未来 IPC 模板建议采用 Codex 风格的分层：

```text
Transport adapter
  -> versioned IPC envelope
  -> shared IPC runtime
  -> ClientRequest / ClientNotification / ClientResponse
  -> ApiDispatcher
  -> domain handler / engine
  -> ServerNotification / ServerRequest / Error / Lagged
  -> shared IPC runtime
  -> Transport adapter
```

核心原则：

1. `/api/ipc/ws` 保留为 legacy compatibility bridge，不作为新功能模板继续扩张。
2. 未来新增 IPC v2 wire contract，基于 `IpcEnvelope<T>`，所有新 transport 共享同一套 payload。
3. 可映射为 API 的命令未来进入 `ApiDispatcher`，不要继续在 WebSocket handler 中扩写 engine 零散调用。
4. permission/question 未来改成显式 `ServerRequest`，客户端用 request id 回复或拒绝。
5. 背压策略未来需要明确区分 lossless 和 best-effort，不能静默丢失审批、问题、completion、assistant stream。

---

## 当前资产

| 资产 | 当前作用 | 未来处理 |
|---|---|---|
| `crates/allthecodes-web/src/ws/ipc.rs` | `/api/ipc/ws` legacy bridge，内联业务处理和 callback 安装 | 保留兼容入口，逐步瘦身为 legacy adapter |
| `crates/allthecodes-ipc-protocol/src/envelope.rs` | `IpcEnvelope<T>` 已有版本、seq、correlation metadata | 未来升级为 IPC v2 canonical envelope |
| `crates/allthecodes-ipc-protocol/src/normalized.rs` | legacy backend message 分类 | 作为 legacy -> v2 adapter 的基础 |
| `crates/allthecodes-ipc-transport/src/event_class.rs` | lossless/best-effort 分类和队列压力诊断 | 未来并入 shared IPC runtime 的 outbound queue policy |
| `crates/allthecodes-ipc-client/src/callbacks.rs` | permission/question/tool-progress callback builders | 未来抽到 runtime/adapters 共享，避免 WS handler 复制逻辑 |
| `crates/allthecodes-ipc-transport/src/jsonl.rs` | JSONL stdio transport | 当前先兼容 bare legacy message，未来支持 envelope mode |
| `crates/allthecodes-protocol` | API request/response/metadata | IPC v2 中可映射业务命令的 canonical request 类型来源 |

如果先执行 `01-crate-consolidation-plan.zh.md` 的 IPC crate 合并，本计划中的模块应落到合并后的 `allthecodes-ipc` 子模块；否则先在当前 split crates 中实现同名模块，后续机械移动。

---

## 目标文件布局

合并 IPC crate 后的目标布局（规划，当前未创建）：

```text
crates/allthecodes-ipc/src/
  protocol/
    envelope.rs          # IpcEnvelope, version negotiation
    payload.rs           # IpcPayload, Client/Server variants
    legacy.rs            # legacy FrontendMessage/BackendMessage adapters
    normalized.rs        # existing normalized payloads
  runtime/
    mod.rs               # IpcRuntime
    session.rs           # connection/session lifecycle
    pending.rs           # pending server requests
    outbound.rs          # lossless/best-effort queue
    dispatcher.rs        # ClientRequest -> ApiDispatcher adapter
  transport/
    jsonl.rs             # stdin/stdout JSONL
    websocket.rs         # generic WS adapter primitives
    memory.rs            # in-process tests/direct clients
  client/
    callbacks.rs         # callback builders
    sink.rs              # frontend sink
```

在 crate 合并前，建议使用的临时位置为：

| 目标模块 | 临时位置 |
|---|---|
| `protocol/*` | `crates/allthecodes-ipc-protocol/src/` |
| `runtime/*` | `crates/allthecodes-ipc/src/runtime/` |
| `transport/*` | `crates/allthecodes-ipc-transport/src/` |
| `client/*` | `crates/allthecodes-ipc-client/src/` |
| WebSocket route adapter | `crates/allthecodes-web/src/ws/ipc_v2.rs` |

---

## 拟议 IPC v2 wire contract

### Envelope

沿用并扩展当前 `IpcEnvelope<T>`：

```rust
pub struct IpcEnvelope<T> {
    pub version: u16,
    pub id: String,
    pub seq: u64,
    pub timestamp: i64,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
    pub correlation_id: Option<String>,
    pub payload: T,
}
```

未来实现要求：

1. `seq` 必须在单连接内单调递增。
2. `id` 表示 envelope id，不等同于 request id。
3. `correlation_id` 用于跨 transport/trace 关联。
4. request/response/server-request 必须有自己的 protocol request id。

### Payload

建议未来新增 canonical payload：

```rust
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum IpcPayload {
    Hello(ClientHello),
    Ready(ServerReady),
    ClientRequest(ClientRequestEnvelope),
    ClientNotification(ClientNotificationEnvelope),
    ClientResponse(ClientResponseEnvelope),
    ClientError(ClientErrorEnvelope),
    ServerNotification(ServerNotificationEnvelope),
    ServerRequest(ServerRequestEnvelope),
    ServerError(ServerErrorEnvelope),
    Lagged(LaggedEvent),
}
```

### Handshake

未来 v2 客户端第一帧：

```json
{
  "version": 2,
  "id": "env-1",
  "seq": 1,
  "timestamp": 1700000000,
  "payload": {
    "kind": "hello",
    "data": {
      "client_name": "allthecodes-web",
      "client_version": "x.y.z",
      "supported_versions": [1, 2],
      "capabilities": {
        "server_requests": true,
        "lagged_events": true,
        "typed_client_requests": true
      },
      "notification_subscriptions": ["conversation", "tool", "permission", "flow_control"]
    }
  }
}
```

未来服务端回复 `Ready`，包含：

- accepted protocol version
- session id
- model/cwd/permission mode
- available models
- server capabilities
- legacy compatibility mode flag

---

## Legacy 兼容策略

### 路由

| 路由 | 状态 | wire format | 处理策略 |
|---|---|---|---|
| `/api/ipc/ws` | 已存在 | bare `FrontendMessage` / `BackendMessage` | 保留兼容，内部未来逐步接入 shared runtime |
| `/api/v2/ipc/ws` | 未实现 | `IpcEnvelope<IpcPayload>` | 未来新增 canonical IPC WebSocket |
| JSONL stdio | 已存在 legacy | bare legacy message first | 未来增加 envelope detection，支持 v2 opt-in |
| memory/in-process | 未实现 v2 | typed envelope | 未来用于测试和本进程客户端 |

### Adapter

legacy frontend input 映射：

| `FrontendMessage` | 拟议 v2 payload | 未来执行路径 |
|---|---|---|
| `SubmitPrompt { text, id }` | `ClientRequest` | 未来进入 `ApiDispatcher`；迁移前由 runtime 调 engine |
| `AbortQuery` | `ClientRequest` or `ClientNotification` | 未来进入 `ApiDispatcher` 或 runtime cancellation |
| `PermissionResponse` | `ClientResponse` | 未来回复 pending `ServerRequest` |
| `QuestionResponse` | `ClientResponse` | 未来回复 pending `ServerRequest` |
| `SlashCommand` | `ClientRequest` | 等 slash command DTO 稳定后迁移 |
| `Resize` | unsupported/no-op | PTY 专属，不进入 IPC v2 |
| subsystem commands | `ClientRequest` | 等 subsystem API 完成后迁移 |
| `Quit` | `ClientNotification` | 未来关闭当前 IPC runtime session |

legacy backend output 映射：

| `BackendMessage` | 拟议 v2 payload |
|---|---|
| `Ready` | `Ready` 或 `ServerNotification::Lifecycle` |
| stream/thinking/assistant/tombstone | `ServerNotification::Conversation` |
| tool use/result/progress | `ServerNotification::Tool` |
| permission/question request | `ServerRequest` |
| usage/status/suggestions/subsystem | `ServerNotification::FlowControl` |
| error | `ServerError` |

---

## Server request 模板

permission/question 未来不再只是 callback side effect，而是协议显式交互。以下类型尚未实现：

```rust
pub struct ServerRequestEnvelope {
    pub request_id: String,
    pub method: ServerRequestMethod,
    pub params: serde_json::Value,
    pub timeout_ms: Option<u64>,
}

pub enum ServerRequestMethod {
    PermissionDecision,
    AskUserQuestion,
}
```

未来客户端回复结构：

```rust
pub struct ClientResponseEnvelope {
    pub request_id: String,
    pub result: serde_json::Value,
}
```

未来客户端拒绝或 transport 失败结构：

```rust
pub struct ClientErrorEnvelope {
    pub request_id: String,
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}
```

未来实现规则：

1. `ServerRequest` 必须 lossless。
2. 连接断开时 pending permission 默认 deny，pending question 默认空字符串或显式 error。
3. 队列满时不能丢弃 `ServerRequest`；必须阻塞、拒绝 request，或关闭连接并回填 error。
4. legacy `/api/ipc/ws` 可以继续以 `BackendMessage::PermissionRequest` 发给前端，但 runtime 内部用 `ServerRequest` 管理 pending 状态。

---

## 背压和事件投递

### Delivery class

lossless：

- `Ready`
- stream start/delta/end
- thinking delta
- assistant message
- tombstone
- tool use/result
- permission/question server request
- final usage/result
- error
- lifecycle shutdown

best-effort：

- tool progress
- status line
- suggestions
- notification sent
- subsystem status/event
- telemetry refresh
- LSP recommendation progress

### Queue 行为

1. 每个 connection 未来应有 bounded outbound queue。
2. best-effort 满队列时可替换/丢弃旧 best-effort，并累计 drop count。
3. lossless 满队列时等待容量或触发 overload/close，不静默丢弃。
4. 从 best-effort 恢复到 lossless 前发送 `Lagged { skipped, last_dropped_type }`。
5. legacy bridge 未来可把 `Lagged` 映射为 recoverable `BackendMessage::Error`，v2 直接发送 `IpcPayload::Lagged`。

---

## 与 REST/API dispatcher 的关系

REST 未来仍值得保留，但定位是 HTTP adapter：

```text
REST route
  -> ClientRequest
  -> ApiDispatcher
```

IPC v2 规划为交互式 adapter：

```text
IPC envelope
  -> ClientRequest / ClientResponse / ClientNotification
  -> ApiDispatcher or pending server-request resolver
```

两者共享：

- typed request/response DTO
- serialization metadata
- experimental gate
- tracing/error conversion
- domain handler

两者不同：

- REST 是 request/response，没有 server request。
- IPC 是全双工流，支持 server notification/server request/lagged/backpressure。
- REST 不承载 assistant stream 的实时渲染事件，除非显式增加 SSE/WS adapter。

---

## 分阶段实施

### 阶段 0：冻结 legacy 行为

动作：

1. 给 `/api/ipc/ws` 的 bare `FrontendMessage` parse error、ready、submit、permission response、question response 增加 focused tests。
2. 给 JSONL stdio legacy roundtrip 增加测试。
3. 给 `IpcEnvelope` 增加 version compatibility、seq/correlation roundtrip 测试。
4. 给 `event_class` 增加 permission/question 为 lossless 的测试。

验证：

```bash
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes-ipc-transport
cargo test -p allthecodes-web --lib ipc
```

### 阶段 1：定义 IPC v2 payload

动作：

1. 新增 `IpcPayload`、`ClientHello`、`ServerReady`、`ServerRequestEnvelope`、`ClientResponseEnvelope`。
2. 保持 `IpcEnvelope<T>` 泛型不破坏旧测试。
3. 增加 legacy backend/frontend adapter 函数：
   - `legacy_frontend_to_payload`
   - `legacy_backend_to_payload`
   - `payload_to_legacy_backend`
4. 明确不支持映射的 legacy message 返回 typed adapter error。

验证：

```bash
cargo test -p allthecodes-ipc-protocol
```

### 阶段 2：抽 shared IPC runtime

动作：

1. 新增 `IpcRuntime`，负责：
   - handshake
   - seq 分配/校验
   - pending server requests
   - outbound queue
   - dispatch client request
   - cleanup
2. `ws/ipc.rs` 先不改 wire format，只把 callback pending/outbound queue 移入 runtime。
3. `allthecodes-ipc-client/src/callbacks.rs` 与 runtime pending store 合流。

验证：

```bash
cargo test -p allthecodes-ipc
cargo test -p allthecodes-ipc-client
cargo test -p allthecodes-web --lib ipc
```

### 阶段 3：permission/question 迁移为 server request

动作：

1. runtime 内部用 `ServerRequestEnvelope` 创建 pending request。
2. legacy adapter 把 server request 渲染成 `BackendMessage::PermissionRequest` / `QuestionRequest`。
3. legacy `PermissionResponse` / `QuestionResponse` 映射成 `ClientResponseEnvelope`。
4. 连接关闭时统一 reject/deny pending requests。

验证：

```bash
cargo test -p allthecodes-ipc permission
cargo test -p allthecodes-web --lib ipc
```

### 阶段 4：接入 ApiDispatcher

前置：等待 `02-transport-unification-plan.zh.md` 的 `ApiDispatcher` 阶段完成；当前尚未实现。

动作：

1. `SubmitPrompt`、`AbortQuery`、file search、completion 等命令逐步转为 `ClientRequest`。
2. runtime 对 `ClientRequest` 调 `ApiDispatcher::dispatch`。
3. response 转成 `ClientResponseEnvelope` 或相应 `ServerNotification`。
4. 尚未有 DTO 的命令保留 legacy fallback，并在映射表中标记。

验证：

```bash
cargo test -p allthecodes-web --lib api_dispatcher
cargo test -p allthecodes-ipc
```

### 阶段 5：新增 `/api/v2/ipc/ws`

动作：

1. 新增 `crates/allthecodes-web/src/ws/ipc_v2.rs`。
2. 使用 envelope JSON text frame。
3. 首帧要求 `Hello`，否则返回 protocol error。
4. outbound 发送 `IpcEnvelope<IpcPayload>`。
5. 不复用 `/api/ipc/ws` 的 bare message wire contract。

验证：

```bash
cargo test -p allthecodes-web --lib ipc_v2
```

### 阶段 6：JSONL/headless opt-in

动作：

1. JSONL stdio 默认继续 bare legacy。
2. 未来支持第一行 `Hello` envelope 后切换到 v2 envelope mode。
3. headless runtime 复用 shared `IpcRuntime`。
4. memory transport 使用 v2 mode，作为协议测试 harness。

验证：

```bash
cargo test -p allthecodes-ipc
cargo test -p allthecodes-ipc-transport
```

### 阶段 7：前端迁移窗口

动作：

1. 前端先继续用 `/api/ipc/ws`。
2. 新 UI 或实验 flag 未来使用 `/api/v2/ipc/ws`。
3. 双写/对照测试 legacy adapter 与 v2 event 是否等价。
4. 只有当前端稳定支持 v2 后，才考虑把 `/api/ipc/ws` 标记 deprecated。

未来接受条件（全部待实现）：

- legacy route 未破坏。
- v2 route 实现后可独立完成 hello、submit、stream、permission、abort、close。
- server request 不因队列满或断连挂起。

---

## 测试矩阵

| 测试 | 覆盖 |
|---|---|
| envelope roundtrip | version/id/seq/session/correlation |
| legacy adapter roundtrip | `FrontendMessage`/`BackendMessage` 兼容 |
| server request lifecycle | permission/question create、resolve、reject、disconnect cleanup |
| outbound queue pressure | best-effort drop、lossless preserve、lagged marker |
| v2 handshake | supported version/capabilities/subscriptions |
| dispatcher mapping | client request 进入 `ApiDispatcher` |
| WebSocket integration | `/api/ipc/ws` legacy + `/api/v2/ipc/ws` envelope |
| JSONL opt-in | legacy default + envelope mode |

---

## 风险和缓解

| 风险 | 描述 | 缓解 |
|---|---|---|
| frontend breakage | `/api/ipc/ws` 已被前端依赖 | legacy route 不改 wire format，新建 v2 route |
| 双协议长期分叉 | legacy/v2 逻辑各写一套 | shared runtime + adapter，避免 handler 复制业务逻辑 |
| server request 挂起 | permission/question 队列满或断连 | pending store cleanup + reject/deny fallback |
| 背压导致 stream 丢字 | assistant delta 被当 best-effort | lossless 分类测试固定 |
| dispatcher 未成熟 | submit/abort DTO 尚未稳定 | 先用 runtime fallback，DTO 稳定后逐步迁移 |
| crate 合并顺序冲突 | 01 计划可能移动 IPC 文件 | 先按模块名实现，合并时机械搬迁 |

---

## 完成后的接受标准

- `IpcEnvelope<IpcPayload>` 成为未来新增 IPC transport 的唯一模板。
- `/api/ipc/ws` 保持 legacy bare message 兼容。
- `/api/v2/ipc/ws` 实现后支持 hello/ready、submit、stream、permission/question、abort、close。
- permission/question 在 runtime 内部表现为 `ServerRequest`，不会静默丢弃或无限等待。
- 可映射 API 命令通过 `ApiDispatcher`，不再散落在 WebSocket handler。
- outbound queue 有 lossless/best-effort 测试和 `Lagged`/diagnostic 行为。
- JSONL stdio 可继续 legacy，并在未来支持 envelope opt-in。
