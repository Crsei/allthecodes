# IPC 模板未来应用计划

> 基于 Codex app-server/client 实现对照，以及本项目当前 `/api/ipc/ws`、JSONL stdio、IPC protocol/envelope/event-class 的现状。
> 目标：把 IPC 从“某个 WebSocket handler 的前后端消息枚举”升级为“协议级 request/event/server-request envelope”，同时保留 legacy frontend contract。

状态：滚动实施中。`IpcPayload`、`IpcRuntime`、`ServerRequestEnvelope`、legacy `/api/ipc/ws` runtime 接入、server-request 超时、outbound lossless/best-effort 分类投递已经实现；`/api/v2/ipc/ws`、JSONL envelope opt-in、`ApiDispatcher` 全量接入仍为未来目标。

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

---

## 实现回顾 (2026-06-07)

> 对照当前代码库检查计划与实际实现的差距。

### 文档状态已同步

文档头部已从"规划草案，尚未实现"更新为滚动实施状态。当前代码已实现 IPC v2 payload、shared runtime、server request pending store、legacy `/api/ipc/ws` runtime 接入、server-request 超时和 outbound 分类投递；尚未实现的是 `/api/v2/ipc/ws`、JSONL envelope opt-in、以及完整 `ApiDispatcher` 接入。

### 各阶段实际完成状态速览

| 阶段 | 文档状态 | 实际状态 | 核心差异 |
|---|---|---|---|
| 0: 冻结 legacy 行为 | 待做 | ⚠️ 大部分 | `IpcEnvelope` roundtrip、JSONL legacy parse、`event_class`、`ws/ipc.rs` focused tests 已存在；仍需 broader integration 覆盖 |
| 1: 定义 IPC v2 payload | 待做 | ✅ **已完成** | `payload.rs` 含完整 `IpcPayload` 枚举、所有 envelope 类型、legacy adapter 函数、12 个测试 |
| 2: 抽 shared IPC runtime | 待做 | ✅ **当前拆分 crate 下已完成** | `runtime.rs` 含 `IpcRuntime`、`PendingInteractions`、`SessionRuntime`、分类 `IpcOutboundQueue`；`ws/ipc.rs` 已集成 runtime |
| 3: permission/question → server request | 待做 | ✅ **当前阶段已完成** | `request_permission()`、`request_question()` 使用 `ServerRequestEnvelope` + oneshot channel + 30s timeout；cleanup 会 deny/empty pending |
| 4: 接入 ApiDispatcher | 待做 | ❌ 未开始 | `dispatch_client_request()` 方法存在但需外部传入 dispatch 函数 |
| 5: `/api/v2/ipc/ws` | 待做 | ❌ 未开始 | `ws/ipc_v2.rs` 不存在 |
| 6: JSONL/headless opt-in | 待做 | ❌ 未开始 | 无 envelope mode |
| 7: 前端迁移窗口 | 待做 | ❌ 未开始 | |

### 1. Phase 1 实际上已完成

`crates/allthecodes-ipc-protocol/src/payload.rs` 实现了计划要求的所有 IPC v2 payload 类型：

计划要求的枚举与实现完全一致：

```
IpcPayload::Hello(ClientHello)            ✅
IpcPayload::Ready(ServerReady)            ✅
IpcPayload::ClientRequest(ClientRequestEnvelope)       ✅
IpcPayload::ClientNotification(ClientNotificationEnvelope)  ✅
IpcPayload::ClientResponse(ClientResponseEnvelope)     ✅
IpcPayload::ClientError(ClientErrorEnvelope)           ✅
IpcPayload::ServerNotification(ServerNotificationEnvelope)  ✅
IpcPayload::ServerRequest(ServerRequestEnvelope)       ✅
IpcPayload::ServerError(ServerErrorEnvelope)           ✅
IpcPayload::Lagged(LaggedEvent)           ✅
```

Legacy adapter 函数已完整实现：

| 函数 | 位置 | 映射覆盖 |
|---|---|---|
| `legacy_frontend_to_payload()` | `payload.rs:214` | `SubmitPrompt`/`AbortQuery`/`SlashCommand`/`Quit` → ClientNotification；`PermissionResponse`/`QuestionResponse` → ClientResponse；`Resize` → 返回 `UnsupportedLegacyFrontend` 错误 |
| `legacy_backend_to_payload()` | `payload.rs:285` | `Ready` → Ready；`PermissionRequest`/`QuestionRequest` → ServerRequest；`Error` → ServerError；其余 fallback 到 `normalized.rs` 分类 → ServerNotification |
| `payload_to_legacy_backend()` | `payload.rs:372` | Ready/ServerRequest/ServerError/Lagged → BackendMessage；其余返回 UnsupportedPayload 错误 |

测试覆盖（`payload.rs:507-720`）：12 个测试覆盖 hello roundtrip、permission→ServerRequest roundtrip、question→ClientResponse、submit→notification、unsupported message 分类、lagged→legacy error、client request/response 序列化。

**状态**：文档头部和 Phase 1 状态已更新为"已完成"。

### 2. Phase 2 当前阶段已完成——`IpcRuntime` 已集成到 `ws/ipc.rs`

`crates/allthecodes-ipc/src/runtime.rs` 实现了 `IpcRuntime`（第 304-585 行），包含：

- `IpcRuntime` 结构体（session、outbound queue、seq counter、inbound seq validator）
- `SessionRuntime`（session_id、run_id、current turn tracking）
- `PendingInteractions`（scoped + legacy permission pending store、question pending store、server request store、cleanup）
- `IpcOutboundQueue`（mpsc channel wrapper，使用 `classify_event()` 区分 lossless/best-effort）
- `request_permission()` / `request_question()` — 通过 ServerRequestEnvelope + oneshot channel 交互
- `dispatch_client_request()` — 接受通用 dispatch 回调的函数
- `resolve_legacy_client_response()` — 将 FrontendMessage 映射到 pending interaction
- 17 个单元测试（scoped permission matching、legacy fallback、session turn tracking、cleanup、outbound queue pressure、permission/question lifecycle、timeout、seq 验证、hello 版本检查、ready payload、client request dispatch）

**最关键的是**：`crates/allthecodes-web/src/ws/ipc.rs` 第 38 行已导入并使用 `IpcRuntime`：

```
ws/ipc.rs:117  →  let (ipc_runtime, mut outbound_rx) = IpcRuntime::new(actual_session_id.clone(), 256);
ws/ipc.rs:120-139  →  安装 permission/ask_user callback（使用 runtime.request_permission/question）
ws/ipc.rs:154  →  runtime.send_backend(ready).await 发送 Ready
ws/ipc.rs:157-163  →  runtime outbound 转发到 WebSocket
ws/ipc.rs:171-218  →  runtime.resolve_legacy_client_response() 处理 PermissionResponse/QuestionResponse
```

目前 permission/question 交互已走"ServerRequest → oneshot channel → 前端回复/超时/cleanup → resolve"路径，与计划第 3 章"Server request 模板"描述一致。outbound queue 已接入 lossless/best-effort 分类分派（详见下方第 4 点）。

### 3. 文档 Phase 3（permission→server request）已完成当前阶段

计划 Phase 3 的要求与当前实现对照：

| 要求 | 实际 | 状态 |
|---|---|---|
| 3.1 runtime 内部用 `ServerRequestEnvelope` 创建 pending request | `request_permission()` 第 393 行创建 `ServerRequestEnvelope`，第 408 行 `insert_server_request` | ✅ |
| 3.2 legacy adapter 渲染为 `BackendMessage::PermissionRequest` / `QuestionRequest` | `payload_to_legacy_backend` 第 406 行 `server_request_to_legacy_backend` | ✅ |
| 3.3 `PermissionResponse` / `QuestionResponse` 映射为 `ClientResponseEnvelope` | `resolve_legacy_client_response()` 第 464 行 + `legacy_frontend_to_payload` 第 249 行（PermissionResponse/QuestionResponse → ClientResponse） | ✅ |
| 3.4 连接关闭时统一 reject/deny pending requests | `PendingInteractions::cleanup()` 第 178 行 drain 所有 pending store，permission 发 deny、question 发空字符串 | ✅ |
| 3.5 正常连接但用户不响应时超时 | `request_permission()`/`request_question()` 设置 `timeout_ms=30000` 并通过 `tokio::time::timeout` 回落到 deny/empty | ✅ |

剩余注意点：当前超时值为 runtime 默认 30s，尚未暴露为配置项；前端断连仍通过 socket task 退出时的 cleanup 统一处理。

### 4. `IpcOutboundQueue` 已接入 lossless/best-effort 分派

`crates/allthecodes-ipc-transport/src/event_class.rs` 定义的 `classify_event()` 已接入 `crates/allthecodes-ipc/src/runtime.rs` 的 `IpcOutboundQueue`：

```rust
pub struct IpcOutboundQueue {
    sender: mpsc::Sender<BackendMessage>,
    dropped_best_effort: Arc<Mutex<DroppedBestEffort>>,
}
```

| 计划要求的行为 | 实际 | 状态 |
|---|---|---|
| best-effort 满队列时可替换/丢弃旧 best-effort，累计 drop count | `send_best_effort()` 使用 `try_send`；满队列时丢弃 incoming best-effort，累计 skipped 和 last dropped type | ✅ |
| lossless 满队列时等待容量或触发 overload/close | `send_lossless()` 使用 `sender.send(...).await`，不会静默丢弃 lossless；sender 关闭时返回 `OutboundClosed` | ✅ |
| 从 best-effort 恢复到 lossless 前发送 `Lagged { skipped, last_dropped_type }` | `send_lossless()` 发送下一条 lossless 前注入 legacy `BackendMessage::Error` 形式的 `IpcPayload::Lagged` | ✅ |

当前实现没有维护独立 `VecDeque` 或替换已排队的 best-effort 事件，而是在 bounded mpsc 已满时丢弃 incoming best-effort。这个策略满足“不阻塞 best-effort、不丢 lossless、恢复时通知 lagged”的核心要求，且不改变 legacy `/api/ipc/ws` wire format。

### 5. `ServerRequest` 已有超时机制

`ServerRequestEnvelope` 定义的 `timeout_ms: Option<u64>` 已在 `request_permission()` 和 `request_question()` 中写入默认 30000ms。pending request 等待使用 `tokio::time::timeout`：

- permission 超时、sender dropped、连接 cleanup 时默认返回 `PermissionResponsePayload::deny()`。
- question 超时、sender dropped、连接 cleanup 时默认返回空字符串。
- 超时发生后会移除 pending permission/question 和对应 `ServerRequestEnvelope`。

测试覆盖：`permission_request_times_out_with_deny_and_cleans_pending`、`question_request_times_out_with_empty_answer_and_cleans_pending`。

### 6. `dispatch_client_request()` 存在但未接 `ApiDispatcher`

`runtime.rs:555-584` 实现了 `dispatch_client_request<F, Fut>()`：

```rust
pub async fn dispatch_client_request<F, Fut>(
    &self,
    request: ClientRequestEnvelope,
    dispatch: F,
) -> Result<ClientResponseEnvelope, ServerErrorEnvelope>
where
    F: FnOnce(ClientRequest) -> Fut,
    Fut: Future<Output = Result<ClientResponse, ApiError>>,
```

但：
1. `ws/ipc.rs` 的 inbound handler 中未调用此方法
2. 没有与 `allthecodes_protocol::ApiDispatcher` 连接
3. `SubmitPrompt` 仍然直接调 `engine.submit_query()`（legacy 路径）

这与计划一致——Phase 4 明确说"等待 `02-transport-unification-plan.zh.md` 的 `ApiDispatcher` 阶段完成"。当前 `ApiDispatcher` 只覆盖了 5 个 endpoint，Chat/SubmitPrompt 尚无 `ClientRequest` DTO 稳定可用。

### 7. `try_claim(SessionOwner::IpcWs)` 仍然阻塞

`ws/ipc.rs` 第 125 行仍然使用 `state.try_claim(SessionOwner::IpcWs, active_session_id)`。计划文档 Phase 5 第 9 条说移除 `try_claim` 的前提是：
- Phase 4 typed serialization queue 完成
- permission/question server request 不依赖全局 owner

两个前提都未满足（Phase 4 在 02 计划中也未开始）。

### 8. 文件分布跨 4 个 crate

文档计划的目标文件布局（单 crate `allthecodes-ipc/src/{protocol,runtime,transport,client}/`）与实际分布对比：

| 目标模块 | 计划位置 | 实际位置 | 状态 |
|---|---|---|---|
| protocol/envelope.rs | `allthecodes-ipc/src/protocol/` | `allthecodes-ipc-protocol/src/envelope.rs` | 独立 crate |
| protocol/payload.rs | `allthecodes-ipc/src/protocol/` | `allthecodes-ipc-protocol/src/payload.rs` | 独立 crate |
| protocol/legacy.rs | `allthecodes-ipc/src/protocol/` | 无专用文件；存在于 `payload.rs` + `normalized.rs` | 未拆分 |
| protocol/normalized.rs | `allthecodes-ipc/src/protocol/` | `allthecodes-ipc-protocol/src/normalized.rs` | 独立 crate |
| runtime/mod.rs | `allthecodes-ipc/src/runtime/` | `allthecodes-ipc/src/runtime.rs` | 单文件非目录 |
| runtime/pending.rs | `allthecodes-ipc/src/runtime/` | 与 runtime.rs 合并 | 未拆分 |
| transport/jsonl.rs | `allthecodes-ipc/src/transport/` | `allthecodes-ipc-transport/src/jsonl.rs` | 独立 crate |
| transport/event_class.rs | `allthecodes-ipc/src/transport/` | `allthecodes-ipc-transport/src/event_class.rs` | 独立 crate；且 `allthecodes-ipc-client` 也有同名 `event_class.rs` |
| client/callbacks.rs | `allthecodes-ipc/src/client/` | `allthecodes-ipc-client/src/callbacks.rs` | 独立 crate |

Crate 合并（01）尚未执行。两个 `event_class.rs` 文件（client 和 transport）增加了混淆——它们的功能相似但属于不同模块。

### 9. 文档与实际保持一致的部分

| 计划要求 | 实际 | 状态 |
|---|---|---|
| `/api/ipc/ws` 保留为 legacy bridge，不作为新模板扩张 | 仍使用 `FrontendMessage`/`BackendMessage` wire format；未增加新 wire format | ✅ |
| `IpcEnvelope<T>` 已有版本/seq/correlation metadata | `envelope.rs` 结构体完全符合计划定义 | ✅ |
| 可映射为 API 的命令未来进入 `ApiDispatcher` | 当前 SubmitPrompt 仍走 legacy engine path；`dispatch_client_request()` 等待 | ✅ |
| PTY/MCP/browser 不进入此系统 | 未受影响 | ✅ |

### 建议的下一步

| 优先级 | 动作 | 风险 | 依赖 |
|---|---|---|---|
| **Done** | 更新文档头部和阶段状态表（标注 Phase 1 完成、Phase 2/3 当前阶段完成） | — | — |
| **Done** | 在 `IpcOutboundQueue` 中实现 `send_lossless()` / `send_best_effort()`，使用 `event_class::classify_event()` + legacy `Lagged` | — | — |
| **Done** | 在 `request_permission()/request_question()` 中添加 `tokio::time::timeout` | — | — |
| **P1** | `dispatch_client_request()` 接 `ApiDispatcher`（当 ApiDispatcher 覆盖足够 endpoint 后） | 中 | 02 计划 Phase 1-2 |
| **P2** | `/api/v2/ipc/ws` 路由（`ws/ipc_v2.rs`） | 中——需 handshake + envelope frame | Phase 1-3 完成 |
| **P3** | JSONL envelope opt-in | 低 | — |
| **P3** | 把 30s server-request timeout 暴露为配置项或 session 参数 | 低 | runtime policy 定稿 |
