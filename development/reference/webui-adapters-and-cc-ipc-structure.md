# WebUI adapters and cc-ipc structure notes

本文记录对两个外部 WebUI 项目的只读探查结论，并给出
`crates/cc-ipc` 面向后续 Rust WebUI / gateway 的结构建议。

探查对象：

- Hermes Web UI: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/hermes-web-ui`
- CloudCLI / Claude Code UI: `/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claudecodeui`
- cc-rust IPC: `crates/cc-ipc`、`crates/cc-ipc-protocol`、`crates/cc-ipc-client`

## 1. Executive summary

两套 WebUI 都没有把浏览器直接接到原始 CLI stdout 上作为主聊天协议。
它们都在服务端建立了一个适配层，把 provider / agent / gateway 的事件
归一成前端可消费的 run、message、tool、approval、usage、complete
等事件。

主要差异：

| 维度 | hermes-web-ui | claudecodeui |
| --- | --- | --- |
| 后端形态 | Koa BFF + Hermes Gateway manager + Python agent bridge | Express + provider registry + WebSocket gateway |
| 主聊天传输 | Socket.IO namespace `/chat-run` | 原生 WebSocket `/ws` |
| Claude/Hermes 语义运行 | Hermes Gateway `/v1/responses` SSE 或 Python bridge in-process `AIAgent` | Claude 走 `@anthropic-ai/claude-agent-sdk` async iterator |
| CLI/PTY | Web terminal 单独走 WebSocket + `node-pty` | `/shell` 单独走 WebSocket + `node-pty` |
| 会话状态 | Web UI SQLite 会话库 + 内存 `SessionState` | SQLite session/project index + provider 原生历史文件 |
| 历史来源 | Web UI 自建消息库，Hermes `state.db` 主要只读参考 | `~/.claude`、`~/.codex`、`~/.gemini` 等 provider 原生文件 |
| 事件模型 | `run.started`、`message.delta`、`tool.*`、`approval.*`、`compression.*` 等 Socket.IO events | provider-native event -> `NormalizedMessage { kind, provider, sessionId, ... }` |
| 权限交互 | bridge 工具审批发 `approval.requested`，前端回 `approval.respond` | Claude SDK `canUseTool` 发 `permission_request`，前端回 `claude-permission-response` |

对 cc-rust 的核心建议：

1. 不要把 `cc-ipc` 改成 Socket.IO 或某个 JS WebUI 的松散协议。
   保留 Rust 强类型协议，外层增加 transport adapter。
2. `cc-ipc-protocol` 需要一个统一 event envelope，覆盖 JSONL、SSE、
   WebSocket、daemon gateway 的共同字段。
3. Web/daemon/headless/gateway 不应各自映射 raw `SdkMessage`。
   应统一从 `SdkMessage` 映射到 `cc-ipc-protocol` 事件，再由 transport
   编码。
4. 会话、turn、pending approval/question、cancel token 必须成为一等概念。
   当前只按 `tool_use_id` 或单进程状态管理，不足以支撑多 session WebUI。
5. 语义聊天通道与 PTY 终端通道必须分离。PTY 可以存在，但不能混入
   conversation event stream。

## 2. hermes-web-ui adaptation

### 2.1 后端进程与边界

关键文件：

- `bin/hermes-web-ui.mjs`
- `packages/server/src/index.ts`
- `packages/server/src/services/hermes/gateway-manager.ts`
- `packages/server/src/routes/hermes/proxy-handler.ts`
- `packages/server/src/services/hermes/run-chat/index.ts`
- `packages/server/src/services/hermes/run-chat/handle-api-run.ts`
- `packages/server/src/services/hermes/run-chat/handle-bridge-run.ts`
- `packages/server/src/services/hermes/agent-bridge/{manager.ts,client.ts,hermes_bridge.py}`
- `packages/server/src/db/hermes/{session-store.ts,schemas.ts}`

整体结构：

```text
Browser
  -> Koa REST / Socket.IO / WebSocket
  -> Web UI server
      -> Hermes Gateway HTTP + SSE
      -> Hermes CLI execFile/spawn
      -> Python agent bridge over local newline JSON socket
      -> node-pty terminal
```

主要边界：

- 浏览器到 Web UI:
  - REST `/api/*`
  - Socket.IO `/chat-run`
  - WebSocket `/api/hermes/terminal`
  - WebSocket `/api/hermes/kanban/events`
- Web UI 到 Hermes Gateway:
  - HTTP API
  - SSE streaming, especially `/v1/responses`
- Web UI 到 Hermes CLI:
  - `execFile` / `spawn` for profile, gateway, sessions, logs, config
- Web UI 到 Python bridge:
  - Unix socket `ipc:///tmp/hermes-agent-bridge.sock`
  - Windows TCP `tcp://127.0.0.1:18765`
  - newline-delimited JSON request/response

Hermes Web UI 的主聊天不是 CLI stdout proxy。默认路径是服务端向
Hermes Gateway 发 `/v1/responses`，消费上游 SSE，再向浏览器发
Socket.IO 事件。另一个 `source=cli` 路径通过 Python bridge 在进程内
创建 Hermes `AIAgent`，Node 轮询 bridge `get_output` 并映射事件。

### 2.2 会话与流式协议

服务端维护 Web UI 自建 SQLite `sessions/messages` 表，同时在内存中维护
`SessionState`：

```text
session_id
  -> messages
  -> isWorking / isAborting
  -> events
  -> queue
  -> runId / activeRunMarker
  -> abortController
  -> source: api_server | cli
  -> profile
```

Socket.IO `/chat-run` 入站动作：

- `run`
- `resume`
- `abort`
- `cancel_queued_run`
- `approval.respond`

常见出站事件：

- `resumed`
- `run.started`
- `run.queued`
- `message.delta`
- `reasoning.delta`
- `thinking.delta`
- `reasoning.available`
- `tool.started`
- `tool.completed`
- `approval.requested`
- `approval.resolved`
- `compression.started`
- `compression.completed`
- `usage.updated`
- `abort.started`
- `abort.completed`
- `run.completed`
- `run.failed`
- `session.command`

重要行为：

- 同一个 `session_id` 同时只跑一个 run。
- 新 run 在当前 session 正忙时进入 per-session queue。
- `resume` 会 join `session:<id>` room，并返回已持久化消息、运行状态、
  queue length 和未完成事件。
- `activeRunMarker` 用来抑制旧流或 abort 后的迟到 terminal event。
- bridge 路径把工具、reasoning、approval、compression 请求都转成
  WebUI 事件，而不是把它们塞进文本。

### 2.3 前端消费方式

关键文件：

- `packages/client/src/api/client.ts`
- `packages/client/src/api/hermes/chat.ts`
- `packages/client/src/stores/hermes/chat.ts`
- `packages/client/src/views/hermes/ChatView.vue`
- `packages/client/src/components/hermes/chat/ChatPanel.vue`
- `packages/client/src/components/hermes/chat/ChatInput.vue`
- `packages/client/src/components/hermes/chat/MessageList.vue`
- `packages/client/src/components/hermes/chat/MessageItem.vue`
- `packages/client/src/components/hermes/chat/TerminalPanel.vue`

前端结构清晰：

- API client 只处理 REST token、base URL、active profile header。
- `api/hermes/chat.ts` 只处理 Socket.IO 连接和按 `session_id` 分发事件。
- Pinia `chat` store 是 UI 状态权威，负责 sessions、active session、
  stream state、server working state、pending approvals、queue length。
- 组件只消费 store 中的可渲染消息。

这个模式值得借鉴：transport client 不直接操作组件树，而是把事件归一到
session-scoped store。

### 2.4 配置、认证、权限、恢复

认证：

- `AUTH_TOKEN` 或自动生成 token 到 `HERMES_WEB_UI_HOME/.token`
- `AUTH_DISABLED=1` 可关闭认证
- REST 使用 bearer token
- Socket.IO 使用 handshake `auth.token`
- terminal WebSocket 使用 query token

配置：

- Hermes profile 写入 `~/.hermes/config.yaml`、`.env`
- `safe-file-store.ts` 做串行化、临时文件替换、可选备份
- `GatewayManager` 负责 profile gateway 的端口分配、PID 检测、health check

权限：

- bridge 设置执行审批环境，工具执行前发 `approval.requested`
- 前端回 `once/session/always/deny`
- Web UI 自身没有完整沙箱模型，文件和 terminal 能力需要单独约束

恢复：

- gateway transient error 会 wait-ready 后重试一次
- run marker 防止旧流串包
- `resume` 恢复正在运行的 session view
- shutdown 时按配置决定是否停止托管 gateway，并关闭 bridge、Socket.IO、
  HTTP server、SQLite

### 2.5 可借鉴与风险

可借鉴：

- run/message/tool/approval/usage/complete 的事件语义。
- session room + resume + per-session queue。
- BFF 管理 agent/gateway 进程，不让浏览器直接碰本地进程。
- 工具审批作为结构化事件。
- profile 隔离、端口分配、PID/health check。
- 持久会话库 + 内存运行态组合。

不建议照搬：

- Socket.IO 不是 cc-rust 核心 IPC 的理想基础。Rust 内核应保持 typed
  JSONL/SSE/WebSocket 多 transport。
- Python bridge 的 100ms 轮询不适合高质量 Rust gateway。应采用 push stream、
  cancel token、backpressure。
- `workspace` 只注入 prompt 不够。cc-rust 应把 cwd、project root、sandbox
  写入真实执行上下文。
- catch-all proxy `/api/hermes/*` 边界较模糊。cc-rust gateway 应使用显式路由。
- Web terminal 和文件写入面大，必须绑定权限、审计和开关。

## 3. claudecodeui adaptation

### 3.1 后端进程与边界

关键文件：

- `server/index.js`
- `server/modules/websocket/services/websocket-server.service.ts`
- `server/modules/websocket/services/chat-websocket.service.ts`
- `server/modules/websocket/services/shell-websocket.service.ts`
- `server/modules/websocket/services/websocket-writer.service.ts`
- `server/claude-sdk.js`
- `server/openai-codex.js`
- `server/gemini-cli.js`
- `server/cursor-cli.js`
- `server/modules/providers/provider.registry.ts`
- `server/modules/providers/list/claude/claude-sessions.provider.ts`
- `server/modules/providers/list/claude/claude-session-synchronizer.provider.ts`
- `server/shared/types.ts`

整体结构：

```text
Browser
  -> Express HTTP APIs
  -> WebSocket /ws for semantic chat
  -> WebSocket /shell for PTY terminal

/ws
  -> Claude SDK
  -> Codex SDK
  -> Gemini CLI stream-json
  -> Cursor CLI stream-json
  -> provider-specific normalizer
  -> NormalizedMessage
```

主聊天通道 `/ws` 只处理语义事件；交互 terminal 独立在 `/shell`。
这点和 Hermes Web UI 一致，也是 cc-rust 应保留的边界。

### 3.2 主聊天入站协议

`chat-websocket.service.ts` 按 `data.type` 分派：

- `claude-command`
- `cursor-command`
- `codex-command`
- `gemini-command`
- `cursor-resume`
- `abort-session`
- `claude-permission-response`
- `cursor-abort`
- `check-session-status`
- `get-pending-permissions`
- `get-active-sessions`

Claude 前端提交示例：

```json
{
  "type": "claude-command",
  "command": "...",
  "options": {
    "cwd": "...",
    "projectPath": "...",
    "sessionId": "...",
    "resume": true,
    "toolsSettings": {},
    "permissionMode": "default",
    "model": "...",
    "images": []
  }
}
```

### 3.3 统一出站模型

统一出站核心是 `NormalizedMessage`，定义在 `server/shared/types.ts`。
核心字段：

```text
id
sessionId
timestamp
provider
kind
role?
content?
toolName?
toolInput?
toolId?
toolResult?
requestId?
tokenBudget?
```

主要 `kind`：

- `text`
- `tool_use`
- `tool_result`
- `thinking`
- `stream_delta`
- `stream_end`
- `error`
- `complete`
- `status`
- `permission_request`
- `permission_cancelled`
- `session_created`
- `interactive_prompt`
- `task_notification`

同时仍有 legacy/control 消息：

- `session-status`
- `pending-permissions-response`
- `active-sessions`
- `projects_updated`
- `loading_progress`

这个混用是风险点。对 cc-rust 来说，最好从一开始就用统一 envelope 表达
control 与 domain event，避免 `type` 和 `kind` 两套并存。

### 3.4 Claude 适配链路

Claude 使用 `@anthropic-ai/claude-agent-sdk`：

```text
frontend useChatComposerState
  -> WebSocket { type: "claude-command" }
  -> chat-websocket.service.ts
  -> queryClaudeSDK()
  -> query({ prompt, options })
  -> async iterator of SDK messages
  -> sessionsService.normalizeMessage("claude", ...)
  -> WebSocket NormalizedMessage
  -> frontend session store
```

`server/claude-sdk.js` 做的关键事情：

- 映射 cwd、model、resume、permissionMode。
- 设置 `allowedTools`、`disallowedTools`、`permissionMode`。
- 使用 Claude Code preset tools 和 system prompt。
- 从 `~/.claude.json` 合并 MCP 配置。
- 图片先写入临时文件，再把路径加入 prompt。
- 首次收到 `message.session_id` 后发送 `session_created`。
- 每个 SDK message 通过 provider normalizer 转成 `NormalizedMessage`。
- result message 额外发 token budget status。
- abort 调 `queryInstance.interrupt()`。

Claude 权限：

```text
canUseTool(toolName, input, context)
  -> send permission_request { requestId, toolName, input, sessionId }
  -> pendingToolApprovals[requestId] waits
  -> frontend sends claude-permission-response
  -> resolveToolApproval(requestId, decision)
  -> canUseTool returns allow/deny/updatedInput
```

`get-pending-permissions` 支持浏览器刷新后恢复 pending approval view。
这是 cc-rust WebUI 需要补上的能力：pending interaction 不能只存在
当前 socket 的瞬时 UI 状态里。

### 3.5 Provider registry 与历史索引

`server/modules/providers` 以 provider 为单位拆分：

```text
<provider>.provider.ts
<provider>-auth.provider.ts
<provider>-mcp.provider.ts
<provider>-skills.provider.ts
<provider>-sessions.provider.ts
<provider>-session-synchronizer.provider.ts
```

每个 provider 暴露：

- `auth`
- `mcp`
- `skills`
- `sessions`
- `sessionSynchronizer`

Claude 历史索引：

- 扫描 `~/.claude/projects/**/*.jsonl`
- 用 `~/.claude/history.jsonl` 恢复 title
- 解析 JSONL 中的 assistant text、tool_use、thinking、user tool_result
- 对本地 slash command、compact summary、subagent tools 做额外归一

这说明 WebUI 的“实时消息协议”和“历史回放格式”必须共享同一套 normalize
逻辑。cc-rust 如果未来有 WebUI，也应避免实时 renderer 与历史 renderer 各写
一套协议解释。

### 3.6 前端消费方式

关键文件：

- `src/contexts/WebSocketContext.tsx`
- `src/components/chat/hooks/useChatComposerState.ts`
- `src/components/chat/hooks/useChatRealtimeHandlers.ts`
- `src/stores/useSessionStore.ts`
- `src/components/chat/hooks/useChatMessages.ts`
- `src/components/chat/tools/ToolRenderer.tsx`
- `src/components/chat/view/subcomponents/PermissionRequestsBanner.tsx`

前端做法：

- `WebSocketContext` 持有连接，并暴露 `latestMessage`。
- `useChatRealtimeHandlers` 处理 normalized/legacy 事件。
- `useSessionStore` 以 `sessionId` 为 key 保存 server messages 和 realtime messages。
- `normalizedToChatMessages` 把 `NormalizedMessage` 转成现有 chat UI 模型。
- permission request 独立进入 pending permission banner。

值得借鉴的是 session-keyed store 与 provider-neutral message。
不建议照搬的是单个 `latestMessage` state，高频 streaming 下更适合事件队列。

### 3.7 可借鉴与风险

可借鉴：

- provider registry 把 auth、MCP、skills、sessions、session sync 分离。
- provider-native live event 先转 provider-neutral message，再给 UI。
- 语义 chat 与 PTY shell 分离。
- pending permission 可查询，用于重连恢复。
- 会话索引 DB + 原生历史文件 watcher。

风险：

- 协议混用 legacy `type` 与 normalized `kind`。
- `/shell` 用 `bash -c` 拼 provider resume 命令，存在转义/注入风险。
- 工具权限设置主要来自前端 localStorage，后端权威性不足。
- session 主键不应只用裸 `session_id`，跨 provider 需要 namespace。
- Claude SDK 在某些 permission mode 下可能绕过交互 callback，Rust 不能照搬。
- claudecodeui 是 AGPL 项目，只适合参考结构与协议思想，不应复制实现代码。

## 4. Current cc-ipc shape

关键文件：

- `crates/cc-ipc/src/headless.rs`
- `crates/cc-ipc/src/agent_handlers.rs`
- `crates/cc-ipc/src/subsystem_handlers.rs`
- `crates/cc-ipc/src/file_search.rs`
- `crates/cc-ipc-protocol/src/protocol/mod.rs`
- `crates/cc-ipc-protocol/src/protocol/base.rs`
- `crates/cc-ipc-client/src/callbacks.rs`
- `crates/cc-ipc-client/src/query_runner.rs`
- `crates/cc-ipc-client/src/sdk_mapping.rs`
- `crates/cc-ipc-client/src/sink.rs`
- `crates/cc-ipc-client/src/event_class.rs`
- `crates/claude-code-rs/src/app_runtime_adapters/ingress.rs`
- `crates/claude-code-rs/src/app_runtime_adapters/sdk_mapper.rs`

当前抽取状态：

- `cc-ipc-protocol` 持有 `FrontendMessage` / `BackendMessage` DTO。
- `cc-ipc-client` 持有 callback bridge、SDK mapping helper、query turn helper、
  sink、transport parser、event classification。
- `cc-ipc` 持有 headless JSONL runtime，以及 agent/subsystem/file_search facade。
- root binary 通过 `HeadlessRuntimeHost` 和 runtime adapters 注入 engine、commands、
  agent、subsystem 实现。

这是比旧单体 `ipc/headless.rs` 更好的结构，但仍不是完整 IPC 总线。
主要问题在于协议和 runtime 还没有 session / turn / envelope / replay 的公共模型。

## 5. Structural gaps in cc-ipc

### 5.1 Flat enum protocol lacks envelope

`BackendMessage` 是 flat enum。`Ready` 带 `session_id`，但普通事件没有统一：

- protocol version
- event id
- sequence
- timestamp
- session id
- turn id
- run id
- source
- correlation id
- capability flags
- structured error

这会限制 WebSocket/SSE replay、ack、debug、跨 transport 一致性。

### 5.2 Web / daemon / headless mapping is split

当前存在多条 mapping 线：

- headless: `SdkMessage -> BackendMessage`
- daemon routes: `SdkMessage -> SseEvent`
- web crate: 直接 SSE 输出 raw / near-raw SDK event
- gateway: 自己的 `RunRequest/RunStatus/GatewayDiagnostic`

这会导致 WebUI renderer、history replay、remote gateway 和 headless UI 看到的事件
不是同一个 contract。

### 5.3 Runtime globals limit multi-session WebUI

`agent_handlers`、`subsystem_handlers` 使用进程全局 host。
`AGENT_TREE`、event bus、pending permission map 都偏单 runtime。
未来多 session、多 frontend、多租户时，事件归属与清理会变复杂。

### 5.4 Pending interaction is under-scoped

当前 permission/question pending map 主要按 `tool_use_id` 或 `question_id`。
缺少：

- session id
- turn id
- request id
- created timestamp
- timeout policy
- requester metadata
- reconnect query API
- cancellation cleanup

Hermes 和 claudecodeui 都证明了 WebUI 需要 pending approval 恢复能力。

### 5.5 Flow control exists but is not wired through

`cc-ipc-client::event_class::ClientEventQueue` 已经有 lossless/best-effort 分类，
但 headless runtime 仍直接 `sink.send()`。

需要统一：

- lossless event 不可静默丢失
- best-effort event 可 coalesce/drop
- queue pressure diagnostic 结构化上报
- WebSocket/SSE replay buffer
- client ack 或 reconnect cursor

### 5.6 PTY / browser / gateway protocols are separate islands

browser native host、gateway run protocol、daemon event channel、headless IPC 都有各自
channel 语义。长期应收敛到共享 envelope 和 domain payload，而不是各自发展。

## 6. Recommended cc-ipc target shape

### 6.1 Add a typed event envelope

建议在 `cc-ipc-protocol` 新增 envelope，不必一次性删除现有 enum。
可以先支持 `BackendEnvelope { event, payload }` 包裹现有 payload。

建议字段：

```rust
pub struct IpcEnvelope<T> {
    pub version: u16,
    pub id: String,
    pub seq: u64,
    pub timestamp: i64,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub run_id: Option<String>,
    pub source: IpcEventSource,
    pub correlation_id: Option<String>,
    pub event: String,
    pub payload: T,
    pub error: Option<IpcError>,
}
```

事件源示例：

- `headless`
- `daemon`
- `web`
- `gateway`
- `browser`
- `agent`
- `subsystem`

错误模型示例：

```rust
pub struct IpcError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub retryable: bool,
    pub source_phase: Option<String>,
    pub action: Option<String>,
    pub context: serde_json::Value,
}
```

### 6.2 Split protocol by domain

建议模块布局：

```text
crates/cc-ipc-protocol/src/
  protocol/
    envelope.rs
    conversation.rs
    tool.rs
    permission.rs
    session.rs
    subsystem.rs
    agent.rs
    gateway.rs
    browser.rs
    error.rs
    capabilities.rs
```

`FrontendMessage` / `BackendMessage` 可以暂时保留为兼容层，但新增 WebUI/gateway
不要继续往一个 mega enum 塞所有领域。

### 6.3 Split runtime and transport

建议 `cc-ipc` 内部按职责拆：

```text
crates/cc-ipc/src/
  runtime/
    session_registry.rs
    turn_runner.rs
    pending_interactions.rs
    event_bus.rs
    flow_control.rs
  transport/
    jsonl.rs
    sse.rs
    websocket.rs
    sink.rs
  adapters/
    sdk_mapper.rs
    headless_host.rs
    daemon.rs
    web.rs
    gateway.rs
```

目标依赖方向：

```text
cc-ipc-protocol
  <- cc-ipc runtime
  <- cc-ipc-client / UI / web gateway

engine/query/tools
  -> domain SdkMessage
  -> adapter maps to cc-ipc-protocol events
```

### 6.4 Make session and turn first-class

建议引入：

```text
SessionRuntime
  create / resume / attach / detach / close
  current cwd / project root / permission scope / model
  event replay buffer
  pending interactions

TurnRuntime
  turn_id
  request prompt / content blocks
  cancel token
  status: queued | running | cancelling | completed | failed
  output stream
```

`SubmitPrompt` 应变为 session-scoped command：

```text
session.submit {
  session_id,
  turn_id?,
  prompt,
  content_blocks?,
  model?,
  cwd?,
  queue_policy
}
```

`AbortQuery` 应变为 turn-scoped cancel：

```text
turn.cancel {
  session_id,
  turn_id,
  reason
}
```

### 6.5 Normalize conversation events

建议以 WebUI 需要的语义作为公共 domain event：

```text
session.ready
session.resumed
turn.queued
turn.started
message.delta
thinking.delta
message.completed
tool.started
tool.progress
tool.completed
approval.requested
approval.resolved
question.requested
question.answered
usage.updated
status.updated
turn.completed
turn.failed
turn.cancelled
conversation.replaced
```

现有 `BackendMessage::StreamStart/StreamDelta/StreamEnd/ToolUse/ToolResult/...`
可以映射到这些 event。这样 WebUI 不需要理解内部 `SdkMessage`。

### 6.6 Pending interactions

建议 pending key：

```text
session_id + turn_id + request_id
```

pending record 字段：

```text
request_id
session_id
turn_id
kind: permission | question
tool_use_id?
tool_name?
input?
question?
options?
created_at
timeout_at?
status
response_sender
```

需要提供命令：

- `interaction.respond`
- `interaction.cancel`
- `interaction.list_pending`

这样浏览器刷新后能恢复 permission dialog。

### 6.7 Backpressure and replay

事件分级：

- Lossless:
  - ready
  - session lifecycle
  - message completed
  - tool started/completed
  - approval/question requested/resolved
  - turn completed/failed/cancelled
- Best effort:
  - stream delta
  - tool progress
  - usage update
  - status line update
  - subsystem status heartbeat

Web transport 应支持：

- `last_seq` reconnect replay
- bounded replay buffer per session
- best-effort coalescing
- queue pressure diagnostic

JSONL headless 可以先不做 ack，但仍应使用同一 envelope seq，以便日志和测试一致。

### 6.8 Auth and permission boundary

WebUI/gateway 能力必须显式 capability 化：

- submit prompt
- abort turn
- approve tool
- answer question
- read session history
- file search
- file read/write
- terminal spawn
- browser tool
- daemon/gateway management

建议：

- loopback WebUI 使用 daemon token 或 origin-bound session token。
- 不把 token 放 query string，除非只作为兼容路径。
- 所有高风险能力进入审计日志。
- 文件、terminal、browser 能力默认按 backend permission scope 判定，而不是前端 localStorage。

## 7. Suggested implementation sequence

### P0: Contract alignment

1. 在 `cc-ipc-protocol` 新增 envelope、capabilities、structured error。
2. 为现有 `BackendMessage` 提供 `into_envelope(...)` 兼容包装。
3. 把 headless JSONL 输出改为 envelope-aware，但保留旧协议 feature flag。
4. 抽出单一 `SdkMessage -> conversation events` mapper，供 headless/daemon/web 共用。
5. 为 permission/question 增加 `request_id`，pending map 扩为 session/turn/request 维度。

### P0: Lifecycle correctness

1. 引入 `TurnRuntime` handle，`spawn_query_turn` 返回可取消句柄。
2. `AbortQuery` 改成 cancel 当前 turn，Web/gateway 可指定 turn。
3. 增加 `interaction.list_pending` 和 reconnect recovery。
4. daemon gateway 的 approval/ask-user response 要真正注入对应 pending waiter。

### P1: Transport convergence

1. 把 `FrontendSink` 扩展为 trait，支持 stdout、memory、SSE、WebSocket。
2. JSONL/SSE/WebSocket 都发送同一 envelope。
3. 接入 `ClientEventQueue`，区分 lossless 和 best-effort。
4. 加 per-session replay buffer 和 `last_seq` 恢复。

### P1: Runtime cleanup

1. 将 `agent_handlers`、`subsystem_handlers` 的 global host 收敛到 runtime context。
2. 将 agent tree、subsystem event bus 挂到 session/runtime scope，至少带 session id。
3. 把 `FrontendMessage` mega enum 拆成 domain command payload。

### P2: WebUI-facing provider model

1. 为未来 WebUI 增加 provider-neutral normalized view，但放在 gateway/UI 层。
2. 保持 Rust 核心协议比 WebUI view 更细，不把 UI 展示字段写死进 engine。
3. 历史回放使用同一 event normalizer，避免 realtime/history renderer 分叉。

## 8. Test strategy

协议测试：

- envelope golden JSON
- old flat enum compatibility
- unknown event/capability negotiation
- structured error serialization

映射测试：

- `SdkMessage -> conversation event`
- tool use/result pairing
- thinking delta
- tombstone/conversation replacement
- usage/status events

transport 测试：

- JSONL parse error diagnostics
- SSE encoding
- WebSocket replay with `last_seq`
- backpressure lossless rejection
- best-effort coalescing

runtime 测试：

- multi-session concurrent turns
- per-session queue policy
- abort current turn
- pending approval timeout/cancel
- reconnect pending approval listing
- gateway approval/ask-user e2e

security tests:

- token redaction
- file search path constraints
- terminal spawn disabled by default
- browser framing fuzz
- high-risk command audit event

## 9. Decision notes

What to copy conceptually:

- Event names and lifecycle ordering from Hermes Web UI.
- Provider-neutral normalized message idea from claudecodeui.
- session-scoped store/replay model from both projects.
- pending approval recovery from claudecodeui.

What not to copy directly:

- Socket.IO as core IPC.
- Python bridge polling architecture.
- query-string token for privileged WebSocket.
- frontend-local permission authority.
- raw PTY output as semantic chat protocol.
- claudecodeui implementation code, because the project is AGPL.

Recommended target:

```text
QueryEngine / tools / agents
  -> SdkMessage / domain runtime events
  -> cc-ipc-protocol envelope events
  -> transport adapters
       - headless JSONL
       - daemon SSE
       - WebSocket gateway
       - tests/memory sink
  -> frontend session store / renderer
```

This keeps cc-rust's core protocol typed and Rust-owned while still enabling
Claude/Hermes-style WebUI behavior: multi-session, resume, replay, approvals,
tool rendering, usage, and durable diagnostics.

## 10. Current cc-ipc crate boundary

The first structure split keeps legacy JSONL compatible while narrowing
`cc-ipc` to runtime/router responsibilities:

- `cc-ipc-protocol` owns the legacy wire enums plus additive
  `IpcEnvelope<T>` and normalized lifecycle/conversation/tool/permission/control
  payloads.
- `cc-ipc-transport` owns JSONL parsing, stdio framing, frontend sinks,
  memory transport, and queue pressure classification.
- `cc-ipc-adapters` owns pure SDK/stream/tool-result mapping and legacy message
  to envelope conversion.
- `cc-services::file_search` owns frontend file search behavior.
- `cc-services::agent_definitions` owns agent definition list/write/generate
  behavior.
- `cc-tools::system_status` owns the SystemStatus tool and receives subsystem
  snapshots through a host trait.
- `cc-ipc` keeps headless runtime coordination, subsystem/agent command routing,
  and the subsystem event bus compatibility re-export.

The default headless transport still emits bare legacy `BackendMessage` JSONL.
Envelope streams are additive API for WebUI/gateway work and do not require
old TUI/headless clients to migrate yet.
