# KAIROS API Control Implementation Plan

> 状态：已实施（2026-07-15；daemon stop/restart 采用 202 + detached helper 完成语义）
> 前置：[KAIROS Runtime Control Plan](2026-07-15-kairos-runtime-control-plan.md)

## 目标

为 KAIROS 增加稳定、typed、可生成 schema 的 product API，并让 daemon loopback API 暴露相同 domain snapshot。所有 mutating handler 调用共享 `KairosController`；handler 不直接 spawn process、写 settings 或拼 shell command。

## 两个 API host 的边界

| Host | 用途 | 能力 |
| --- | --- | --- |
| `allthecodes-web` protocol API | Web UI/本地产品前端；host 在 daemon 停止时仍可存在 | config、start、stop、restart、完整 snapshot |
| KAIROS daemon loopback API | daemon 运行期运维、CLI/本地集成、SSE | snapshot、config、stop、restart；start 仅作幂等校验，不能作为 stopped daemon 的唯一 bootstrap 入口 |

原因：当 daemon 已停止时，它自己的 HTTP port 不存在，因此 `/api/kairos/start` 不能替代 CLI、TUI 或 web host 中的 controller。

## API 契约

### Product API

```text
GET  /api/kairos
PUT  /api/kairos/config
POST /api/kairos/start
POST /api/kairos/stop
POST /api/kairos/restart
```

建议请求：

```rust
pub struct KairosConfigUpdateRequest {
    pub profile: KairosFeatureProfilePatch,
    pub scope: KairosConfigScope,
    pub apply: KairosApplyMode, // none | reconcile
}

pub struct KairosControlRequest {
    pub cwd: Option<String>,
    pub port: Option<u16>,
    pub readiness_timeout_ms: Option<u64>,
}

pub struct KairosResponse {
    pub snapshot: KairosRuntimeSnapshot,
    pub operation: Option<KairosOperationSummary>,
}
```

`PUT config` 使用 patch，未提供字段保持原值；显式 false 必须可写入。`apply=reconcile` 保证“保存并应用”是单个 controller operation。

### Daemon 兼容 API

- `GET /api/status` 保留当前 top-level 字段，并新增 `kairos: KairosRuntimeSnapshot`。
- 增加与 product API 同名的 `/api/kairos*` routes，复用 domain DTO。
- daemon 已 ready 时调用 start 返回 idempotent result；profile drift 返回 restart-required/conflict，不虚构“已启动”。
- mutating routes 继续要求 `x-allthecodes-daemon-token` 或 bearer token。

## Task A1：Protocol DTO 与 route metadata

### 文件

- Create: `crates/allthecodes-protocol/src/v1/kairos.rs`
- Modify: `crates/allthecodes-protocol/src/v1/mod.rs`
- Modify: `crates/allthecodes-protocol/src/request.rs`
- Modify: `crates/allthecodes-protocol/src/notification.rs`
- Test: `crates/allthecodes-protocol/src/codegen.rs`
- Test: protocol schema/OpenAPI tests

### 实施

- protocol request/response 引用 `allthecodes-types::kairos` domain DTO，不复制不同字段名的第二套 snapshot。
- 为五个 endpoints 增加 `ApiMethod`、metadata、serialization policy 和 error declarations。
- mutating operations 标记为本地受保护写操作，不进入匿名 capability。
- 增加 `KairosLifecycleChanged` server notification，payload 包含 operation id、transition 和最新 snapshot version；不包含 token。
- codegen 输出 TypeScript discriminated unions，child gate、lifecycle 和 error code 不退化为自由字符串。

### 错误契约

至少稳定区分：

- `kairos_invalid_profile`
- `kairos_disabled`
- `kairos_operation_conflict`
- `kairos_stale_state`
- `kairos_spawn_failed`
- `kairos_readiness_timeout`
- `kairos_restart_required`
- `kairos_settings_write_failed`

错误 body 带 `retryable` 和可选 latest snapshot；禁止返回原始 secret 或完整环境变量。

### 测试

- JSON roundtrip、unknown field compatibility、explicit false patch。
- endpoint path/method/codegen/OpenAPI snapshot。
- notification schema 和 secret redaction。

## Task A2：allthecodes-web handlers

### 文件

- Create: `crates/allthecodes-web/src/handlers/kairos.rs`
- Modify: `crates/allthecodes-web/src/handlers/mod.rs`
- Modify: `crates/allthecodes-web/src/handler_registry.rs`
- Modify: `crates/allthecodes-web/src/api_operation_registry.rs`
- Modify: `crates/allthecodes-web/src/state.rs`（仅在需要注入 controller/event publisher 时）
- Test: handler registry and route tests

### 实施

- 注册五个 protocol routes；先作为 REST handler，除非现有 dispatcher 能无阻塞地承载 process lifecycle operation。
- controller 的阻塞文件/process 操作使用 `spawn_blocking`，不能阻塞 Axum runtime worker。
- cwd 通过现有 workspace/session context 解析；请求中的路径先 canonicalize，并遵循当前 workspace authorization。
- handler 返回统一 `KairosResponse`，不返回 daemon CLI 文本。
- operation transition 通过现有 server notification/event publisher 广播；慢消费者不能阻塞 controller。
- start/restart 是有副作用操作，不加入 GET cache 或无认证 direct transport。

### 测试

- stopped snapshot 在无 daemon 时仍返回 200。
- start/config/restart/stop 使用 fake controller 的 handler tests。
- invalid scope/path、operation conflict 和 timeout 的 HTTP mapping。
- route registry 检查所有 `ApiMethod::Kairos*` 有 handler，且 `/api/v2` alias 行为与项目规则一致。

## Task A3：daemon loopback routes 与 status 兼容

### 文件

- Modify: `crates/allthecodes-daemon/src/routes.rs`
- Modify: `crates/allthecodes-daemon/src/server.rs`
- Modify: `crates/allthecodes-daemon/src/state.rs`
- Test: daemon route/token tests

### 实施

- `StatusResponse` 新增 nested `kairos`，保留 `kairos_active`、`proactive`、workers 等旧字段至少一个兼容周期。
- daemon state 持有 controller handle 或安全 adapter；不要在 route 内调用 CLI management parser。
- config/control route 全部执行第二层 token 校验；保留 server middleware 的统一校验作为第一层。
- stop/restart response 在响应写出后安排 lifecycle operation，避免先杀死当前连接导致客户端只看到 transport error。
- restart 采用 operation id；客户端可通过 snapshot/SSE 判断新 daemon ready，不能把“旧进程接受请求”等同于完成。
- GET routes 不泄露 control token path 内容、provider credentials 或完整 environment。

### 测试

- missing/invalid token 为 401；状态探针规则不改变。
- `GET /api/status` 的旧字段与新 nested snapshot 同时存在。
- restart response 可先送达，随后 lifecycle transition 可 replay。
- stopped/stale/failed/ready snapshots 的 JSON fixtures。

## Task A4：SSE lifecycle event

### 文件

- Modify: `crates/allthecodes-daemon/src/sse.rs`
- Modify: `crates/allthecodes-daemon/src/state.rs`
- Modify: controller transition publisher location
- Test: SSE replay and lag tests

### 事件

```json
{
  "event": "kairos_lifecycle",
  "data": {
    "operation_id": "...",
    "from": "starting",
    "to": "ready",
    "restart_required": false,
    "timestamp": "..."
  }
}
```

### 实施

- transition 先持久化，再进入现有 bounded event log；重连可 replay。
- daemon startup 将最后的 durable transition 注入 event log；daemon 内发起的操作通过 controller lifecycle sink 实时 broadcast，避免不同启动入口产生两套事件语义。
- 一个 operation 的 transition sequence 单调有序。
- lagged client 使用现有 `lagged` contract，不为 KAIROS 创建第二套 SSE transport。
- lifecycle event 只表示控制进程状态；模型 stream、worker command 和 automation events 保持现有 event type。

### 测试

- starting -> ready、restarting -> failed 的 replay 顺序。
- 旧 `Last-Event-ID`/query cursor 路径兼容。
- 满 channel 与断开连接不阻塞 lifecycle operation。

## Task A5：API 文档与兼容矩阵

### 文件

- Modify: `development/reference/DAEMON_OPERATIONS.md`
- Modify: `docs/WORK_STATUS.md`
- Modify: generated route/OpenAPI artifacts（仅按仓库现有生成流程）

记录 product API 与 daemon loopback API 的 host、认证、bootstrap 限制、restart completion 语义和 curl 示例。不要把 loopback API 描述为公网 remote-control service。

## API 完成门禁

```bash
cargo fmt --all -- --check
cargo test -p allthecodes-protocol kairos
cargo test -p allthecodes-protocol codegen
cargo test -p allthecodes-web handler_registry
cargo test -p allthecodes-web kairos
cargo test -p allthecodes-daemon routes
cargo test -p allthecodes-daemon sse
cargo check -p allthecodes-protocol
cargo check -p allthecodes-web
cargo check -p allthecodes-daemon
```

完成标准：Web host 能在 daemon 停止时启动它；daemon loopback API 能安全地报告/配置/重启/停止它；两个 host 返回同一 domain snapshot 和稳定错误语义。
