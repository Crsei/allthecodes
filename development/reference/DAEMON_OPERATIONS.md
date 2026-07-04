# Daemon 操作与发布检查

> 状态日期：2026-07-04
> 范围：`crates/allthecodes-daemon/**`、root `allthecodes` daemon CLI、KAIROS HTTP/SSE 控制面。

## 当前可用能力

KAIROS daemon 已是可由外部 CLI 管理的后台进程形态。所有持久化状态必须留在 allthecodes 隔离数据根：

- 默认数据根：`~/.allthecodes/`
- 测试/临时数据根：`ALLTHECODES_HOME=<dir>`
- 项目配置：`<repo>/.allthecodes/settings.json`
- 禁止写入上游 Claude/Codex 持久化路径，如 `~/.Codex/`、`.Codex/`、旧 `~/.cc-rust/`。

daemon 状态位于 `{ALLTHECODES_HOME:-~/.allthecodes}/daemon/`：

- `supervisor.json`：supervisor PID、端口、ready URL、worker 摘要和 shutdown 标记。
- `workers/<worker-id>.json`：worker PID、状态、日志路径和重启次数。
- `commands/<worker-id>/<command-id>.json`：durable command DTO。
- `events/<worker-id>.ndjson`：worker event log，`/events` 连接时会 replay。
- `control-token.json`：mutating HTTP endpoint token，stop 时清理。
- `sleep-state.json`：proactive/scheduler sleep state，过期、wake 或 stop 时清理。

固定 worker IDs：

- `assistant-session-1`
- `bridge-sync-1`
- `proactive-1`
- `scheduler-1`

## CLI 管理命令

`daemon start` 和隐藏 `--daemon` 运行面需要 `FEATURE_KAIROS=1`。管理命令可以从另一个 CLI 进程操作同一个后台 supervisor。

```bash
FEATURE_KAIROS=1 allthecodes daemon start
FEATURE_KAIROS=1 allthecodes --port 19837 daemon start

allthecodes daemon status
allthecodes daemon token
allthecodes daemon submit "hello"
allthecodes daemon abort
allthecodes daemon command <command-id> [worker-id]
allthecodes daemon events [worker-id]
allthecodes daemon sleep 60 "pause proactive"
allthecodes daemon wake
allthecodes daemon stop
FEATURE_KAIROS=1 allthecodes --port 19837 daemon restart
```

`daemon stop` writes a shutdown request and the daemon runtime now observes that request directly, so normal stop should not wait for the fallback terminate grace period.

## Slash 命令

`/daemon` 是会话内轻量入口：

- `/daemon` 或 `/daemon status`：读取跨进程 supervisor/worker 状态。
- `/daemon stop`：写入 shutdown request，让后台 supervisor 优雅退出。
- `/daemon start` 与 `/daemon restart`：提示使用 shell 管理命令，不在当前 REPL 内 fork 后台进程。

`/sleep <seconds>` 写入 daemon sleep state；`/api/status`、proactive worker 和 scheduler worker 读取同一份状态。

## HTTP 控制面

daemon 默认监听 `127.0.0.1:19836`，可通过 `--port` 调整。

- `GET /health`、`GET /healthz`、`GET /readyz`、`GET /startupz`：探针。
- `GET /api/status`：返回 QueryEngine/KAIROS flags、automation state、supervisor、workers、command root、assistant event log、sleep state。
- `GET /api/history`：返回 current history、history snapshots 和 daemon worker event log。
- `GET /events`：SSE stream，连接时 replay assistant worker event log。
- `POST /api/submit`：投递 `Submit` command；assistant worker claim 后拥有 QueryEngine 执行和 worker event log。
- `POST /api/abort`：投递 `Abort` command；assistant worker 执行 abort 并写入 `abort_ack` event。
- `POST /api/permission`：投递 `PermissionResponse` command，并接回 live permission response path。
- `POST /api/command`：执行 slash command。
- `POST /api/resize`：更新 daemon-visible resize DTO。
- gateway/channel endpoints：通过 allthecodes local gateway/protocol 映射本地 channel、bridge session、remote run 能力；不把 `/api/*` 直接声明为公网 remote-control API。

所有 mutating endpoint 都必须带 token：

```bash
TOKEN="$(allthecodes daemon token)"
curl -H "x-allthecodes-daemon-token: $TOKEN" \
  -H "content-type: application/json" \
  -d '{"text":"hello"}' \
  http://127.0.0.1:19836/api/submit
```

也可以使用 `Authorization: Bearer <token>`。

## 验证命令

本任务的默认发布检查：

```bash
cargo fmt --all -- --check
cargo test -p allthecodes-config partition_functions_all_root_under_data_root --lib
cargo test -p allthecodes-daemon protocol
cargo test -p allthecodes-daemon routes
cargo test -p allthecodes-daemon supervisor
cargo test -p allthecodes-daemon gateway_bridge
cargo test -p allthecodes-engine system_prompt
cargo test -p allthecodes-tools sleep_tool
cargo test -p allthecodes --test e2e_cli
cargo check --workspace
cargo build --workspace --release
```

`crates/allthecodes/tests/e2e_cli.rs` 使用临时 `ALLTHECODES_HOME`、假 Anthropic-compatible key、`ANTHROPIC_BASE_URL=http://127.0.0.1:9` 和随机端口，覆盖 stopped status、start/readiness、status DTO、history DTO、submit command ownership、sleep state 和 graceful stop，不需要真实模型凭据或外网。

## Live Smoke

可选脚本：`development/reference/kairos-live-smoke.sh`

```bash
ALLTHECODES_BIN=target/release/allthecodes \
PORT=19846 \
TIMEOUT_SECS=120 \
PROMPT="KAIROS live smoke: reply with exactly 'kairos live smoke ok'." \
development/reference/kairos-live-smoke.sh
```

该脚本会启动 daemon、读取 token、`POST /api/submit`、轮询 `/api/history` 直到 terminal event，并验证 `/events` SSE replay。它需要真实可用的 provider 凭据、provider 网络访问和本机可用端口，因此不属于默认自动验证门禁；缺少凭据或网络时应记录为 skipped，而不是失败。

2026-07-04 closeout：本轮未运行 live smoke；原因是该脚本会触发真实 provider 调用，需要明确可用的模型凭据和外网访问。默认验证以离线 E2E 覆盖 daemon start/status/submit/sleep/stop。

## 当前 intentional deviations

- allthecodes 使用本地 gateway/protocol 作为 Bridge/GrowthBook 类能力的收口面；除非后续产品契约要求，不直接复制上游公网 Bridge/GrowthBook 行为。
- Telegram/Lark 入站会话、schedule remote trigger 和完整公网 remote-control exposure 仍登记为 intentional/deferred scope；当前只承诺本地 daemon/gateway control plane 和 outbound adapter status/test-message。
