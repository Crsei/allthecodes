# cc-daemon 迁移执行计划

> 日期：2026-05-13
> 范围：将 `crates/claude-code-rs/src/daemon/**` 迁移到 `crates/cc-daemon/**`，同时保持 `claude-code-rs` 作为最终 binary 入口。

## 目标

把当前仍嵌在 `claude-code-rs` root crate 内的 daemon 实现，逐步迁移为独立 workspace crate `cc-daemon`。

迁移完成后：

- `cc-daemon` 拥有 daemon 状态目录、状态文件、control token、sleep state。
- `cc-daemon` 拥有 worker command/event 文件协议。
- `cc-daemon` 拥有本地 daemon/gateway client。
- `cc-daemon` 拥有 supervisor/worker 生命周期管理。
- `cc-daemon` 拥有 HTTP/SSE 控制面和 gateway route wiring。
- `claude-code-rs` 只负责 CLI 参数、binary 启动、runtime adapter 注入和最终进程入口。
- `cc-daemon` 不依赖 `claude-code-rs` 私有模块，避免 workspace split 后重新形成单体耦合。

## 当前状态

`crates/cc-daemon` 当前仍是 Phase 7 scaffold：

- `crates/cc-daemon/src/lib.rs` 只有 crate 说明。
- `crates/cc-daemon/Cargo.toml` 目前只依赖 `cc-types`。

`crates/claude-code-rs/src/daemon` 已经不是空壳，当前约 5.2K 行，包含：

- `process_state.rs`：跨进程 supervisor、worker、control token、sleep state。
- `protocol.rs`：daemon worker command/event 文件协议。
- `supervisor.rs`：assistant worker lifecycle、heartbeat、restart、shutdown。
- `routes.rs`、`server.rs`、`sse.rs`：HTTP API 与 SSE 控制面。
- `gateway_routes.rs`、`gateway_bridge.rs`、`gateway_client.rs`、`gateway_run_events.rs`：remote-control gateway 集成。
- `webhook.rs`：GitHub/Slack/generic webhook handling。
- `team_memory_proxy.rs`：Bun team-memory-server subprocess proxy。
- `tick.rs`、`memory_log.rs`、`notification.rs`、`channels.rs`：proactive tick、daily log、notification 和 channel 基础能力。

当前 daemon 可用化程度：

- 已经有 `daemon start/status/stop/restart/submit/abort/events/token/sleep/wake` 管理面。
- 已经有 worker state、command state、event log、control token 和 sleep state。
- 已经有 assistant-session worker，但尚未完整拆出 bridge/proactive/team-memory worker。
- `/api/submit` 已经写 command 给 assistant worker。
- SSE handler 当前只在连接时 replay worker event log，尚未持续 tail 新事件，因此 HTTP submit 到实时 SSE result 链路仍不完整。
- `/api/history` 仍是 stub，`/api/resize` 是 no-op。
- permission/ask-user command 可以排队，但尚未完整接入真实审批恢复链路。
- notification consumer 有实现，但主 daemon 启动路径未接入。

## 迁移原则

1. 先迁移低耦合协议层，再迁移 runtime 层。
2. `cc-daemon` 不能依赖 `claude-code-rs`，只能依赖 workspace library crates。
3. root crate 私有 runtime 通过 trait adapter 注入，不直接被 `cc-daemon` 调用。
4. 每个阶段都保持 `claude-code-rs` 可编译，避免中间态大面积破坏。
5. 迁移期间可以保留 root crate shim，但 shim 只能 re-export 或转发，不能继续承载真实实现。
6. 所有 daemon 持久化状态继续写入 `~/.cc-rust/daemon/`，不能写入上游 Claude/Codex 路径。

## 非目标

- 不在迁移阶段重写 daemon 功能语义。
- 不同时解决完整 upstream daemon parity。
- 不把 `claude-code-rs` root crate 的 tools、commands、plugins、teams 大块反向拖入 `cc-daemon`。
- 不在第一阶段改 HTTP API JSON contract。
- 不在第一阶段引入新的远程监听安全模型，继续保持 loopback + control token。

## 目标目录结构

建议最终 `crates/cc-daemon/src` 结构：

```text
lib.rs
constants.rs
process_state.rs
protocol.rs
memory_log.rs
gateway_client.rs
gateway_run_events.rs
channels.rs
notification.rs
runtime.rs
supervisor.rs
gateway_bridge.rs
state.rs
server.rs
routes.rs
sse.rs
gateway_routes.rs
webhook.rs
team_memory_proxy.rs
tick.rs
```

`constants.rs` 至少包含：

- `ASSISTANT_WORKER_ID`
- default daemon port helper 或默认常量
- daemon event/channel 共享常量

`runtime.rs` 用于定义 root crate adapter trait，避免 `cc-daemon` 直接依赖 `claude-code-rs`。

## 依赖分层

### 允许 `cc-daemon` 直接依赖

- `anyhow`
- `chrono`
- `serde`
- `serde_json`
- `uuid`
- `tokio`
- `tokio-stream`
- `futures`
- `reqwest`
- `axum`
- `tower`
- `tower-http`
- `tracing`
- `parking_lot`
- `hmac`
- `sha2`
- `hex`
- `urlencoding`
- `notify-rust`
- `gateway`
- `cc-config`
- `cc-types`
- `cc-engine` 仅在其公开 API 足够稳定后使用
- `cc-commands` 仅用于稳定 command contract，不用于 root-only command registry

### 不允许 `cc-daemon` 依赖

- `claude-code-rs` crate 或其私有 `crate::...` 模块。
- root-only `crate::tools`、`crate::commands`、`crate::plugins`、`crate::teams`、`crate::plan_workflow`。
- 任何通过 `#[path = "..."]` 反向引用 root crate 源码的做法。

## Phase 0：冻结基线

目标：迁移前锁定当前 daemon 行为和已知缺口，避免把既有失败误判为迁移回归。

工作：

- 记录当前 daemon 文件清单：
  - `crates/claude-code-rs/src/daemon/*.rs`
  - `crates/cc-daemon/src/lib.rs`
  - `crates/cc-daemon/Cargo.toml`
- 记录当前 root crate daemon 入口：
  - `crates/claude-code-rs/src/main.rs` 的 `--daemon`、`--daemon-worker`、management command fast path。
  - `crates/claude-code-rs/src/cli.rs` 的 daemon CLI 参数。
- 跑 daemon 专项测试，记录迁移前结果：

```bash
cargo test -p claude-code-rs daemon::process_state
cargo test -p claude-code-rs daemon::protocol
cargo test -p claude-code-rs daemon::routes
cargo test -p claude-code-rs daemon::sse
cargo test -p claude-code-rs daemon::supervisor
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs tools::exec::sleep
```

完成标准：

- 有迁移前测试记录。
- 明确哪些测试失败属于既有问题。
- 不修改 runtime 行为。

## Phase 1：迁移低耦合协议层

目标：先让 `cc-daemon` 拥有状态协议和本地客户端能力。

优先迁移文件：

- `process_state.rs`
- `protocol.rs`
- `memory_log.rs`
- `gateway_run_events.rs`
- `gateway_client.rs`
- `channels.rs`

必要改造：

- `crate::config::paths` 改为 `cc_config::paths`。
- `crate::config::features` 改为 `cc_config::features`。
- `super::supervisor::ASSISTANT_WORKER_ID` 改为 `crate::constants::ASSISTANT_WORKER_ID`。
- `process_state::try_run_management_command` 可以先保留，但其 `start_daemon` 仍启动当前 binary 的 `--daemon`，因为最终 binary 仍是 `claude-code-rs`。
- `gateway_client.rs` 中的 `crate::daemon::process_state` 改为 `crate::process_state`。
- `protocol.rs` 继续复用 `process_state::atomic_write_json`，或把 atomic write 下沉为 `fs_atomic.rs`。

`cc-daemon/Cargo.toml` 增加依赖：

```toml
[dependencies]
anyhow = { workspace = true }
chrono = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
tokio = { workspace = true }
reqwest = { workspace = true }
tracing = { workspace = true }
cc-config = { workspace = true }
cc-types = { workspace = true }
gateway = { workspace = true }
```

测试迁移：

- 把 `process_state.rs`、`protocol.rs`、`gateway_client.rs` 内部测试随文件迁到 `cc-daemon`。
- 保留 `serial_test`、`tempfile` dev-dependencies。

完成标准：

```bash
cargo test -p cc-daemon process_state
cargo test -p cc-daemon protocol
cargo test -p cc-daemon gateway_client
```

并且 root crate 仍可通过旧路径测试，或通过 shim 过渡。

## Phase 2：root crate 切换到 `cc-daemon` 协议层

目标：让 `claude-code-rs` 不再拥有 daemon 状态协议。

改动范围：

- `crates/claude-code-rs/src/main.rs`
  - `daemon::process_state::try_run_management_command` 改为 `cc_daemon::process_state::try_run_management_command`。
  - `daemon::process_state::write_started/write_stopped` 改为 `cc_daemon::process_state::*`。
- `crates/claude-code-rs/src/commands/daemon_cmd.rs`
  - 改用 `cc_daemon::process_state`。
- `crates/claude-code-rs/src/commands/sleep_cmd.rs`
  - 改用 `cc_daemon::process_state::write_sleep_state`。
- `crates/claude-code-rs/src/tools/exec/sleep.rs`
  - 改用 `cc_daemon::process_state::write_sleep_state`。
- `crates/claude-code-rs/src/commands/remote_cmd.rs`
  - 删除 `#[path = "../daemon/gateway_client.rs"]`。
  - 改用 `cc_daemon::gateway_client::LocalGatewayClient`。

过渡策略：

- 可以先在 `crates/claude-code-rs/src/daemon/process_state.rs` 和 `protocol.rs` 放 re-export shim：

```rust
pub use cc_daemon::process_state::*;
```

- 但 shim 只允许短期存在，下一阶段必须删除。

完成标准：

```bash
cargo test -p cc-daemon process_state
cargo test -p cc-daemon protocol
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs tools::exec::sleep
```

并且：

```bash
rg "crate::daemon::process_state|crate::daemon::protocol|#\\[path = \"../daemon/gateway_client.rs\"\\]" crates/claude-code-rs/src
```

不再命中需要旧实现的位置。

## Phase 3：定义 runtime adapter 边界

目标：为迁移 HTTP、worker 和 gateway bridge 做解耦准备。

当前强耦合点：

- `gateway_bridge.rs` 直接使用：
  - `crate::engine::lifecycle::QueryEngine`
  - `crate::types::config::{QueryEngineConfig, QuerySource}`
  - `crate::tools::registry`
  - `crate::tools::tool_search`
  - `crate::tools::hooks`
  - `crate::plugins`
  - `crate::commands`
- `routes.rs` 直接使用：
  - `crate::commands`
  - `crate::engine::sdk_types::SdkMessage`
  - `crate::types::plan_workflow::PlanWorkflowRecord`
  - `crate::plan_workflow`
- `webhook.rs` 直接使用：
  - `crate::tools::pr_activity`
  - `crate::teams`
- `team_memory_proxy.rs` 直接使用：
  - `crate::utils::git`
  - `crate::config::paths`

新增 `crates/cc-daemon/src/runtime.rs`：

```rust
use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub trait DaemonEngineRuntime: Send + Sync + 'static {
    fn submit_message(
        &self,
        text: String,
        source: DaemonQuerySource,
    ) -> Pin<Box<dyn futures::Stream<Item = DaemonSdkEvent> + Send>>;

    fn abort(&self);
    fn wake_up(&self);
    fn is_sleeping(&self) -> bool;
    fn status(&self) -> DaemonEngineStatus;
}

pub trait DaemonCommandRuntime: Send + Sync + 'static {
    fn parse_command(&self, raw: &str) -> Option<ParsedDaemonCommand>;
    fn execute_command<'a>(
        &'a self,
        command: ParsedDaemonCommand,
    ) -> Pin<Box<dyn Future<Output = Result<DaemonCommandOutput>> + Send + 'a>>;
}

pub trait DaemonWebhookRuntime: Send + Sync + 'static {
    fn route_github_pr_activity(
        &self,
        payload: &Value,
        event: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<DaemonWebhookDelivery>;
}

pub trait TeamMemoryRuntime: Send + Sync + 'static {
    fn github_repo_for_cwd(&self, cwd: &std::path::Path) -> Option<String>;
    fn team_memory_dir(&self, cwd: &std::path::Path) -> std::path::PathBuf;
}
```

配套数据类型：

- `DaemonSdkEvent`
- `DaemonQuerySource`
- `DaemonEngineStatus`
- `ParsedDaemonCommand`
- `DaemonCommandOutput`
- `DaemonWebhookDelivery`

原则：

- `DaemonSdkEvent` 是 daemon-facing event，不直接暴露 root crate `SdkMessage`。
- root crate adapter 负责把 `SdkMessage` 映射成 `DaemonSdkEvent`。
- `routes::sdk_message_to_sse` 后续改为 `daemon_event_to_sse`。

完成标准：

- `cc-daemon` 内不出现 `crate::engine`、`crate::tools`、`crate::commands`、`crate::teams`、`crate::plugins`。
- root crate 新增 adapter 模块，例如 `crates/claude-code-rs/src/daemon_runtime.rs` 或 `src/startup/daemon_runtime.rs`。

## Phase 4：迁移 supervisor 与 worker bridge

目标：把 worker lifecycle 放入 `cc-daemon`，把 root runtime 作为 adapter 注入。

迁移文件：

- `supervisor.rs`
- `gateway_bridge.rs`

改造要点：

- `WorkerKind` 先保留 `AssistantSession`。
- `ASSISTANT_WORKER_ID` 改用 `cc_daemon::constants`。
- `AssistantWorkerRuntime::new(&cwd)` 从 `cc-daemon` 中移除。
- `run_worker_mode` 接收 `Arc<dyn DaemonEngineRuntime>` 或 runtime factory。
- root binary 在 `--daemon-worker` fast path 中构造 adapter，再调用 `cc_daemon::supervisor::run_worker_mode(...)`。
- `handle_worker_command` 使用 `DaemonEngineRuntime` 执行 submit/abort。
- gateway run event 更新仍由 `cc-daemon` 负责。

建议 API：

```rust
pub trait DaemonWorkerRuntimeFactory: Send + Sync + 'static {
    fn assistant_runtime(&self, cwd: &std::path::Path) -> anyhow::Result<std::sync::Arc<dyn DaemonEngineRuntime>>;
}

pub async fn run_worker_mode(
    factory: std::sync::Arc<dyn DaemonWorkerRuntimeFactory>,
    kind: &str,
    worker_id: &str,
    cwd: std::path::PathBuf,
) -> anyhow::Result<()>;
```

完成标准：

```bash
cargo test -p cc-daemon supervisor
cargo test -p cc-daemon gateway_bridge
cargo test -p claude-code-rs daemon::supervisor
```

其中 root crate `daemon::supervisor` 测试可以先改为 adapter/shim 测试，最终删除。

## Phase 5：迁移 HTTP/SSE 控制面

目标：把 HTTP API、SSE、gateway route wiring 迁入 `cc-daemon`。

迁移文件：

- `state.rs`
- `server.rs`
- `routes.rs`
- `sse.rs`
- `gateway_routes.rs`
- `webhook.rs`
- `team_memory_proxy.rs`
- `notification.rs`
- `tick.rs`

关键改造：

- `DaemonState` 不直接持有 root crate `QueryEngine`，改持有 runtime adapters。
- `/api/submit` 继续 enqueue command，不直接提交到 supervisor process 内 engine。
- `/api/abort` enqueue abort command。
- `/api/command` 通过 `DaemonCommandRuntime` 执行。
- `/api/status` 汇总：
  - process state
  - worker state
  - command/event paths
  - engine adapter status
  - sleep state
  - connected SSE clients
- `/api/history` 返回真实 worker event history；如果要返回 session history，必须通过 runtime adapter 明确提供。
- `/api/resize` 保持明确 no-op 或改为 command protocol，不再模糊。
- `webhook.rs` 中 GitHub PR activity routing 通过 `DaemonWebhookRuntime`。
- `team_memory_proxy.rs` 中 repo/path resolution 通过 `TeamMemoryRuntime` 或下沉到 `cc-config`/`cc-utils` 后再直接依赖。
- `notification_consumer` 在 daemon 启动路径接入。

必须修正的既有缺口：

- SSE 当前只在连接时 replay worker event log。迁移时要增加 worker event tail/broadcast loop，否则 HTTP submit 后前端无法实时看到 worker 新写入的 stream events。

可选实现：

- `event_forwarder.rs` 周期性读取 `events/<worker-id>.ndjson` 新增 offset。
- 或在 worker append event 后通过 local event bus 通知 supervisor，但跨进程仍需要文件 tail 作为恢复路径。

完成标准：

```bash
cargo test -p cc-daemon routes
cargo test -p cc-daemon sse
cargo test -p cc-daemon gateway_routes
cargo test -p cc-daemon webhook
```

并补一个 live-ish 测试：

- enqueue submit command。
- worker append `stream_delta` event。
- SSE client 能收到新增 event，而不是只在重连时 replay。

## Phase 6：切换 daemon 启动入口

目标：root binary 只做入口编排，daemon 逻辑在 `cc-daemon`。

改动范围：

- `crates/claude-code-rs/src/main.rs`
  - `--daemon` 分支调用 `cc_daemon::run_daemon_supervisor(...)` 或 `cc_daemon::server::serve_http(...)`。
  - `--daemon-worker` 分支调用 `cc_daemon::supervisor::run_worker_mode(...)`。
  - root 只负责构造 runtime adapter/factory。
- `crates/claude-code-rs/src/cli.rs`
  - 参数保留在 root binary。
- `crates/claude-code-rs/src/commands/daemon_cmd.rs`
  - 只依赖 `cc_daemon::process_state`。
- `crates/claude-code-rs/src/commands/remote_cmd.rs`
  - 只依赖 `cc_daemon::gateway_client`。

完成标准：

```bash
cargo test -p cc-daemon
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs commands::remote_cmd
cargo test -p claude-code-rs tools::exec::sleep
```

本地 smoke：

```bash
CC_RUST_HOME="$(mktemp -d)" FEATURE_KAIROS=1 cargo run -p claude-code-rs -- daemon start --port 21983
cargo run -p claude-code-rs -- daemon status
TOKEN="$(cargo run -p claude-code-rs -- daemon token)"
curl -H "x-cc-rust-daemon-token: $TOKEN" http://127.0.0.1:21983/api/status
cargo run -p claude-code-rs -- daemon stop
```

## Phase 7：删除旧 daemon 模块

目标：移除 root crate 中的旧实现，避免双实现漂移。

工作：

- 删除 `crates/claude-code-rs/src/daemon/**`。
- 删除 `mod daemon;`。
- 如仍需要兼容路径，使用 `use cc_daemon as daemon;`，但不保留旧目录。
- 更新文档中的路径：
  - `docs/plan/daemon-usability-plan.md`
  - `docs/reference/DAEMON_OPERATIONS.md`
  - 其他引用 `crates/claude-code-rs/src/daemon/**` 的计划或报告。
- 更新发布验证命令，从 `cargo test -p claude-code-rs daemon::...` 改为 `cargo test -p cc-daemon ...`。

检查命令：

```bash
rg "crates/claude-code-rs/src/daemon|mod daemon|crate::daemon|#\\[path = \"../daemon" crates docs
```

完成标准：

- 不再存在旧 daemon 目录。
- 所有 daemon 单元测试归属 `cc-daemon`。
- root crate 中只剩 adapter 和入口 wiring。

## 验收矩阵

### 每阶段必跑

```bash
cargo fmt --all --check
cargo test -p cc-daemon
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs tools::exec::sleep
```

### 迁移完成后补跑

```bash
cargo test -p claude-code-rs commands::remote_cmd
cargo test -p claude-code-rs --test e2e_cli daemon_management_reports_stopped_state_without_running_daemon
cargo clippy -p cc-daemon --all-targets -- -D warnings
```

### 干净工作区发布前补跑

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

如果 workspace clippy/test 被非 daemon 既有问题阻塞，需要在迁移记录中列出阻塞文件和错误，不应把它们归为 cc-daemon 迁移回归。

## 风险与处理

### 风险：`cc-engine` 尚未完全承载 root `engine`

`cc-engine` 当前仍是 Phase 6 incremental extraction，`QueryEngine` 公开边界未完全稳定。不要让 `cc-daemon` 直接绑定 root `QueryEngine`。

处理：

- Phase 3 先做 runtime trait。
- root crate 实现 adapter。
- 等 `cc-engine` 完成后，再考虑把 adapter 简化为直接依赖 `cc-engine` API。

### 风险：HTTP supervisor 和 assistant worker 双 engine

当前 `--daemon` 分支创建一个 `QueryEngine`，assistant worker 也创建自己的 `QueryEngine`。迁移时如果不明确所有权，容易出现状态不一致。

处理：

- `/api/submit`、`/api/abort`、permission 一律走 worker command。
- supervisor process 的 engine 只用于 status/command runtime 时必须明确职责，最终应尽量移除。

### 风险：SSE 只 replay 不 tail

当前 SSE 连接时 replay worker event log，但 worker 新增事件不会自动 broadcast 到已连接 client。

处理：

- Phase 5 必须实现 event tailer。
- 增加测试覆盖 worker event append 后 SSE client 可收到新事件。

### 风险：root command registry 仍在 root crate

`routes.rs` 中 `/api/command` 需要 slash command registry，当前 registry 在 root crate。

处理：

- 通过 `DaemonCommandRuntime` adapter 调用 root command registry。
- 不把 root `commands::get_all_commands()` 迁入 `cc-daemon`。

### 风险：webhook deliver-only 依赖 PR activity 和 teams mailbox

这些能力还在 root crate。

处理：

- 通过 `DaemonWebhookRuntime` adapter。
- 或先把 PR activity/team mailbox 拆到 `cc-teams`/`cc-tools` 后再让 `cc-daemon` 依赖稳定 crate。

### 风险：team memory proxy 依赖 Bun 和路径推导

`team_memory_proxy.rs` 现在同时做 subprocess lifecycle、repo 解析、path resolution、HTTP proxy。

处理：

- 先迁移 proxy HTTP 逻辑。
- repo/path resolution 通过 `TeamMemoryRuntime` adapter 或移到 `cc-config`/`cc-utils`。

## 推荐执行顺序

优先做 Phase 1 和 Phase 2。这两步风险最低，能让 `cc-daemon` 从 scaffold 变成真实协议 crate，并减少 root daemon 的持久化状态职责。

随后执行 Phase 3。只有 runtime adapter 边界清楚后，才迁移 supervisor、worker、HTTP/SSE 和 webhook，否则会形成 `cc-daemon` 对 root crate 私有模块的隐式依赖。

最后执行 Phase 7 删除旧目录，避免长期双实现。
