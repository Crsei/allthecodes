# Channels — 外部通信渠道

> 实现状态：完整实现
> 来源类型：MCP 服务器 / Webhook 端点
> 安全机制：白名单准入

## 一、功能概述

Channels 系统负责将外部消息（来自 MCP 服务器、Webhook）路由到 allthecodes 的 QueryEngine，使模型能够通过外部渠道接收和响应消息。每条外部消息被包装为 XML 格式注入对话上下文，允许模型感知并响应多来源的外部输入。

## 二、实现架构

### 2.1 核心类型

`channels.rs` 定义了两个核心类型：

**ChannelOrigin** — 消息来源标识：

```rust
pub enum ChannelOrigin {
    Mcp { server_name: String },     // MCP 服务器来源
    Webhook { endpoint: String },    // Webhook 端点来源
}
```

**ChannelEvent** — 渠道事件负载：

| 字段 | 类型 | 说明 |
|------|------|------|
| `source` | `String` | 来源名称（如 slack/github） |
| `sender` | `Option<String>` | 发送者标识 |
| `content` | `String` | 消息内容 |
| `meta` | `Value` | 附加元数据（JSON） |
| `origin` | `ChannelOrigin` | 来源类型与标识 |

**ChannelManager** — 渠道管理器：

| 方法 | 说明 |
|------|------|
| `new(allowlist, event_tx)` | 创建管理器，指定白名单和事件通道 |
| `submit(event)` | 提交事件（经白名单检查后发送） |

### 2.2 数据流

```
外部来源
       │
       ├── MCP 服务器 (slack-mcp, github-mcp...)
       │     └── ChannelOrigin::Mcp { server_name }
       │
       └── Webhook 端点 (/hooks/github, /hooks/slack...)
             └── ChannelOrigin::Webhook { endpoint }
       │
       ▼
  ChannelManager::submit(event)
       │
       ├── 构建白名单 key（"mcp:server-name" / "webhook:/endpoint"）
       ├── 检查 allowlist 是否包含该 key
       │     ├── 通过 → 发送到 event_tx channel
       │     └── 拒绝 → 记录 warn 日志
       │
       ▼
  QueryEngine 消费 ChannelEvent
       │
       ▼
  转换为 XML 注入对话上下文
```

### 2.3 XML 格式

渠道消息在上下文中以以下 XML 格式呈现：

```xml
<channel source="slack" sender="alice">
hello world
</channel>
```

当没有发送者信息时：

```xml
<channel source="github">
PR merged
</channel>
```

这种 XML 格式使模型能够清晰区分外部消息的来源和发送者，便于按来源分配注意力。

### 2.4 安全机制

白名单准入是 Channel Manager 的核心安全策略：

```rust
pub struct ChannelManager {
    allowlist: HashSet<String>,           // 白名单
    event_tx: mpsc::UnboundedSender<ChannelEvent>,  // 事件通道
}
```

白名单 key 格式：
- MCP 来源：`"mcp:<server_name>"`
- Webhook 来源：`"webhook:<endpoint>"`

所有不在白名单中的来源都会被记录警告日志并拒绝，确保只有经过配置的外部来源才能向模型发送消息。

### 2.5 集成路径

**MCP 集成：**

```
MCP 服务器 (Slack/GitHub 等)
       │
       ▼
  MCP Client Manager
       │  从 MCP 消息构造 ChannelEvent
       ▼
  ChannelManager::submit(event)
       │
       ▼
  QueryEngine 处理
```

**Webhook 集成：**

```
外部系统 (GitHub Webhook / Slack 事件 API)
       │
       ▼
  Daemon HTTP Server (/webhook/* 路由)
       │  从 HTTP 请求构造 ChannelEvent
       ▼
  ChannelManager::submit(event)
       │
       ▼
  QueryEngine 处理
```

Daemon 的 Webhook 路由通过 `routes.rs` 中的 `/webhook/*` 端点注册，支持多个 Webhook 来源。

## 三、配置与管理

渠道白名单通过 daemon 配置管理：

```rust
// daemon 初始化时
let allowlist = vec![
    "mcp:slack-mcp".into(),
    "mcp:github-mcp".into(),
    "webhook:/hooks/github".into(),
];
let (tx, rx) = mpsc::unbounded_channel();
let manager = ChannelManager::new(allowlist, tx);
```

ChannelEvent 通过 `to_xml()` 方法转换为字符串，以 `<channel>` XML 标签形式注入到模型对话上下文。

## 四、关键设计决策

1. **白名单准入**：所有外部消息必须显式配置来源，防止未授权的消息注入
2. **XML 上下文注入**：使用 XML 标签标记外部消息，便于模型解析来源和发送者
3. **统一的事件模型**：MCP 和 Webhook 来源共享相同的 ChannelEvent 结构
4. **异步无界通道**：使用 `mpsc::unbounded_channel` 保证高吞吐量消息处理
5. **轻量实现**：单个文件（`channels.rs`），无外部依赖

## 五、使用方式

```bash
# 配置渠道白名单（daemon 启动参数或配置文件）
# 示例：允许 Slack MCP 和 GitHub Webhook
ALLTHECODES_CHANNELS="mcp:slack-mcp,webhook:/hooks/github" allthecodes
```

## 六、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-daemon/src/channels.rs` | ChannelManager 实现 |
| `crates/allthecodes-daemon/src/routes.rs` | Webhook HTTP 路由 |
| `crates/allthecodes-daemon/src/webhook.rs` | Webhook 处理器 |
| `crates/allthecodes-browser/src/mcp_bridge.rs` | MCP 桥接集成 |
