# KAIROS — 常驻助手模式设计文档

**日期**: 2026-04-11
**范围**: 全量实现 L0-L4 (Feature Gate → BriefTool → Proactive Tick → MCP Channel/Webhook → Bridge/Memory)

---

## 1. Feature Gate 系统 + AppState 扩展

### Feature Gate

新增 `src/config/features.rs`，启动时从环境变量读取，全局不可变：

```rust
pub struct FeatureFlags {
    pub kairos: bool,                    // FEATURE_KAIROS
    pub kairos_brief: bool,              // FEATURE_KAIROS_BRIEF
    pub kairos_channels: bool,           // FEATURE_KAIROS_CHANNELS
    pub kairos_push_notification: bool,  // FEATURE_KAIROS_PUSH_NOTIFICATION
    pub kairos_github_webhooks: bool,    // FEATURE_KAIROS_GITHUB_WEBHOOKS
    pub proactive: bool,                 // FEATURE_PROACTIVE
}
```

用 `once_cell::sync::Lazy<FeatureFlags>` 全局单例，任意位置 `feature::is_enabled(Feature::Kairos)` 查询。

**依赖规则**：
- `kairos_brief` 需要 `kairos`
- `kairos_channels` 需要 `kairos`
- `proactive` 可独立，也可被 `kairos` 隐含启用
- 启动时校验依赖，不满足则 warn + 自动禁用子功能

### AppState 扩展

`src/types/app_state.rs` 新增字段：

```rust
pub struct AppState {
    // ... 现有字段 ...
    pub kairos_active: bool,              // KAIROS 模式是否激活
    pub is_brief_only: bool,              // Brief 输出模式
    pub is_assistant_mode: bool,          // 常驻助手模式
    pub autonomous_tick_ms: Option<u64>,  // Tick 间隔 (None = 禁用)
    pub terminal_focus: bool,             // 用户是否在看终端
}
```

---

## 2. Daemon 架构

### 启动模式

`cc-rust --daemon` 启动 daemon 进程，不进入 TUI。main.rs 新增分支：

```
main.rs 启动流程:
  --version / --dump-system-prompt  →  快速退出
  --daemon                          →  run_daemon()     ← 新增
  --headless                        →  run_headless()   (保留，向后兼容)
  --json / --print                  →  非交互模式
  默认                              →  TUI 模式
```

### Daemon 进程结构

新增 `src/daemon/` 模块：

```
src/daemon/
├── mod.rs           // run_daemon() 入口
├── server.rs        // axum HTTP server
├── routes.rs        // 路由定义
├── sse.rs           // SSE 事件流
├── state.rs         // DaemonState (共享状态)
└── tick.rs          // Proactive tick 调度器
```

### DaemonState

```rust
pub struct DaemonState {
    pub engine: Arc<QueryEngine>,
    pub features: Arc<FeatureFlags>,
    pub clients: Arc<RwLock<HashMap<String, SseClient>>>,   // 连接的前端
    pub is_query_running: Arc<AtomicBool>,                   // 防并发查询
    pub tick_handle: Option<JoinHandle<()>>,                 // tick 任务句柄
    pub webhook_queue: Arc<Mutex<VecDeque<WebhookEvent>>>,   // 外部事件队列
    pub notification_tx: mpsc::UnboundedSender<Notification>,// 推送通道
}
```

### HTTP 路由

```
POST   /api/submit          ← 提交用户消息 (替代 JSONL SubmitPrompt)
POST   /api/abort            ← 中止查询
POST   /api/command          ← 执行斜杠命令
POST   /api/permission       ← 权限响应
GET    /api/events           ← SSE 事件流 (替代 JSONL stdout)
GET    /api/status           ← daemon 状态
POST   /api/resize           ← 终端尺寸更新
POST   /api/attach           ← 前端连接
POST   /api/detach           ← 前端断开 (daemon 继续运行)

POST   /webhook/github       ← GitHub Webhook
POST   /webhook/slack        ← Slack Webhook
POST   /webhook/generic      ← 通用 Webhook

GET    /health               ← 健康检查
```

### 事件循环 (`run_daemon`)

```rust
async fn run_daemon(engine: QueryEngine, config: DaemonConfig) {
    let state = DaemonState::new(engine, config);

    // 并行启动
    tokio::select! {
        // HTTP server (axum)
        _ = serve_http(state.clone(), config.port) => {}

        // Proactive tick 调度器
        _ = tick_loop(state.clone()), if feature::enabled(Proactive) => {}

        // MCP Channel 监听
        _ = channel_listener(state.clone()), if feature::enabled(KairosChannels) => {}

        // 推送通知消费者
        _ = notification_consumer(state.clone()), if feature::enabled(KairosPushNotification) => {}

        // 优雅关闭信号
        _ = shutdown_signal() => {}
    }
}
```

### ink-terminal 适配

前端从 spawn headless 子进程改为连接 daemon HTTP：

```
现在:  run.sh → spawn cc-rust --headless → JSONL over stdio
改为:  run.sh → cc-rust --daemon (如果没在跑则启动) → HTTP/SSE over localhost
```

`ui/src/ipc/client.ts` 从 `child_process.spawn` + JSONL 改为 `fetch` + `EventSource`。协议语义保持一致，只是传输层变化。

### 端口管理

- 默认端口: `19836` (可通过 `--port` 或 `CC_RUST_DAEMON_PORT` 覆盖)
- PID 文件: `~/.cc-rust/daemon.pid`，启动时检查是否已有 daemon 运行
- 端口冲突时自动递增或报错

---

## 3. BriefTool + Brief 模式

### BriefTool

新增 `src/tools/brief.rs`，实现 `Tool` trait：

```rust
pub struct BriefTool;

// 工具名: "SendUserMessage" (与 TypeScript 一致)
// 输入 schema:
{
    "message": string,                    // markdown 格式消息 (必填)
    "attachments": [string],              // 文件路径列表 (可选)
    "status": "normal" | "proactive"      // 消息类型 (可选，默认 normal)
}
```

**行为**：
- `call()` 将消息封装为 `ToolResult`，带特殊标记 `is_brief_message: true`
- 不执行任何副作用，只是格式化输出的通道
- `is_read_only() = true`，`is_concurrency_safe() = true`
- `is_enabled()` 检查 `feature::enabled(KairosBrief)`

### Brief 模式

当 `app_state.is_brief_only = true` 时：

- 系统提示注入 Brief 段落，指示模型**所有用户可见输出必须通过 SendUserMessage 工具**
- 模型的纯文本输出被视为内部思考，前端不显示（或灰色显示）
- 只有 BriefTool 产生的消息才作为正式输出渲染

### `/brief` 命令

新增 `src/commands/brief.rs`：

```
/brief          ← 切换 Brief 模式 on/off
/brief on       ← 启用
/brief off      ← 禁用
/brief status   ← 显示当前状态
```

修改 `app_state.is_brief_only`，同时调用 `prompt_sections::clear_cache()` 刷新系统提示。

### 系统提示段落

在 `system_prompt.rs` 的动态段落中添加：

```markdown
# Brief Mode

All user-facing communication MUST go through the SendUserMessage tool.
Do not produce plain text output intended for the user outside of this tool.
Plain text you emit will be treated as internal reasoning and may be hidden.

Use SendUserMessage for:
- Status updates and progress reports
- Questions that need user input
- Final results and summaries
- Proactive notifications (set status: "proactive")
```

仅当 `is_brief_only == true` 时注入。若 Proactive 模式也启用，Brief 段落合并到 Proactive 段落中避免重复。

### 前端渲染

daemon SSE 事件流中，BriefTool 的结果以专用事件类型发送：

```json
{"type": "brief_message", "message": "...", "status": "normal", "attachments": [...]}
```

ink-terminal 用独立组件渲染 brief 消息，和普通 tool result 区分样式。

---

## 4. Proactive Tick 系统 + SleepTool

### Tick 调度器

新增 `src/daemon/tick.rs`，作为 daemon 事件循环的一个分支：

```rust
pub async fn tick_loop(state: Arc<DaemonState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(30)); // 默认 30s
    loop {
        interval.tick().await;

        // 前置检查
        if state.is_query_running.load(Ordering::SeqCst) { continue; }  // 正在处理查询
        if state.engine.is_sleeping() { continue; }                       // SleepTool 主动休眠中

        // 构造 tick 消息
        let tick_prompt = format!(
            "<tick_tag>\nLocal time: {}\nTerminal focus: {}\n</tick_tag>",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            state.terminal_focus()
        );

        // 作为系统消息提交给 QueryEngine
        state.submit_autonomous(tick_prompt, QuerySource::ProactiveTick).await;
    }
}
```

### Tick 决策规则 (系统提示)

在 `system_prompt.rs` 添加 Proactive 段落，仅当 `feature::enabled(Proactive)` 时注入：

```markdown
# Proactive Mode

You receive periodic <tick_tag> messages containing the user's local time
and terminal focus state.

## Rules
- First tick: Greet briefly, ask what to work on. Do NOT explore unprompted.
- Subsequent ticks: Look for useful work — investigate, verify, check, commit.
- No useful work: Call Sleep tool. Do NOT emit "still waiting" text.
- Don't spam the user. If you already asked a question, wait for their reply.
- Bias toward action: read files, search code, make changes, commit.

## Terminal Focus
- `focus: false` (user away) → Highly autonomous, execute pending tasks
- `focus: true` (user watching) → More collaborative, ask before large changes

## Output
All user-facing output MUST go through SendUserMessage tool.
```

### SleepTool

新增 `src/tools/sleep.rs`：

```rust
pub struct SleepTool;

// 工具名: "Sleep"
// 输入 schema:
{
    "duration_seconds": integer,  // 休眠秒数 (必填, 1-3600)
    "reason": string              // 休眠原因 (可选, 用于日志)
}
```

**行为**：
- `call()` 设置 `engine.set_sleep_until(Instant::now() + duration)` 标记
- **不做真正的 `tokio::sleep`**，立即返回。tick 调度器检查 `is_sleeping()` 来跳过 tick
- 这样模型主动控制自己的唤醒频率，而不是阻塞线程
- `is_enabled()` 检查 `feature::enabled(Proactive) || feature::enabled(Kairos)`

### QueryEngine 扩展

```rust
// 新增到 QueryEngineState
pub(crate) sleep_until: Option<Instant>,

// 新增方法
pub fn set_sleep_until(&self, until: Instant);
pub fn is_sleeping(&self) -> bool;    // Instant::now() < sleep_until
pub fn wake_up(&self);                // 清除 sleep_until (外部事件可唤醒)
```

**唤醒条件**（即使在 Sleep 期间也立即唤醒）：
- 用户提交消息
- Webhook 事件到达
- MCP Channel 通知
- 后台代理完成

### QuerySource 扩展

```rust
pub enum QuerySource {
    ReplMainThread,      // 现有: 用户输入
    BackgroundAgent,     // 现有: 后台代理
    ProactiveTick,       // 新增: tick 触发
    WebhookEvent,        // 新增: 外部 Webhook
    ChannelNotification, // 新增: MCP Channel
}
```

不同 source 影响：
- `ProactiveTick` 不触发 `UserPromptSubmit` hook
- `WebhookEvent` / `ChannelNotification` 携带来源元数据注入上下文

---

## 5. MCP Channel 通知 + Webhook 端点

### MCP Channel 协议

扩展现有 `src/mcp/` 模块，新增 `src/mcp/channel.rs`。

**发现机制**：MCP server 在 `capabilities` 中声明 channel 支持：

```json
{ "experimental": { "claude/channel": {} } }
```

**消息流**：

```
MCP Server ──notifications/claude/channel──► ChannelManager ──► QueryEngine
                                                    │
                                                    ▼
                                              注入为系统消息:
                                              <channel source="slack-mcp" sender="user@org">
                                                message content
                                              </channel>
```

### ChannelManager

新增 `src/daemon/channels.rs`：

```rust
pub struct ChannelManager {
    sources: HashMap<String, ChannelSource>,                // 已注册的 channel 来源
    allowlist: HashSet<String>,                              // 允许列表
    event_tx: mpsc::UnboundedSender<ChannelEvent>,          // 发送到 QueryEngine
}

pub struct ChannelEvent {
    pub source: String,         // "slack-mcp", "github-webhook", etc.
    pub sender: Option<String>, // 发送者标识
    pub content: String,        // 消息内容
    pub meta: Value,            // 原始元数据
    pub origin: ChannelOrigin,  // MCP 还是 Webhook
}

pub enum ChannelOrigin {
    Mcp { server_name: String },
    Webhook { endpoint: String },
}
```

### MCP Channel 权限协议

和 TypeScript 一致的双向权限流：

1. 模型需要权限 → daemon 发送 permission_request 到 MCP server
2. MCP server 通知用户 (如 Slack bot 发消息 "Allow? Reply YES XXXXX")
3. 用户在外部渠道回复
4. MCP server 发送 `notifications/claude/channel/permission` → daemon
5. daemon 自动批准对应的权限请求

### Webhook 端点

daemon HTTP server 的路由，在 `src/daemon/routes.rs`：

```
POST /webhook/github
  ├─ 验证: X-Hub-Signature-256 (HMAC-SHA256, secret 在配置中)
  ├─ 解析: event type (push, pull_request, issues, etc.)
  └─ 转换为 ChannelEvent { origin: Webhook, source: "github", ... }

POST /webhook/slack
  ├─ 验证: X-Slack-Signature + timestamp
  ├─ 处理: URL verification challenge (首次配置)
  └─ 转换为 ChannelEvent

POST /webhook/generic
  ├─ 可选验证: Authorization header 或 query param token
  └─ body 直接作为 ChannelEvent.content
```

### Webhook 配置

`settings.json` 或 `~/.cc-rust/webhooks.json`：

```json
{
  "webhooks": {
    "github": {
      "enabled": true,
      "secret": "whsec_xxx",
      "events": ["pull_request", "issues", "push"]
    },
    "slack": {
      "enabled": true,
      "signing_secret": "xxx"
    },
    "generic": {
      "enabled": true,
      "token": "my-secret-token"
    }
  }
}
```

### Channel Allowlist

启动参数或配置控制哪些 channel 可以注入消息：

```bash
# 命令行
cc-rust --daemon --channels mcp:slack-server --channels webhook:github

# 配置文件
{ "channels_allowlist": ["mcp:slack-server", "webhook:github", "webhook:generic"] }
```

未在 allowlist 中的消息静默丢弃并记录 warn 日志。

### 系统提示段落

当 `feature::enabled(KairosChannels)` 时注入：

```markdown
# External Channels

You may receive messages from external channels wrapped in <channel> tags.
These are real messages from external services (Slack, GitHub, etc.).
Respond to channel messages via SendUserMessage with appropriate context.
Do NOT fabricate channel messages or pretend to have received one.
```

---

## 6. Push Notification 系统

### 架构

新增 `src/daemon/notification.rs`，统一管理通知分发：

```rust
pub struct NotificationManager {
    windows_enabled: bool,
    webhook_enabled: bool,
    webhook_url: Option<String>,
    webhook_headers: HashMap<String, String>,
    rx: mpsc::UnboundedReceiver<Notification>,
}

pub struct Notification {
    pub title: String,
    pub body: String,
    pub level: NotificationLevel,     // Info, Warning, Error, Success
    pub source: NotificationSource,
}

pub enum NotificationSource {
    TaskComplete { task_id: String },
    BackgroundAgentDone { agent_id: String },
    ChannelMessage { source: String },
    ProactiveAction { summary: String },
    Error { detail: String },
}
```

### Windows Toast 通知

使用 `notify-rust` crate：

```rust
async fn send_windows_toast(notif: &Notification) {
    Toast::new("cc-rust")
        .title(&notif.title)
        .text1(&notif.body)
        .icon(/* cc-rust logo path */)
        .show();
}
```

- 仅当前端未连接（detached）或 `terminal_focus: false` 时发送，避免重复打扰
- 点击 Toast 可打开 ink-terminal 并 attach 到 daemon

### Webhook 回调

```rust
async fn send_webhook(notif: &Notification, config: &WebhookNotifyConfig) {
    let payload = json!({
        "title": notif.title,
        "body": notif.body,
        "level": notif.level,
        "source": notif.source,
        "timestamp": Utc::now().to_rfc3339()
    });

    reqwest::Client::new()
        .post(&config.url)
        .headers(config.headers.clone())
        .json(&payload)
        .send()
        .await;
}
```

适配常见推送服务：

| 服务 | 配置方式 |
|------|---------|
| Bark (iOS) | `url: "https://api.day.app/YOUR_KEY"` |
| Pushover | `url + headers (API token)` |
| 钉钉 Robot | `url: "https://oapi.dingtalk.com/robot/send?access_token=xxx"` |
| 企业微信 | `url: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=xxx"` |
| 通用 | 任意 POST URL + 自定义 headers |

### 通知配置

`settings.json`：

```json
{
  "notifications": {
    "windows_toast": {
      "enabled": true,
      "only_when_detached": true
    },
    "webhook": {
      "enabled": true,
      "url": "https://api.day.app/YOUR_KEY",
      "headers": {},
      "events": ["task_complete", "agent_done", "error"]
    }
  }
}
```

### 通知触发点

| 位置 | 事件 | 条件 |
|------|------|------|
| `query/loop_impl.rs` 查询结束 | TaskComplete | Brief 模式下查询完成 |
| `background_agents.rs` 代理完成 | BackgroundAgentDone | 始终 |
| `daemon/channels.rs` 收到外部消息 | ChannelMessage | 前端未连接时 |
| `daemon/tick.rs` 自主执行完成 | ProactiveAction | 执行了有意义的操作时 |
| `query/loop_impl.rs` 错误 | Error | 始终 |

### `/notify` 命令

新增 `src/commands/notify.rs`：

```
/notify              ← 显示通知配置状态
/notify test         ← 发送测试通知
/notify on/off       ← 快速开关
```

---

## 7. Bridge 会话管理 + 每日日志记忆

### 会话生命周期

```
cc-rust --daemon 启动
  │
  ├── 创建 Session (或恢复上次 session)
  │     session_id 写入 ~/.cc-rust/daemon.pid + session info
  │
  ├── 前端 attach (POST /api/attach)
  │     ├── 返回当前 session 的历史消息 (分页)
  │     ├── 建立 SSE 连接接收后续事件
  │     └── terminal_focus = true
  │
  ├── 前端 detach (POST /api/detach 或连接断开)
  │     ├── terminal_focus = false
  │     └── daemon 继续运行, tick 继续
  │
  ├── 前端 re-attach
  │     ├── 返回 detach 期间的消息 (cursor-based)
  │     └── 恢复 SSE 流
  │
  └── daemon 关闭 (SIGTERM 或 /quit)
        ├── 保存 session 到磁盘
        └── 清理 PID 文件
```

### 会话历史 API

```
GET /api/history?limit=100&before_id=xxx   ← 向前翻页 (旧消息)
GET /api/history?limit=100&anchor=latest   ← 最新消息
```

daemon 在内存中维护完整消息列表，同时持久化到 `~/.cc-rust/sessions/{session_id}.json`（复用现有 `session/storage.rs`）。

### Attach/Detach 协议

```rust
// POST /api/attach
pub struct AttachRequest {
    pub client_id: String,                    // 前端实例 ID
    pub last_seen_event: Option<String>,      // 上次收到的事件 ID
}

pub struct AttachResponse {
    pub session_id: String,
    pub missed_events: Vec<BackendMessage>,   // detach 期间的事件
    pub sse_endpoint: String,                  // SSE 连接地址
}
```

daemon 为每个事件分配递增 `event_id`。前端 re-attach 时，daemon 从 `last_seen_event` 开始补发遗漏事件。用环形缓冲区保留最近 1000 条事件，超出的从磁盘 session 文件回读。

### 每日日志记忆

新增 `src/daemon/memory_log.rs`。

**存储路径**: `~/.cc-rust/logs/YYYY/MM/YYYY-MM-DD.md`

**格式**（与 TypeScript 一致）：

```markdown
## 2026-04-11

### 12:30
- User is working on KAIROS implementation for cc-rust
- Preference: environment variable feature gates matching TypeScript conventions

### 14:15
- Completed BriefTool design, user approved
- Project context: full L0-L4 KAIROS implementation planned
```

**写入时机**：
- 模型通过 BriefTool 输出时，摘要自动追加
- Proactive tick 执行有意义操作后追加
- 新的 session 开始时写入日期分隔线

**读取**：作为系统提示的一部分注入，仅注入今日日志。

### 日志提示段落

当 `feature::enabled(Kairos) && kairos_active` 时注入系统提示：

```markdown
# Daily Log

You maintain a daily append-only log at ~/.cc-rust/logs/.
Log user preferences, project context, decisions, and external pointers.
Do NOT log: code diffs, conversation filler, internal reasoning.
One entry per significant interaction. Use ### HH:MM headers.

Today's log so far:
{today_log_content}
```

### `/dream` 命令 (蒸馏)

新增 `src/commands/dream.rs`：

```
/dream            ← 将近期日志蒸馏为 MEMORY.md + 主题文件
/dream --days 7   ← 蒸馏最近 7 天
```

蒸馏过程：将日志内容作为用户消息提交给 QueryEngine，提示词指示模型提取关键信息并写入 `~/.cc-rust/MEMORY.md` 和 `~/.cc-rust/memory/` 下的主题文件。

---

## 8. IPC 协议迁移 + ink-terminal 适配

### 协议迁移策略

现有 JSONL stdio 协议 → HTTP/SSE 协议。**语义保持一致，只换传输层**：

| JSONL (现有) | HTTP/SSE (新增) | 方向 |
|---|---|---|
| `FrontendMessage::SubmitPrompt` | `POST /api/submit` | 前端→daemon |
| `FrontendMessage::AbortQuery` | `POST /api/abort` | 前端→daemon |
| `FrontendMessage::PermissionResponse` | `POST /api/permission` | 前端→daemon |
| `FrontendMessage::SlashCommand` | `POST /api/command` | 前端→daemon |
| `FrontendMessage::Resize` | `POST /api/resize` | 前端→daemon |
| `FrontendMessage::Quit` | `POST /api/detach` | 前端→daemon |
| `BackendMessage::*` (stdout) | `SSE event: message` | daemon→前端 |

### SSE 事件格式

```
GET /api/events?client_id=xxx&last_event_id=xxx

event: stream_start
id: evt_001
data: {"query_id": "q_abc", "source": "user"}

event: stream_delta
id: evt_002
data: {"query_id": "q_abc", "text": "Let me..."}

event: tool_use
id: evt_003
data: {"query_id": "q_abc", "tool": "Bash", "input": {...}}

event: permission_request
id: evt_004
data: {"tool_use_id": "tu_xxx", "tool": "Bash", "command": "rm -rf ..."}

event: brief_message
id: evt_005
data: {"message": "Task completed.", "status": "normal"}

event: autonomous_start
id: evt_006
data: {"source": "proactive_tick", "time": "2026-04-11T14:30:00"}

event: notification
id: evt_007
data: {"title": "Agent done", "level": "success", "source": {...}}

event: background_agent_complete
id: evt_008
data: {"agent_id": "bg_xxx", "description": "...", "had_error": false}
```

每个事件带递增 `id`，支持 `Last-Event-ID` 断线重连。

### ink-terminal 改造

`ui/src/ipc/client.ts` 改造：

```typescript
// 现有
class JsonlClient {
    private proc: ChildProcess;  // spawn cc-rust --headless
    send(msg: FrontendMessage) { this.proc.stdin.write(JSON.stringify(msg) + '\n'); }
    onMessage(cb) { /* parse stdout lines */ }
}

// 改为
class DaemonClient {
    private baseUrl: string;     // http://127.0.0.1:19836
    private eventSource: EventSource;

    async connect() {
        await this.ensureDaemon();
        this.eventSource = new EventSource(`${this.baseUrl}/api/events?client_id=${this.clientId}`);
    }

    async submit(text: string) {
        await fetch(`${this.baseUrl}/api/submit`, {
            method: 'POST', body: JSON.stringify({ text, id: uuid() })
        });
    }

    async abort() { await fetch(`${this.baseUrl}/api/abort`, { method: 'POST' }); }
}
```

### 向后兼容

保留 `--headless` 模式和 JSONL 协议不动。ink-terminal 通过配置决定使用哪种模式：

```typescript
function createClient(mode: 'headless' | 'daemon'): IpcClient {
    if (mode === 'daemon') return new DaemonClient();
    return new JsonlClient();  // 原有逻辑
}
```

`run.sh` 检测 `FEATURE_KAIROS`，有则用 daemon 模式，否则保持 headless：

```bash
if [ "$FEATURE_KAIROS" = "1" ]; then
    cc-rust --daemon --background || true
    exec bun run ui -- --mode daemon
else
    exec bun run ui  # 现有 headless 模式
fi
```

---

## 9. 命令汇总 + 文件清单

### 新增命令

| 命令 | 文件 | 功能 | 依赖 Feature |
|------|------|------|------|
| `/brief` | `commands/brief.rs` | 切换 Brief 输出模式 | `KairosBrief` |
| `/assistant` | `commands/assistant.rs` | 助手状态/配置查看 | `Kairos` |
| `/notify` | `commands/notify.rs` | 通知配置与测试 | `KairosPushNotification` |
| `/dream` | `commands/dream.rs` | 日志蒸馏为记忆 | `Kairos` |
| `/channels` | `commands/channels.rs` | 查看/管理 Channel 连接 | `KairosChannels` |
| `/sleep` | `commands/sleep.rs` | 手动设置 Sleep 时长 | `Proactive` |
| `/daemon` | `commands/daemon_cmd.rs` | 查看 daemon 状态/停止 | `Kairos` |

每个命令的 `execute()` 入口先检查对应 feature 是否启用，未启用则返回提示信息。

### 新增工具

| 工具 | 文件 | 模型名 | 依赖 Feature |
|------|------|--------|------|
| BriefTool | `tools/brief.rs` | `SendUserMessage` | `KairosBrief` |
| SleepTool | `tools/sleep.rs` | `Sleep` | `Proactive \|\| Kairos` |

在 `tools/registry.rs` 的 `get_all_tools()` 中条件注册：

```rust
if feature::enabled(Feature::KairosBrief) {
    tools.push(Arc::new(BriefTool));
}
if feature::enabled(Feature::Proactive) || feature::enabled(Feature::Kairos) {
    tools.push(Arc::new(SleepTool));
}
```

### 新增/修改文件完整清单

```
新增:
  src/config/features.rs           Feature Gate 系统 (~80行)
  src/daemon/
  ├── mod.rs                       run_daemon() 入口 (~60行)
  ├── server.rs                    axum HTTP server 启动 (~50行)
  ├── routes.rs                    路由定义 + handler (~250行)
  ├── sse.rs                       SSE 事件流 + 断线重连 (~120行)
  ├── state.rs                     DaemonState 共享状态 (~80行)
  ├── tick.rs                      Proactive tick 调度器 (~70行)
  ├── channels.rs                  ChannelManager (~150行)
  ├── notification.rs              NotificationManager (~130行)
  ├── memory_log.rs                每日日志读写 (~100行)
  └── webhook.rs                   Webhook 验证 + 解析 (~120行)
  src/tools/brief.rs               BriefTool (~90行)
  src/tools/sleep.rs               SleepTool (~70行)
  src/commands/brief.rs            /brief 命令 (~50行)
  src/commands/assistant.rs        /assistant 命令 (~60行)
  src/commands/notify.rs           /notify 命令 (~50行)
  src/commands/dream.rs            /dream 命令 (~70行)
  src/commands/channels.rs         /channels 命令 (~50行)
  src/commands/sleep.rs            /sleep 命令 (~30行)
  src/commands/daemon_cmd.rs       /daemon 命令 (~50行)
  src/mcp/channel.rs               MCP Channel 协议扩展 (~100行)
  ui/src/ipc/daemon_client.ts      DaemonClient (~120行)

修改:
  src/main.rs                      新增 --daemon 分支 + feature 初始化
  src/types/app_state.rs           新增 5 个字段
  src/engine/system_prompt.rs      新增 3 个条件段落 (Brief/Proactive/Channel)
  src/engine/lifecycle/mod.rs      新增 sleep_until 字段 + 方法
  src/tools/mod.rs                 声明 brief, sleep 模块
  src/tools/registry.rs            条件注册新工具
  src/commands/mod.rs              声明 + 注册 7 个新命令
  src/query/loop_impl.rs           QuerySource 扩展 + 通知触发点
  src/ipc/protocol.rs              BackendMessage 新增事件类型
  src/mcp/mod.rs                   声明 channel 模块
  ui/src/ipc/client.ts             createClient() 模式选择
  ui/run.sh                        FEATURE_KAIROS 检测
  Cargo.toml                       新增依赖
```

### 新增 Cargo 依赖

```toml
axum = "0.8"            # HTTP server
tower-http = "0.6"      # CORS, logging middleware
reqwest = "0.12"        # Webhook 回调 HTTP client
hmac = "0.12"           # Webhook 签名验证
sha2 = "0.10"           # SHA256
notify-rust = "4"       # 跨平台系统通知 (含 Windows Toast)
```
