# Local Socket Transport 执行计划

> 计划日期：2026-06-17
> 依赖：Phase 2 `TransportEvent`，Phase 3 `OutboundRouter` / replay / lag

## 目标

新增跨平台本地传输，让本机客户端不必依赖 TCP loopback 端口。Unix 使用 Unix domain socket；Windows 使用 named pipe 等价传输。两者都必须进入统一 transport event 层。

## Public Interface

- 新增 `TransportKind::LocalSocket`。
- 新增 listen 形态：
  - `local://auto`
  - `local://path=<path>`（Unix）
  - `local://pipe=<name>`（Windows）
  - `all://web=...,daemon=...,local=auto`
- local transport origin 固定为 `ConnectionOrigin::LocalNative`。
- local transport 承载现有 JSON-RPC frame，不新增独立 RPC method。

## 路径与隔离

- Unix:
  - socket 文件放在 `/tmp/allthecodes-{user}/` 下。
  - discovery metadata 写入 `~/.allthecodes/daemon/local-transport.json`。
  - socket 目录权限设为 `0700`，socket 文件 best-effort 设为 `0600`。
- Windows:
  - pipe 名称使用 `\\.\pipe\allthecodes-{user}-{hash}`。
  - discovery metadata 同样写入 `~/.allthecodes/daemon/local-transport.json`。
- 不使用 `~/.Codex/`、`.Codex/` 或 `"Codex"` keychain service。

## Implementation Notes

- Unix listener:
  - bind 前清理 stale socket。
  - accept 后用 WebSocket-over-UDS framing，复用 JSON-RPC dispatch。
  - guard 在 drop 时删除 socket 文件。
- Windows listener:
  - 使用 `tokio::net::windows::named_pipe`。
  - 每个连接创建一个 named pipe server instance。
  - frame 格式与 JSON-RPC WebSocket 语义一致；如无法直接复用 tungstenite，建立最小 length-delimited JSON frame adapter，但输出仍进入 `TransportEvent`。
- startup:
  - `ServerMode::All` 先完成所有 listener bind，再 spawn serve loop。
  - local bind 失败时不留下部分启动 server。
  - readiness diagnostics 输出 local path/pipe。
- shutdown:
  - graceful shutdown 关闭 listener。
  - `OutboundRouter::DisconnectAll` 断开连接。
  - Unix guard 清理 socket 文件。

## Tests

- `cargo test -p allthecodes-server`
  - listen URL parse、duplicate local entries、all mode validation。
- `cargo test -p allthecodes-web`
  - local transport JSON-RPC dispatch 与 WebSocket dispatch 等价。
  - connection open/message/close event。
  - slow local client 不阻塞其他连接。
- Platform tests:
  - Unix：stale socket cleanup、socket file guard cleanup。
  - Windows：named pipe listener accepts multiple sequential clients。
- Manual verification:
  - `--listen local://auto`
  - `--listen all://web=127.0.0.1:17322,daemon=127.0.0.1:19836,local=auto`
