# Phase 2：TransportEvent 与连接抽象执行记录

> 执行日期：2026-06-16

## 已落地

- `allthecodes-server` 新增内部 transport primitives：
  - `TransportKind::{HeadlessStdio, IpcWebSocket, ApiRpcWebSocket}`
  - `ConnectionId`
  - `ConnectionOrigin`
  - `ConnectionClosedReason`
  - `TransportEvent<T>`
  - `OutboundEnvelope<T>`
- payload 保持泛型，`allthecodes-server` 不依赖 IPC 或 JSON-RPC 协议 crate。
- WebSocket Origin 校验已加入 `/api/ipc/ws` 与 `/api/rpc/ws`：
  - 缺失 `Origin` 视为本地/native client，允许。
  - `http`/`https` loopback origin 允许，包括 `localhost`、`127.0.0.1`、`[::1]`。
  - 非 loopback browser origin 返回 `403 Forbidden`。
- `/api/ipc/ws` 内部接入 `TransportEvent<FrontendMessage>`：
  - text frame 解析后包装为 `IncomingMessage`。
  - 现有 `handle_frontend_message` 分发路径保持不变。
  - outbound 仍发送原有 `BackendMessage` JSON text frame，内部仅用 `OutboundEnvelope<BackendMessage>` 标注目标连接。
  - open/close event 仅用于日志与 cleanup 前状态表达。
- `/api/rpc/ws` 内部接入连接生命周期事件：
  - 每个连接生成 `ConnectionId` 与 `ConnectionOrigin`。
  - JSON-RPC frame dispatch 仍走现有 `handle_api_rpc_text` / `ApiDispatcher`。
  - invalid frame 和 error response 行为保持不变。
- headless JSONL 内部接入 `TransportEvent<FrontendMessage>`：
  - 单一 stdio connection 使用 `TransportKind::HeadlessStdio`。
  - stdin line parse 后包装为 `IncomingMessage`。
  - stdin EOF 映射为 `ConnectionClosedReason::StdinEof`，随后继续走原有 shutdown cleanup。

## 未实现，留给后续阶段

- outbound router。
- bounded writer fanout 与慢客户端隔离。
- event log。
- replay / catch-up。
- Unix socket transport。
- 外部 wire shape、REST route、JSONL 协议、JSON-RPC method 均未在本阶段调整。

## 验证目标

- `cargo test -p allthecodes-server`
- `cargo test -p allthecodes-ipc-transport`
- `cargo test -p allthecodes-web`
- `cargo test -p allthecodes-ipc`
- `cargo check -p allthecodes --bin allthecodes`
- `cargo build --workspace`
- `cargo clippy --workspace --lib --bins`
