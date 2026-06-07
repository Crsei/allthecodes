# 架构格局判断：请求分发中心化

## Thesis

**本项目有三个独立的请求分发系统（REST handlers、IPC WebSocket 内联逻辑、daemon HTTP/SSE），它们的业务分发规则各自复制了一遍。正确模型是：一个 typed dispatcher + 一个 typed serialization queue + 薄 transport adapter。** 文件组织、DTO 命名、error 类型统一都是这个判断的下游产物——没有中心化分发路径，其他都只是局部优化。

## Confidence

Medium-high。Codex app-server 已经用 `MessageProcessor` + typed queue 验证了这个模型可行。本项目的 crate 结构（`allthecodes-protocol` 定义 DTO、`allthecodes-web` 持有 handler）能够支撑，但有 trait 化改造的工作量。

不确定性来源：IPC 的全双工 streaming 能否平滑穿过 dispatcher，需要在 proto 阶段验证。

## The Trap

**"文件搬迁 + 统一命名就可以。"** 三个 plan 当前都把大量篇幅放在文件归属和命名约定上——哪里放 processor、哪里放 route helper、v1 还是 v2。但如果 dispatcher 本身不存在，这些文件归属只是换了个抽屉放同一个错误模型。

继承来的约束：
- `handler_registry.rs` 同时负责路由注册 + 部分跨域业务逻辑——这是历史积累，不是设计选择
- `/api/ipc/ws` 已经是一个前后端全功能 bridge，内部直接嵌了 engine 调用——它事实上已经是第二个 dispatcher
- 当前 `SerializationLayer` 用了字符串 key + semaphore——这限制了并发粒度，但不妨碍先做 dispatcher

判断：这些约束中**只有 `/api/ipc/ws` 的 wire format 是真契约**（前端生产依赖）。其他都是内部实现形状，不应约束目标架构。

## High-格局 Direction

```
Transport adapter → ClientRequest → ApiDispatcher → typed serialization queue → domain processor
                                                        ↓
                                              Rest: 返回 ClientResponse
                                              IPC:  通过 sink 发 ServerNotification + ServerRequest
```

核心拆分：
- **`ApiDispatcher`** — 一条路径处理所有 typed ClientRequest。集中 experimental gate、serialization scope、error conversion、tracing。
- **Typed serialization queue** — 替代 `SessionOwnership`。FIFO + Exclusive/SharedRead，不是字符串 semaphore。
- **Transport adapter** — REST、WS、JSON-RPC、direct 各一个 adapter，只做 wire format 转换。
- **Domain handler** — 不知道自己被哪个 transport 调用。

## Frame-Opening Move

从终局倒推（打开格局打法 #1）：

六个月后，新增一个 transport（例如 Unix socket IPC）不应该改 domain handler 一行代码，不应该在 domain handler 里看到 `match transport_kind { ... }`。当前架构中，transport 是 handler 的入参——目标架构中，transport 是 handler 的前置管道。

## Bold Takes

**该删：**
- `SessionOwnership` 和 `try_claim_*`/`release_owner` 整套 API —— 它用全局 owner 概念做并发控制，本质上是单客户端假设的产物
- `handlers::ApiError` —— 历史遗留，它在 handlers/mod.rs 里和 domain handler 混在一起
- `processors.rs` 中的具体 domain processor —— 它们属于 `handlers/<domain>.rs`
- `v1/mod.rs` 作为 DTO dump —— 应该只是 module index，DTO 按 domain 拆分到 `v1/<domain>.rs`
- 字符串 key + semaphore 的 serialization 模型

**该合：**
- 三个分散的业务分发入口（REST handlers / IPC WebSocket / daemon HTTP）合入一个 `ApiDispatcher`
- 但 daemon 第一阶段保持独立，只合 REST + direct + 未来 JSON-RPC WS + IPC v2 中可映射为 ClientRequest 的命令

**该重塑：**
- Permission/question 从隐式 callback side effect 重塑为显式 `ServerRequest` —— 有 request id、有超时、有断连 deny 策略。这是 IPC v2 真正的新能力，不是 wire format 升级。
- IPC runtime 从"WebSocket handler 里的一堆 callback 安装代码"重塑为独立的 `IpcRuntime`，有 pending store、outbound queue、cleanup 策略。

**不该仅因已存在就保留：**
- 旧的代码布局（"processors.rs 里已经有 CapabilitiesProcessor 了，就先不放别处吧"）
- `/api/ipc/ws` 作为新功能的默认入口（新 IPC 能力走 v2 route，legacy route 只兼容）
- 非 typed 的序列化 scope（等清理完再改 typed，等于一直不清理）

## Options

| 维度 | 保守路线 | 干净目标 | 分阶段抵达（推荐） |
|------|----------|----------|------------------|
| Dispatcher | 保持 handler_registry 现状，逐步文件搬迁 | 新建 ApiDispatcher，所有 transport 接入 | 先建 ApiDispatcher trait + REST 接入（02-阶段1/2），IPC 和 direct 后续接入 |
| Serialization | 保留 SessionOwnership + 字符串 semaphore | 直接上 typed FIFO queue，一次性替换 | 先建 typed scope + 队列原型替换一个端到端路径（11-Phase F 最小版），验证公平性后批量替换 |
| IPC | 只在 ws/ipc.rs 内部重构 | 删除 legacy route，新建 v2 route | 保留 legacy wire format，内部抽 IpcRuntime + 新增 `/api/v2/ipc/ws`（03 原方案） |
| Error 类型 | 边迁移边改，不改调用方 | 一次性全局替换为 protocol ApiError | domain by domain 替换，每 domain 对比测试 + 删除旧 error（11-Phase D）|
| Permission | 保持 callback 模式 | 一次性改为 ServerRequest 协议消息 | 先封 IpcRuntime 内部 pending store，对外保持 BackendMessage 兼容（03-阶段3）|

**推荐：分阶段抵达干净目标。**

第一刀是 `ApiDispatcher trait + REST 接入 + 第一个 typed serialization queue 原型`。这一个判断点能验证中心化路径是否走得通，再决定后续投入。

## What Not To Do

- 不要在做 dispatcher 之前先花大量精力做文件搬迁 —— 没有 dispatcher 的文件搬迁只是换抽屉
- 不要试图在第一轮就统一 daemon —— 它的 SSE replay、supervisor、control token 生命周期不同
- 不要为新的 typed queue 写一套复杂的泛型框架 —— 先用具体类型验证语义，再抽象
- 不要把 PTY、MCP、browser native host 塞进 dispatcher —— 它们是 independent protocol，不是 API request/response
- 不要为了"平滑迁移"保留双份已经完全解耦的代码路径超过一个版本周期

## First Proof Point

**最小的端到端证明：**

1. 在 `allthecodes-protocol` 中定义 `trait ApiDispatcher { async fn dispatch(...) -> Result<ClientResponse, ApiError> }`
2. 在 `allthecodes-web` 中实现这个 trait，迁移一个 endpoint（例如 capabilities）从旧 handler 路径到 dispatcher
3. 验证 REST 请求通过 dispatcher 返回和旧路径相同的 response（新旧对比测试）
4. 同一个 dispatcher 实现能同时被 `DirectTransport` 调用并返回相同结果

这个证明点完成时间：**在 02-阶段1 + 阶段2 的一个 subset 之后**，不应超过两周的工作量。

## Falsifier

**什么证据会推翻这个 thesis：**

- `ApiDispatcher` trait 化后，IPC streaming（tool use → tool result → assistant message 序列）无法通过 dispatcher 发出而不暴露 transport 细节
  - 如果是 interface 不够抽象：调整 trait 签名，不推翻 thesis
  - 如果 streaming 语义从根本上无法与 request/response dispatch 共存：thesis 需要修正为非全统一，REST 和 IPC 各保留一个 dispatcher
- Typed serialization queue 的 Exclusive/SharedRead 语义在生产负载下导致吞吐下降超过 15%（和当前 semaphore 比）
  - 如果是实现问题：优化 queue，不推翻
  - 如果 FIFO 语义本身带来不可接受的竞争：thesis 需要修正，保留部分 endpoint 的直接执行路径
- 在 dispatcher 前再加一个 serialization queue 让延迟翻倍
  - 加 queue 是设计选择，不是 thesis 的必要部分。可以调调度策略或用无锁路径优化

**真正的证伪条件：** streaming 用例发现 typed dispatch 和 streaming 是互斥的抽象，无法通过同一个 trait 表达。如果出现这个结论，正确模型可能是一个 REST dispatcher + 一个独立 IPC streaming runtime，而不是一个统一入口。
