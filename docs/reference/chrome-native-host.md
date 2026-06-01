# Claude in Chrome — 原生版传输层

> 本文档对应 Issue #4 + #5。`browser MCP` 的外部集成在
> [`browser-mcp-config.md`](browser-mcp-config.md)，请先读那篇。

## 总览

原生版 Chrome 集成由三个进程 + 一段共享的 socket/pipe 组成：

```
┌──────────────────────┐   JSON-RPC over stdio   ┌────────────────────────┐
│ cc-rust (MCP client) │ ──────────────────────▶ │ --claude-in-chrome-mcp │
└──────────────────────┘                         │   (MCP bridge)         │
                                                 └──────────┬─────────────┘
                                                            │
                                      4-byte-framed JSON    │
                                      over Unix socket /    │
                                      Windows named pipe    │
                                                            ▼
                                                 ┌────────────────────────┐
                                                 │ --chrome-native-host   │
                                                 │ (native messaging host)│
                                                 └──────────┬─────────────┘
                                                            │
                                      Chrome native         │
                                      messaging (stdio,     │
                                      4-byte-framed JSON)   │
                                                            ▼
                                                 ┌────────────────────────┐
                                                 │  Anthropic Chrome      │
                                                 │  extension             │
                                                 └────────────────────────┘
```

这三个进程都是同一个 cc-rust 二进制，靠隐藏 flag 切模式：

| 进程 | 启动者 | Flag | 职责 |
|------|--------|------|------|
| cc-rust 主进程 | 用户 | `--chrome` | 展示 UI、管理 MCP、生成工具调用 |
| MCP bridge | cc-rust 主进程通过 MCP manager fork | `--claude-in-chrome-mcp` | 把 MCP stdio 翻译成 socket 上的 JSON |
| Native host | Chrome 拉起 | `--chrome-native-host` | 把 Chrome 的 native messaging 翻译成 socket 上的 JSON |

## Socket 路径

| 平台 | 路径 |
|------|------|
| Unix | `/tmp/claude-mcp-browser-bridge-{user}/{pid}.sock`，权限 `0600` |
| Windows | `\\.\pipe\claude-mcp-browser-bridge-{user}` |

Unix 下每个 native host 进程开自己的 `.sock` 文件；MCP bridge
`connect_native_host()` 会扫目录里所有 `*.sock`，挑第一个能连上的。这就允许一台机器
上同时有多个 Chrome 实例运行（多账号、Dev/Stable 并存），各自有自己的 bridge。

Windows 下命名管道是全局的，一次只能有一个 server 持有同名 pipe；多实例需要在
后续版本改为 per-PID 后缀。

## Chrome ↔ Native host 帧格式

Chrome 的 native messaging 协议：

```
 ┌────────┬──────────────────────────────────────────┐
 │ len32  │ UTF-8 JSON 负载，长度 = len32            │
 └────────┴──────────────────────────────────────────┘
```

- `len32` 是小端序 `uint32`
- 最大 1 MiB（`transport::MAX_MESSAGE_SIZE`）；超出直接关闭链接，Chrome 会重拉
- 方向双向：Chrome → native host 写到我们的 stdin；native host → Chrome 写到 stdout

Native host 处理的 Chrome 消息 `type` 字段：

| `type` | 方向 | 说明 |
|--------|------|------|
| `ping` | Chrome → host | 健康探测，host 以 `pong` 回应 |
| `pong` | host → Chrome | 含 `timestamp` |
| `get_status` | Chrome → host | host 回 `status_response { native_host_version, mcp_client_count }` |
| `mcp_connected` | host → Chrome | 有 bridge 连到 socket 时 |
| `mcp_disconnected` | host → Chrome | bridge 断开时 |
| `tool_request` | host → Chrome | bridge 传过来的调用 |
| `tool_response` | Chrome → host | 扩展处理完的响应；host 转发给 bridge |
| `notification` | Chrome → host | 扩展主动推送；host 转发给 bridge |
| `error` | 双向 | 错误消息 |

## MCP bridge ↔ Native host 帧格式

同样的 4 字节小端长度 + UTF-8 JSON。这条链路上不走 `type` 字段：

- **Bridge → host**: 直接发 `{ method, params, request_id }`。Host 在转给 Chrome 时
  包一层 `{ type: "tool_request", ... }`。
- **Host → bridge**: Host 从 Chrome 收到 `tool_response` / `notification` 后，
  **去掉 `type` 字段**，把剩下的 `{ request_id, result, ... }` 原样转发。Bridge
  用 `request_id` 路由到正确的 oneshot 等待者。

## MCP bridge 暴露的工具

`mcp_bridge::tool_catalogue()` 定义了一套跟 bun 参考实现对齐的核心工具集：

- `navigate`（导航）
- `tabs_context_mcp` / `tabs_create_mcp`（标签管理）
- `get_page_text`（读取页面文本）
- `click` / `form_input`（交互）
- `javascript_tool`（执行 JS）
- `read_console_messages` / `read_network_requests`（观测）

Input schema 故意放宽——实际字段名要等扩展回来确认。这些工具都走 bridge → host → 扩展的完整链路。

## /chrome reconnect 流程

1. 用户运行 `/chrome reconnect`
2. `ChromeSession::reconnect()` 再跑一遍 `start()`：
   - `setup::detect_extension_installed()` 重新扫磁盘
   - `setup::install_native_host_manifest()` 检查并写 manifest（idempotent，内容没变就跳过）
3. 状态回到 `Enabled`
4. 用户打开 https://clau.de/chrome/reconnect，Chrome 扩展检测到配置变动自动重连
5. 扩展再次连上 native host 后，我们已经在 socket 上挂着 bridge，工具重新可用

目前 `/chrome reconnect` **不会**主动重启 bridge 子进程（MCP manager 的子进程
生命周期跟着主进程走）。如果 bridge 需要重启，重启 cc-rust 主进程是最稳的路径。

## 没做 / 等后续

- **Bridge-via-WebSocket**（bun 里的 `tengu_copper_bridge` 路径，走
  `wss://bridge.claudeusercontent.com`）：还没实现。native 路径要求用户本地装扩展，
  对远端 dev env 不友好，bun 就是这么兜底的。
- **OAuth pairing flow**：扩展跟 bridge 之间的 OAuth token 对账，本实现默认走纯
  native，不做 OAuth。
- **Tool schema 校验**：`mcp_bridge::tool_catalogue()` 里的 input schema 没跟真实扩展端对过
  （那些定义住在 Anthropic 私有包 `@ant/claude-for-chrome-mcp` 里）。首次跑通可能要调。
- **GIF 录制、多 profile 切换、多浏览器切换**：Issue #5 明确说不是第一版目标。

## 手动调试 tips

- **只看 native host 有没有被 Chrome 拉起**：`tail -F ~/.cc-rust/logs/cc-rust.log.*`
  （`--chrome-native-host` 路径走正常的 tracing，会进这个文件）。
- **socket 是不是开着**：`ls /tmp/claude-mcp-browser-bridge-$USER/`，每个 native host
  进程一个 `.sock`。
- **bridge 是不是连上了 socket**：正常情况下 `/mcp list` 里 `claude-in-chrome`
  会标 `[browser]` 且 `tools_count > 0`。
- **扩展到底有没有发消息过来**：`--verbose` 启动，grep `chrome-native-host:`。

## 已知边界

- Native host 用 `tracing` 打日志时会把 warn/error 级别也写到 stderr——Chrome 会把
  这些当错误上报。目前靠 filter 把 stderr level 拉到 warn 避免噪音；后续可能要
  完全静音 stderr。
- MCP bridge 的 stdio JSON-RPC 实现是**最小可用**版本——只实现了 `initialize`、
  `tools/list`、`tools/call` 三个方法。其他（resources、prompts、notifications/*）
  返回 -32601 method not found。
- 一个 bridge 进程只连一个 native host（首次 `connect_native_host()` 成功之后
  不再尝试 failover）。如果 native host 中途挂掉，bridge 直接退出，MCP 层会把
  `claude-in-chrome` server 标为 disconnected，用户需要重启 cc-rust。
