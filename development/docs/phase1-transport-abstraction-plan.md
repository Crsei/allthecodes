
# Phase 1：传输层抽象实现计划

> 计划日期：2026-06-14  
> 基于 Codex `app-server-transport` 架构分析与 allthecodes 当前服务器架构对比  
> 相关分析文档：`development/docs/codex-portapi-comparison-analysis.md`、`development/docs/api-interface-analysis.md`

---

## 目录

1. [概述与动机](#1-概述与动机)
2. [设计目标](#2-设计目标)
3. [新 Crate: `allthecodes-server`](#3-新-crate-allthecodes-server)
4. [类型定义](#4-类型定义)
5. [CLI 变更](#5-cli-变更)
6. [`full_init.rs` 重构](#6-full_initrs-重构)
7. [Daemon `server.rs` 重构](#7-daemon-serverrs-重构)
8. [Web `mod.rs` 变更](#8-web-modrs-变更)
9. [优雅关闭策略](#9-优雅关闭策略)
10. [迁移路径（9 步）](#10-迁移路径9-步)
11. [测试策略](#11-测试策略)
12. [文件改动清单](#12-文件改动清单)
13. [风险与缓解](#13-风险与缓解)

---

## 1. 概述与动机

### 1.1 当前问题

allthecodes 有两个独立的 Axum HTTP 服务器，它们通过互斥的 CLI flag 启动，不能同时运行：

| 服务器 | 默认端口 | CLI Flag | 职责 | 状态类型 |
|--------|---------|----------|------|---------|
| Web 服务器 | 17322 | `--web` | 协议 API handlers（~200 个端点）、SPA 静态文件、`/api/rpc/ws` WebSocket | `WebState` |
| Daemon 服务器 | 19836 | `--daemon` | KAIROS API（submit/abort/status/attach/detach）、SSE `/events`、webhook、Gateway 远程控制 | `DaemonState` |

**核心问题：**
1. `--web` 和 `--daemon` 是 `full_init.rs` 中的两个互斥 `if` 分支，一个进程只能选其一
2. 绑定地址和端口硬编码为 `127.0.0.1`，没有统一的 `--listen` CLI flag
3. 端口是魔术数字（17322、19836），无端口分配策略
4. 无优雅关闭协调（Web 服务器仅 `await`，Daemon 用 `tokio::select!`）
5. 无 Unix socket、TLS 等传输扩展性

### 1.2 Codex 的借鉴

Codex 的 `app-server-transport` crate 使用 `AppServerTransport` enum + `TransportEvent` 通道实现了传输无关的消息处理器。allthecodes 不能直接复制（因为它是 HTTP REST API 而非 JSON-RPC），但可以借鉴其统一传输抽象的思路，结合 allthecodes 自身特点进行适配。

## 当前实现状态（2026-06-16）

本节记录当前工作区事实，用于区分“已经落地的 Phase 1 基础设施”和“仍需验收或后续实现的内容”。

### 已完成

- `allthecodes-server` crate 已加入 workspace。
- `ListenUrl` 和 `ServerMode` 类型已实现。
- CLI 已加入隐藏 `--listen` 参数，支持 `web://...`、`daemon://...`、`all://...` 和 `off`。
- `ServerMode::All` 已支持同进程启动 Web 与 Daemon 两个服务器。
- daemon 侧已提取并导出 `build_router()`。
- `full_init` 已归并到统一 server path。
- server lifecycle 已接入 graceful shutdown。
- `All` 模式已拒绝相同非零 Web/Daemon 地址；`:0` 仍允许由 OS 分配实际端口。
- `All` 模式先完成两个 listener bind 再 spawn/serve，避免第二个 bind 失败时留下部分启动的 server。
- `--listen` 传入但解析失败时直接报错，不再静默 fallback 到 legacy flags。
- Gateway `/remote-control/v1/capabilities` 已恢复控制令牌认证，支持 `x-allthecodes-daemon-token`，并保留旧 `x-cc-rust-daemon-token` 兼容。
- `allthecodes-server` integration tests 已加入。
- `development/docs/allthecodes-server-architecture.md` 已补充架构说明。

### 验收记录

使用隔离状态目录 `ALLTHECODES_HOME=/tmp/allthecodes-phase1-verify` 和空 cwd `/tmp/allthecodes-phase1-verify-cwd` 验证。

| 项目 | 结果 |
|---|---|
| `cargo test -p allthecodes-server` | 通过：29 个 unit tests、8 个 integration tests、0 个 doctests |
| `cargo test -p allthecodes-gateway` | 通过：29 个 unit tests、0 个 doctests |
| `cargo check -p allthecodes --bin allthecodes` | 通过 |
| `cargo build --workspace` | 通过 |
| `cargo clippy --workspace --lib --bins` | 命令通过；workspace 仍有既有 clippy warnings， touched crates 未新增 warning |
| `--web --web-port 17331 --no-open` | `/` 返回 backend-only static fallback，`/api/web/health` 返回 200 |
| 默认 `--web` 端口 `17322` | 环境中已有 `target/release/allthecodes --web --web-port 17322 --no-open` 占用，改用 `17331` 验证 |
| `FEATURE_KAIROS=1 --daemon --port 19836` | `/health` 返回 200，`/events?client_id=phase1` 返回 `text/event-stream` |
| Gateway capabilities | 无 token 返回 401；从隔离 `control-token.json` 读取 token 后带 `x-allthecodes-daemon-token` 返回 200 |
| `--listen web://127.0.0.1:17332` | `/api/web/health` 返回 200 |
| `--listen daemon://127.0.0.1:19837` | `/health` 返回 200 |
| `--listen all://web=127.0.0.1:17333,daemon=127.0.0.1:19838` | Web `/api/web/health` 和 daemon `/health` 均返回 200 |
| `--listen off --headless` | 进入非 server path，输出 headless `ready` 后在 stdin EOF 时退出 |
| `--listen garbage://xyz --web` | 明确解析失败，不 fallback 到 `--web` |
| `all://web=127.0.0.1:17334,daemon=127.0.0.1:17334` | 明确解析失败 |
| `--web --daemon --web-port 17335 --port 17335` | 明确 mode 校验失败 |
| Ctrl-C shutdown | Web、Daemon、All 手动验证通过，端口释放 |
| SIGTERM shutdown | Daemon PID 收到 SIGTERM 后退出，`19836` 端口释放，`supervisor.json` 状态为 `stopped` |
| backend-only static assets | 默认构建 `/` 返回 “Web UI assets are not bundled...” fallback |
| `web-ui` feature static assets | `cargo build -p allthecodes --bin allthecodes --features web-ui` 通过；运行时验证受阻：feature-built debug binary 在本机验证中未在 15s 内 bind `17336` |

### 未实现后续项

- `/api/resize` 当前仍是 daemon noop stub，未转发到 Rust TUI 或 terminal session，也未写入 daemon client metadata。具体语义和 wire shape 不在 Phase 0/当前状态中决策。

---

## 2. 设计目标

### 2.1 核心设计原则

1. **渐进式重构** — 不破坏现有功能，保持向后兼容
2. **保持一致的外部接口** — `--web` 和 `--daemon` flag 继续工作
3. **最小化 crate 依赖** — 新 crate (`allthecodes-server`) 只依赖 `axum`、`tokio`、`tokio-util`，不依赖 `allthecodes-web` 或 `allthecodes-daemon`
4. **路由所有权不变** — 每个 crate 继续拥有自己的路由构建函数；`ServerManager` 接收已构建好的 `Router` 实例
5. **渐进启用** — 先引入类型和基础设施，再逐步迁移启动流程

### 2.2 新增能力

| 能力 | 描述 |
|------|------|
| `--listen` 统一接口 | 接受 `web://127.0.0.1:17322`、`daemon://127.0.0.1:19836`、`all://web=...,daemon=...`、`off` |
| 同进程双服务器 | `ServerMode::All` 在同一进程同时运行 Web 和 Daemon 服务器 |
| 优雅关闭 | Ctrl-C → 停止接受新连接 → 等待请求完成（30s grace period） |
| `:0` 随机端口 | 绑定到 OS 分配的随机端口（Phase 2 充分启用） |

### 2.3 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        allthecodes 进程                          │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │               ServerManager (allthecodes-server)          │   │
│  │  - ServerMode::All { web_addr, daemon_addr }             │   │
│  │  - CancellationToken (root)                              │   │
│  │  - serve_with_graceful_shutdown()                        │   │
│  └──────────┬────────────────────────────┬──────────────────┘   │
│             │                            │                      │
│     ┌───────▼──────────┐        ┌───────▼──────────┐           │
│     │ Web Router       │        │ Daemon Router    │           │
│     │ (allthecodes-web)│        │(allthecodes-daemon)│          │
│     │ - Protocol APIs  │        │ - /api/submit     │           │
│     │ - SPA static     │        │ - /api/abort      │           │
│     │ - /api/rpc/ws    │        │ - /events (SSE)   │           │
│     │ - WebState       │        │ - /webhook/*      │           │
│     └───────┬──────────┘        │ - /remote-control/*│          │
│             │                   │ - DaemonState     │           │
│             │                   └────────┬──────────┘           │
│             │                            │                      │
│     ┌───────▼────────────────────────────▼──────────┐           │
│     │        tokio::select! / tokio::spawn          │           │
│     │  - http serve + graceful_shutdown              │           │
│     │  - tick_loop (if Proactive enabled)            │           │
│     │  - scheduler_loop                             │           │
│     │  - supervisor_loop                            │           │
│     │  - ctrl_c signal                              │           │
│     └───────────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. 新 Crate: `allthecodes-server`

### 3.1 目录结构

```
crates/allthecodes-server/
  Cargo.toml
  src/
    lib.rs              -- 模块声明与 re-exports
    transport.rs        -- ListenUrl 枚举 + 解析器
    server_mode.rs      -- ServerMode 枚举 + from_listen_and_fallback()
    server_manager.rs   -- ServerManager 结构体 + lifecycle
```

### 3.2 Cargo.toml

```toml
[package]
name = "allthecodes-server"
version = "0.1.0"
edition.workspace = true
description = "Transport abstraction and server lifecycle for allthecodes HTTP servers."

[dependencies]
anyhow = { workspace = true }
axum = { workspace = true }
tokio = { workspace = true }
tokio-util = { workspace = true }
tracing = { workspace = true }
```

### 3.3 依赖最小化的意义

`allthecodes-server` 不依赖 `allthecodes-web` 或 `allthecodes-daemon`，所以：
- 不会引入循环依赖
- Web 和 Daemon crate 可以各自独立使用它的类型
- 编译期间不会触发不必要的 crate 编译

---

## 4. 类型定义

### 4.1 `ListenUrl` (`src/transport.rs`)

```rust
use std::net::SocketAddr;

/// 一个解析后的 `--listen` 值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenUrl {
    /// Web 服务器仅（chat UI、静态文件、协议 handlers）
    Web(SocketAddr),
    /// Daemon 服务器仅（KAIROS API、SSE、webhook、gateway）
    Daemon(SocketAddr),
    /// 同时启动两个服务器
    All { web_addr: SocketAddr, daemon_addr: SocketAddr },
    /// stdio 模式（headless IPC）
    Stdio,
    /// 无 HTTP 服务器（TUI 模式或 headless）
    Off,
}

impl ListenUrl {
    /// 从 CLI 字符串解析。
    /// 语法：
    ///   - "off"               → Off
    ///   - "stdio://"          → Stdio
    ///   - "web://addr:port"   → Web(addr:port)
    ///   - "daemon://addr:port" → Daemon(addr:port)
    ///   - "all://web=addr1:port1,daemon=addr2:port2" → All { ... }
    /// 不合法的输入返回 None。
    pub fn parse(input: &str) -> Option<Self> { ... }
}
```

**解析规则**（`parse` 函数）：
1. `input == "off"`（大小写不敏感）→ `ListenUrl::Off`
2. `input.starts_with("stdio://")` → `ListenUrl::Stdio`
3. `input.starts_with("web://")` → 解析剩余部分为 `SocketAddr`，返回 `ListenUrl::Web`
4. `input.starts_with("daemon://")` → 解析剩余部分为 `SocketAddr`，返回 `ListenUrl::Daemon`
5. `input.starts_with("all://")` → 按 `,` 分割剩余部分，解析 `key=value` 对中的 `web` 和 `daemon` 键
6. 不匹配任何格式 → `None`

### 4.2 `ServerMode` (`src/server_mode.rs`)

```rust
use std::net::SocketAddr;

/// 描述在本进程中启动哪个（些）Axum 服务器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMode {
    /// Web UI 服务器
    Web { addr: SocketAddr },
    /// Daemon API 服务器
    Daemon { addr: SocketAddr },
    /// 同进程同时运行两个服务器
    All { web_addr: SocketAddr, daemon_addr: SocketAddr },
    /// 不启动 HTTP 服务器（TUI 或 headless）
    None,
}

impl ServerMode {
    /// 从可选的 `--listen` 值和遗留 CLI flag 构造 ServerMode。
    ///
    /// 优先级：
    /// 1. 如果 `listen` 为 Some，直接映射
    /// 2. 否则回退到 `web_flag` / `daemon_flag` / 默认端口
    pub fn from_listen_and_fallback(
        listen: Option<ListenUrl>,
        web_flag: bool,
        daemon_flag: bool,
        web_port: u16,
        daemon_port: u16,
    ) -> Self { ... }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }
}
```

**`from_listen_and_fallback` 逻辑**：

```
if listen == Some(Web(addr))       → Web { addr }
if listen == Some(Daemon(addr))     → Daemon { addr }
if listen == Some(All { ... })      → All { ... }
if listen == Some(Stdio)            → None
if listen == Some(Off)              → None
if listen == None && web_flag       → Web { addr: 127.0.0.1:web_port }
if listen == None && daemon_flag    → Daemon { addr: 127.0.0.1:daemon_port }
if listen == None && both true      → All { ... }
if listen == None && neither        → None
```

### 4.3 `ServerManager` (`src/server_manager.rs`)

```rust
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use axum::Router;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// 单个 Axum 服务器的句柄。
#[derive(Debug)]
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub cancel: CancellationToken,
}

/// 管理一个或两个 Axum HTTP 服务器的生命周期。
///
/// 职责：
/// 1. 为每个活跃服务器绑定 TCP listener
/// 2. 通过 CancellationToken 实现优雅关闭
/// 3. 协调 Ctrl-C / SIGTERM 跨所有服务器
/// 4. 强制执行 grace period（30s）
pub struct ServerManager {
    mode: ServerMode,
    root_cancel: CancellationToken,
}

impl ServerManager {
    /// 创建一个新的 ServerManager。
    pub fn new(mode: ServerMode) -> Self { ... }

    /// 主入口：启动所有服务器，等待关闭信号。
    ///
    /// 如果 mode 是 None，返回错误。
    /// 当关闭信号触发时，启动优雅关闭。
    pub async fn run(
        self,
        web_router: Router,
        daemon_router: Router,
    ) -> anyhow::Result<()> { ... }

    /// 在后台任务中启动服务器并返回句柄。
    /// 调用者负责在 select! 中等待句柄或 Ctrl-C。
    pub async fn start(
        &self,
        web_router: Router,
        daemon_router: Router,
    ) -> anyhow::Result<(Option<ServerHandle>, Option<ServerHandle>)> { ... }

    /// 发出优雅关闭信号。
    pub fn shutdown(&self) {
        self.root_cancel.cancel();
    }

    /// 获取根取消令牌的只读引用。
    pub fn shutdown_token(&self) -> CancellationToken { ... }

    // --- 内部辅助方法 ---

    /// 启动单个服务器并等待完成或取消。
    async fn serve_with_graceful_shutdown(
        router: Router,
        addr: SocketAddr,
        cancel: CancellationToken,
        label: &'static str,
    ) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("{label} listening on http://{addr}");
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                cancel.cancelled().await;
            })
            .await?;
        Ok(())
    }

    /// 启动单个服务器并在后台任务中运行，返回句柄。
    async fn start_server(
        router: Router,
        addr: SocketAddr,
        child_cancel: CancellationToken,
        label: &'static str,
    ) -> anyhow::Result<ServerHandle> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;
        let cancel = child_cancel.clone();

        tokio::spawn(async move {
            tracing::info!("{label} listening on http://{actual_addr}");
            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    cancel.cancelled().await;
                })
                .await
            {
                tracing::error!("{label} server error: {e}");
            }
        });

        Ok(ServerHandle {
            addr: actual_addr,
            cancel: child_cancel,
        })
    }
}
```

**`run()` 方法内部结构**：

```rust
pub async fn run(self, web_router: Router, daemon_router: Router) -> anyhow::Result<()> {
    match self.mode {
        ServerMode::None => {
            anyhow::bail!("ServerManager::run called with ServerMode::None");
        }
        ServerMode::Web { addr } => {
            self.serve_with_graceful_shutdown(web_router, addr, self.root_cancel, "web").await
        }
        ServerMode::Daemon { addr } => {
            self.serve_with_graceful_shutdown(daemon_router, addr, self.root_cancel, "daemon").await
        }
        ServerMode::All { web_addr, daemon_addr } => {
            let cancel = self.root_cancel.clone();
            // 在 All 模式下，run() 只负责服务器。
            // 后台循环（tick、scheduler、supervisor）由调用者在外部 select! 中处理。
            tokio::select! {
                result = Self::serve_with_graceful_shutdown(
                    web_router, web_addr, cancel.clone(), "web"
                ) => result,
                result = Self::serve_with_graceful_shutdown(
                    daemon_router, daemon_addr, cancel, "daemon"
                ) => result,
            }
        }
    }
}
```

### 4.4 取消令牌层级

```
root_cancel_token (ServerManager)
    ├── web_child_cancel      → Axum serve graceful_shutdown
    ├── daemon_child_cancel   → Axum serve graceful_shutdown
    └── watchdog_child_cancel → 30s grace period 超时监视器
```

### 4.5 `src/lib.rs` — Re-export

```rust
mod server_mode;
mod server_manager;
mod transport;

pub use server_mode::ServerMode;
pub use server_manager::{ServerHandle, ServerManager};
pub use transport::ListenUrl;
```

---

## 5. CLI 变更

### 5.1 在 `crates/allthecodes/src/cli.rs` 中新增 flag

在 `Cli` 结构体中新增：

```rust
/// 统一的服务器监听 URL。
///
/// 示例：
///   web://127.0.0.1:17322           — Web UI 仅
///   daemon://127.0.0.1:19836        — Daemon API 仅
///   all://web=127.0.0.1:17322,daemon=127.0.0.1:19836  — 两者
///   off                             — 无 HTTP 服务器
#[arg(long, hide = true)]
pub listen: Option<String>,
```

**重要：保留所有现有 flags**（`--web`、`--daemon`、`--port`、`--web-port`、`--no-open`）保证向后兼容。新增的 `--listen` flag 目前设为 `hide = true`，直到接口稳定后再公开。

### 5.2 在 `full_init.rs` 中归并

在 `run_full_init()` 顶部，CLI 解析之后立即归并：

```rust
let server_mode = allthecodes_server::ServerMode::from_listen_and_fallback(
    cli.listen.as_ref().and_then(|s| allthecodes_server::ListenUrl::parse(s)),
    cli.web,
    cli.daemon,
    cli.web_port,
    cli.port,
);

// 如果是 headless 模式且 server_mode 为 None，走 headless 路径
// 如果是 TUI 模式且 server_mode 为 None，走 TUI 路径
```

### 5.3 向后兼容行为验证

| 旧用法 | 等效新语法 | 行为 |
|--------|-----------|------|
| `--web` | `--listen web://127.0.0.1:17322` | Web 服务器 |
| `--daemon --port 19836` | `--listen daemon://127.0.0.1:19836` | Daemon 服务器 |
| `--web --daemon`（当前会报错） | `--listen all://web=...,daemon=...` | 两个服务器（新增能力） |
| （无 flag） | `--listen off` | TUI 模式 |

---

## 6. `full_init.rs` 重构

### 6.1 当前模式分支（修改前）

| 行范围 | 模式 | 说明 |
|--------|------|------|
| ~830-840 | `--dump-system-prompt` | 提前返回 |
| ~845-860 | `--version` | 提前返回 |
| ~865-895 | `--init-only` | 提前返回 |
| ~900-918 | `--output-format json` | JSON 输出模式 |
| ~920-928 | `-p` print mode | 打印模式 |
| **~930-944** | **`--web`** | **Web 服务器** |
| **~954-1032** | **`--daemon`** | **Daemon 服务器** |
| ~1036-1043 | `--headless` | Headless IPC |
| ~1046-1077 | (default) | TUI 模式 |

### 6.2 新的统一启动流（修改后）

```rust
// === 移除 B.10 (--web 分支) 和 B.11 (--daemon 分支) ===
// === 替换为统一的服务器启动路径 ===

// 在 B.9 (print mode) 之后，B.10 (old --web) 之前插入：

if server_mode.is_active() {
    // 统一服务器启动路径
    let exit_code = run_server_mode(
        server_mode,
        engine,
        &cli,
        initial_prompt,
    ).await?;

    persist_skill_usage();
    return Ok(exit_code);
}
// 如果 server_mode 是 None，继续到后续分支（headless / TUI）
```

### 6.3 `run_server_mode()` 新函数

```rust
/// 根据 ServerMode 启动一个或多个 HTTP 服务器。
async fn run_server_mode(
    server_mode: ServerMode,
    engine: Arc<QueryEngine>,
    cli: &Cli,
    initial_prompt: Option<String>,
) -> anyhow::Result<ExitCode> {
    // --- 始终构建 Web 状态和路由器（如果需要） ---
    let (web_router, is_streaming) = if matches!(server_mode, ServerMode::Web | ServerMode::All) {
        web::handlers::set_command_provider(allthecodes_commands::get_all_commands);
        let is_streaming = Arc::new(AtomicBool::new(false));
        let web_state = web::state::WebState::new_with_version(
            engine.clone(),
            is_streaming.clone(),
            env!("CARGO_PKG_VERSION"),
        );
        (Some(web::build_router(web_state)), Some(is_streaming))
    } else {
        (None, None)
    };

    // --- 可选地构建 Daemon 状态和路由器 ---
    let (daemon_router, daemon_state) = if matches!(server_mode, ServerMode::Daemon | ServerMode::All) {
        let features = Arc::new(allthecodes_config::features::FLAGS.clone());
        let ds = allthecodes_daemon::state::DaemonState::new(
            engine.clone(),
            features,
            daemon_port(server_mode).unwrap_or(cli.port),
        );
        let dr = allthecodes_daemon::build_router(ds.clone());
        (Some(dr), Some(ds))
    } else {
        (None, None)
    };

    // --- 创建 ServerManager ---
    let manager = allthecodes_server::ServerManager::new(server_mode);

    // --- 根据模式选择启动方式 ---
    match server_mode {
        ServerMode::Web { .. } => {
            let cancel = manager.shutdown_token();
            let ctrlc_token = cancel.clone();
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                ctrlc_token.cancel();
            });
            manager.run(
                web_router.expect("Web mode requires web_router"),
                Router::new(),
            ).await?;
            Ok(ExitCode::SUCCESS)
        }
        ServerMode::Daemon { .. } | ServerMode::All { .. } => {
            run_daemon_with_server(
                manager,
                web_router.unwrap_or(Router::new()),
                daemon_router.expect("Daemon mode requires daemon_router"),
                daemon_state.expect("Daemon mode requires daemon_state"),
                engine,
                cli,
            ).await
        }
        ServerMode::None => {
            unreachable!("run_server_mode called with ServerMode::None");
        }
    }
}
```

### 6.4 `run_daemon_with_server()` 新函数

```rust
/// 启动服务器 + Daemon 后台循环。
async fn run_daemon_with_server(
    manager: allthecodes_server::ServerManager,
    web_router: Router,
    daemon_router: Router,
    daemon_state: allthecodes_daemon::state::DaemonState,
    engine: Arc<QueryEngine>,
    cli: &Cli,
) -> anyhow::Result<ExitCode> {
    // --- 设置 KAIROS 引擎状态 ---
    engine.update_app_state(|app| {
        app.kairos_active = true;
        app.is_assistant_mode = true;
        app.autonomous_tick_ms = Some(30_000);
    });

    // --- 写入进程状态 ---
    let cwd = cli.cwd.as_deref().unwrap_or(".");
    let daemon_port = match manager.mode() {
        ServerMode::Daemon { addr } => addr.port(),
        ServerMode::All { daemon_addr, .. } => daemon_addr.port(),
        _ => cli.port,
    };
    allthecodes_daemon::process_state::write_started(daemon_port, std::path::Path::new(cwd))?;

    // --- 启动后台循环（作为独立任务） ---
    let tick_enabled = allthecodes_config::features::enabled(
        allthecodes_config::features::Feature::Proactive
    );
    let supervisor_cwd = std::path::PathBuf::from(cwd);

    if tick_enabled {
        let tick_state = daemon_state.clone();
        tokio::spawn(async move {
            allthecodes_daemon::tick::tick_loop(tick_state).await;
        });
    }
    {
        let scheduler_state = daemon_state.clone();
        tokio::spawn(async move {
            allthecodes_daemon::scheduler_loop::scheduler_loop(scheduler_state).await;
        });
    }
    let supervisor_handle = tokio::spawn(async move {
        allthecodes_daemon::supervisor::run_supervisor_loop(
            supervisor_cwd, daemon_port
        ).await
    });

    // --- 启动服务器 ---
    let (web_handle, daemon_handle) = manager.start(web_router, daemon_router).await?;
    let cancel = manager.shutdown_token();

    // --- 等待关闭信号 ---
    tokio::select! {
        _ = cancel.cancelled() => {
            tracing::info!("daemon shutdown signal received");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("daemon shutting down via Ctrl-C");
            cancel.cancel();
        }
        // 服务器意外退出时，也关闭 daemon
        Some(Err(e)) = async {
            if let Some(handle) = web_handle {
                handle.join_handle.await.ok()
            } else { None }
        } => {
            tracing::error!("web server exited unexpectedly: {e}");
            cancel.cancel();
        }
        Some(Err(e)) = async {
            if let Some(handle) = daemon_handle {
                handle.join_handle.await.ok()
            } else { None }
        } => {
            tracing::error!("daemon server exited unexpectedly: {e}");
            cancel.cancel();
        }
    }

    // --- 清理 ---
    if let Err(err) = allthecodes_daemon::supervisor::terminate_known_workers() {
        tracing::warn!(error = %err, "failed to terminate daemon workers");
    }
    allthecodes_daemon::process_state::write_stopped(daemon_port, std::path::Path::new(cwd))?;

    Ok(ExitCode::SUCCESS)
}
```

---

## 7. Daemon `server.rs` 重构

### 7.1 当前代码（修改前）

`crates/allthecodes-daemon/src/server.rs` 的 `serve_http` 同时负责构建路由和启动服务器：

```rust
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .merge(routes::team_memory_routes())
        .route("/health", axum::routing::get(routes::health))
        .route("/events", axum::routing::get(sse::sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state.clone())
        .merge(gateway_routes::gateway_routes());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

### 7.2 修改后：提取 `build_router`

```rust
/// 从状态构建 Daemon 路由器。
pub fn build_router(state: DaemonState) -> Router {
    Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .merge(routes::team_memory_routes())
        .route("/health", axum::routing::get(routes::health))
        .route("/events", axum::routing::get(sse::sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state.clone())
        .merge(gateway_routes::gateway_routes())
}

/// [遗留兼容] 在指定端口启动 Daemon HTTP 服务器。
///
/// 新代码应使用 `build_router` + `allthecodes_server::ServerManager` 替代。
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

### 7.3 `lib.rs` 更新

确保 `build_router` 在 crate 的 public API 中导出：

```rust
// crates/allthecodes-daemon/src/lib.rs
pub mod server;
// ... 其他 mod
// re-export for convenience:
pub use server::build_router;
```

---

## 8. Web `mod.rs` 变更

### 8.1 当前代码（几乎不需要修改）

`crates/allthecodes-web/src/mod.rs` 的 `build_router(state)` 已经是一个独立的函数。**不需要修改** — `ServerManager` 直接使用它构建的 `Router`。

### 8.2 可选：添加 `start_server` 包装器

虽然没有破坏性变更，但为了对称性，可以添加一个薄的 `serve_http` 风格包装函数（但 `start_server` 已经存在且签名不同，所以可以让它保持原样）。

### 8.3 添加 Cargo 依赖

在 `crates/allthecodes-web/Cargo.toml` 中添加 `allthecodes-server` 依赖，以便未来使用其类型：

```toml
allthecodes-server = { path = "../allthecodes-server" }
```

---

## 9. 优雅关闭策略

### 9.1 整体流程

```
正常运行
  │
  ├── Ctrl-C 或 SIGTERM
  │     │
  │     └── root_cancel.cancel()
  │           │
  │           ├── Axum serve with_graceful_shutdown 开始关闭
  │           │     ├── 停止接受新连接
  │           │     └── 等待正在处理的请求完成
  │           │
  │           └── 30s 超时监视器启动
  │                 │
  │                 ├── 所有服务器在 30s 内关闭 → 正常退出
  │                 └── 30s 后仍有服务器运行 → 日志警告，强制退出
  │
  └── 后台循环（TICK、Scheduler、Supervisor）
        └── 随进程退出自动终止（tokio::spawn 的任务）
```

### 9.2 关键实现细节

- 使用 `axum::serve(listener, router).with_graceful_shutdown(cancel_token)` 而不是 `TcpListener::bind` + `axum::serve`
- `CancellationToken` 来自 `tokio-util` crate（已在 workspace 依赖中）
- Ctrl-C 信号注册一次，在 `run_daemon_with_server` 中处理（通过 `tokio::signal::ctrl_c()`）

### 9.3 与 TUI 模式的协调

当 `server_mode.is_active()` 为真时，`ServerManager` 负责关闭，`shutdown.rs` 的现有 TUI 关闭路径不运行。
当 `server_mode` 为 `None` 时（TUI 模式），现有关闭路径继续工作。

---

## 10. 迁移路径（9 步）

### Step 1: 创建 `allthecodes-server` crate
- 创建目录结构和 `Cargo.toml`
- 实现 `src/transport.rs`（`ListenUrl` enum + `parse()`）
- 实现 `src/server_mode.rs`（`ServerMode` enum + `from_listen_and_fallback()`）
- 实现 `src/server_manager.rs`（`ServerManager` struct + lifecycle）
- 实现 `src/lib.rs` re-export
- 添加到 workspace `Cargo.toml` members

### Step 2: 添加 `--listen` CLI flag
- 在 `cli.rs` 中添加 `pub listen: Option<String>`
- 在 `allthecodes/Cargo.toml` 中添加 `allthecodes-server` 依赖

### Step 3: 从 daemon `server.rs` 提取 `build_router`
- 添加 `pub fn build_router(state: DaemonState) -> Router`
- 重构 `serve_http` 为调用 `build_router` 的薄包装层
- 在 `lib.rs` 中导出 `build_router`
- 在 `allthecodes-daemon/Cargo.toml` 中添加 `allthecodes-server` 依赖

### Step 4: 添加 `allthecodes-web` 依赖
- 在 `allthecodes-web/Cargo.toml` 中添加 `allthecodes-server` 依赖

### Step 5: 重构 `full_init.rs` — 计算 `server_mode`
- 在 `run_full_init()` 顶部 CLI 解析后立即计算 `server_mode`
- 保持现有 if 分支不变（先验证不影响）

### Step 6: 添加 `run_server_mode()` 和 `run_daemon_with_server()` 新函数
- 实现 `run_server_mode()`（构建 Web/Daemon 路由器，启动服务器）
- 实现 `run_daemon_with_server()`（后台循环 + 服务器协调）

### Step 7: 替换 `--web` 分支
- 将 `~930-944` 行的 `if cli.web { ... }` 替换为 `if server_mode.is_active() { ... }`
- 保留 `if cli.daemon { ... }` 块暂时原样，作为 `ServerMode::Daemon` 的后备
- 编译验证

### Step 8: 替换 `--daemon` 分支
- 将 `~954-1032` 行的 `if cli.daemon { ... }` 也替换为统一路径
- 通过 `server_mode` 区分 Web、Daemon、All 模式
- 编译验证 + 运行测试

### Step 9: 添加测试 + 文档
- 为 `ListenUrl::parse()`、`ServerMode::from_listen_and_fallback()` 添加单元测试
- 添加集成测试（`allthecodes-server/tests/integration.rs`）
- 更新 `development/docs/allthecodes-server-architecture.md`

---

## 11. 测试策略

### 11.1 单元测试

**`transport.rs` 测试：**

| 测试用例 | 输入 | 期望输出 |
|---------|------|---------|
| 解析 Web URL | `"web://127.0.0.1:17322"` | `Ok(Web(127.0.0.1:17322))` |
| 解析 Daemon URL | `"daemon://127.0.0.1:19836"` | `Ok(Daemon(127.0.0.1:19836))` |
| 解析 All URL | `"all://web=127.0.0.1:17322,daemon=127.0.0.1:19836"` | `Ok(All { ... })` |
| 解析 Stdio | `"stdio://"` | `Ok(Stdio)` |
| 解析 Off | `"off"` | `Ok(Off)` |
| 大小写不敏感 Off | `"OFF"` | `Ok(Off)` |
| 空字符串 | `""` | `None` |
| 非法协议 | `"garbage://xyz"` | `None` |
| 非法端口 | `"web://127.0.0.1:99999"` | `None` |

**`server_mode.rs` 测试：**

| 测试用例 | 输入 | 期望输出 |
|---------|------|---------|
| listen overrides all flags | `Some(Web(addr)), web=true, daemon=true` | `Web(addr)` |
| --web only | `None, web=true, daemon=false` | `Web(127.0.0.1:17322)` |
| --daemon only | `None, web=false, daemon=true` | `Daemon(127.0.0.1:19836)` |
| --web --daemon together | `None, web=true, daemon=true` | `All { web: 127.0.0.1:17322, daemon: 127.0.0.1:19836 }` |
| neither | `None, false, false` | `None` |

### 11.2 集成测试

新文件 `crates/allthecodes-server/tests/integration.rs`：

1. **`test_start_web_server`** — 构建含一个健康端点的简单路由器，创建 `ServerMode::Web { "[::1]:0" }`，启动服务器，用 `reqwest` 验证 200 响应，调用 shutdown() 验证服务器在超时内停止

2. **`test_start_both_servers`** — 构建两个不同前缀的简单路由器，创建 `ServerMode::All`（两个 `:0` 端口），验证两个服务器都响应，关机后验证两者停止

3. **`test_graceful_shutdown_drains_requests`** — 启动含 100ms 延迟端点的服务器，发送请求，立即发送关闭信号，验证请求完成（收到 200 非连接重置）

### 11.3 手动验证清单

- [ ] `cargo build --workspace` 编译成功
- [ ] `--web` 在 17322 启动 Web 服务器（不变）
- [ ] `--daemon` 在 19836 启动 Daemon 服务器（不变）
- [ ] `--listen web://127.0.0.1:17322` 启动 Web 服务器
- [ ] `--listen daemon://127.0.0.1:19836` 启动 Daemon 服务器
- [ ] `--listen all://web=...,daemon=...` 同时启动两者
- [ ] `--listen off` 进入 TUI 模式
- [ ] `npm run dev` + 前端操作确认所有 API 调用正常
- [ ] Ctrl-C 优雅关闭两个服务器
- [ ] 两种模式下静态文件服务正常工作
- [ ] Daemon 模式下 SSE 和 Gateway 路由正常工作
- [ ] `cargo clippy --workspace --lib --bins` 无新增警告

---

## 12. 文件改动清单

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `crates/allthecodes-server/Cargo.toml` | **新建** | 新 crate 清单 |
| `crates/allthecodes-server/src/lib.rs` | **新建** | 模块声明与 re-export |
| `crates/allthecodes-server/src/transport.rs` | **新建** | `ListenUrl` enum + 解析器 |
| `crates/allthecodes-server/src/server_mode.rs` | **新建** | `ServerMode` enum + `from_listen_and_fallback()` |
| `crates/allthecodes-server/src/server_manager.rs` | **新建** | `ServerManager` struct + lifecycle |
| `crates/allthecodes-server/tests/integration.rs` | **新建** | 集成测试 |
| `Cargo.toml` (workspace root) | **修改** | 添加 `allthecodes-server` member 和 dep |
| `crates/allthecodes/src/cli.rs` | **修改** | 添加 `--listen` flag（`hide = true`） |
| `crates/allthecodes/src/full_init.rs` | **修改** | 替换 `--web`/`--daemon` 分支为统一路径，添加 `run_server_mode()`、`run_daemon_with_server()` |
| `crates/allthecodes/Cargo.toml` | **修改** | 添加 `allthecodes-server` dep |
| `crates/allthecodes-web/src/mod.rs` | **无变更**（可选添加 `serve_http` 包装） | `build_router` 保持原样 |
| `crates/allthecodes-web/Cargo.toml` | **修改** | 添加 `allthecodes-server` dep |
| `crates/allthecodes-daemon/src/server.rs` | **修改** | 提取 `build_router()`；`serve_http` 保持为薄包装 |
| `crates/allthecodes-daemon/src/lib.rs` | **修改** | 导出 `build_router` |
| `crates/allthecodes-daemon/Cargo.toml` | **修改** | 添加 `allthecodes-server` dep |

### 新增文件统计

- **新建**：5 个文件（4 个源文件 + 1 个测试文件）
- **修改**：7 个文件
- **无变更**：1 个文件（`allthecodes-web/src/mod.rs`）

### 预估行数

| 文件 | 预估行数 |
|------|---------|
| `transport.rs` | ~80 行 |
| `server_mode.rs` | ~60 行 |
| `server_manager.rs` | ~150 行 |
| `lib.rs` (server) | ~5 行 |
| `full_init.rs` 新增代码 | ~150 行 |
| `daemon/server.rs` 新增代码 | ~20 行 |
| `integration.rs` | ~100 行 |
| **总计** | **~565 行新增代码** |

---

## 13. 风险与缓解

### 13.1 端口绑定冲突

**风险**：在 `ServerMode::All` 中，两个服务器同时调用 `TcpListener::bind()`。如果它们绑定到相同端口，第二个 bind 会失败。

**缓解**：`ServerMode::All` 通过构造强制使用不同端口，`ListenUrl::parse` 可以验证 web_addr 和 daemon_addr 的端口不同。

### 13.2 Daemon 后台循环生命周期

**风险**：Tick 循环、Scheduler 循环、Supervisor 循环是无限 `loop { ... }` 模式。如果其中一个意外退出，可能会关闭整个进程。

**缓解**：
- 在 Phase 1 中，这些循环通过 `tokio::spawn` 作为独立任务运行，不会直接导致进程退出
- `select!` 只等待服务器句柄和 Ctrl-C，不等待后台循环
- 未来可以添加重启逻辑

### 13.3 WebSocket / SSE 连接排空

**风险**：当服务器停止接受新连接时，现有的 WebSocket 和 SSE 连接需要优雅排空。

**缓解**：`axum::serve(...).with_graceful_shutdown(...)` 处理此情况 — 服务器停止接受新请求但完成正在进行的请求，包括 WebSocket 帧。

### 13.4 共享引擎状态并发

**风险**：当两个服务器同时运行时，它们共享同一个 `Arc<QueryEngine>`。`WebState` 的 `engine_slot` 是 `Arc<RwLock<Arc<QueryEngine>>>`，`DaemonState` 的 `engine` 是 `Arc<QueryEngine>`。可能存在并发问题。

**缓解**：
- `WebState::engine()` 对 `engine_slot.read()` 快照，然后在整个请求期间使用 `Arc<QueryEngine>`。并发读不会阻塞
- `WebState::replace_engine()` 使用 `write()` 锁，但在非 Web 模式下不运行
- Daemon 模式在任何现有部署中已经是单用户模式

### 13.5 `--web --daemon` 同时设置

**现状**：当前代码不允许同时设置 `--web` 和 `--daemon`（因为它们是互斥分支）。

**新行为**：在 Phase 1 中，同时设置两者会映射到 `ServerMode::All { 127.0.0.1:17322, 127.0.0.1:19836 }`。这是一个向后兼容的增强，不是破坏性变更。如果用户依赖当前行为（只有一个启动），他们的脚本继续工作，因为只有 `--web` 或 `--daemon` 被设置。

---

## 附录：相关文件引用

| 文件 | 用途 |
|------|------|
| `crates/allthecodes/src/cli.rs` | Phase A CLI 参数定义 — `--listen` flag 添加于此 |
| `crates/allthecodes/src/full_init.rs` | Phase B 初始化 — 主要重构目标 |
| `crates/allthecodes-web/src/mod.rs` | Web 服务器路由构建 — 保持 `build_router()` 不变 |
| `crates/allthecodes-web/src/state.rs` | `WebState` — 不变化 |
| `crates/allthecodes-web/src/handler_registry.rs` | Handler 注册系统 — 不变化 |
| `crates/allthecodes-daemon/src/server.rs` | Daemon 服务器 — 提取 `build_router()` |
| `crates/allthecodes-daemon/src/state.rs` | `DaemonState` — 不变化 |
| `crates/allthecodes-daemon/src/routes.rs` | Daemon 路由 handlers — 不变化 |
| `crates/allthecodes-daemon/src/gateway_routes.rs` | Gateway 路由挂载 — 不变化 |
| `crates/allthecodes-gateway/src/api.rs` | Gateway API — 不变化 |
| `Cargo.toml` (workspace) | Workspace 成员与依赖声明 |
