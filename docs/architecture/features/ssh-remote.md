# SSH Remote — SSH 远程开发

> 实现状态：规划阶段，通过 Gateway 框架实现
> 核心机制：Gateway 控制平面 + 远程源身份认证 + 传输适配

## 一、功能概述

SSH Remote 允许开发者在远程服务器上运行 allthecodes 会话，通过 SSH 协议进行安全的双向通信。该功能建立在 Gateway（远程控制网关）框架之上，复用其远程源身份、会话密钥派生、策略管理等基础设施。

## 二、实现架构

### 2.1 依赖关系

SSH Remote 功能本身不在专用 crate 中实现，而是作为 Gateway 框架的一个**远程源类型（RemoteSource）** 和 **传输适配器（Transport）**。

关键依赖 crate：

| Crate | 作用 |
|-------|------|
| `allthecodes-gateway` | 远程源身份、会话密钥、策略、运行管理 |
| `allthecodes-daemon` | 执行宿主、网桥、HTTP/SSE 服务 |
| `allthecodes-ipc-transport` | 传输层复用（JSONL/Frame） |

### 2.2 Gateway 远程源模型

`RemoteSource` 枚举定义了远程连接的来源类型：

```rust
pub enum RemoteSource {
    // SSH 远程连接
    Ssh {
        host: String,
        user: String,
        fingerprint: String,
    },
    // 其他远程来源...
}
```

`RemoteSourceMetadata` 携带连接元数据（IP、用户代理、协议版本等），用于身份验证和审计。

### 2.3 会话密钥派生

`SessionKey` 和 `SessionKeyPolicy` 负责远程会话的安全密钥管理：

| 组件 | 说明 |
|------|------|
| `SessionKey` | 确定性密钥派生（基于远程源 + 会话上下文） |
| `SessionKeyPolicy` | 密钥轮换策略与有效期管理 |

SSH 连接使用 SSH 指纹作为会话密钥的熵源之一，确保同一 SSH 连接的不同会话可关联，同时防止会话劫持。

### 2.4 认证模式

`GatewayAuthMode` 和 `RemoteGatewayAuth` 定义远程连接的安全策略：

| 模式 | 说明 |
|------|------|
| `Token` | Token 认证（用于非 SSH 远程连接） |
| `MutualTls` | mTLS 双向认证 |
| `Ssh` | SSH 连接认证（使用 SSH 密钥对） |

### 2.5 传输适配

SSH 远程连接通过以下传输适配路径工作：

```
远程 allthecodes (SSH 客户端端)
       │  SSH 隧道
       ▼
SSH 服务器 + Gateway 适配器
       │
       ├── Gateway API（/remote-control/v1/**）
       │     - 运行提交与状态查询
       │     - 事件消费（SSE）
       │     - 审批与用户询问响应
       │
       ├── Daemon Bridge（GatewayCommandSink）
       │     - 将 Gateway 命令映射为 daemon worker 协议
       │     - 管理运行生命周期
       │
       └── 投递适配器（DeliveryRouter）
             - 通过 SSH 通道投递状态通知
             - 可扩展为 Telegram/Lark 等渠道
```

## 三、Gateway 框架的核心能力

### 3.1 运行管理

`GatewayRunner` 提供完整的远程运行生命周期管理：

| 操作 | 说明 |
|------|------|
| `submit_run` | 提交运行请求（含忙策略评估） |
| `stop_run` | 终止运行 |
| `approve_run` | 批准工具使用权限 |
| `answer_user` | 响应用户询问 |
| `deliver_event` | 投递运行事件到注册的适配器 |
| `recover_on_startup` | 启动时恢复未完成的运行 |
| `start_next_queued` | 启动队列中的下一个运行 |

### 3.2 忙策略

`GatewayPolicy` 控制同一远程源的多运行并发策略：

| 决策 | 行为 |
|------|------|
| `StartNow` | 立即开始新运行 |
| `Queue` | 排队等待（当前运行完成后启动） |
| `Interrupt` | 中断当前运行，启动新运行 |
| `Reject` | 拒绝新运行（含诊断原因） |

### 3.3 事件系统

`RunEvent` 和 `RunEventKind` 定义远程运行的完整事件模型：

| 事件类型 | 说明 |
|---------|------|
| `Started` | 运行开始 |
| `Completed` | 运行完成 |
| `Failed` | 运行失败 |
| `Diagnostic` | 诊断信息 |
| `ApprovalRequested` | 请求用户批准工具使用 |
| `AskUserRequested` | 请求用户回答 |
| `ToolEvent` | 工具调用与结果 |
| `Custom` | 自定义事件 |

### 3.4 持久化

Gateway 使用文件系统存储持久化远程状态：

| 存储目录 | 内容 |
|---------|------|
| `gateway_dir` | 网关根目录 |
| `runs_dir` | 运行记录（事件日志） |
| `adapters_dir` | 适配器状态与凭证 |
| `webhooks_dir` | Webhook 注册信息 |

## 四、当前实现边界

- SSH 作为 RemoteSource 类型已在 Gateway 模型中预留
- 完整的 SSH 传输层（基于 `ssh2` crate 的直接连接）是后续实现目标
- 当前 Remote Gateway 功能通过 HTTP API（`/remote-control/v1/**`）暴露，可先基于此进行 SSH 隧道包装
- Telegram/Lark 适配器是第一波外部队列适配器，SSH 适配与其共享 DeliveryRouter 框架

## 五、安全考虑

| 方面 | 措施 |
|------|------|
| 身份认证 | SSH 密钥对 / Token / mTLS |
| 授权 | RemoteGatewayAuth 验证策略 |
| 防重放 | 会话密钥 + 时间戳 |
| 起源检查 | allowed_origins 白名单 |
| 安全诊断 | GatewayDiagnostic 事件记录 |

## 六、使用方式

```bash
# 通过 SSH 远程连接（规划中）
allthecodes remote connect ssh://user@host

# 通过 Gateway HTTP API 远程提交
curl https://remote-host:3447/remote-control/v1/runs \
  -H "Authorization: Bearer <token>" \
  -d '{"prompt": "..."}'
```

## 七、文件索引

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-gateway/src/source.rs` | RemoteSource 类型与传输元数据 |
| `crates/allthecodes-gateway/src/session_key.rs` | 会话密钥派生策略 |
| `crates/allthecodes-gateway/src/auth.rs` | 认证模式与验证器 |
| `crates/allthecodes-gateway/src/runner.rs` | 运行生命周期管理 |
| `crates/allthecodes-gateway/src/policy.rs` | 忙策略与限流 |
| `crates/allthecodes-gateway/src/events.rs` | 运行事件模型 |
| `crates/allthecodes-gateway/src/config.rs` | 安全配置与持久化路径 |
| `crates/allthecodes-daemon/src/gateway_bridge.rs` | Gateway 命令到 daemon 的桥接 |
