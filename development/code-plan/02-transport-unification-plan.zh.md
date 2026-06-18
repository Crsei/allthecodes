# Transport 业务对比与统一计划

> 基于本项目当前 transport 分布，以及本地 Codex app-server 架构对照。
> 目标不是把所有网络/进程通信代码合成一个模块，而是让相同 API 语义只走一条请求分发和并发控制路径。

状态：规划文档，尚未实现。本文中的 `ApiDispatcher`、API JSON-RPC WebSocket、typed serialization queue、IPC v2 bridge 等均为未来目标；当前已经存在的是文中列出的 REST routes、legacy `/api/ipc/ws`、PTY WebSocket、daemon HTTP/SSE、JSONL stdio、MCP/browser transports 等现状入口。

---

## 总览

Codex 的 transport 架构分两层：

1. app-server transport 层：`stdio://`、`unix://`、`ws://IP:PORT`、`off`、in-process、remote-control。
2. 业务协议中的 transport 字段：realtime 的 websocket/webrtc，MCP 的 stdio/streamable-http。

本项目也有多种 transport，但现在按产品功能分散：

1. `allthecodes-web`：REST API、IPC WebSocket、PTY terminal WebSocket、legacy TUI WebSocket。
2. `allthecodes-daemon`：HTTP API、SSE event stream、gateway routes。
3. `allthecodes-ipc-*`：JSONL stdio、memory transport、frontend sink。
4. `allthecodes-protocol`：`JsonRpcFrame`、`Transport` trait、`DirectTransport`，但尚未成为运行时统一入口。
5. `allthecodes-mcp`：MCP stdio、legacy SSE、streamable HTTP reader loops。
6. `allthecodes-browser`：Chrome native host socket path and listener support。

核心差距：Codex 先把不同 app-server transport 归一为连接事件，然后统一进入 `MessageProcessor` 和 serialization queue；本项目的 REST、IPC WS、PTY WS、daemon HTTP/SSE 目前分别直接绑定业务 handler。

---

## 目标架构

### 需要统一的部分

这些 transport 应逐步共享同一个 API dispatcher：

- `allthecodes-web` REST API routes。
- 将来新增的 JSON-RPC WebSocket API。
- `allthecodes-protocol::DirectTransport` in-process 调用。
- 未来 IPC v2 中能映射到 `ClientRequest` 的命令；legacy `/api/ipc/ws` 先只作为兼容 adapter。

目标路径：

```text
Transport adapter
  -> ClientRequest / ClientNotification
  -> ApiDispatcher
  -> typed serialization queue
  -> domain processor / handler
  -> ClientResponse / ServerNotification
  -> transport adapter
```

### 应保持专用的部分

这些 transport 不应强行并入 API dispatcher：

- PTY terminal WebSocket：它是终端字节流/resize/control 协议，不是 API request/response。
- MCP client transports：它们是连接外部 MCP server 的客户端协议，不是本项目 app API transport。
- Browser native host socket：它是 Chrome native messaging bridge 的进程间通道。
- Realtime websocket/webrtc 字段：这是业务能力的 media/control transport 选择，不是 app API 承载层。

### 暂缓统一的部分

`allthecodes-daemon` 暂时保持独立 HTTP/SSE API。它有 supervisor、command store、SSE replay、control token 等 daemon 专属语义。后续可以把可复用 DTO 和事件映射抽到协议层，但不要第一阶段并入 web API dispatcher。

IPC 模板的细化迁移计划见 `development/code-plan/03-ipc-template-future-application-plan.zh.md`。02 只定义 transport 统一的大方向，03 负责 IPC v2、server request、legacy adapter、背压和 JSONL/headless opt-in 的具体步骤。

---

## Codex 对照结论

| Codex 做法 | 本项目现状 | 计划影响 |
|---|---|---|
| `TransportEvent::{ConnectionOpened, IncomingMessage, ConnectionClosed}` 统一入口 | web REST、web WS、daemon SSE、IPC stdio 各自处理 | 新增 API transport runtime 时采用连接事件模型，不改 PTY/MCP |
| `MessageProcessor` 同时处理 JSON-RPC 和 typed in-process request | `allthecodes-protocol::DirectTransport` 仅有协议雏形 | 建立 `ApiDispatcher`，让 REST/direct/JSON-RPC 共用 |
| 出站消息统一 `OutgoingEnvelope`，按连接状态过滤 | daemon SSE、IPC WS、terminal WS 各自 fanout | API WebSocket 采用统一 outbound router；daemon SSE 保持独立 |
| protocol metadata 生成 serialization scope | 当前 `SerializationLayer` 是字符串 key + semaphore | 改为 typed key + `Exclusive`/`SharedRead` FIFO queue |
| remote-control 是独立业务 transport | 本项目尚无同类 app-server remote-control | 不作为当前目标，后续如做远控另立模块 |

---

## 文件归属目标

| 关注点 | 最终位置 |
|---|---|
| API dispatcher and shared request lifecycle | `crates/allthecodes-web/src/api_dispatcher.rs` |
| Axum REST route adapter | `crates/allthecodes-web/src/handler_registry.rs` and `handlers/<domain>.rs` |
| JSON-RPC WebSocket adapter | `crates/allthecodes-web/src/ws/api.rs` |
| In-process direct adapter | `crates/allthecodes-protocol/src/transport.rs` plus web dispatcher implementation |
| Typed serialization queues | `crates/allthecodes-web/src/serialization.rs` |
| Protocol request/response/metadata | `crates/allthecodes-protocol/src/{request.rs,response.rs,transport.rs,macros.rs}` |
| IPC compatibility bridge | `crates/allthecodes-web/src/ws/ipc.rs` 保留 legacy `/api/ipc/ws` |
| IPC v2 protocol payload/adapters | 规划：`crates/allthecodes-ipc-protocol/src/{payload.rs,legacy.rs}` 或合并后的 `allthecodes-ipc/src/protocol/` |
| Shared IPC runtime | 规划：`crates/allthecodes-ipc/src/runtime/` |
| IPC v2 WebSocket adapter | 规划：`crates/allthecodes-web/src/ws/ipc_v2.rs` |
| PTY transport | `crates/allthecodes-web/src/ws/terminal.rs` |
| Daemon HTTP/SSE | `crates/allthecodes-daemon/src/{routes.rs,sse.rs,state.rs}` |
| MCP external server transport | `crates/allthecodes-mcp/src/transport.rs` |
| Browser native host transport | `crates/allthecodes-browser/src/transport.rs` |

---

## 阶段 0：补齐 transport 盘点和边界测试

### 目标

先冻结现状行为，避免统一过程中改变协议语义。

### 动作

1. 增加一份 transport inventory 测试或文档片段，列出当前公开入口：
   - `/api/*` REST
   - `/api/v2/*` mirror
   - `/api/terminal/*`
   - `/api/terminal/sessions/{id}/ws`
   - `/api/tui/ws`
   - `/api/ipc/ws`
   - daemon `/api/*`
   - daemon `/events`
   - IPC JSONL stdio
2. 为 `handler_registry::protocol_routes_handler` 保留覆盖测试，确保 REST 和 v2 mirror 继续注册。
3. 为 `allthecodes-protocol::JsonRpcFrame` 补充 invalid frame/error frame roundtrip 测试。
4. 明确标记不进入 API dispatcher 的 transport：PTY、MCP、browser native host。

### 验证

```bash
cargo test -p allthecodes-protocol
cargo test -p allthecodes-web --lib
```

---

## 阶段 1：建立 `ApiDispatcher`

### 目标

把请求生命周期从 Axum route handler 中抽出，形成 REST/direct/JSON-RPC 可共享的中心层。

### 动作

1. 新增 `crates/allthecodes-web/src/api_dispatcher.rs`。
2. 定义核心接口：

```rust
pub struct ApiRequestContext {
    pub connection_id: Option<ApiConnectionId>,
    pub transport: ApiTransportKind,
}

pub enum ApiTransportKind {
    Rest,
    JsonRpcWebSocket,
    Direct,
    IpcBridge,
}

pub async fn dispatch(
    state: WebState,
    context: ApiRequestContext,
    request: allthecodes_protocol::ClientRequest,
) -> Result<allthecodes_protocol::ClientResponse, allthecodes_protocol::ApiError>;
```

3. `dispatch` 内部集中处理：
   - experimental gate
   - request serialization scope
   - domain processor routing
   - shared error conversion
   - tracing fields
4. 先只迁移已经 processor 化的 endpoint，例如 session list/detail/resume/archive 和 capabilities。
5. 保持 `handler_registry.rs` 继续作为 Axum route composition root。

### 验证

```bash
cargo test -p allthecodes-web --lib
cargo test -p allthecodes-protocol
```

---

## 阶段 2：REST route adapter 接入 dispatcher

### 目标

REST API 不再直接决定业务分发规则，而是把 HTTP request 转成 `ClientRequest` 后进入 `ApiDispatcher`。

### 动作

1. 在 `processors.rs` 中保留 Axum adapter helper，但让 helper 调用 `ApiDispatcher`。
2. 对已迁移 endpoint：
   - 从 path/query/body 组合 params。
   - 构造 `ClientRequest::<Variant>(params)`。
   - 调用 `dispatch`。
   - 将 `ClientResponse::<Variant>` 转回 JSON。
3. 未迁移 endpoint 继续使用现有 handler，避免一次性大改。
4. 将迁移状态写入注释或测试表，避免 route 混乱。

### 验证

```bash
cargo test -p allthecodes-web --lib
cargo fmt --all --check
```

---

## 阶段 3：新增 API JSON-RPC WebSocket

### 目标

提供 Codex 风格的 app API WebSocket，但不替代 PTY 和 IPC WS。

### 动作

1. 新增 `crates/allthecodes-web/src/ws/api.rs`。
2. 使用 `allthecodes_protocol::JsonRpcFrame` 作为 wire frame。
3. 连接处理采用类似 Codex 的事件模型：
   - `ConnectionOpened`
   - `IncomingFrame`
   - `ConnectionClosed`
4. 出站使用 bounded channel，慢连接不阻塞 dispatcher。
5. 支持 request/response/error/notification frame。
6. 添加 route，例如 `GET /api/ws` 或 `GET /api/v2/ws`。
7. 不复用 `/api/ipc/ws`，避免破坏旧 frontend contract。

### 验证

```bash
cargo test -p allthecodes-web --lib
cargo test -p allthecodes-protocol
```

---

## 阶段 4：替换字符串 semaphore serialization

### 目标

把当前 `SerializationLayer` 从字符串 key + single semaphore 升级到 Codex 风格 typed queue。

### 动作

1. 定义 typed key：

```rust
pub enum RequestSerializationQueueKey {
    Global,
    Session(String),
    Connection(ApiConnectionId),
    Process,
    DomainKey { domain: &'static str, key: String },
}
```

2. 定义 access mode：

```rust
pub enum RequestSerializationAccess {
    Exclusive,
    SharedRead,
}
```

3. `allthecodes-protocol` metadata 需要能表达 read/write：
   - 默认 `Concurrent`
   - mutating endpoint 使用 `Exclusive`
   - read-only endpoint 可使用 `SharedRead`
4. queue 行为：
   - 同 key FIFO。
   - `Exclusive` 独占。
   - 连续 `SharedRead` 可并发 batch。
   - 不同 key 并发。
5. 删除或废弃 `SessionOwnership` 作为并发控制机制。

### 验证

```bash
cargo test -p allthecodes-web serialization
cargo test -p allthecodes-protocol
```

---

## 阶段 5：IPC v2 模板与 legacy bridge 迁移

### 目标

保持 `/api/ipc/ws` wire format 不变，同时为未来新增 IPC v2 模板做准备。当前阶段不是宣称 IPC v2 已实现，而是明确实现顺序：先抽 shared runtime 和 adapter，再新增 `/api/v2/ipc/ws`，最后逐步把可映射命令接入 `ApiDispatcher`。

### 动作

1. 以 `03-ipc-template-future-application-plan.zh.md` 作为 IPC 详细执行文档，避免在本计划中重复所有协议细节。
2. 冻结 legacy `/api/ipc/ws` 行为：
   - 继续接收 bare `FrontendMessage`。
   - 继续发送 bare `BackendMessage`。
   - 不在该路由直接切换成 envelope wire format。
3. 规划新增 IPC v2 payload，但不要影响 legacy route：
   - `IpcPayload::Hello`
   - `IpcPayload::Ready`
   - `IpcPayload::ClientRequest`
   - `IpcPayload::ClientResponse`
   - `IpcPayload::ServerNotification`
   - `IpcPayload::ServerRequest`
   - `IpcPayload::ServerError`
   - `IpcPayload::Lagged`
4. 抽 shared IPC runtime：
   - pending permission/question store
   - outbound bounded queue
   - lossless/best-effort 分类
   - connection cleanup
   - legacy adapter
5. 将 permission/question 从隐式 callback pending 状态改为 runtime 内部的 server request 模型：
   - legacy 前端仍看到 `BackendMessage::PermissionRequest` / `QuestionRequest`。
   - runtime 内部用 request id 等待 `ClientResponse`。
   - 断连或队列过载时必须 deny/reject，不能无限等待。
6. 给 `FrontendMessage` 建 future mapping 表：

| FrontendMessage | 可映射到 ClientRequest | 处理 |
|---|---|---|
| `SubmitPrompt` | 是，前提是 chat/session request DTO 稳定 | 未来进入 dispatcher；迁移前保持 legacy engine path |
| `AbortQuery` | 是 | 未来进入 dispatcher 或 runtime cancellation |
| `PermissionResponse` | 不是普通 API request | 未来映射为 `ClientResponse`，回复 pending `ServerRequest` |
| `QuestionResponse` | 不是普通 API request | 未来映射为 `ClientResponse`，回复 pending `ServerRequest` |
| `SlashCommand` | 可选 | 等 slash command DTO 稳定后迁移 |
| `Resize` | 否 | PTY 专属，IPC v2 不承载 |
| subsystem commands | 是，前提是 subsystem API 完成 | 未来进入 dispatcher；当前保持 legacy/TODO |

7. 新增 `/api/v2/ipc/ws` 必须晚于 shared runtime 和 v2 payload：
   - v2 route 使用 `IpcEnvelope<IpcPayload>`。
   - 首帧需要 `Hello`。
   - 出站事件使用 v2 envelope。
   - 不复用 `/api/ipc/ws` 的 bare message contract。
8. `SubmitPrompt` 和 `AbortQuery` 接入 dispatcher 的前提：
   - 阶段 1 `ApiDispatcher` 完成并可用。
   - 对应 `ClientRequest` DTO 已稳定。
   - streaming response 能被转换为 `ServerNotification` 或 legacy `BackendMessage`。
9. 移除 `try_claim(SessionOwner::IpcWs, ...)` 的前提：
   - 阶段 4 typed serialization queue 完成并可用。
   - permission/question server request 不依赖全局 owner。
   - legacy `/api/ipc/ws` 已迁移到 runtime 管理连接生命周期。

### 验证

```bash
cargo test -p allthecodes-ipc
cargo test -p allthecodes-ipc-protocol
cargo test -p allthecodes-ipc-transport
cargo test -p allthecodes-web --lib
```

---

## 阶段 6：daemon 边界整理

### 目标

不把 daemon 第一时间塞进 web dispatcher，但消除可复用 DTO 和事件映射重复。

### 动作

1. 保持 daemon `/api/*` 和 `/events` 路由独立。
2. 将可复用的 daemon request/response DTO 移到 daemon 专属 protocol module，或未来的 `allthecodes-protocol::daemon`。
3. 保持 SSE replay、client registry、control token 在 daemon 内。
4. 如果未来需要统一，先新增 daemon adapter，而不是直接复用 web `WebState`。

### 验证

```bash
cargo test -p allthecodes-daemon --lib
```

---

## 阶段 7：收敛文档和生成物

### 动作

1. 更新 API 架构计划，标明：
   - REST/direct/JSON-RPC 是统一 dispatcher 范围。
   - PTY/MCP/browser native host 不在统一范围。
   - daemon 只做边界整理。
2. 更新 route docs/openapi/codegen，确保新增 JSON-RPC WS 不影响 REST schema。
3. 增加架构测试：
   - 每个 `ClientRequest` variant 有 dispatcher owner 或明确 unsupported。
   - 每个 endpoint 有 serialization metadata。
   - `SharedRead` queue 行为不阻塞同 key reads。

### 验证

```bash
cargo fmt --all --check
cargo clippy --workspace --lib --bins
cargo build --workspace --release
```

---

## 风险矩阵

| 风险 | 描述 | 缓解 |
|---|---|---|
| REST 行为变化 | route handler 转 dispatcher 后错误格式或状态码变化 | 每个 endpoint 分批迁移，保留现有 handler 测试 |
| IPC 前端破坏 | `/api/ipc/ws` wire format 已被 frontend 依赖 | 只替换内部路径，不改 `FrontendMessage`/`BackendMessage` |
| 过度统一 PTY/MCP | 字节流或外部协议被错误塞进 request/response 模型 | 明确列为专用 transport |
| daemon 语义丢失 | SSE replay/control token/supervisor 与 web API 生命周期不同 | daemon 第一阶段不并入 dispatcher |
| serialization deadlock | 新 queue 支持 SharedRead/Exclusive 后行为复杂 | 独立单元测试覆盖 FIFO、不同 key 并发、shared read batch、panic/closed cleanup |

---

## 完成后的接受标准

- `allthecodes-protocol::DirectTransport` 能通过同一个 dispatcher 执行至少 session/capabilities API。
- REST 中已迁移 endpoint 与 direct/JSON-RPC path 共享同一 domain processor。
- 新 API WebSocket 使用 `JsonRpcFrame`，不复用 IPC/PTY wire format。
- `SerializationLayer` 不再依赖 `SessionOwnership` 做跨 transport 独占。
- PTY、MCP、browser native host 保持专用协议边界。
- daemon HTTP/SSE 行为保持兼容，除非有单独 daemon API 迁移计划。

---

## 实现回顾 (2026-06-07)

> 对照当前代码库检查计划与实际实现的差距。

### 各阶段完成状态速览

| 阶段 | 目标 | 状态 | 关键发现 |
|---|---|---|---|
| 0 | Transport 盘点与边界测试 | ⚠️ 部分 | `protocol_routes_handler` 测试存在；`JsonRpcFrame` roundtrip 测试存在但缺 invalid frame 覆盖；无 transport inventory 文档 |
| 1 | 建立 `ApiDispatcher` | ⚠️ **部分实现** | `api_dispatcher.rs` 已存在，但仅处理 5 个 `ClientRequest` 变体；`MessageProcessor` trait 已实现 |
| 2 | REST route adapter 接入 dispatcher | ❌ **未开始** | REST handler 完全绕过 `ApiDispatcher` |
| 3 | JSON-RPC WebSocket | ❌ 未开始 | `ws/api.rs` 不存在 |
| 4 | 替换 semaphore serialization | ❌ 未开始 | `SerializationLayer` 仍用 `Semaphore(1)`；无 `SharedRead`/`Exclusive` 访问模式 |
| 5 | IPC v2 模板与 legacy bridge 迁移 | ❌ 未开始 | `ws/ipc_v2.rs` 不存在；`IpcPayload` 枚举未实现 |
| 6 | Daemon 边界整理 | ✅ **保持独立** | Daemon 有自含 `routes.rs`/`sse.rs`/`state.rs`，不引用 web `WebState` |
| 7 | 收敛文档和生成物 | ❌ 未开始 | |

### 1. `ApiDispatcher` 核心 gap：存在但未接入

`api_dispatcher.rs` 实现了计划定义的 `ApiRequestContext`、`ApiTransportKind`、`dispatch()` 和 `MessageProcessor` trait。`DirectTransport` 可以正常工作（有单元测试）。

**但 REST 路由完全不使用 `ApiDispatcher`。** `handler_registry.rs` 中的所有 `*_handlers()` 函数绑定 Axum adapter 的方式保持不变：

```rust
// handler_registry.rs 第 159 行 —— 不走 dispatcher
get(processor_no_params_handler::<handlers::SessionListProcessor>),
```

结果：目前 `ApiDispatcher` 和 `DirectTransport` 是"悬空的"——没有实际 transport 入口使用它们。

两条 `dispatch_processor` 调用路径对比：

```
计划目标：
  Axum handler → 构造 ClientRequest → ApiDispatcher::dispatch → dispatch_processor → domain processor

当前 REST 实际路径：
  handler_registry::processor_no_params_handler → process_processor → dispatch_processor → domain processor
  （绕过 ApiDispatcher，无 experimental gate、无统一错误转换）

当前 DirectTransport 路径（仅测试用）：
  DirectTransport → ApiDispatcher::dispatch → dispatch_processor → domain processor
```

**影响**：若现在新增 JSON-RPC WebSocket（Phase 3），同一 domain 会有两套分发逻辑，违背计划"相同 API 语义只走一条请求分发路径"的根本目标。

#### 2. `dispatch()` 覆盖 endpoint 过少

`api_dispatcher.rs:111-149` 的 `match request` 只处理：

| ClientRequest 变体 | 状态 |
|---|---|
| `Capabilities` | ✅ 已实现 |
| `SessionList` | ✅ 已实现 |
| `SessionDetail` | ✅ 已实现 |
| `SessionResume` | ✅ 已实现 |
| `SessionArchive` | ✅ 已实现 |
| 其余 60+ 变体 | ❌ 返回 `ApiError::NotImplemented` |

协议定义的 `ALL_ENDPOINTS` 有 60+ 个 entry，而 dispatcher 只覆盖了其中 5 个。即使 Phase 2 启动，也只能迁移这 5 个 已 processor 化的 endpoint。

**缺少 migration tracker**。`api_dispatcher.rs` 中没有注释说明哪些 endpoint 已就绪可以迁移、哪些已排除（PTY/MCP/browser/daemon）、哪些待 processor 化，这会产生架构混乱。

#### 3. `dispatch_processor()` 和 `process_processor()` 的关系重构了但不对称

`processors.rs` 已正确地做了重构：

```
旧: process_processor() 包含序列化和分发
新: process_processor() = dispatch_processor() + JSON 序列化
    dispatch_processor() = 纯业务分发（serialization scope + handle）
```

`ApiDispatcher::dispatch()` 也调用 `dispatch_processor()`。这个抽象方向正确。但：

- `dispatch_processor()` 是 `pub` 的（非 `pub(crate)`），理论上可从 crate 外部绕过 `ApiDispatcher` 直接调用。
- REST 路径仍然通过 `process_processor()` 调用 `dispatch_processor()`，同一业务逻辑有两层 wrapper，增加了调用栈复杂度。

#### 4. `SerializationLayer` 的实际实现与计划差距

| 计划 Phase 4 要求 | 实际实现 | 差距 |
|---|---|---|
| Typed queue key 枚举 | 不存在，使用 `scope + String` | ❌ |
| `Exclusive` / `SharedRead` 访问模式 | 不存在，全部 `Semaphore(1)` | ❌ |
| FIFO queue（同 key 有序） | Semaphore 不保证 FIFO | ❌ |
| 不同 key 并发 | `Semaphore(1)` 按 key 独立 => 对 | ✅ |
| 删除 `SessionOwnership` | 仍然存在且被多处使用 | ❌ |

`SerializationScope` 枚举已在协议层定义（`Concurrent`、`PerProcess`、`PerConnection`、`PerKey`），macro 也生成了 `ClientRequest::serialization_scope()`。但 `SerializationLayer::run_scoped()` 未利用 scope 的语义差异——全部映射到相同的 `Semaphore(1)`。

当前 `serialization.rs` 的调用流程：

```
run_scoped(scope, key, f):
  queue_key = queue_key(scope, fallback_key)  // scope 仅决定 key namespace，不决定并发语义
  semaphore = queues.entry(queue_key).or_insert(Semaphore::new(1))
  semaphore.acquire().await
  f().await
  // 所有 scope 都拿到 Semaphore(1) = 互斥
```

#### 5. `SessionOwnership` 仍被多处使用且构成死锁依赖

`try_claim()` / `release_owner()` / `SessionOwner` 的使用链：

| 文件 | 用法 | 说明 |
|---|---|---|
| `handlers/chat.rs:116` | `state.try_claim_chat(id)` | streaming 开始前 claim |
| `handlers/chat.rs:172,178,197` | `state.release_owner(ChatStream)` | streaming 结束释放 |
| `ws/ipc.rs:125` | `state.try_claim(IpcWs, ...)` | IPC WS 连接开始 claim |
| `ws/ipc.rs:316,484` | `state.release_owner(IpcWs)` | IPC WS 断开释放 |
| `handlers/sessions.rs:771` | `owner.owner == SessionOwner::None` | 变异操作前的 ownership check |
| `handlers/sessions.rs:904` | `ownership_conflict_response` | REST 端点的 guard |
| `handlers/admin.rs:1148` | `state.release_owner(ChatStream)` | admin 路径释放 |
| `state.rs:62-80` | wrapper 方法 | WebState 暴露 API |

**死锁依赖**：计划 Phase 5 section 8-9 说移除 `try_claim` 需要 Phase 4 typed serialization queue 完成。Phase 4 尚未开始且依赖 Phase 1-2。当前 `SessionOwnership` 阻塞着所有 streaming 路径的迁移——不仅是 IPC v2，也包括 REST chat handler。

建议的解依赖顺序：

```
Phase 4a: SerializationScope + AccessMode（添加 SharedRead/Exclusive 到协议层）
Phase 4b: SerializationLayer → RwLock queue（支持 shared-read 并发）
Phase 4c: 迁移 try_claim/release_owner → PerConnection queue key
Phase 1-2: ApiDispatcher 全面接入
Phase 5: IPC v2（依赖 Phase 4c 完成）
```

#### 6. `IpcEnvelope<T>` 已存在但 `IpcPayload` 枚举未实现

`crates/allthecodes-ipc-protocol/src/envelope.rs` 已实现通用版本化 envelope：

```rust
pub struct IpcEnvelope<T> {
    pub version: u16,
    pub id: String,
    pub seq: u64,
    pub timestamp: i64,
    pub payload: T,
}
```

但计划 Phase 5 要求的 `IpcPayload` 枚举未实现：

| 计划定义的 variant | 存在？ |
|---|---|
| `IpcPayload::Hello` | ❌ |
| `IpcPayload::Ready` | ❌ |
| `IpcPayload::ClientRequest` | ❌ |
| `IpcPayload::ClientResponse` | ❌ |
| `IpcPayload::ServerNotification` | ❌ |
| `IpcPayload::ServerRequest` | ❌ |
| `IpcPayload::ServerError` | ❌ |
| `IpcPayload::Lagged` | ❌ |

目前 `IpcEnvelope<T>` 仅作为通用容器（在 `normalized.rs` 中用于包裹 `ConversationEvent`），没有约束 payload 类型集合。IPC v2 的 wire contract 尚未定型。

#### 7. 常见问题一致的部分

| 计划要求 | 实际 | 状态 |
|---|---|---|
| Legacy IPC WS 保持 wire format | `ws/ipc.rs` 未使用 `JsonRpcFrame` 或 `ApiDispatcher` | ✅ |
| Daemon 不并入 web dispatcher | `DaemonState` 独立，`routes.rs` 不引用 `WebState` | ✅ |
| PTY/MCP/browser 不在统一范围 | `ws/terminal.rs`、`allthecodes-mcp`、`allthecodes-browser` 各自独立 | ✅ |
| `allthecodes-protocol` 中的 `JsonRpcFrame` 存在 | `transport.rs` 定义 + roundtrip 测试 | ✅ |
| `DirectTransport` 通过 `MessageProcessor` 分发 | 测试已验证 | ✅ |

#### 8. 缺少 transport inventory（Phase 0 未完成）

计划 Phase 0 要求"增加一份 transport inventory 测试或文档片段，列出当前公开入口"。当前没有一处文档列出所有公开 transport 入口及其 dispatcher 绑定状态。

建议的 inventory 格式（可放在 `api_dispatcher.rs` 或独立测试中）：

```
Transport entry              | Dispatcher? | Status
-----------------------------|-------------|--------
/api/* REST                  | No          | Phase 2 目标
/api/v2/* mirror             | No          | Phase 2 目标
/api/terminal/*              | Excluded    | PTY 专用
/api/tui/ws                  | Excluded    | Legacy TUI
/api/ipc/ws                  | IpcBridge   | Phase 5 目标（保留 legacy wire）
/api/ws (future)             | Planned     | Phase 3 — JSON-RPC WS
daemon /api/*                | Excluded    | Phase 6 边界
daemon /events               | Excluded    | SSE 独立
IPC JSONL stdio              | IpcBridge   | Phase 5 目标
```

### 建议的下一阶段工作

| 优先级 | 步骤 | 影响范围 | 风险 |
|---|---|---|---|
| **P0** | 添加 migration tracker 到 `api_dispatcher.rs`（列出每个 `ClientRequest` 的迁移状态） | 仅文档 | 无风险，解决架构可视性 |
| **P0** | 将已 processor 化的 5 个 endpoint 的 REST 路径改为走 `ApiDispatcher` | `handler_registry.rs`、`api_dispatcher.rs` | 低——已有 processor 和测试 |
| **P1** | Phase 4a：为 `SerializationScope` / macro 添加 `AccessMode`（Exclusive/SharedRead） | `allthecodes-protocol` | 低——协议层添加枚举 |
| **P1** | Phase 4b：扩展 `SerializationLayer` 支持 RwLock queue | `serialization.rs` | 中——并发正确性需要测试覆盖 |
| **P2** | Phase 2：将更多 endpoint 迁移到 dispatcher（files、skills 等 processor 化完成的后做） | 跨多个 domain handler | 低——逐个迁移 |
| **P2** | Phase 4c：迁移 `try_claim`/`release_owner` → PerConnection queue key | `chat.rs`、`ipc.rs`、`sessions.rs`、`admin.rs` | **高**——streaming 生命周期最脆弱 |
| **P3** | Phase 3：新增 JSON-RPC WebSocket（前提：Phase 1-2 完成） | `ws/api.rs` | 中 |
| **P3** | Phase 5：实现 IPC v2（前提：Phase 4c 完成） | `ws/ipc_v2.rs` + `IpcPayload` 枚举 | 高 |
