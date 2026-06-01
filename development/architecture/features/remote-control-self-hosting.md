# Remote Control / Self-Hosting — 远程控制与自托管

> 功能门控：`FEATURE_KAIROS=1`
> 实现状态：核心网关控制面已实现，入站渠道会话为后续工作
> 稳定 API 前缀：`/remote-control/v1/**`

## 一、功能概述

Remote Control 允许用户通过 HTTP API 远程提交查询、审批工具请求、接收运行事件，以及通过第三方适配器（Telegram / Lark）接收通知和控制 allthecodes 实例。自托管模式让用户可以部署 allthecodes 作为后台服务，通过网络远程使用。

## 二、实现架构

### 2.1 分层架构

```
外部世界
       │
       ├── HTTP 客户端 (curl / 自定义面板)
       │     │
       │     ▼
       ├── Telegram Bot          ───┐
       ├── Lark Bot              ───┤   适配器层 (Adapters)
       └── Webhook (自定义)      ───┘
       │
       ▼
  ┌──────────────────────────────────────┐
  │   Gateway 控制平面                   │
  │   (allthecodes-gateway crate)        │
  │                                      │
  │   - 远程源身份 (RemoteSource)        │
  │   - 运行生命周期 (Runner)            │
  │   - 策略管理 (Policy/Busy)           │
  │   - 事件系统 (Events)                │
  │   - 投递路由 (DeliveryRouter)        │
  │   - 认证 (Auth)                      │
  │   - 持久化 (Store)                   │
  └──────────────────────────────────────┘
       │
       ▼
  ┌──────────────────────────────────────┐
  │   Daemon 执行宿主                    │
  │   (allthecodes-daemon crate)         │
  │                                      │
  │   - HTTP/SSE 服务                    │
  │   - Worker 监督 (Supervisor)         │
  │   - Gateway 桥接 (GatewayDaemonBridge)│
  │   - 查询引擎 (QueryEngine)           │
  │   - 本地 API (/api/*)                │
  └──────────────────────────────────────┘
       │
       ▼
  ┌──────────────────────────────────────┐
  │   IPC 本地桥接                       │
  │   (allthecodes-ipc-protocol/transport)│
  │                                      │
  │   - 本地无头 JSONL 桥接              │
  │   - 前端进程通信                     │
  └──────────────────────────────────────┘
```

### 2.2 运行时端点

| 端点 | 角色 | 远程控制边界 |
|------|------|-------------|
| `crates/allthecodes-gateway` | 持久化远程控制控制面 | 拥有远程源、会话、运行模型、Gateway HTTP API、认证策略、适配器注册、事件持久化 |
| `crates/allthecodes-daemon` | 后台 HTTP/SSE 服务和 Worker/监督宿主 | 仅执行宿主。将 Gateway 命令/事件桥接到 Worker 协议和 QueryEngine |
| `crates/allthecodes-ipc-*` | 本地无头 JSONL 桥接 | 本地 UI 桥接，与 Gateway 传输隔离 |

### 2.3 API 路由

**稳定外部 API（`/remote-control/v1/**`）：**

| 路由 | 方法 | 说明 |
|------|------|------|
| `/remote-control/v1/capabilities` | GET | 获取网关能力声明 |
| `/remote-control/v1/runs` | POST | 提交新运行 |
| `/remote-control/v1/runs/{id}` | GET | 查询运行状态 |
| `/remote-control/v1/runs/{id}` | DELETE | 中止运行 |
| `/remote-control/v1/runs/{id}/events` | GET/SSE | 获取运行事件流 |
| `/remote-control/v1/runs/{id}/approve` | POST | 批准工具使用 |
| `/remote-control/v1/runs/{id}/ask-user` | POST | 响应用户询问 |
| `/remote-control/v1/webhooks` | POST | 注册 Webhook |
| `/remote-control/v1/adapters/status` | GET | 适配器状态查询 |
| `/remote-control/v1/adapters/connect` | POST | 适配器连接测试 |
| `/remote-control/v1/adapters/test-message` | POST | 适配器测试消息 |

## 三、Gateway 核心组件

### 3.1 适配器系统

适配器架构支持向外部队列投递通知和接收控制指令：

| 适配器 | 状态 | 说明 |
|--------|------|------|
| Telegram | 第一波 | 出站 HTTP 连接检查、状态诊断、白名单测试消息 |
| Lark | 第一波 | 同上 |
| 自定义 Webhook | 支持 | HTTP POST 回调投递 |

适配器接口：

```rust
trait RemoteAdapter {
    fn provider(&self) -> AdapterProvider;      // Telegram / Lark
    fn status(&self) -> AdapterStatus;          // 连接状态
    fn send_test(&self, msg: &str) -> Result;   // 发送测试消息
}
```

### 3.2 认证与安全

| 组件 | 说明 |
|------|------|
| `GatewayAuthMode` | Token / MutualTls / 无认证 |
| `GatewayAuthVerifier` | 请求验证（Token 校验 / TLS 证书） |
| `RemoteGatewayAuth` | 完整的远程网关认证流程 |
| `GatewaySecurityConfig` | 安全配置：allowed_origins、token、TLS |

回环调用使用 daemon 控制 Token，非回环模式在未配置远程 Token 和来源策略时拒绝请求。

### 3.3 运行管理

`GatewayRunner` 提供端到端的运行生命周期：

```
submit_run(request)
       │
       ├── create_run() → 存储运行元数据
       ├── evaluate_busy() → 忙策略决策
       │     ├── StartNow → 立即提交
       │     ├── Queue → 排队等待
       │     ├── Interrupt → 中断当前运行
       │     └── Reject → 拒绝
       │
       ▼
  dispatch(GatewayCommand) → daemon bridge
       │
       ▼
  Worker 协议文件 → QueryEngine 执行
       │
       ▼
  RunEvent 持久化 + 适配器投递
```

### 3.4 投递系统

`DeliveryRouter` 将运行事件路由到多个目标：

```rust
pub struct DeliveryRouter {
    callback_sinks: Vec<CallbackDeliverySink>,  // HTTP 回调
    channel_sinks: Vec<ChannelDeliverySink>,    // 适配器渠道
}
```

每个投递记录 `DeliveryRecord` 跟踪投递状态和诊断信息。

## 四、部署模式

### 4.1 自托管架构

```bash
# daemon 模式启动（启用远程控制）
allthecodes daemon --kairos --port 3447
```

启动后的服务布局：

```
[allthecodes daemon]
       │
       ├── HTTP :3447
       │     ├── /api/*              ← 本地控制 API（非公开）
       │     ├── /remote-control/v1/* ← 稳定远程控制 API
       │     ├── /webhook/*          ← Webhook 接收端点
       │     ├── /events             ← SSE 事件流
       │     └── /health             ← 存活探针
       │
       ├── Worker 监督
       │     ├── Assistant Worker
       │     └── Team Workers
       │
       └── Gateway 持久化
             ├── runs/               ← 运行事件日志
             ├── adapters/           ← 适配器状态
             └── webhooks/           ← Webhook 注册
```

### 4.2 本地用户操作

| 命令 | 说明 |
|------|------|
| `/remote status` | 查看远程连接状态 |
| `/remote adapters` | 查看适配器列表与状态 |

`/remote` 命令通过回环 Gateway 客户端操作，不直接写入 Worker 命令文件。

### 4.3 安全边界

| 区域 | 暴露级别 | 认证要求 |
|------|---------|---------|
| `/api/*` | 本地仅限 | daemon 控制 Token |
| `/remote-control/v1/*` | 外部公开 | Gateway Token / mTLS |
| Webhook | 外部公开 | webhook 签名验证 |

## 五、当前实现边界

- Telegram/Lark 入站渠道会话不在当前发布范围内
- 定时远程触发器不在当前发布范围内
- 真正的轮次中 `steer` 尚不支持（需 QueryEngine 注入语义）
- 公共托管 Gateway 和多租户 SaaS 操作不在当前发布范围内
- `mid-turn steer` 能力必须明确报告不支持

## 六、使用方式

```bash
# 启动 daemon（自托管模式）
FEATURE_KAIROS=1 allthecodes daemon --port 3447

# 远程提交查询
curl -X POST http://localhost:3447/remote-control/v1/runs \
  -H "Content-Type: application/json" \
  -d '{"prompt": "列出当前目录文件"}'

# 查看运行状态
curl http://localhost:3447/remote-control/v1/runs/<run-id>

# SSE 事件流
curl -N http://localhost:3447/remote-control/v1/runs/<run-id>/events
```

## 七、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-gateway/src/runner.rs` | 运行生命周期管理 |
| `crates/allthecodes-gateway/src/api.rs` | Gateway API 状态 |
| `crates/allthecodes-gateway/src/auth.rs` | 认证与授权 |
| `crates/allthecodes-gateway/src/policy.rs` | 忙策略与限流 |
| `crates/allthecodes-gateway/src/delivery.rs` | 事件投递路由 |
| `crates/allthecodes-gateway/src/adapters/` | Telegram / Lark 适配器 |
| `crates/allthecodes-gateway/src/webhook.rs` | Webhook 注册与验证 |
| `crates/allthecodes-gateway/src/store.rs` | 持久化存储 |
| `crates/allthecodes-daemon/src/gateway_bridge.rs` | Gateway-Daemon 桥接 |
| `crates/allthecodes-daemon/src/gateway_routes.rs` | Gateway HTTP 路由 |
| `crates/allthecodes-daemon/src/gateway_client.rs` | 本地 Gateway 客户端 |
| `crates/allthecodes-daemon/src/routes.rs` | Daemon HTTP 路由 |
