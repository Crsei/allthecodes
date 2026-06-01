# Daemon 操作与发布检查

> 状态日期：2026-05-17
> 范围：`crates/cc-daemon/**`、daemon CLI 管理命令、KAIROS HTTP/SSE 控制面。

## 当前可用能力

Rust 端 daemon 现在已经从单进程 `--daemon` HTTP demo 推进为可由外部 CLI 管理的后台进程形态：

- supervisor 状态写入 `~/.cc-rust/daemon/supervisor.json`。
- worker 状态写入 `~/.cc-rust/daemon/workers/<worker-id>.json`。
- command 文件写入 `~/.cc-rust/daemon/commands/<worker-id>/<command-id>.json`。
- worker event 写入 `~/.cc-rust/daemon/events/<worker-id>.ndjson`。
- control token 写入 `~/.cc-rust/daemon/control-token.json`，daemon stop 时清理。
- sleep state 写入 `~/.cc-rust/daemon/sleep-state.json`，过期、wake 或 stop 时清理。

所有 daemon 持久化状态必须继续留在 `~/.cc-rust/daemon/`，不能写入上游 Claude/Codex 路径。

## CLI 管理命令

daemon 管理命令是首选入口；`--daemon` 仍是隐藏运行面。

```bash
FEATURE_KAIROS=1 claude-code-rs daemon start
FEATURE_KAIROS=1 claude-code-rs daemon start --port 19837
claude-code-rs daemon status
claude-code-rs daemon token
claude-code-rs daemon submit "hello"
claude-code-rs daemon abort
claude-code-rs daemon command <command-id> [worker-id]
claude-code-rs daemon events [worker-id]
claude-code-rs daemon sleep 60 "pause proactive"
claude-code-rs daemon wake
claude-code-rs daemon stop
claude-code-rs daemon restart --port 19837
```

`daemon start` 需要 `FEATURE_KAIROS=1`。`daemon status/stop/restart` 可以从另一个 CLI 进程操作同一个后台 supervisor。

## Slash 命令

`/daemon` 是会话内的轻量入口：

- `/daemon` 或 `/daemon status`：读取跨进程 supervisor/worker 状态。
- `/daemon stop`：写入 shutdown request，让后台 supervisor 优雅退出。
- `/daemon start` 与 `/daemon restart`：只提示使用 shell 管理命令，不在当前 REPL 内 fork 后台进程。

`/sleep <seconds>` 会写入 daemon sleep state，使 proactive tick 与 `/api/status` 都能观察到同一份休眠状态。

## HTTP 控制面

daemon 默认监听 `127.0.0.1:19836`，可通过 `--port` 调整。

- `GET /health`：健康检查。
- `GET /api/status`：返回 QueryEngine、supervisor、workers、command root、event log、daemon sleep state。
- `GET /api/history`：返回当前 SSE buffer 与 daemon worker event log。
- `GET /events`：SSE stream，连接时 replay assistant worker event log。
- `POST /api/submit`：只投递 `Submit` command；assistant worker claim 后拥有 QueryEngine 执行和 worker event log。
- `POST /api/abort`：只投递 `Abort` command；assistant worker 执行 abort 并写入 `abort_ack` event。
- `POST /api/permission`：投递 `PermissionResponse` command 并持久化 ack；尚未完整恢复到 live permission waiter。
- `POST /api/command`：执行 slash command。
- `POST /api/resize`：当前明确为 no-op。

所有 mutating endpoint 都必须带 token：

```bash
TOKEN=$(claude-code-rs daemon token)
curl -H "x-cc-rust-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -d '{"decision":"allow","tool_use_id":"example"}' \
  http://127.0.0.1:19836/api/permission
```

也可以使用 `Authorization: Bearer <token>`。

## 发布验证命令

Phase 1-6 已经用目标测试和本地 smoke 验证过核心路径。发布前至少运行：

```bash
cargo test -p cc-config partition_functions_all_root_under_data_root --lib
cargo test -p cc-daemon protocol
cargo test -p cc-daemon routes
cargo test -p cc-daemon supervisor
cargo test -p cc-daemon gateway_bridge
cargo test -p claude-code-rs commands::daemon_cmd
cargo test -p claude-code-rs --test e2e_cli daemon_management_reports_stopped_state_without_running_daemon
```

仍应在干净工作区补跑完整门槛：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

2026-05-17 当前专项结论：

- `/api/submit`、`/api/abort` 和 `/api/permission` route 不再直接执行 assistant QueryEngine submit/abort/permission response；route 只做 token 校验、command enqueue 和事件提示。
- `AssistantWorkerRuntime` claim command 后执行 submit/abort，并把 `submit_started`、SDK-derived SSE event、`submit_completed`、`abort_ack` 写入 worker event log。
- 本轮验证覆盖共享 engine lifecycle：`cargo test -p cc-engine lifecycle -- --nocapture`。

不要把 workspace 级非 daemon 失败误判为 daemon 专项回归；daemon 目标测试集应以本节上方命令为准。

## 仍未完成

以下能力尚未达到完整上游 parity：

- permission response command 当前只有 durable ack，尚未接回 live permission waiter/replay 队列。
- resize 仍是明确 no-op；history 只返回 SSE buffer 与 worker event log，尚未形成 worker-owned history/resize DTO。
- bridge worker 尚未接入 remote-control server 注册、远程 submit/abort/permission 映射和结果回传。
- scheduler/proactive worker 尚未独立化，cron-style task 与 daily log 结构化双轨仍未接入。
- 30 分钟以上 daemon soak、worker 崩溃自动 e2e、HTTP submit + SSE result live e2e 仍需在有模型凭据和干净工作区时执行。
