# Codex Port API 对齐 Phase 执行计划

> 创建日期：2026-06-16
> 来源文档：`development/docs/codex-portapi-comparison-analysis.md`
> 目标：把 Codex 架构调研中的可迁移模式拆成可执行、可验证、可逐步提交的 allthecodes 全量构建路线图。
> 原则：保持 allthecodes 现有 REST API、Rust TUI、daemon/gateway 能力不回退；每个 Phase 独立可验收，避免跨 Phase 大爆炸重构。

---

## 总体路线

| Phase | 主题 | 目的 | 依赖 |
|---|---|---|---|
| Phase 0 | 基线与边界修正 | 固定当前事实、补齐文档/测试基线 | 无 |
| Phase 1 | HTTP server lifecycle 硬化 | 完成已开始的传输抽象第一阶段 | Phase 0 |
| Phase 2 | TransportEvent 与连接抽象 | 为 WebSocket/stdio/Unix socket 统一打底 | Phase 1 |
| Phase 3 | Outbound 两循环与事件日志 | 防慢客户端阻塞，并支持 replay/catch-up | Phase 2 |
| Phase 4 | Exec/Agent 输出与会话恢复 | 引入 seq、bounded retention、resume TTL | Phase 3 |
| Phase 5 | Health/Readiness 与 daemon 操作锁 | 让启动、探活、停止行为可诊断可恢复 | Phase 1 |
| Phase 6 | Provider/streaming API 统一 | 统一 provider 配置、流协议和错误分类 | Phase 0 |
| Phase 7 | Layered config 与 requirements | 引入配置层、禁用原因、profile/requirements | Phase 0 |
| Phase 8 | PID/process lifecycle 管理 | 长运行 agent/daemon 进程可靠管理 | Phase 5 |

Phase 1 当前已有实现基础；后续 Phase 应按顺序推进，Phase 6/7 可与 Phase 2-5 并行，但每次提交必须保持 workspace build 可过。

---

## Phase 0：基线与边界修正

### 目标

建立准确的实现状态和测试基线，避免后续根据过期文档做错决策。

### 任务

1. 修正 `development/docs/api-interface-analysis.md` 中已过期的端口描述，统一为当前 CLI/Phase1 事实：
   - Web 默认 `17322`
   - Daemon 默认 `19836`
   - 如果某文档段落描述的是历史/参考端口，必须显式标为历史。
2. 给 `development/docs/phase1-transport-abstraction-plan.md` 增加“实现状态”小节：
   - 已完成：`allthecodes-server` crate、`--listen`、`ServerMode::All`、daemon `build_router()`、graceful shutdown、integration tests、architecture doc。
   - 待验收：手动启动、SSE/Gateway、static assets、workspace build/clippy。
3. 将 `/api/resize` 标为明确后续任务：
   - 当前 daemon route 是 `noop` stub。
   - 需要后续决定它应转发到 TUI、terminal session，还是仅作为 daemon client metadata。
   - Phase 0 不实现 `/api/resize`，只登记其当前状态和后续决策点。
4. 建立最小回归命令清单：
   - `cargo test -p allthecodes-server`
   - `cargo check -p allthecodes --bin allthecodes`
   - Phase 相关扩展测试在各 Phase 追加。

### `/api/resize` 后续决策

待定方向：

- 转发到 Rust TUI，用于驱动 TUI viewport/terminal resize。
- 转发到 terminal session，用于 xterm/PTY session resize。
- 仅保存为 daemon client metadata，用于状态展示或后续连接恢复。

Phase 0 不决定具体 wire shape，也不改变 daemon handler 行为。

### 执行记录

| 命令/核对项 | 结果 |
|---|---|
| `cargo test -p allthecodes-server` | 通过：24 个 unit tests、6 个 integration tests、0 个 doctests |
| `cargo check -p allthecodes --bin allthecodes` | 通过 |
| `crates/allthecodes/src/cli.rs` defaults | 已核对：Web `17322`，Daemon `19836` |
| `crates/allthecodes-daemon/src/routes.rs` `/api/resize` | 已核对：当前返回 `status: noop`，未转发 resize 事件 |

### 验收标准

- 文档端口与当前代码一致。
- Phase1 文档不再把已实现项和待验收项混在一起。
- `/api/resize` 的状态明确为未实现，不再只藏在 API 表格中。
- 两条最小回归命令通过，或失败原因记录到本 Phase 执行记录。

---

## Phase 1：HTTP Server Lifecycle 硬化

### 目标

完成 `allthecodes-server` 第一阶段收尾，让 Web/Daemon/All 模式的 lifecycle 达到可发布质量。

### 任务

1. 补齐 `ListenUrl::All` 校验：
   - `web_addr == daemon_addr` 时返回解析失败或 mode 构造失败。
   - 增加单元测试覆盖相同 host/port。
2. 完成手动验证清单：
   - `--web`
   - `--daemon`
   - `--listen web://...`
   - `--listen daemon://...`
   - `--listen all://...`
   - `--listen off`
   - Ctrl-C / SIGTERM shutdown。
3. 验证 Web static assets：
   - backend-only npm 构建仍返回 unbundled/static fallback。
   - `web-ui` feature 构建仍能服务嵌入资源。
4. 验证 daemon routes：
   - `/health`
   - `/events`
   - `/remote-control/v1/capabilities`
   - 控制令牌认证仍生效。
5. 运行 workspace 级验证：
   - `cargo build --workspace`
   - `cargo clippy --workspace --lib --bins`，修复新增 warning。

### 验收标准

- Phase1 手动清单全部完成并记录结果。
- 相同端口的 All mode 不会进入部分启动状态。
- Web-only、Daemon-only、All mode 都能正常启动和 shutdown。
- 无新增 clippy/build warning。

### 执行记录

| 命令/核对项 | 结果 |
|---|---|
| `cargo test -p allthecodes-server` | 通过：29 个 unit tests、8 个 integration tests、0 个 doctests |
| `cargo test -p allthecodes-gateway` | 通过：29 个 unit tests、0 个 doctests |
| `cargo check -p allthecodes --bin allthecodes` | 通过 |
| `cargo build --workspace` | 通过 |
| `cargo clippy --workspace --lib --bins` | 命令通过；仍有既有 workspace warnings，未见 touched crates 新增 warning |
| `--web --web-port 17331 --no-open` | 通过；`17322` 被已有 release 进程占用，改用 `17331` |
| `FEATURE_KAIROS=1 --daemon --port 19836` | 通过；`/health` 200，SSE `/events?client_id=phase1` 返回 `text/event-stream` |
| `--listen web://127.0.0.1:17332` | 通过；Web health 200 |
| `--listen daemon://127.0.0.1:19837` | 通过；Daemon health 200 |
| `--listen all://web=127.0.0.1:17333,daemon=127.0.0.1:19838` | 通过；两个 health endpoint 均 200 |
| `--listen off --headless` | 通过；进入非 server path |
| invalid `--listen` + legacy fallback flags | 通过；解析失败直接退出 |
| duplicate All addresses | 通过；显式 `all://` 与 legacy fallback 同端口均失败 |
| Gateway capabilities auth | 通过；无 token 401，`x-allthecodes-daemon-token` 200 |
| Ctrl-C / SIGTERM | 通过；端口释放，SIGTERM 后 daemon state 为 `stopped` |
| backend-only static fallback | 通过；默认构建 `/` 返回 unbundled/static fallback |
| `web-ui` feature static assets | `cargo build -p allthecodes --bin allthecodes --features web-ui` 通过；运行时 embedded HTML 验证受阻：feature-built debug binary 未在 15s 内 bind 验证端口 |

---

## Phase 2：TransportEvent 与连接抽象

### 目标

引入 Codex 风格的传输无关事件层，但不破坏现有 REST API。第一步只统一双向消息通道，不移动 HTTP REST handlers。

### 设计边界

- REST API 继续归 `allthecodes-web` / `allthecodes-daemon` 所有。
- 新抽象只覆盖双向连接类通道：
  - stdio/headless IPC
  - `/api/ipc/ws`
  - `/api/rpc/ws`
  - 后续 Unix socket/WebSocket-over-UDS。

### 任务

1. 新增或扩展 transport crate，定义：
   - `TransportKind`
   - `ConnectionId`
   - `ConnectionOrigin`
   - `TransportEvent::{ConnectionOpened, ConnectionClosed, IncomingMessage}`
   - `OutboundEnvelope`
2. 为现有 IPC WebSocket 添加 adapter：
   - WebSocket 读循环只负责转换 inbound frame 为 `TransportEvent`。
   - 写循环通过 per-connection writer 接收 outbound envelope。
3. 为 headless stdio 添加 adapter：
   - JSONL 读写保持现有 wire shape。
   - 只在内部转换为统一事件。
4. 保留 `/api/rpc/ws` 当前协议，先接入连接生命周期事件，不重写全部 method dispatch。
5. 增加 origin 安全边界：
   - 浏览器 WebSocket 检查 `Origin`。
   - loopback/local client 明确允许。

### 验收标准

- 现有 Web IPC、RPC WS、headless JSONL 行为不变。
- 新增 transport adapter 单元测试和至少一个 WebSocket integration test。
- 慢客户端/断连不会 panic，不会丢失其他连接状态。

### 执行记录

| 命令/核对项 | 结果 |
|---|---|
| `cargo test -p allthecodes-server` — transport tests | 通过：17 个 transport 单元测试、22 个 server 测试、8 个 integration tests、0 个 doctests |
| `cargo test -p allthecodes-ipc-transport` | 编译通过 |
| `cargo test -p allthecodes-web` — WS transport tests | 通过：11 个 IPC WS transport tests（origin 校验、frame 转换、close event）、8 个 API RPC tests（JSON-RPC dispatch、transport event 包装） |
| `cargo test -p allthecodes-ipc` | 通过：17 个 runtime/headless 测试 |
| `cargo check -p allthecodes --bin allthecodes` | 通过 |
| `cargo build --workspace` | 通过 |
| `cargo clippy --workspace --lib --bins` | 通过，无新增 error |
| WebSocket Origin 校验 — 缺失 Origin → `LocalNative` | 通过，`transport.rs:82-84` |
| WebSocket Origin 校验 — loopback（localhost/127.0.0.1/[::1]）→ 允许 | 通过，`transport.rs:272-297` |
| WebSocket Origin 校验 — 非 loopback → 403 | 通过，`ipc.rs:83-86`、`api_rpc.rs:28-31` |
| `/api/ipc/ws` — text frame → `TransportEvent::IncomingMessage<FrontendMessage>` | 通过，`ipc.rs:208-215, 286-295` |
| `/api/ipc/ws` — outbound 仍用 `BackendMessage` JSON text frame；`OutboundEnvelope` 仅内部标注目标 | 通过，`ipc.rs:184-193` |
| `/api/ipc/ws` — `handle_frontend_message` 分发路径未变 | 通过，`ipc.rs:217-223` |
| `/api/ipc/ws` — open/close event 仅用于日志与 cleanup 前状态表达 | 通过，`ipc.rs:135-139, 248-251` |
| `/api/rpc/ws` — 每连接生成 `ConnectionId` 与 `ConnectionOrigin` | 通过，`api_rpc.rs:33-34, 26-32` |
| `/api/rpc/ws` — JSON-RPC dispatch 仍走 `handle_api_rpc_text` / `ApiDispatcher` | 通过，`api_rpc.rs:129-131, 191-219` |
| `/api/rpc/ws` — invalid frame 和 error response 行为不变 | 通过，`api_rpc.rs:211-218` |
| headless JSONL — 单一 stdio connection 使用 `TransportKind::HeadlessStdio` | 通过，`headless.rs:83-87` |
| headless JSONL — stdin line parse → `IncomingMessage` | 通过，`headless.rs:126-132` |
| headless JSONL — stdin EOF → `ConnectionClosedReason::StdinEof` | 通过，`headless.rs:117-119` |
| headless JSONL — EOF 后继续走原有 shutdown cleanup | 通过，`headless.rs:233-244` |

### 补充记录

1. **路由注册不统一**：`/api/ipc/ws` 通过 `handler_registry.rs:576` 的 `ApiMethod::IpcWs` 注册，`/api/rpc/ws` 通过 `mod.rs:29` 直接 `.route()` 注册。两者均可达且功能正确，但风格不一致。建议 Phase 3 统一为 handler_registry 模式。

2. **`OutboundEnvelope` 当前为过渡状态**：IPC WS（`ipc.rs:188`）仅序列化了 `envelope.message`，`connection_id` 未被实际使用。这是预期状态，等待 Phase 3 outbound router 启用。

3. **`TransportEvent::IncomingMessage` 在 IPC WS 中被解构丢弃了 `connection_id` 和 `kind`**（`ipc.rs:209`）。单连接场景下合理，但 Phase 3 实现 outbound router 后应考虑携带这些信息。

4. **`handler_registry` 新增传输隔离测试**：`non_web_transports_are_not_registered_as_web_api_routes`、`dedicated_transports_are_marked_as_websocket_routes`、`transport_inventory_keeps_public_web_entries_visible` 强化了传输层与 Web API 的路由隔离，可在 Phase 3 标准化后补充到文档。

### 未实现项（留给后续阶段）

- Unix socket transport 仍未纳入本轮。
- Phase 4b 继续统一 agent/worker 输出恢复能力。
- 外部 wire shape、REST route、JSONL 协议、JSON-RPC method 均未调整

---

## Phase 3：Outbound 两循环与事件日志

### 目标

拆分 inbound processing 和 outbound writing，避免单个慢客户端阻塞消息处理，并为后续 replay/catch-up 打底。

### 任务

1. 建立 outbound router：
   - 管理 `connection_id -> writer channel`。
   - 支持 `Opened`、`Closed`、`DisconnectAll` 控制事件。
2. 每个连接使用 bounded outbound channel：
   - 默认容量先使用保守值，例如 1024。
   - 满队列时记录 lag/drop 事件，不阻塞 processor 主循环。
3. 引入 `EventLog` 基础类型：
   - bounded memory buffer。
   - monotonic `seq`。
   - replay by `after_seq`。
   - live broadcast。
4. IPC/SSE 消息桥接到 `EventLog`：
   - 新连接先 replay，再进入 live stream。
   - lagged 时发送明确 lag notification。
5. 给 daemon SSE `/events` 复用同一 replay 语义，保留 `last_event_id` 兼容。

### 验收标准

- 一个慢 WebSocket client 不影响另一个 client 收到 stream events。
- 新 client 能用 `after_seq` 或兼容参数补齐最近事件。
- bounded buffer 溢出时有可观察 lag 信号。

### 执行记录

Phase 3 已闭环到代码与测试：

- `allthecodes-server::EventLog` 提供 bounded memory buffer、monotonic `seq`、`replay_after(after_seq)` 与 compacted replay 状态。
- `allthecodes-server::OutboundRouter` 使用 bounded per-connection writer channel；满队列连接会被标记为 lagging 并移出 fanout，不阻塞其他连接。
- IPC WebSocket hub 接入 `EventLog`/`OutboundRouter`，支持 `after_seq` replay、lag notification 和慢客户端隔离。
- daemon SSE `/events` 支持 `after_seq`，并保留 `last_event_id` 兼容；`after_seq` 优先级高于 `last_event_id`。
- compacted replay 会先发送 `lagged` 事件，携带 `requested_after_seq`、`oldest_seq`、`latest_seq`、`skipped`，且不推进 SSE `Last-Event-ID`。

新增/确认测试：

- `crates/allthecodes-server/src/event_log.rs`：sequence、replay、bounded compaction、zero capacity。
- `crates/allthecodes-server/src/outbound_router.rs`：bounded fanout、慢连接隔离、unregister/disconnect。
- `crates/allthecodes-web/src/ipc_streams.rs`：IPC hub fanout、lagged disconnect、single owner。
- `crates/allthecodes-daemon/src/sse.rs`：`after_seq`/`last_event_id` 解析、buffered replay、compacted lag notification。

---

## Phase 4：Exec/Agent 输出与会话恢复

### 目标

把 Codex exec-server 的输出可靠性能力迁移到 allthecodes agent/terminal/worker 输出路径。

### 任务

1. 为 agent/worker/terminal 输出定义统一 `OutputEvent`：
   - `seq`
   - `stream` (`stdout`/`stderr`/`pty`/`system`)
   - `chunk`
   - `timestamp`
   - `process_or_run_id`
2. 增加 bounded output retention：
   - 默认每 run/process 1 MiB。
   - 超限丢弃旧 chunk，并更新 first retained seq。
3. 支持增量读取：
   - `read(after_seq, limit_bytes)`。
   - 返回 `next_seq`、`truncated`、`first_available_seq`。
4. 引入 session detach/resume TTL：
   - 断连后保留 10-30 秒。
   - TTL 内重连继续接收输出和状态。
5. 明确 lifecycle states：
   - `Starting`
   - `Running`
   - `Exited`
   - `Expired`
   - `Failed`
6. 保留 exited output：
   - 退出后至少保留 30 秒，防止 late output 丢失。

### 验收标准

- Terminal/PTY 输出可断线重连后补齐。
- Terminal/PTY 大输出不会无限占用内存。
- Terminal/PTY late output after exit 有测试覆盖。
- 前端可用 `seq` 做 terminal 输出幂等读取。

### 执行记录

Phase 4 本轮闭环 terminal/PTY 输出恢复，并为 task/agent output 工具路径补上兼容的增量输出事件；daemon gateway/worker run-event wire shape 仍拆为 Phase 4b：

- 新增共享 `OutputEvent`、`OutputRetention`、`OutputReadBatch`、`OutputLifecycleState`，字段覆盖 `seq`、`stream`、`chunk`、`timestamp_ms`、`process_or_run_id`。
- Terminal session 输出从单一字符串 buffer 改为 bounded output retention，默认 1 MiB，保留 `first_available_seq` 与 `latest_seq`。
- 新增 `GET /api/terminal/sessions/{id}/output?after_seq&limit_bytes`，返回 `events`、`next_seq`、`truncated`、`first_available_seq`、`status`。
- Terminal WebSocket 支持 `after_seq` replay；`ready` frame 增加 `first_available_seq`、`latest_seq`、`next_seq`；`output` frame 保留旧 `data` 字段并追加 `seq`、`stream`、`timestamp_ms`、`process_or_run_id`。
- Terminal WebSocket broadcast lag 后发送 `lagged` frame 并用 retention replay 补齐可用输出。
- detach/resume TTL 使用 `DEFAULT_DETACH_RESUME_TTL`；exited output 使用 `DEFAULT_EXITED_OUTPUT_RETENTION_TTL` 防止 late output 丢失。
- `TaskStore::append_output` 继续维护旧 `.output.log` 和 `output` 字段，同时写入 bounded `.output.events.ndjson`，事件使用共享 `OutputEvent`、monotonic `seq`、`stdout` stream、`process_or_run_id=task_id`。
- `TaskStore::read_output_events(after_seq, limit_bytes)` 支持 task/agent output 增量读取，返回共享 `OutputReadBatch`，并把 `TaskStatus` 映射到 `OutputLifecycleState`。
- `TaskOutput` 工具新增可选 `after_seq`/`limit_bytes`；未传游标时保持旧 payload，传入后追加 `output_events`、`output_next_seq`、`output_first_available_seq`、`output_truncated_by_limit`、`output_state`。

新增/确认测试：

- `crates/allthecodes-server/src/output.rs`：增量读取、bounded retention、byte limit、late output after exit、zero retention、future cursor `next_seq`。
- `crates/allthecodes-web/src/ws/terminal.rs`：terminal output WS frame 保留 `data` 并序列化 seq 元数据；REST output response 序列化 cursor 字段。
- `crates/allthecodes-tasks/src/store.rs`：task output event incremental replay、legacy output seed、bounded event retention。
- `crates/allthecodes-tools/src/semantic_tool_tests.rs`：`TaskOutput` 旧 payload 兼容和 `after_seq` event cursor 字段。

### Phase 4b 保留缺口

- daemon gateway run events、worker NDJSON、agent supervisor 输出合约尚未统一为 `OutputEvent` wire shape。
- 本轮不修改 gateway/worker 外部 wire shape，避免扩大到 remote-control run protocol migration。

---

## Phase 5：Health/Readiness 与 Daemon 操作锁

### 目标

让本地 server、daemon、gateway 的启动和探活行为可诊断、可轮询、可避免并发操作冲突。

### 任务

1. 统一健康端点约定：
   - `/healthz`：进程存活。
   - `/readyz`：HTTP server 可接收请求。
   - `/startupz`：可选，表示依赖初始化中。
   - 保留 daemon 现有 `/health` 兼容。
2. 增加 readiness polling helper：
   - 50ms interval。
   - 默认 10s timeout。
   - 返回最后一次错误用于诊断。
3. 为 daemon management 命令增加 operation lock：
   - start/stop/restart/status 写操作串行。
   - lock timeout 必须有明确错误。
4. 增加 stale state cleanup：
   - stale pid/state/socket 文件可检测。
   - 清理前校验进程是否仍存活。
5. 将 readiness 结果暴露到 Web/Gateway diagnostics。

### 验收标准

- 并发 start/restart 不会产生两个 daemon/server。
- readiness polling 不依赖固定 sleep。
- stale state 有可读诊断。

### 执行记录

Phase 5 已闭环到代码与测试：

- Web 和 daemon 均提供根级 `/healthz`、`/readyz`、`/startupz`；daemon 保留 `/health` 兼容响应。
- `allthecodes-daemon::readiness` 提供 `ready_url`、`probe_ready`、`wait_for_ready`，默认 50ms poll interval、10s timeout，并在错误中保留最后一次探测错误。
- daemon `start` 在 spawn 后轮询 `/readyz`，输出 `ready=`、`attempts=`、`elapsed_ms=`、`log=` 诊断。
- daemon management 的 `start`、`status`、`stop`、`restart`、`sleep`、`wake` 进入 `operation_lock::with_operation_lock` 串行化。
- stale supervisor/worker state cleanup 会移除 dead pid 状态并打印 retained/removed 诊断。
- Web gateway status diagnostics 暴露 `ready_url` 和 readiness 探测结果。

新增/确认测试：

- `crates/allthecodes-web/src/mod.rs`、`crates/allthecodes-daemon/src/server.rs`：root probe endpoint shape。
- `crates/allthecodes-daemon/src/readiness.rs`：200/non-200 probe、timeout last_error。
- `crates/allthecodes-daemon/src/operation_lock.rs`：并发互斥、timeout holder 信息、dead pid stale lock cleanup。
- `crates/allthecodes-daemon/src/process_state.rs`：stale supervisor/worker cleanup。
- `crates/allthecodes-web/src/handlers/gateways.rs`：gateway status readiness diagnostics。

---

## Phase 6：Provider 与 Streaming API 统一

### 目标

降低 API provider、retry、stream parser、错误分类的分散度，为多 provider 行为一致性打底。

### 任务

1. 设计统一 `ProviderEndpoint`/`ProviderRuntime`：
   - base URL
   - headers
   - query params
   - auth
   - retry policy
   - timeout / idle timeout
2. 统一 SSE 和 WebSocket streaming 输出：
   - 都产出同一 `ResponseEvent`。
   - consumer 不关心底层 stream transport。
3. 引入语义化错误分类：
   - `ContextWindowExceeded`
   - `QuotaExceeded`
   - `RateLimited`
   - `PolicyBlocked`
   - `ServerOverloaded`
   - `RetryableTransport`
   - `UnknownProviderError`
4. 支持 turn state / request correlation passthrough：
   - 捕获 provider response headers。
   - 记录到 telemetry/session event。
5. 增加 WebSocket probe：
   - 不发送模型请求，仅验证连接、认证和基础 metadata。
   - 提供给 diagnostics/settings UI。

### 验收标准

- Claude/OpenAI/Gemini/OpenAI-compatible 至少一个路径接入新 provider runtime。
- 旧 API client 行为保持兼容。
- 错误分类有 fixture tests。
- diagnostics 能区分认证失败、网络失败、quota/rate limit。

### 执行记录（2026-06-16）

已完成：

- 新增 `allthecodes-api::api::provider_runtime`，集中 `ProviderEndpoint`、transport、response metadata、typed provider error、HTTP/WS probe report。
- Anthropic/Claude、OpenAI-compatible、Google/Gemini streaming 路径接入 runtime helper；外部 `ApiClient::messages_stream` 仍返回既有 `StreamEvent`，保持调用方兼容。
- 新增语义错误分类：`ContextWindowExceeded`、`QuotaExceeded`、`RateLimited`、`PolicyBlocked`、`ServerOverloaded`、`RetryableTransport`、`AuthenticationFailed`、`InvalidRequest`、`UnknownProviderError`；retry 兼容层复用新分类。
- provider response metadata 捕获 request id、retry-after、x-ratelimit 等安全 headers，并在 stream 建立时写 tracing diagnostic。
- 新增 `POST /api/providers/{id}/probe` 协议和 web handler；HTTP probe 做无模型请求的可达性诊断，WebSocket probe 对支持 WS probe path 的 provider 做握手后立即关闭，不支持时返回 `unsupported` diagnostic。

保留边界：

- `StreamProvider` trait 和 `StreamEvent` public consumer contract 保持不变；完整 `ResponseEvent` 对外迁移留给后续兼容窗口。
- Bedrock/Vertex 未做全量 runtime 迁移，仅继续走现有 stream provider；后续迁移时应复用 `ProviderEndpoint`/`ProviderErrorKind`。
- 默认测试不使用真实 provider credential，不发送模型请求。

---

## Phase 7：Layered Config 与 Requirements

### 目标

从 flat settings 过渡到可诊断的配置层，支持系统级、用户级、项目级、CLI、runtime override 和 admin requirements。

### 任务

1. 定义配置层顺序：
   - system defaults
   - user global
   - user profile
   - project
   - CLI override
   - runtime/session override
2. 新增 `ConfigLayerEntry`：
   - source
   - path
   - raw value
   - effective value
   - disabled reason。
3. 项目配置 trust gate：
   - 未信任目录中的 project config 不生效。
   - disabled layer 仍在 diagnostics 中展示。
4. 引入 requirements：
   - 与普通 config 分离。
   - 只表达约束，不表达偏好。
   - 支持 allowed/denied values。
5. 实现 TOML/JSON recursive merge：
   - table 递归合并。
   - scalar 覆盖。
   - path 字段在加载时按所在配置文件目录解析。
6. 支持 profile：
   - `--profile <name>`。
   - profile 只覆盖部分字段。

### 验收标准

- `settings_map` 能展示 effective value 和来源。
- diagnostics 能解释某个 project config 为什么被忽略。
- requirements 能阻止不允许的 sandbox/permission/provider 设置。
- 现有 `.allthecodes/settings.json` 不被破坏，提供迁移/兼容读取。

### 执行记录

| 命令/核对项 | 结果 |
|---|---|
| `cargo test -p allthecodes-config` | 通过：159 个 unit tests、0 个 doctests |
| `cargo test -p allthecodes-protocol --features codegen` | 通过；同步更新 sibling `allthecodes-web/src/lib/generated/*` |
| `cargo test -p allthecodes-web settings` | 通过：10 个 settings/provider 相关 tests |
| `cargo check -p allthecodes-config -p allthecodes-protocol` | 通过 |
| `/api/settings/layers` | 已加入协议元数据、Web handler、generated API docs |
| `/api/state` | 保持 `settings_map`，追加 `settings_sources` 与 `settings_diagnostics` |
| requirements | 支持 `ALLTHECODES_REQUIREMENTS` 指向 JSON/TOML；首批约束覆盖 `permissions.defaultMode`、`sandbox.mode`、`apiProvider`、`backend` |
| trust gate | `TrustConfiguredOnly` 下未信任 project/local config 不参与 effective merge，并返回 disabled layer/entry |

---

## Phase 8：PID / Process Lifecycle 管理

### 目标

为长运行 daemon、agent worker、未来 sandbox/exec 进程建立可靠的 PID 文件、日志、停止、重启机制。

### 任务

1. 定义 process record：
   - pid
   - process start time
   - command kind
   - cwd
   - binary version
   - stderr log path
   - health/readiness URL 或 socket path。
2. 写入 PID/state 文件：
   - 使用 allthecodes 路径隔离：`~/.allthecodes/`。
   - 不写入 Codex 原版路径。
3. 启动子进程时捕获 stderr：
   - 写入固定 log 文件。
   - diagnostics API 支持 tail。
4. stop/restart 使用 grace period：
   - SIGTERM。
   - 等待 60s。
   - 未退出则 SIGKILL。
   - 再等待 10s 确认退出。
5. restart 支持 version-aware：
   - binary version 未变且进程健康时可跳过。
6. 操作锁复用 Phase 5：
   - start/stop/restart 串行。
   - lock timeout 有明确错误。

### 验收标准

- PID reuse 可检测，不会误杀无关进程。
- stderr log 可通过 diagnostics 读取尾部。
- stop/restart 在 Unix 上可靠；Windows 路径需有等价实现或明确 fallback。
- 所有持久化路径保持 allthecodes/Codex 隔离。

### 执行记录（2026-06-17）

Phase 8 已闭环到 daemon supervisor / worker lifecycle：

- `process_state` schema 升级到 v2；supervisor/worker 状态新增 `command_kind`、`binary_version`、`binary_path`、`log_path`、`ready_url`、`process_start_key`，并保持 v1 JSON/SQLite JSON 反序列化兼容。
- 新增 PID identity helper：Linux 使用 `/proc/<pid>/stat` starttime，其他 Unix 使用 `ps -o lstart= -p <pid>`，Windows 使用 PowerShell/CIM 创建时间；identity 不可得时按 best-effort 处理并输出 `identity=unknown`。
- `status_snapshot`、stale supervisor/worker cleanup、worker heartbeat stale terminate、known worker terminate、daemon stop/restart termination 均校验 process identity；PID reused 时标记 stale 并拒绝误杀。
- daemon `stop` 改为 shutdown request 60s grace、soft terminate、force kill、最终 10s 确认退出；`restart --if-version-changed` 在版本相同且 `/readyz` OK 时跳过重启并打印诊断。
- 新增 `daemon logs [supervisor|worker-id] [--tail-bytes N]`，tail 读取限制在 `daemon_dir()` 下，默认 16 KiB。
- `daemon status` 与 Web gateway diagnostics 暴露 version、binary/log path、ready URL、identity 信息；不新增独立 Web API 字段。

新增/确认测试：

- `cargo test -p allthecodes-daemon process_state`
- `cargo test -p allthecodes-daemon supervisor`
- `cargo test -p allthecodes-web gateways`

---

## 跨 Phase 执行规则

1. 每个 Phase 必须先补测试再提交：
   - 类型/解析逻辑用 unit tests。
   - HTTP/WS/lifecycle 用 integration tests。
   - 配置 merge 用 fixture tests。
2. 每个 Phase 完成后至少运行：
   - 该 Phase 涉及 crate 的 `cargo test -p ...`
   - `cargo check -p allthecodes --bin allthecodes`
3. 涉及公共协议或端点时同步更新：
   - `development/docs/api-interface-analysis.md`
   - `docs/api/routes.md` / OpenAPI schema（如适用）
4. 所有新增持久化路径必须使用 allthecodes 隔离路径：
   - `~/.allthecodes/`
   - `.allthecodes/`
   - keychain service `allthecodes`
5. 不允许以 “Lite” 为由缩减行为；无法实现的分支必须写入 gaps/known issues。

---

## 建议实施顺序

1. 先做 Phase 0 和 Phase 1，清理当前文档/手动验收尾巴。
2. Phase 2 和 Phase 3 作为一个里程碑：统一连接事件和 outbound 写路径。
3. Phase 4 接在 Phase 3 后做，复用 event log/replay 能力。
4. Phase 5 与 Phase 8 可组成 daemon/process reliability 里程碑。
5. Phase 6、Phase 7 可并行给独立 worker 做，但必须保持 API/config 兼容层。

---

## 剩余目标执行入口

Phase 7 和 Phase 8 已有执行记录，不再作为当前待办列出。剩余目标已拆分到新的分阶段文档中推进：

1. 总路线图：`development/docs/codex-portapi-remaining-roadmap.md`
2. Phase 4b 输出恢复：`development/docs/phase4b-output-recovery-plan.md`
3. Local socket / named pipe transport：`development/docs/local-socket-transport-plan.md`
4. Phase 6 `ResponseEvent` 全链路迁移：`development/docs/phase6-response-event-migration-plan.md`
5. 最终文档、schema 与复验：`development/docs/codex-portapi-final-verification-plan.md`

执行顺序按总路线图推进。每个代码阶段完成后，将执行记录同步回对应阶段文档，并在本文件补充摘要。
