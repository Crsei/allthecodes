# Bridge CLI 依赖恢复在 allthecodes 侧的配合计划

日期：2026-07-03

状态：计划

关联计划：`allthecodes-bridge-cli/development-docs/dependency-recovery-and-health-plan.md`

## 结论

`allthecodes-bridge-cli` 可以在 MCP 工具层实现 dependency health registry、bounded retry、circuit breaker、run stale/blocked 状态和用户可见报告，但有几类能力必须由 `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes` 配合完成：

1. terminal backend 的 HTTP/API 语义由 `allthecodes` 提供，bridge-cli 只能消费。
2. terminal session create 的幂等性必须在 `allthecodes` 的 session manager 中落地，否则 bridge-cli retry 可能创建重复 agent。
3. `allthecodes` 是 MCP host/client 的一方，stdio MCP server 的启动、initialize、stderr、重连、UI/IPC 状态需要在 `allthecodes-mcp` 和 `allthecodes` UI/IPC 层补齐。
4. agent 是否真的能用注入 MCP bridge，不能只靠 terminal session 创建成功判断；`allthecodes` 至少需要提供可被复用的 MCP proof-of-life/probe 能力或状态面。

## 当前代码基线

### Terminal backend

相关文件：

- `crates/allthecodes-protocol/src/v1/terminal.rs`
- `crates/allthecodes-protocol/src/request.rs`
- `crates/allthecodes-web/src/ws/terminal.rs`

当前形态：

- 协议已定义 `GET /api/terminal/sessions`、`POST /api/terminal/sessions`、`GET /api/terminal/sessions/{id}`、`GET /api/terminal/sessions/{id}/output`、`DELETE /api/terminal/sessions/{id}`、terminal WebSocket。
- `TerminalCreateRequest` 已有 `profile`、`cwd`、`label`、`command`、`session_id`、`persist`、`initial_size`，但没有 `client_request_id` 或其他幂等字段。
- `TerminalManager::create_session` 每次生成新的 `terminal-{timestamp}-{counter}`，并立即 spawn PTY 进程。
- 创建失败统一返回 `terminal_create_failed`，当前 handler 把错误映射为 `400 BAD_REQUEST`。
- `TerminalSessionSnapshot` 已有 `status`、`pid`、`created_at`、`updated_at`、`exit_code`、`error`、output seq，但没有创建请求幂等键、创建阶段错误分类、agent MCP 验证状态。

### Health/readiness

相关文件：

- `crates/allthecodes-protocol/src/v1/health.rs`
- `crates/allthecodes-protocol/src/request.rs`
- `crates/allthecodes-web/src/mod.rs`
- `crates/allthecodes-web/src/handlers/health.rs`

当前形态：

- 已有根路径 `/healthz`、`/readyz`、`/startupz`。
- 已有协议路径 `GET /api/healthz`，返回 `HealthResponse { status, version, db }`。
- 当前健康响应主要覆盖 web/db 存活，不包含 terminal subsystem 是否可创建 session、active session 数、最近 PTY spawn 错误等下游诊断。

### MCP host/client

相关文件：

- `crates/allthecodes-mcp/src/client/mod.rs`
- `crates/allthecodes-mcp/src/client/stdio.rs`
- `crates/allthecodes-mcp/src/manager.rs`
- `crates/allthecodes-mcp/src/lib.rs`
- `crates/allthecodes/src/app_runtime_adapters/mod.rs`
- `crates/allthecodes/src/app_subsystem_handlers/mcp.rs`
- `crates/allthecodes/src/app_subsystem_handlers/snapshot.rs`
- `crates/allthecodes/src/ui/mcp/index.rs`
- `crates/allthecodes/src/ui/command_surface/adapters/mcp.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`
- `crates/allthecodes-web/src/handlers/plugins.rs`

当前形态：

- `McpClient::connect()` 负责 stdio/SSE/streamable-http transport。
- `McpClient::initialize()` 已执行 JSON-RPC `initialize` 和 `notifications/initialized`。
- `send_request_with_timeout()` 已有请求超时，initialize 默认使用 `CONNECT_TIMEOUT_SECS`。
- `McpManager::connect_ready_client_with_retries()` 已有 3 次短重试，延迟 50ms 到 250ms。
- stdio transport 已捕获 stderr 并按 debug log 输出，且会做 env secret redaction。
- `McpSubsystemEvent::ServerStateChanged` 只有 `server_name`、`state`、`error`，缺少 attempt、next_retry_at、last_success_at、stderr tail、error kind、circuit 状态。
- UI 状态模型只有 `Connected`、`Connecting`、`Failed`、`Disabled`。
- plugin handler 中已有一次性的 MCP connection test：connect、initialize、tools/list、resources/list。这是 proof-of-life 能力的现有雏形，可以抽成复用逻辑。

## 职责拆分

### allthecodes 负责

1. 扩展 terminal backend 协议和 handler。
2. 在 `TerminalManager` 中实现幂等 session create。
3. 标准化 terminal API 的错误分类和状态码。
4. 在 health/readiness 响应中暴露 terminal 子系统健康信息。
5. 扩展 `allthecodes-mcp` 的连接健康快照、stderr tail、重试/熔断状态和 proof-of-life。
6. 把 MCP 健康状态通过 IPC/UI/Web API 暴露给用户。

### bridge-cli 负责

1. 消费 `allthecodes` 提供的 terminal health 和幂等 create 能力。
2. 管理 bridge-cli 自己的 `DependencyHealth` registry。
3. 在 `launch_workbench_agents`、`sync_run`、`sync_runs` 中返回结构化 dependency failure。
4. 落库 `blocked_by_dependency`、`stale`、`next_retry_at` 等 run/task 状态。
5. 提供 `workbench_health`、`allthecodes doctor` 等面向 bridge-cli 用户的汇总视图。

### 外部运维负责

1. systemd/launchd/Windows Service/Docker/Kubernetes 对长期 daemon 的进程级 supervisor。
2. 容器或系统层的 startup/readiness/liveness probes。
3. OpenTelemetry、日志采集、告警规则。

## allthecodes 侧实施计划

### P0. Terminal backend health 细分

目标：让 bridge-cli 能区分“HTTP 端口可达”和“terminal 子系统可用”。

建议实现：

1. 保留现有 `/healthz`、`/readyz`、`/api/healthz` 行为，避免破坏现有客户端。
2. 新增一个 terminal 子系统健康响应，优先放到协议层，例如：

```rust
pub struct TerminalHealthResponse {
    pub status: String,
    pub subsystem: String,
    pub active_sessions: usize,
    pub last_spawn_error: Option<String>,
    pub last_spawn_error_at: Option<i64>,
    pub can_spawn_profile: bool,
}
```

3. 新增 API route，例如 `GET /api/terminal/healthz` 或把同等字段追加到后续 `DiagnosticsSnapshot` 中。
4. `TerminalManager` 记录最近一次 spawn/openpty/resolve profile 的错误和时间。
5. 对 bridge-cli 来说，`200 + status=ok` 才可作为 launch preflight 成功；连接失败、非 2xx、`status != ok` 都应进入 dependency failure。

验收：

1. bridge-cli 能调用 terminal health，并在 daemon 可达但 terminal 子系统不可用时给出明确原因。
2. health API 不会启动 terminal session，也不会有副作用。
3. 旧的 `/api/healthz` shape 保持兼容。

### P0. Terminal session create 幂等性

目标：bridge-cli 对 launch/create session 做 bounded retry 时，不会重复创建 Codex/Claude/Qoder/Kimi agent terminal。

协议变更：

在 `TerminalCreateRequest` 中新增可选字段：

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub client_request_id: Option<String>;
```

`client_request_id` 约束：

1. bridge-cli 用 `task_id` 或 `task_id + backend_kind + attempt_group` 生成稳定值。
2. 同一个 `client_request_id` 在可用保留窗口内重复请求，返回同一个 `TerminalSessionSnapshot`。
3. 如果已有 session 仍存在，直接返回 snapshot，不再 spawn 新进程。
4. 如果已有 session 已被 prune，可以按明确策略返回 `409 terminal_idempotency_expired` 或创建新 session，并在响应中标明不是同一个 session。优先建议返回 409，避免 silent duplication。

`TerminalManager` 变更：

1. 增加 `request_index: HashMap<String, String>`，映射 `client_request_id -> terminal_session_id`。
2. `create_session()` 在解析命令/spawn 之前先查 index。
3. `remove_session()` 和 prune 时清理 index。
4. 如果请求 key 命中但 session 已不存在，返回可分类错误，不要悄悄创建重复进程。

验收：

1. 两次相同 `client_request_id` 的 `POST /api/terminal/sessions` 只产生一个 PTY child。
2. 第二次请求返回的 `id`、`pid`、`created_at` 与第一次一致。
3. 不同 `client_request_id` 仍正常创建不同 session。

### P0. Terminal API 错误分类

目标：让 bridge-cli 可以稳定判断哪些错误可重试，哪些是永久错误。

建议错误码：

- `terminal_profile_not_found`：4xx，永久错误。
- `terminal_command_invalid`：4xx，永久错误。
- `terminal_cwd_invalid`：4xx，永久错误。
- `terminal_spawn_failed`：可按原因 4xx 或 5xx；命令不存在是永久错误，系统资源/PTY 打不开是可重试或 degraded。
- `terminal_not_found`：404；sync detail 时不应让 bridge-cli 把 run 直接标 failed，需要由 bridge-cli 按 run 生命周期处理。
- `terminal_idempotency_expired`：409；提示 bridge-cli 停止自动重试并报告。
- `terminal_backend_unready`：503；可重试。

实现落点：

- `crates/allthecodes-web/src/ws/terminal.rs` 的 `terminal_error` 和 create/detail/output/delete handlers。
- 如已有 `ProtocolApiError` 适合统一错误 shape，优先复用，不额外发明一套 JSON。

验收：

1. bridge-cli 不需要解析自然语言错误字符串来决定 retry。
2. HTTP status 与 JSON error code 一致。
3. 测试覆盖 create 失败、detail missing、idempotency conflict。

### P1. Terminal snapshot 扩展

目标：给 bridge-cli 的 sync/stale/reporting 提供足够稳定的信息。

建议字段：

```rust
pub client_request_id: Option<String>,
pub last_error_at: Option<i64>,
pub lifecycle_reason: Option<String>,
```

可选字段：

```rust
pub agent_mcp_bridge_verified: Option<bool>,
pub agent_mcp_bridge_error: Option<String>,
pub agent_mcp_bridge_checked_at: Option<i64>,
```

注意：

1. `agent_mcp_bridge_*` 如果由 bridge-cli post-launch probe 负责，也可以先不进入 terminal snapshot。
2. 如果未来 `allthecodes` 能直接感知 agent 内部 MCP 状态，再补这些字段。
3. 所有新增字段必须 optional，避免破坏旧客户端。

验收：

1. bridge-cli 能把 terminal snapshot 映射为 run 的 last known state。
2. sync 失败时保留上一次 snapshot，不把 run 错误改写为 dependency 错误。

### P1. MCP stdio supervisor 诊断增强

目标：当 `allthecodes` 作为 MCP host 连接 `allthecodes-bridge-cli serve` 或其他 stdio MCP server 失败时，用户能看到结构化原因，而不是只看到 failed。

建议扩展 `allthecodes-mcp`：

1. 为每个 server 维护 `McpServerHealthSnapshot`：

```rust
pub struct McpServerHealthSnapshot {
    pub server_name: String,
    pub state: String,
    pub transport: String,
    pub last_success_at: Option<i64>,
    pub last_attempt_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_kind: Option<String>,
    pub failure_count: u32,
    pub next_retry_at: Option<i64>,
    pub stderr_tail: Vec<String>,
    pub tools_count: Option<usize>,
    pub resources_count: Option<usize>,
}
```

2. stdio stderr 不只写 debug log，还写入 bounded ring buffer；继续沿用现有 secret redaction。
3. initialize timeout、process exit before initialize、spawn ENOENT/EACCES、protocol parse error、version mismatch 分成不同 `last_error_kind`。
4. `McpSubsystemEvent::ServerStateChanged` 可保持兼容，但新增更丰富事件，例如 `ServerHealthChanged`。
5. `McpManager::connect_ready_client_with_retries()` 的短重试保留；另加可配置 bounded retry policy，记录 attempt 和 next retry。
6. 对明显不可恢复错误少重试或不重试：ENOENT、EACCES、invalid config、protocol version incompatible。
7. 对 timeout、process exited unexpectedly、connection reset 允许 bounded retry。

验收：

1. 一个 stdio MCP command 不存在时，UI/API 能显示 `spawn_failed` 或同类错误，并包含被 redaction 后的诊断。
2. 一个 server initialize 超时时，状态是 initialize timeout，而不是泛化的 failed。
3. stderr 输出不会被当成协议失败；只作为日志/诊断。

### P1. MCP 状态通过 IPC/UI/Web 暴露

目标：把增强后的 MCP 健康状态变成用户可见报告。

落点：

- `crates/allthecodes/src/app_runtime_adapters/mod.rs`
- `crates/allthecodes/src/app_subsystem_handlers/mcp.rs`
- `crates/allthecodes/src/app_subsystem_handlers/snapshot.rs`
- `crates/allthecodes/src/ui/mcp/index.rs`
- `crates/allthecodes/src/ui/command_surface/adapters/mcp.rs`
- `crates/allthecodes-web/src/handlers/mcp_servers.rs`
- `crates/allthecodes-ipc-protocol/src/subsystem_events.rs`

建议：

1. UI 状态在保持现有 `Connected`、`Connecting`、`Failed`、`Disabled` 的基础上，增加 detail 字段，而不是马上扩大 enum。
2. 在 server detail/card 中展示：
   - last_error
   - failure_count
   - next_retry_at
   - stderr_tail
   - tools/resources count
   - reconnect action 的结果
3. IPC/Web API 输出机器可读字段，避免只返回字符串。
4. `/mcp` 或 MCP command surface 应区分：
   - process spawned
   - initialize completed
   - tools/list completed
   - resources/list completed

验收：

1. 用户能看到 “bridge MCP server unavailable: initialize timeout after 5s, retry exhausted” 这类具体状态。
2. reconnect 后状态变化通过 subsystem event 推送。
3. UI 不泄露 token/env secret。

### P2. MCP proof-of-life 抽象复用

目标：复用 plugin handler 中已有 connect/initialize/tools/list/resources/list 检查，给 bridge-cli 和 agent MCP 验证提供统一探针。

当前可复用雏形：

- `crates/allthecodes-web/src/handlers/plugins.rs` 中 `test_mcp_server_connection` 已按 manifest、command、connect、initialize、tools_list、resources_list 生成 checks。

建议：

1. 把这段逻辑从 plugin handler 抽到 `allthecodes-mcp` 或一个 web/internal utility。
2. 定义通用 `McpProbeResult`：

```rust
pub struct McpProbeResult {
    pub server: String,
    pub status: String,
    pub message: String,
    pub checks: Vec<McpProbeCheck>,
    pub tools: Option<usize>,
    pub resources: Option<usize>,
}
```

3. plugin install check、MCP settings test、agent injected bridge proof-of-life 都走同一套 probe。
4. probe 必须断开临时 client，不能污染 live manager clients。

验收：

1. 同一个 broken stdio MCP server 在 plugin test、MCP settings test、bridge health 中给出一致错误。
2. proof-of-life 至少包含 initialize；支持 tools/list 和 resources/list 的 server 继续检查。

### P2. Agent MCP bridge verified 状态

目标：terminal session 创建成功不再被误报为 agent 内部 MCP bridge 已可用。

可选实现路线：

路线 A：`allthecodes` 提供通用 probe，bridge-cli 在 post-launch 阶段调用 probe 并自己落库。

路线 B：`allthecodes` terminal backend 接受可选 probe config，在 `TerminalSessionSnapshot` 上暴露 `agent_mcp_bridge_verified`。

推荐先做路线 A：

1. allthecodes 实现通用 `McpProbeResult`。
2. bridge-cli 按启动 agent 时注入的 MCP 配置生成同等 probe config。
3. bridge-cli 将结果记录为 `agent_mcp_bridge_verified=true/false/unknown`。
4. allthecodes 后续再决定是否把该状态纳入 terminal snapshot。

验收：

1. terminal session created 与 agent MCP usable 是两个独立状态。
2. agent 进程已启动但 MCP bridge 未通过 initialize 时，报告明确为 `unverified` 或 `unreachable`。

### P3. 可观测性与事件

目标：让运维和开发能追踪 dependency recovery 行为。

建议事件：

- `TerminalBackendReady`
- `TerminalBackendUnready`
- `TerminalCreateIdempotencyHit`
- `TerminalCreateIdempotencyExpired`
- `McpServerRetryScheduled`
- `McpServerRetryExhausted`
- `McpServerRecovered`
- `McpServerStderrCaptured`

建议指标：

- terminal active sessions
- terminal create success/failure count
- terminal create idempotency hit count
- MCP initialize latency
- MCP connect retry attempts
- MCP server health state by server
- MCP stderr tail dropped line count

验收：

1. 日志里能从 `client_request_id` 追踪一次 launch。
2. MCP connect failure 的 error kind 可聚合。
3. 重试耗尽和恢复都有事件。

## 与 bridge-cli 计划的映射

| bridge-cli 计划项 | allthecodes 是否需要配合 | allthecodes 侧工作 |
| --- | --- | --- |
| DependencyHealth registry | 否 | bridge-cli 内部状态，allthecodes 只提供可消费的健康来源 |
| terminal backend preflight health | 是 | 提供 terminal 子系统 health/detail，不只 web/db health |
| bounded retry / circuit breaker | 部分 | bridge-cli 对 terminal HTTP 调用实现；allthecodes-mcp 对 MCP host 连接实现 |
| task_id launch blocked_by_dependency | 部分 | bridge-cli 状态机负责；allthecodes 必须提供 create 幂等键 |
| sync_run stale 状态 | 部分 | bridge-cli 状态负责；allthecodes 需保证 detail/output 错误可分类 |
| stdio MCP supervisor diagnostics | 是 | `allthecodes-mcp` 是 host/client，需要补 stderr tail、error kind、retry 状态 |
| agent MCP proof-of-life | 是 | 抽象通用 MCP probe，bridge-cli 可调用或复用 |
| structured project events | 部分 | allthecodes 发 MCP/terminal 子系统事件，bridge-cli 发 run/task 事件 |

## 推荐执行顺序

1. P0：`TerminalCreateRequest.client_request_id` + `TerminalManager` 幂等 create。
2. P0：terminal 子系统 health/detail endpoint。
3. P0：terminal API 错误分类和测试。
4. P1：MCP health snapshot、stderr tail、error kind。
5. P1：IPC/UI/Web API 展示 MCP 详细健康状态。
6. P2：抽通用 MCP probe，并让 plugin test 复用。
7. P2：bridge-cli 接入 probe 做 agent MCP proof-of-life。
8. P3：事件和指标补齐。

## 测试计划

### Unit tests

1. `TerminalManager::create_session` 相同 `client_request_id` 返回同一 session。
2. session prune/remove 会清理 idempotency index。
3. terminal health 在 spawn 错误后记录 last error。
4. MCP stderr redaction 后进入 ring buffer。
5. MCP error classifier 区分 ENOENT、EACCES、timeout、protocol mismatch。

### HTTP/API tests

1. `POST /api/terminal/sessions` 重复请求不会重复 spawn。
2. `GET /api/terminal/healthz` 返回 stable schema。
3. terminal create invalid cwd/profile 返回可分类 4xx。
4. terminal backend unready 返回 503。
5. `/api/mcp-servers` 或新增 health endpoint 输出 MCP health detail，不泄露 secret。

### Integration tests

1. 用不存在的 stdio MCP command 验证 `spawn_failed`。
2. 用能启动但不响应 initialize 的 fake MCP server 验证 `initialize_timeout`。
3. 用向 stderr 写 token 的 fake MCP server 验证 redaction。
4. 用 fake bridge-cli MCP server 验证 connect/initialize/tools-list proof-of-life。

### Cross-repo tests

1. bridge-cli 对 terminal health preflight 的成功/失败路径。
2. bridge-cli 对 `client_request_id` 的 retry 不创建重复 terminal。
3. bridge-cli sync detail 在 backend 不可达时只标 stale，不误标 failed。
4. agent session created 但 MCP probe failed 时，报告 `agent_mcp_bridge_verified=false`。

## 风险与约束

1. 协议兼容：所有新增 DTO 字段都应是 optional；新增 endpoint 不应改变现有 endpoint shape。
2. 幂等窗口：如果 session 被 prune 后重复请求如何处理必须固定，否则 bridge-cli 很难安全 retry。
3. 状态归属：terminal process state、MCP host state、bridge-cli run state 必须分开，不要把 dependency failure 写成 agent failure。
4. 安全：stderr tail、env、command args 都可能含 secret，必须复用并扩展现有 redaction。
5. 并发：`client_request_id` index 查询和 session insert 需要在同一个临界区内保证原子性，避免并发双创建。

## 完成定义

1. bridge-cli 可以在 terminal backend 不可用、MCP host initialize 失败、agent MCP bridge 未验证三种情况下输出不同的结构化状态。
2. bridge-cli launch retry 不会因为响应丢失而创建重复 terminal session。
3. `allthecodes` UI/IPC/Web API 能显示 MCP server 的失败原因、最后错误、stderr tail、retry 状态。
4. terminal API 和 MCP health 状态都有测试覆盖。
5. 用户看到的是 “blocked/stale/unverified”，而不是统一的 “failed”。
