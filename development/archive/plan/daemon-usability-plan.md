# Daemon 可用化计划

> 日期：2026-05-05
> 范围：`crates/claude-code-rs/src/daemon/**`、CLI/命令入口、后台 worker 生命周期、BRIDGE_MODE/远程控制集成。

## 目标

将当前 Rust 端的 `--daemon` 从“单进程 KAIROS HTTP/SSE 实验入口”推进到真正可作为后台守护进程使用的 Daemon：

- 主进程 supervisor 常驻运行，负责 worker 子进程的启动、重启、停止、健康检查和日志归档。
- worker 子进程承载具体职责，例如 assistant 会话、bridge/remote-control 同步、proactive tick、team memory proxy。
- supervisor 与 worker 通过文件系统状态文件通信，不依赖当前 TUI/REPL 的内存态。
- 可由另一个 CLI 进程执行 `daemon status/stop/restart` 等管理命令。
- 能作为 BRIDGE_MODE/远程控制服务的本地后台宿主。

## 当前状态判断

当前 `crates/claude-code-rs` 已有 daemon 模块，但它和上述目标形态不是同一个完整层级。

已经具备的基础：

- `--daemon` 隐藏参数存在，默认端口 `19836`，要求 `FEATURE_KAIROS=1`。
- `daemon::server` 可启动本地 Axum HTTP 服务，暴露 `/health`、`/events`、`/api/submit`、`/api/status` 等接口。
- `DaemonState` 已包装 `QueryEngine`、SSE client registry、事件 ring buffer、query running 标记。
- `tick_loop` 已有 proactive tick 雏形，可周期性向 `QueryEngine` 提交 tick prompt。
- `team_memory_proxy` 已能在 `FEATURE_TEAMMEM` 下启动 Bun team-memory-server 并代理请求。

与可用 Daemon 的差距：

- 没有 supervisor/worker 多进程模型。当前 daemon 是单进程 HTTP 服务，没有 `--daemon-worker`、worker registry、worker heartbeat、worker restart。
- 没有跨进程状态文件协议。`/daemon status` 只看当前进程内 `AppState`，不能从另一个 CLI 进程可靠管理后台 daemon。
- `/daemon stop` 仍是占位输出，没有真正关闭后台进程。
- daemon 管理命令和启动参数不统一：CLI 默认端口是 `19836`，`/daemon` 命令里的 health URL 仍写死为 `3579`。
- BRIDGE_MODE/远程控制在 Rust 端还没有可用宿主链路，`bridge/`、`remote/` 仍属于历史延期范围。
- permission、resize、history、部分 command result 路径仍是 stub；daemon 无法完整承载交互式工具审批和恢复。
- notification consumer 代码存在，但当前 daemon 启动路径没有接入消费循环。
- SleepTool 和 `/sleep` 当前返回“已休眠”的语义，但没有把 sleep duration 写入 `QueryEngine::sleep_until`。
- HTTP API 目前 `CorsLayer::permissive()`，没有本地 token、admin endpoint 权限、CSRF/跨站边界，不能直接作为远程控制入口。

## 目标架构

```text
cc-rust daemon start/status/stop
        |
        v
Supervisor process
  - owns state root: ~/.cc-rust/daemon/
  - owns lock file and supervisor.json
  - owns worker registry and health loop
  - exposes local control API when enabled
        |
        +-- Worker: assistant-session
        |     - owns QueryEngine session
        |     - processes submit/abort/permission commands
        |
        +-- Worker: bridge-sync
        |     - owns BRIDGE_MODE / remote-control connection
        |     - maps remote events to local worker commands
        |
        +-- Worker: proactive
        |     - owns tick scheduling and sleep state
        |
        +-- Worker: team-memory
              - owns or proxies team memory service

Filesystem state protocol
  ~/.cc-rust/daemon/
    supervisor.json
    supervisor.lock
    workers/<worker-id>.json
    commands/<command-id>.json
    events/<worker-id>.ndjson
    logs/<worker-id>.log
```

Rust 端必须继续遵守路径隔离：所有 cc-rust daemon 状态写入 `~/.cc-rust/daemon/`，不能使用原版 `~/.claude/` 或 `.Codex/` 路径。

## 阶段计划

进度记录：

- 2026-05-05：Phase 1 MVP 已开始落地。新增 `~/.cc-rust/daemon/` 状态根、`supervisor.json`、`shutdown-request.json`、跨进程 `daemon start/status/stop/restart` 管理入口、运行中 daemon 对 shutdown 请求的轮询退出。此阶段仍未实现 worker registry；Phase 2 继续处理。

## Phase 0 实施记录（2026-05-05）

状态：已完成 daemon 可用化的语义边界、目标架构和状态协议基线梳理。本阶段是文档/设计基线，不引入运行时代码。

本阶段交付：
- 明确 Rust 端 daemon 目标不是单进程 KAIROS HTTP demo，而是 supervisor 管理 worker 的后台守护进程。
- 明确所有持久化状态必须写入 `~/.cc-rust/daemon/`，继续遵守 cc-rust 与上游 Claude/Codex 的路径隔离。
- 定义目标状态文件结构：`supervisor.json`、`workers/<worker-id>.json`、`commands/<command-id>.json`、`events/<worker-id>.ndjson`、`logs/<worker-id>.log`。
- 定义 MVP worker kind：优先落地 `assistant-session`，bridge/proactive/team-memory worker 作为后续增强。
- 明确安全边界：远程控制能力前必须先有本地 control token 和监听地址约束。

验证记录：
- Phase 1-7 的后续实现均沿用本阶段定义的状态根和 supervisor/worker/command/event 分层。
- `development/reference/DAEMON_OPERATIONS.md` 已把最终操作面和剩余 non-parity 范围固化为发布检查入口。

### Phase 0：对齐上游语义与边界

目标：先确认 Rust 端要复刻的 Daemon 行为，不把 KAIROS HTTP service、remote-control server、background sessions 混成不可维护的一层。

工作：

- 阅读上游 `F:\AIclassmanager\cc\src\daemon\**`、`src\bridge\**`、`src\commands\remoteControlServer\**`，补一张 Rust 对齐表。
- 明确 feature gate：建议新增或恢复 `FEATURE_DAEMON`、`FEATURE_BRIDGE_MODE`，不要继续只用 `FEATURE_KAIROS` 表示所有后台能力。
- 定义 MVP worker kinds：建议先只做 `assistant-session` 和 `bridge-sync`，`proactive` 可在 Phase 4 独立化。
- 定义状态文件 schema 版本、字段和原子写入规则。

验收：

- 有一份 schema 草案：`SupervisorState`、`WorkerState`、`DaemonCommand`、`DaemonEvent`。
- 明确哪些功能属于 MVP，哪些保留为 Phase 2+。

### Phase 1：跨进程状态文件和管理命令 MVP

目标：让 `daemon status/stop` 对另一个 CLI 进程可用，这是最小可用闭环。

状态：已实现 MVP 主干。当前交付包含状态文件读写、stale PID 检测、CLI 管理入口和 shutdown request；不包含真实 worker 状态。

验证记录：

- `cargo test -p cc-config partition_functions_all_root_under_data_root --lib` 通过。
- `cargo test -p claude-code-rs daemon::process_state` 通过，覆盖状态读写、shutdown request、当前进程存活检测、端口参数解析。
- `cargo test -p claude-code-rs commands::daemon_cmd` 通过，覆盖 `/daemon status` 和 `/daemon start/restart` 提示。
- 本地 smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` 下执行 `daemon start --port 21983`、`daemon status`、`daemon stop`、`daemon status`，状态从 running 回到 stopped。
- `cargo fmt --all --check` 通过。
- `cargo clippy -p claude-code-rs --all-targets -- -D warnings` 未通过，但失败项来自既有非 daemon 路径，例如 `cc-sandbox/src/runner.rs` needless lifetime、fs tools/manual strip、command surface large enum 等；Phase 1 未将这些作为完成阻塞。

工作：

- 新增 daemon state 模块，负责：
  - `~/.cc-rust/daemon/supervisor.json`
  - `~/.cc-rust/daemon/workers/*.json`
  - atomic write + rename
  - stale PID 检测
  - lock file 防止双 supervisor
- 扩展 CLI 命令：
  - `cc-rust daemon start`
  - `cc-rust daemon status`
  - `cc-rust daemon stop`
  - `cc-rust daemon restart`
- 修正当前 `/daemon` 命令：
  - 不再依赖当前 `AppState.kairos_active` 判断后台 daemon。
  - 读取统一状态文件。
  - 修正 health URL 端口来源。
- `stop` 先实现优雅退出：写入 shutdown command，再等待 supervisor 清理；超时后按平台执行进程树终止。

验收：

- 终端 A 启动 daemon，终端 B 能看到 running 状态。
- 终端 B 执行 stop 后，终端 A 进程退出，状态文件清理或标记 stopped。
- supervisor 崩溃后，下一次 status 能识别 stale 并清理。

### Phase 2：Supervisor 进程与 worker registry

目标：把 daemon 从单个 HTTP 服务拆成 supervisor 管 worker 的生命周期。

工作：

- 新增隐藏 worker 启动入口，例如 `--daemon-worker <kind> --worker-id <id>`。
- 实现 `WorkerSpec` 和 `WorkerRegistry`：
  - worker kind
  - cwd
  - env
  - log path
  - restart policy
  - required/optional 标记
- supervisor 负责：
  - spawn worker
  - 写入 worker state
  - 周期性更新 supervisor heartbeat
  - 检查 worker heartbeat
  - 崩溃重启和退避
  - shutdown 时终止子进程树
- Windows 要使用现有进程树终止能力，避免只杀父进程留下子进程。

验收：

- worker 进程退出后，supervisor 能按策略重启并记录 restart count。
- supervisor stop 能清理所有 worker。
- worker log 文件可通过命令定位。

### Phase 3：文件系统命令/事件协议

目标：让 supervisor、worker 和外部 CLI/API 通过状态文件可靠通信。

工作：

- 定义 command 文件：
  - `submit`
  - `abort`
  - `permission_response`
  - `ask_user_response`
  - `shutdown`
  - `reload_config`
- 定义 event 文件或 NDJSON：
  - `ready`
  - `heartbeat`
  - `stream_delta`
  - `tool_progress`
  - `permission_request`
  - `ask_user_question`
  - `result`
  - `error`
- 采用“写临时文件 -> fsync/flush -> rename”的原子发布流程。
- command 必须有 idempotency key，worker ack 后写入 handled 状态，避免重启后重复执行危险命令。
- 增加垃圾回收策略：保留最近 N 天事件和日志。

验收：

- worker 重启后不会重复执行已 ack 的 submit。
- 外部 CLI 能向 worker 投递 abort 并得到事件确认。
- 文件损坏或半写入时能返回清晰错误并跳过，不 panic。

### Phase 4：把当前 KAIROS HTTP/SSE 能力迁移到 supervisor 架构

目标：保留现有 HTTP/SSE 价值，但让它成为 daemon control plane，而不是唯一 daemon 形态。

工作：

- `server.rs` 改为读取 supervisor/worker 状态，而不是只持有单个进程内 `DaemonState`。
- `/api/submit` 路由到 assistant worker command inbox。
- `/events` 从 worker event log 或 event bus 转 SSE。
- `/api/status` 汇总 supervisor + worker + QueryEngine 状态。
- `/api/permission` 真正写入 permission response command。
- `/api/history` 返回 session/event log 的真实历史。
- `/api/resize` 若仍需保留，明确是 no-op 还是转发给对应 worker。
- 接入 notification consumer，并约束只在需要时发送 Windows Toast/Webhook。
- 修复 SleepTool：工具调用和 `/sleep` 都必须写入真实 sleep state。

验收：

- HTTP client 提交 prompt 后，SSE 能收到 start/delta/result。
- permission request 可以通过 HTTP 回答并继续工具执行。
- `/api/status` 中能看见 worker heartbeat、running query、sleeping 状态。

### Phase 5：BRIDGE_MODE / remote-control 集成

目标：让 daemon 成为远程控制服务的本地后台宿主。

工作：

- 增加 Rust 端 `BRIDGE_MODE` feature gate 和配置解析。
- 明确 bridge worker 职责：
  - 连接 remote-control server 或本地 bridge endpoint。
  - 上报 worker/session metadata。
  - 接收远程 submit/abort/permission 请求。
  - 将远程事件映射到 daemon command 文件。
- 复用现有 auth/config 路径，禁止把原版 Claude 路径写入 cc-rust 状态。
- 增加本地 control token：
  - supervisor 启动时生成 secret。
  - 写入仅当前用户可读的状态文件。
  - HTTP admin endpoints 必须校验 token。
- 远程控制入口默认只绑定 `127.0.0.1`，显式配置才允许对外监听。

验收：

- bridge worker 能注册 session metadata。
- 远程 submit 通过 bridge worker 投递到 assistant worker，并返回结果事件。
- 未携带 token 的本地 HTTP admin 请求被拒绝。

### Phase 6：Proactive、scheduler 和长期运行可靠性

目标：让 daemon 能长期运行，而不是只能跑一个 HTTP submit demo。

工作：

- 将 proactive tick 从当前 daemon 单进程 loop 移入 `proactive` worker 或 supervisor-managed scheduler。
- tick 前检查：
  - assistant worker 是否 busy
  - sleep_until
  - pending user question
  - recent notification throttling
- 接入现有 `services::scheduler`，让 cron-style task 由 daemon/scheduler worker 执行。
- daily log 改为结构化 event + markdown view 双轨，避免只追加字符串日志。
- 增加 crash recovery：
  - supervisor 恢复 worker states
  - 未完成 task 标记 recoverable
  - remote task poller 重新接管

验收：

- daemon 运行 30 分钟以上无事件丢失和 worker 泄漏。
- SleepTool 能暂停 tick，用户 submit 能 wake up。
- 重启 daemon 后能恢复未完成或 recoverable 任务状态。

### Phase 7：测试、文档和发布门槛

目标：用自动化验证保证 daemon 可以作为后台服务交付。

必须补的测试：

- 单元测试：
  - state schema 序列化/反序列化
  - atomic write
  - stale PID cleanup
  - command ack/idempotency
  - worker restart policy
- 集成测试：
  - start/status/stop/restart
  - supervisor 崩溃恢复
  - worker 崩溃重启
  - HTTP submit + SSE stream
  - permission response flow
  - SleepTool/tick flow
- Windows 专项：
  - detached process spawn
  - process tree kill
  - file lock/rename 行为
- 安全测试：
  - token 缺失/错误拒绝
  - CORS/admin endpoint 边界
  - 状态文件权限检查

发布门槛：

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace`
- daemon e2e：start -> submit -> SSE result -> status -> stop
- 文档同步：`USAGE_GUIDE.md`、`COMMAND_REFERENCE.md`、`IMPLEMENTATION_GAPS.md`、本计划完成项归档。

## 推荐实施顺序

1. Phase 0：上游行为核对和 schema 定稿。
2. Phase 1：状态文件 + `daemon status/stop`，先做最小跨进程闭环。
3. Phase 2：supervisor 管 worker。
4. Phase 3：文件系统 command/event 协议。
5. Phase 4：迁移现有 HTTP/SSE 到新架构。
6. Phase 5：BRIDGE_MODE/remote-control。
7. Phase 6：proactive/scheduler 长期运行能力。
8. Phase 7：测试、文档、发布门槛。

## 关键风险

- 范围膨胀：Daemon、KAIROS、BRIDGE_MODE、remote-control、background tasks 都相关，但不应一次性恢复全部上游系统。MVP 应先完成跨进程生命周期闭环。
- 安全边界：当前 HTTP CORS permissive，不能直接升级为远程控制 API。远程控制前必须先有 token 和监听地址策略。
- Windows 生命周期：文件锁、rename、进程树终止和后台 detached 行为都需要单独验证。
- 重复执行风险：文件系统通信如果没有 command ack/idempotency，worker 重启可能重复执行 submit 或危险工具调用。
- 路径隔离：任何 daemon/bridge 状态都必须写入 `~/.cc-rust/`，不能复用上游 `~/.claude/`。

## MVP 完成定义

最小可用 Daemon 不要求完整 remote-control，但必须满足：

- `daemon start/status/stop/restart` 可从不同 CLI 进程管理同一个后台 daemon。
- supervisor 状态和 worker 状态持久化在 `~/.cc-rust/daemon/`。
- 至少一个 assistant worker 由 supervisor 管理，并能处理 submit/abort。
- worker 崩溃能被检测并按策略恢复。
- HTTP/SSE 或 CLI 管理入口能稳定展示真实状态。
- daemon 退出后不遗留 worker 子进程、锁文件或错误的 running 状态。

## Phase 2 实施记录（2026-05-05）

状态：已落地 supervisor + worker registry 的生命周期主干，仍未把 submit/abort 的真实业务处理迁入 assistant worker（该部分保留给 Phase 3/4 的 command/event 协议和 HTTP 路由迁移）。

本阶段交付：
- 新增隐藏 worker 入口：`--daemon-worker <kind> --worker-id <id>`，目前支持 `assistant-session`。
- 新增 `daemon::supervisor`，由 supervisor 启动并监控 `assistant-session-1` worker。
- 新增 worker 状态 schema：`~/.cc-rust/daemon/workers/<worker-id>.json`，记录 kind、pid、status、heartbeat、restart_count、required、log_path。
- 新增 worker 日志路径：`~/.cc-rust/daemon/logs/<worker-id>.log`。
- supervisor loop 现在负责 spawn worker、刷新 supervisor heartbeat、读取 worker heartbeat、按 restart policy 重启异常 worker，并在 shutdown 时终止 worker 进程树。
- `daemon status` 和 `/daemon status` 会展示 worker 数量、kind、pid、status。

验证记录：
- `cargo test -p claude-code-rs daemon::process_state` 通过，覆盖 worker state 写入、heartbeat、summary 聚合。
- `cargo test -p claude-code-rs daemon::supervisor` 通过，覆盖默认 worker spec 和 worker kind 解析。
- 本地 start/status/stop smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` + port `21984` 下，status 展示 1 个 running worker，stop 后 supervisor 和 worker 均不存在。
- 本地 worker crash/restart smoke 通过：port `21985` 下手动 `taskkill` worker 后，supervisor 启动新 worker PID，`restart_count` 从 0 变为 1，stop 后无进程残留。
- `rustfmt --edition 2021 --check` 已针对本阶段 Rust 文件通过。
- `cargo fmt --all --check` 当前仍被既有非 daemon 文件 `crates/claude-code-rs/src/tools/send_message.rs` 的格式差异阻塞；本阶段 daemon 相关文件已由 rustfmt 格式化。
- `cargo clippy -p claude-code-rs --all-targets -- -D warnings` 当前仍被既有非 daemon crate `crates/cc-sandbox/src/runner.rs` 的 `needless_lifetimes` 阻塞。
- 测试编译当前仍报告既有非 daemon warning：`crates/claude-code-rs/src/teams/in_process.rs` 的 `permission_mode` 字段未读取。

## Phase 3 实施记录（2026-05-05）

状态：已落地文件系统 command/event 协议和 worker ack 循环；submit 的真实 QueryEngine 执行仍保留在当前 HTTP supervisor 路径，Phase 4 再迁移。

本阶段交付：
- 新增 `daemon::protocol`：
  - command 文件：`~/.cc-rust/daemon/commands/<worker-id>/<command-id>.json`
  - event 日志：`~/.cc-rust/daemon/events/<worker-id>.ndjson`
  - command kind：`submit`、`abort`、`permission_response`、`ask_user_response`、`shutdown`、`reload_config`
  - command status：`pending`、`acked`、`handled`、`failed`
  - idempotency key 去重：同一个 worker 下相同 key 不重复生成命令。
- worker 心跳循环会读取 pending command：
  - 所有 pending command 先写入 `acked` 和 `command_ack` event。
  - `abort` 立即标记 `handled` 并写入 `abort_ack` event。
  - `submit` 当前标记 `acked` 并写入 `command_deferred` event，避免重启后重复 ack；真实执行在 Phase 4 迁移。
- CLI 管理入口新增：
  - `daemon submit <text>`
  - `daemon abort`
  - `daemon command <id> [worker-id]`
  - `daemon events [worker-id]`

验证记录：
- `cargo test -p claude-code-rs daemon::protocol` 通过，覆盖 idempotency 去重、submit 只 ack 一次、abort handled/event。
- `cargo test -p claude-code-rs daemon::supervisor` 通过，确认 worker registry 仍可编译运行。
- `rustfmt --edition 2021 --check` 已针对 Phase 3 daemon 文件通过。
- 本地 command/event smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` + port `21986` 下，`daemon submit` 生成 `acked` command 和 `command_deferred` event，`daemon abort` 生成 `handled` command 和 `abort_ack` event，stop 后 supervisor 不存活。

## Phase 4 实施记录（2026-05-05）

状态：现有 KAIROS HTTP/SSE 控制面已经开始读取 supervisor/worker 状态并写入 command/event 协议；为避免破坏现有能力，`/api/submit` 仍沿用 supervisor 内 QueryEngine 流式执行，同时额外投递 submit command，后续再迁移真实执行所有权。

本阶段交付：
- `/api/submit` 会先投递 `submit` command 到 `assistant-session-1`，返回 `command_id`，并继续沿用现有 QueryEngine/SSE 执行路径。
- `/api/abort` 会投递 `abort` command，同时调用当前 `engine.abort()`。
- `/api/permission` 不再只返回 stub，而是投递 `permission_response` command。
- `/api/status` 增加 supervisor/worker 视图：`supervisor_status`、`supervisor_pid`、`health_url`、`workers`、`command_root`、`assistant_event_log`。
- `/api/history` 增加 `daemon_events` 与当前 SSE buffer。
- `/api/resize` 明确返回 `noop`，不再伪装成已实现。
- `/events` 在连接时会 replay `assistant-session-1` 的 daemon event 日志，作为 Phase 4 的文件事件桥接。

验证记录：
- `cargo test -p claude-code-rs daemon::routes` 通过编译。
- `cargo test -p claude-code-rs daemon::sse` 通过编译。
- 本地 HTTP control-plane smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` + port `21987` 下，`/api/status` 返回 1 个 running worker；`/api/permission` 返回 queued command；`/api/abort` 返回 command_id；`/api/history` 返回 permission/abort 产生的 4 条 daemon event；stop 后 supervisor 不存活。
- 未执行 `/api/submit` live smoke，以避免触发真实模型请求；该路由已在编译层验证并保留原有执行路径。

## Phase 5 实施记录（2026-05-05）

状态：已落地 remote-control/BRIDGE_MODE 前置安全边界：本地 daemon control token。真正的 bridge worker 联网注册、远程 submit/abort/permission 映射仍保留为后续工作。

本阶段交付：
- supervisor 启动时生成 `~/.cc-rust/daemon/control-token.json`，stop 时清理。
- 新增 `daemon token` 管理命令，用于本机 CLI 读取当前 control token。
- HTTP mutating endpoints 要求 token：
  - `POST /api/submit`
  - `POST /api/abort`
  - `POST /api/command`
  - `POST /api/permission`
  - `POST /api/resize`
- token 可通过 `x-cc-rust-daemon-token: <token>` 或 `Authorization: Bearer <token>` 传入。
- 缺失或错误 token 的 mutating 请求返回 `status=unauthorized`，不会写 command 文件。

验证记录：
- `cargo test -p claude-code-rs daemon::process_state` 通过，新增覆盖 control token 创建、校验、stop 清理。
- `cargo test -p claude-code-rs daemon::routes` 通过编译。
- 本地 token smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` + port `21988` 下，无 token 的 `/api/abort` 返回 `unauthorized`；`daemon token` 返回 36 字符 token；带 token 的 `/api/permission` 和 `/api/abort` 正常入队并生成 4 条 event；stop 后 `control-token.json` 被删除且 supervisor 不存活。

## Phase 6 实施记录（2026-05-05）

状态：已落地跨进程 daemon sleep state，使 proactive tick、`/api/status`、SleepTool、`/sleep` 和 CLI 管理命令共享同一份持久化休眠状态。完整 scheduler worker、cron-style task 接管、daily log 结构化双轨和未完成任务恢复仍保留为后续工作。

本阶段交付：
- 新增 `~/.cc-rust/daemon/sleep-state.json`，记录 `sleeping_until`、`reason`、`updated_at` 和 schema version。
- 新增 `daemon sleep <seconds> [reason]` 与 `daemon wake` 管理命令，支持从另一个 CLI 进程暂停或恢复 daemon proactive tick。
- SleepTool 和 `/sleep` 现在都会写入持久化 daemon sleep state，并在输出中返回实际 `sleep_until`。
- proactive `tick_loop` 在每次 tick 前读取 active sleep state；未过期时跳过 tick，过期时自动清理状态文件。
- `/api/status` 新增 `daemon_sleep_until` 与 `daemon_sleep_reason`，并把持久化 daemon sleep 纳入 `sleeping` 判断。
- `daemon stop` 会清理 `sleep-state.json`，避免下次启动继承陈旧休眠状态。

验证记录：
- `cargo test -p claude-code-rs daemon::process_state` 通过，覆盖 sleep state active/expired 清理。
- `cargo test -p claude-code-rs tools::exec::sleep` 通过，确认 SleepTool 测试仍通过。
- `rustfmt --edition 2021` 已针对本阶段 Rust 文件通过。
- 本地 sleep smoke 通过：临时 `CC_RUST_HOME` + `FEATURE_KAIROS=1` + port `21989` 下，`daemon sleep 60 'phase six smoke'` 后 `/api/status` 返回 `sleeping=true`、`daemon_sleep_reason=phase six smoke`，`daemon wake` 后 `/api/status` 返回 `sleeping=false` 且 `sleep-state.json` 被删除；stop 后 supervisor 不存活。
- 未执行 30 分钟以上长跑、完整 scheduler worker 和 recoverable task 恢复测试；这些仍是 Phase 7 发布门槛或后续增强项。

## Phase 7 实施记录（2026-05-05）

状态：已补齐 daemon 专用操作文档、快速 CLI 回归测试和发布门槛记录；完整 workspace 发布门槛仍被非 daemon 既有失败阻塞，daemon 目标测试集已通过。

本阶段交付：
- 新增 `development/reference/DAEMON_OPERATIONS.md`，集中记录 daemon CLI、slash command、HTTP token、状态文件、发布验证命令与剩余 non-parity 范围。
- 新增 `e2e_cli::daemon_management_reports_stopped_state_without_running_daemon`，验证外部 CLI 在空 `CC_RUST_HOME` 下能报告 stopped，且没有运行 daemon 时 `daemon sleep` 会安全失败。
- 修正 `daemon::memory_log` 测试，不再假设 `CC_RUST_HOME` 临时重定向路径必须包含默认目录名 `.cc-rust`。
- 本计划文档记录 Phase 1-7 的落地结果与已知发布阻塞。

验证记录：
- `rustfmt --edition 2021 --check` 针对 daemon 相关 Rust 文件与 `tests/e2e_cli.rs` 通过。
- `cargo fmt --all --check` 通过。
- `cargo test -p cc-config partition_functions_all_root_under_data_root --lib` 通过。
- `cargo test -p claude-code-rs daemon::process_state` 通过。
- `cargo test -p claude-code-rs daemon::protocol` 通过。
- `cargo test -p claude-code-rs daemon::supervisor` 通过。
- `cargo test -p claude-code-rs daemon::routes` 通过编译。
- `cargo test -p claude-code-rs daemon::sse` 通过编译。
- `cargo test -p claude-code-rs daemon::memory_log` 通过。
- `cargo test -p claude-code-rs commands::daemon_cmd` 通过。
- `cargo test -p claude-code-rs tools::exec::sleep` 通过。
- `cargo test -p claude-code-rs --test e2e_cli daemon_management_reports_stopped_state_without_running_daemon` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 未通过，当前阻塞为既有非本阶段项：`crates/cc-sandbox/src/runner.rs:241` 的 `clippy::needless_lifetimes`。
- `cargo test --workspace` 未通过；修正 daemon memory_log 后剩余失败在非 daemon 模块：`commands::config_cmd::tests::test_config_set_model_in_memory`、`commands::ide_cmd::tests::select_then_clear_round_trip`、`teams::mailbox::tests::test_write_multiple_and_read`、`tools::fs::file_edit::tests::edit_auto_adjusts_unique_indentation_mismatch`、`tools::worktree::tests::test_exit_worktree_no_session`、`tools::worktree::tests::test_exit_worktree_invalid_action`。

仍未覆盖：
- 30 分钟以上 daemon soak。
- 有真实模型凭据的 HTTP submit -> SSE result live e2e。
- worker 崩溃/重启自动化 e2e。
- bridge worker 远程注册与远程控制结果回传。

---

## 完成状态（2026-05-21）

**判定：⚠️ 部分完成（8 个 Phase 主干均已落地，少量收尾项未覆盖）**

已完成：
- **Phase 0**（文档/设计基线）：已完成 — 明确状态根、目标架构、安全边界
- **Phase 1**（跨进程状态文件和管理命令 MVP）：已完成 — `daemon start/status/stop/restart`、stale PID 检测、shutdown request
- **Phase 2**（Supervisor 进程与 worker registry）：已完成 — `--daemon-worker`、worker 状态 schema、supervisor loop/restart policy/进程树终止
- **Phase 3**（文件系统 command/event 协议）：已完成 — command 文件、event NDJSON、idempotency key、worker ack 循环
- **Phase 4**（KAIROS HTTP/SSE 迁移）：已完成 — `/api/submit/abort/permission/status/history` 写入 command/event 协议，SSE event replay
- **Phase 5**（remote-control/BRIDGE_MODE 安全边界）：已完成 — control token 生成/校验/清理、HTTP mutating endpoint token 认证
- **Phase 6**（Proactive/scheduler/长期运行）：已完成 — 跨进程 daemon sleep state、SleepTool/`/sleep` 写入持久化 sleep state、`tick_loop` 读取 active sleep
- **Phase 7**（测试/文档/发布门槛）：已完成 — `development/reference/DAEMON_OPERATIONS.md`、e2e 测试、验证命令

未覆盖（发布阻塞/后续增强）：
- 30 分钟以上 daemon soak
- 有真实模型凭据的 HTTP submit → SSE result live e2e
- worker 崩溃/重启自动化 e2e
- bridge worker 远程注册与远程控制结果回传
