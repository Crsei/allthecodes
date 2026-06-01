---
title: "守护进程模式"
description: "KAIROS 后台守护进程，提供 HTTP API、SSE 事件流、Worker 生命周期管理和网关桥接能力。"
keywords: ["daemon", "KAIROS", "守护进程", "后台", "HTTP", "SSE"]
---

## 概述

守护进程模式（cc-daemon）是 allthecodes 的 KAIROS 后台运行时。它作为一个持久的 HTTP 服务运行，提供远程查询提交、事件流推送、Worker 进程管理和网关桥接能力。守护进程使 allthecodes 可以脱离终端持续运行，支持自动化工作流和远程控制。

Feature Flag: `FEATURE_KAIROS=1`

## 架构组件

### HTTP 服务

守护进程绑定 `127.0.0.1:{port}`，使用 axum 框架提供以下路由组：

- **API 路由**（`/api/*`）：查询提交、中止、状态查询、attach/detach
- **Webhook 路由**（`/webhook/*`）：GitHub/Slack 等外部服务集成
- **SSE 事件流**（`/events`）：服务端推送事件
- **健康检查**（`/health`）：存活探针
- **网关路由**：Gateway API 桥接

### Worker 生命周期管理

守护进程通过 `WorkerRegistry` 管理子进程 Worker：

- **Worker 类型**：当前支持 `AssistantSession` 类型
- **重启策略**：最多重启 3 次，间隔 250ms
- **心跳监控**：每 1 秒检查，10 秒无响应标记为 Stale
- **资源清理**：Worker 退出时自动清理相关资源

Worker 规范（`WorkerSpec`）包含：
- `worker_id` — 唯一标识
- `kind` — Worker 类型
- `cwd` — 工作目录
- `env` — 环境变量
- `log_path` — 日志路径
- `restart_policy` — 重启策略

### 状态管理

`DaemonState` 是守护进程的核心共享状态，包含：
- `engine` — QueryEngine 实例
- `is_query_running` — 查询运行标志
- `clients` — 连接的客户端集合
- `broadcast` — SSE 事件广播通道
- `terminal_focus` — 终端焦点状态

跨进程状态通过 `process_state` 模块持久化到 `~/.allthecodes/daemon/` 目录：

```
~/.allthecodes/daemon/
  status.json          — 守护进程运行状态
  workers/             — Worker 状态
    assistant-session-1/  — Worker 专属目录
      status.json           — Worker 状态
      events.ndjson         — Worker 事件日志
      commands/             — 命令队列
        <command_id>.json     — 具体命令
```

### 协议层

`DaemonProtocolStore` 提供文件系统级别的命令/事件契约：

**命令**（`DaemonCommand`）：
- `Submit` — 提交查询
- `Abort` — 中止查询
- `PermissionResponse` — 权限响应
- `AskUserResponse` — 询问用户响应
- `Shutdown` — 关闭守护进程
- `ReloadConfig` — 重载配置

**事件**（`DaemonEvent`）：
Worker 通过 NDJSON 格式记录事件，包含 `event_id`、`worker_id`、`command_id`、`event_type`、`data` 等字段。

### 网关桥接

`gateway_bridge` 模块实现 Gateway API 的桥接能力：
- `AssistantWorkerRuntime` — Worker 运行时与 Gateway 的适配层
- 命令路由：将 Gateway 命令转发到对应的 Worker
- 事件回传：Worker 事件通过 Gateway 返回到远程控制端

### 团队记忆代理

`team_memory_proxy` 模块提供团队记忆的 HTTP 代理，使守护进程可以访问和共享跨会话的团队知识。

### SSE 事件流

守护进程通过 SSE（Server-Sent Events）向连接的客户端推送实时事件：
- 查询进度更新
- 权限请求通知
- Worker 状态变更
- 自主模式心跳

## 使用方式

```bash
# 启用 KAIROS 守护进程
FEATURE_KAIROS=1 cargo run

# 健康检查
curl http://127.0.0.1:19836/health

# 提交查询
curl -X POST http://127.0.0.1:19836/api/submit \
  -H "Content-Type: application/json" \
  -d '{"text": "检查代码状态"}'

# 连接 SSE 事件流
curl http://127.0.0.1:19836/events
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `crates/allthecodes-daemon/src/server.rs` | HTTP 服务启动、路由挂载 |
| `crates/allthecodes-daemon/src/routes.rs` | API/Webhook 路由处理器 |
| `crates/allthecodes-daemon/src/state.rs` | 守护进程核心状态 |
| `crates/allthecodes-daemon/src/supervisor.rs` | Worker 生命周期管理 |
| `crates/allthecodes-daemon/src/process_state.rs` | 跨进程状态持久化 |
| `crates/allthecodes-daemon/src/protocol.rs` | 命令/事件文件契约 |
| `crates/allthecodes-daemon/src/runtime.rs` | 运行时适配器插槽 |
| `crates/allthecodes-daemon/src/sse.rs` | SSE 事件流 |
| `crates/allthecodes-daemon/src/gateway_bridge.rs` | Gateway API 桥接 |
| `crates/allthecodes-daemon/src/tick.rs` | 自主模式 tick 循环 |
| `crates/allthecodes-daemon/src/webhook.rs` | Webhook 集成 |
| `crates/allthecodes-daemon/src/web.rs` | Web UI 服务 |
| `crates/allthecodes-daemon/src/channels.rs` | 外部频道消息路由 |
| `crates/allthecodes-ipc/src/runtime.rs` | IPC 运行时 |
| `crates/allthecodes-ipc/src/headless.rs` | 无头模式 IPC |
