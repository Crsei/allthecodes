# Team Memory 最小实现 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Rust daemon 上代理转发 `/api/claude_code/team_memory` 到独立的 Bun TS 服务，TS 服务用 `bun:sqlite` 存储团队共享记忆。

**Architecture:** Rust daemon (axum) 启动时 spawn Bun 子进程运行 `ui/team-memory-server/index.ts`，通过共享密钥认证。daemon 收到 `/api/claude_code/team_memory` 请求后，用 reqwest 转发到 TS 服务。TS 服务使用 SQLite 存储，ETag 乐观锁处理并发。

**Tech Stack:** Rust (axum, reqwest, uuid, tokio), TypeScript/Bun (bun:sqlite, Bun.serve), SQLite

**Spec:** [`../specs/2026-04-11-team-memory-design.md`](../specs/2026-04-11-team-memory-design.md)

---

## File Structure

### Rust 端（修改）

| 文件 | 职责 |
|------|------|
| Create: `src/daemon/team_memory_proxy.rs` | TS 子进程生命周期管理 + reqwest 转发 |
| Modify: `src/daemon/mod.rs` | 新增 `pub mod team_memory_proxy;` |
| Modify: `src/daemon/state.rs` | DaemonState 新增 team_memory_port / secret 字段 |
| Modify: `src/daemon/routes.rs` | 注册 `/api/claude_code/team_memory` 路由 |
| Modify: `src/daemon/server.rs` | 启动时 spawn TS 进程、merge team_memory 路由 |
| Modify: `src/config/features.rs` | 新增 `Feature::TeamMemory` + `FEATURE_TEAMMEM` |

### TS 端（新建）

| 文件 | 职责 |
|------|------|
| Create: `ui/team-memory-server/index.ts` | Bun.serve 入口、CLI 参数解析、密钥中间件 |
| Create: `ui/team-memory-server/db.ts` | SQLite 初始化 + CRUD 操作 |
| Create: `ui/team-memory-server/routes.ts` | GET / PUT 端点处理逻辑 |

---

## Task 1: Feature Gate — 添加 `Feature::TeamMemory`

**Files:**
- Modify: `src/config/features.rs`

- [ ] **Step 1: 添加 TeamMemory 到 Feature enum 和 FeatureFlags**

在 `src/config/features.rs` 中：

```rust
// Feature enum 新增：
pub enum Feature {
    Kairos,
    KairosBrief,
    KairosChannels,
    KairosPushNotification,
    KairosGithubWebhooks,
    Proactive,
    TeamMemory,  // 新增
}

// FeatureFlags 新增字段：
pub struct FeatureFlags {
    pub kairos: bool,
    pub kairos_brief: bool,
    pub kairos_channels: bool,
    pub kairos_push_notification: bool,
    pub kairos_github_webhooks: bool,
    pub proactive: bool,
    pub team_memory: bool,  // 新增
}
```

在 `from_iter()` 中读取：

```rust
let team_memory = read("FEATURE_TEAMMEM");
```

加到 `Self { ... }` 返回值中。

在 `is_enabled()` 中添加：

```rust
Feature::TeamMemory => self.team_memory,
```

- [ ] **Step 2: 添加测试**

```rust
#[test]
fn team_memory_standalone() {
    let f = flags(&[("FEATURE_TEAMMEM", "1")]);
    assert!(f.team_memory);
    assert!(!f.kairos, "team_memory should not imply kairos");
}

#[test]
fn team_memory_default_off() {
    let f = flags(&[]);
    assert!(!f.team_memory);
}
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p cc-rust config::features::tests`
Expected: 所有测试 PASS

- [ ] **Step 4: Commit**

```bash
git add src/config/features.rs
git commit -m "feat(team-memory): add Feature::TeamMemory gate (FEATURE_TEAMMEM)"
```

---

## Task 2: TS 端 — SQLite 数据库操作 (`db.ts`)

**Files:**
- Create: `ui/team-memory-server/db.ts`

- [ ] **Step 1: 创建 `ui/team-memory-server/` 目录**

```bash
mkdir -p ui/team-memory-server
```

- [ ] **Step 2: 编写 `db.ts`**

```typescript
// ui/team-memory-server/db.ts
import { Database } from "bun:sqlite";
import { createHash } from "crypto";

let db: Database;

export function init(dbPath: string): void {
  db = new Database(dbPath, { create: true });
  db.exec("PRAGMA journal_mode=WAL");
  db.exec("PRAGMA foreign_keys=ON");
  db.exec(`
    CREATE TABLE IF NOT EXISTS team_memory (
      repo       TEXT NOT NULL,
      key        TEXT NOT NULL,
      content    TEXT NOT NULL,
      checksum   TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      PRIMARY KEY (repo, key)
    )
  `);
  db.exec(`
    CREATE TABLE IF NOT EXISTS repo_meta (
      repo       TEXT PRIMARY KEY,
      version    INTEGER NOT NULL DEFAULT 1,
      checksum   TEXT NOT NULL,
      updated_at TEXT NOT NULL
    )
  `);
}

export function close(): void {
  db?.close();
}

export function getRepoMeta(repo: string): { version: number; checksum: string; updated_at: string } | null {
  return db.query("SELECT version, checksum, updated_at FROM repo_meta WHERE repo = ?").get(repo) as any;
}

export function getEntries(repo: string): Record<string, string> {
  const rows = db.query("SELECT key, content FROM team_memory WHERE repo = ?").all(repo) as { key: string; content: string }[];
  const result: Record<string, string> = {};
  for (const row of rows) {
    result[row.key] = row.content;
  }
  return result;
}

export function getEntryChecksums(repo: string): Record<string, string> {
  const rows = db.query("SELECT key, checksum FROM team_memory WHERE repo = ?").all(repo) as { key: string; checksum: string }[];
  const result: Record<string, string> = {};
  for (const row of rows) {
    result[row.key] = row.checksum;
  }
  return result;
}

export function getEntryCount(repo: string): number {
  const row = db.query("SELECT COUNT(*) as cnt FROM team_memory WHERE repo = ?").get(repo) as { cnt: number };
  return row.cnt;
}

export function hasEntry(repo: string, key: string): boolean {
  const row = db.query("SELECT 1 FROM team_memory WHERE repo = ? AND key = ?").get(repo, key);
  return row !== null;
}

function sha256(content: string): string {
  return `sha256:${createHash("sha256").update(content, "utf-8").digest("hex")}`;
}

export function computeGlobalChecksum(repo: string): string {
  const checksums = getEntryChecksums(repo);
  const sorted = Object.entries(checksums)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, hash]) => `${key}:${hash}`)
    .join("\n");
  return sha256(sorted);
}

export function upsertEntries(repo: string, entries: Record<string, string>): string {
  const now = new Date().toISOString();
  const upsert = db.prepare(
    "INSERT INTO team_memory (repo, key, content, checksum, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(repo, key) DO UPDATE SET content=excluded.content, checksum=excluded.checksum, updated_at=excluded.updated_at"
  );
  const upsertMeta = db.prepare(
    "INSERT INTO repo_meta (repo, version, checksum, updated_at) VALUES (?, 1, ?, ?) ON CONFLICT(repo) DO UPDATE SET version=version+1, checksum=excluded.checksum, updated_at=excluded.updated_at"
  );

  const tx = db.transaction(() => {
    for (const [key, content] of Object.entries(entries)) {
      const checksum = sha256(content);
      upsert.run(repo, key, content, checksum, now);
    }
    const globalChecksum = computeGlobalChecksum(repo);
    upsertMeta.run(repo, globalChecksum, now);
    return globalChecksum;
  });

  return tx();
}
```

- [ ] **Step 3: 验证语法**

Run: `cd ui && bun build team-memory-server/db.ts --no-bundle --outdir /tmp/tm-check`
Expected: 编译成功无错误

- [ ] **Step 4: Commit**

```bash
git add ui/team-memory-server/db.ts
git commit -m "feat(team-memory): add SQLite database layer (db.ts)"
```

---

## Task 3: TS 端 — 路由处理 (`routes.ts`)

**Files:**
- Create: `ui/team-memory-server/routes.ts`

- [ ] **Step 1: 编写 `routes.ts`**

```typescript
// ui/team-memory-server/routes.ts
import * as db from "./db";

const MAX_ENTRIES = 500;
const MAX_ENTRY_SIZE = 250_000;
const MAX_PUT_BODY = 512_000;

export async function handleGet(url: URL, req: Request): Promise<Response> {
  const repo = url.searchParams.get("repo");
  if (!repo) {
    return Response.json({ error: "missing repo parameter" }, { status: 400 });
  }

  const view = url.searchParams.get("view");
  const meta = db.getRepoMeta(repo);
  const currentChecksum = meta?.checksum ?? "";

  // Conditional request: If-None-Match
  const ifNoneMatch = req.headers.get("if-none-match");
  if (meta && ifNoneMatch && ifNoneMatch === currentChecksum) {
    return new Response(null, { status: 304, headers: { ETag: currentChecksum } });
  }

  if (view === "hashes") {
    const entryChecksums = db.getEntryChecksums(repo);
    return Response.json(
      { repo, checksum: currentChecksum, entryChecksums },
      { headers: { ETag: currentChecksum } }
    );
  }

  const entries = db.getEntries(repo);
  const entryChecksums = db.getEntryChecksums(repo);
  return Response.json(
    {
      repo,
      version: meta?.version ?? 0,
      lastModified: meta?.updated_at ?? "",
      checksum: currentChecksum,
      content: { entries, entryChecksums },
    },
    { headers: { ETag: currentChecksum } }
  );
}

export async function handlePut(url: URL, req: Request): Promise<Response> {
  const repo = url.searchParams.get("repo");
  if (!repo) {
    return Response.json({ error: "missing repo parameter" }, { status: 400 });
  }

  // Body size check
  const contentLength = parseInt(req.headers.get("content-length") ?? "0", 10);
  if (contentLength > MAX_PUT_BODY) {
    return Response.json({ error: "body too large" }, { status: 413 });
  }

  // ETag check
  const ifMatch = req.headers.get("if-match");
  const meta = db.getRepoMeta(repo);
  if (meta && ifMatch !== meta.checksum && ifMatch !== "*") {
    return Response.json(
      { error: "checksum_mismatch", current_checksum: meta.checksum },
      { status: 412 }
    );
  }
  // First write: allow if no meta exists (ifMatch can be "" or "*" or absent)
  if (!meta && ifMatch && ifMatch !== "" && ifMatch !== "*") {
    return Response.json(
      { error: "checksum_mismatch", current_checksum: "" },
      { status: 412 }
    );
  }

  const body = (await req.json()) as { entries: Record<string, string> };
  if (!body.entries || typeof body.entries !== "object") {
    return Response.json({ error: "invalid body: entries required" }, { status: 400 });
  }

  // Per-entry size check
  for (const [key, content] of Object.entries(body.entries)) {
    if (typeof content !== "string") {
      return Response.json({ error: `invalid entry: ${key} must be string` }, { status: 400 });
    }
    if (new TextEncoder().encode(content).byteLength > MAX_ENTRY_SIZE) {
      return Response.json({ error: `entry too large: ${key}` }, { status: 413 });
    }
  }

  // Capacity check
  const currentCount = db.getEntryCount(repo);
  const newKeys = Object.keys(body.entries).filter((k) => !db.hasEntry(repo, k));
  if (currentCount + newKeys.length > MAX_ENTRIES) {
    return Response.json(
      { error: "too_many_entries", extra_details: { max_entries: MAX_ENTRIES } },
      { status: 413 }
    );
  }

  const newChecksum = db.upsertEntries(repo, body.entries);
  return Response.json({ checksum: newChecksum }, { headers: { ETag: newChecksum } });
}
```

- [ ] **Step 2: 验证语法**

Run: `cd ui && bun build team-memory-server/routes.ts --no-bundle --outdir /tmp/tm-check`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add ui/team-memory-server/routes.ts
git commit -m "feat(team-memory): add GET/PUT route handlers (routes.ts)"
```

---

## Task 4: TS 端 — 服务入口 (`index.ts`)

**Files:**
- Create: `ui/team-memory-server/index.ts`

- [ ] **Step 1: 编写 `index.ts`**

```typescript
// ui/team-memory-server/index.ts
import * as db from "./db";
import { handleGet, handlePut } from "./routes";
import { join } from "path";
import { homedir } from "os";

// --- CLI args parsing ---
function parseArgs(): { port: number; secret: string } {
  const args = process.argv.slice(2);
  let port = 19837;
  let secret = "";
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--port" && args[i + 1]) {
      port = parseInt(args[i + 1], 10);
      i++;
    } else if (args[i] === "--secret" && args[i + 1]) {
      secret = args[i + 1];
      i++;
    }
  }
  if (!secret) {
    console.error("error: --secret is required");
    process.exit(1);
  }
  return { port, secret };
}

const { port, secret } = parseArgs();

// --- Database init ---
const dataDir = join(homedir(), ".cc-rust");
// Ensure directory exists
const { mkdirSync } = await import("fs");
mkdirSync(dataDir, { recursive: true });

const dbPath = join(dataDir, "team-memory.db");
db.init(dbPath);

// --- Graceful shutdown ---
process.on("SIGTERM", () => {
  console.log("team-memory-server: shutting down");
  db.close();
  process.exit(0);
});
process.on("SIGINT", () => {
  db.close();
  process.exit(0);
});

// --- HTTP Server ---
const server = Bun.serve({
  port,
  hostname: "127.0.0.1",
  async fetch(req) {
    const url = new URL(req.url);

    // Health endpoint (no auth)
    if (url.pathname === "/health") {
      return Response.json({ status: "ok" });
    }

    // Shared secret auth
    if (req.headers.get("x-team-memory-secret") !== secret) {
      return Response.json({ error: "unauthorized" }, { status: 401 });
    }

    // Route dispatch
    if (url.pathname === "/api/claude_code/team_memory") {
      if (req.method === "GET") return handleGet(url, req);
      if (req.method === "PUT") return handlePut(url, req);
      return Response.json({ error: "method not allowed" }, { status: 405 });
    }

    return Response.json({ error: "not found" }, { status: 404 });
  },
});

console.log(`team-memory-server listening on http://127.0.0.1:${server.port}`);
```

- [ ] **Step 2: 验证可以启动**

Run: `cd ui && bun run team-memory-server/index.ts --port 19837 --secret test123`
Expected: 输出 `team-memory-server listening on http://127.0.0.1:19837`，然后 Ctrl+C 退出

- [ ] **Step 3: 端到端快速验证**

在另一个终端：

```bash
# Health
curl http://127.0.0.1:19837/health
# -> {"status":"ok"}

# PUT (first write)
curl -X PUT "http://127.0.0.1:19837/api/claude_code/team_memory?repo=test/repo" \
  -H "x-team-memory-secret: test123" \
  -H "If-Match: *" \
  -H "Content-Type: application/json" \
  -d '{"entries":{"readme.md":"# Hello"}}'
# -> {"checksum":"sha256:..."}

# GET
curl "http://127.0.0.1:19837/api/claude_code/team_memory?repo=test/repo" \
  -H "x-team-memory-secret: test123"
# -> full response with entries

# GET hashes
curl "http://127.0.0.1:19837/api/claude_code/team_memory?repo=test/repo&view=hashes" \
  -H "x-team-memory-secret: test123"
# -> checksums only
```

- [ ] **Step 4: Commit**

```bash
git add ui/team-memory-server/index.ts
git commit -m "feat(team-memory): add Bun HTTP server entry point (index.ts)"
```

---

## Task 5: Rust 端 — DaemonState 扩展

**Files:**
- Modify: `src/daemon/state.rs`

- [ ] **Step 1: 添加 team memory 字段到 DaemonState**

在 `DaemonState` struct 中新增：

```rust
pub struct DaemonState {
    // ... 现有字段 ...
    pub port: u16,

    // Team memory proxy (populated when Feature::TeamMemory is enabled)
    pub team_memory_port: Option<u16>,
    pub team_memory_secret: Option<String>,
}
```

在 `DaemonState::new()` 中初始化为 `None`：

```rust
pub fn new(engine: Arc<QueryEngine>, features: Arc<FeatureFlags>, port: u16) -> Self {
    let (notification_tx, notification_rx) = mpsc::unbounded_channel();
    Self {
        // ... 现有字段 ...
        port,
        team_memory_port: None,
        team_memory_secret: None,
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo build 2>&1 | head -20`
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add src/daemon/state.rs
git commit -m "feat(team-memory): extend DaemonState with team_memory_port/secret"
```

---

## Task 6: Rust 端 — Team Memory Proxy 模块

**Files:**
- Create: `src/daemon/team_memory_proxy.rs`
- Modify: `src/daemon/mod.rs`

- [ ] **Step 1: 添加模块声明**

在 `src/daemon/mod.rs` 末尾添加：

```rust
pub mod team_memory_proxy;
```

- [ ] **Step 2: 编写 `team_memory_proxy.rs`**

```rust
//! Team Memory proxy: spawns a Bun TS subprocess and forwards HTTP requests.

use std::process::Stdio;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::process::{Child, Command};
use tracing::{error, info, warn};

use super::state::DaemonState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HEALTH_CHECK_TIMEOUT_MS: u64 = 5000;
const HEALTH_CHECK_INTERVAL_MS: u64 = 100;

// ---------------------------------------------------------------------------
// Subprocess lifecycle
// ---------------------------------------------------------------------------

/// Spawn the Bun team-memory-server subprocess.
///
/// Returns `(child, port, secret)` on success.
pub async fn spawn_team_memory_server(
    base_port: u16,
) -> anyhow::Result<(Child, u16, String)> {
    let port = base_port + 1;
    let secret = uuid::Uuid::new_v4().to_string();

    // Resolve the script path relative to the binary location.
    let exe_dir = std::env::current_exe()?
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    // Try multiple candidate paths for the TS server script.
    let candidates = [
        exe_dir.join("../ui/team-memory-server/index.ts"),
        exe_dir.join("../../ui/team-memory-server/index.ts"),
        std::path::PathBuf::from("ui/team-memory-server/index.ts"),
    ];
    let script_path = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .unwrap_or_else(|| candidates.last().unwrap().clone());

    info!(
        port,
        script = %script_path.display(),
        "spawning team-memory-server"
    );

    let child = Command::new("bun")
        .arg("run")
        .arg(&script_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--secret")
        .arg(&secret)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // Wait for health check.
    let health_url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::new();
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(HEALTH_CHECK_TIMEOUT_MS);

    loop {
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "team-memory-server failed to start within {}ms",
                HEALTH_CHECK_TIMEOUT_MS
            );
        }
        match client.get(&health_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(port, "team-memory-server is ready");
                break;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS))
                    .await;
            }
        }
    }

    Ok((child, port, secret))
}

// ---------------------------------------------------------------------------
// Proxy handler
// ---------------------------------------------------------------------------

/// Proxy handler for `/api/claude_code/team_memory`.
///
/// Forwards the request to the Bun TS subprocess, transparently relaying
/// method, query string, headers (If-Match, If-None-Match), and body.
pub async fn proxy_team_memory(
    State(state): State<DaemonState>,
    method: Method,
    query: Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let port = match state.team_memory_port {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_GATEWAY,
                "team-memory-server not available",
            )
                .into_response();
        }
    };
    let secret = match &state.team_memory_secret {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::BAD_GATEWAY,
                "team-memory-server not configured",
            )
                .into_response();
        }
    };

    // Build query string.
    let qs: String = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let url = format!(
        "http://127.0.0.1:{}/api/claude_code/team_memory?{}",
        port, qs
    );

    let client = reqwest::Client::new();
    let mut req = client
        .request(method.clone(), &url)
        .header("X-Team-Memory-Secret", &secret);

    // Forward relevant headers.
    if let Some(v) = headers.get("if-match") {
        req = req.header("If-Match", v.to_str().unwrap_or(""));
    }
    if let Some(v) = headers.get("if-none-match") {
        req = req.header("If-None-Match", v.to_str().unwrap_or(""));
    }

    // Forward body for PUT.
    if method == Method::PUT {
        req = req
            .header("Content-Type", "application/json")
            .body(body);
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut builder = axum::http::Response::builder().status(status);

            // Forward ETag header from TS response.
            if let Some(etag) = resp.headers().get("etag") {
                builder = builder.header("ETag", etag);
            }
            builder = builder.header("Content-Type", "application/json");

            let body_bytes = resp.bytes().await.unwrap_or_default();
            builder
                .body(axum::body::Body::from(body_bytes))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "response build error")
                        .into_response()
                })
        }
        Err(e) => {
            error!(error = %e, "failed to proxy to team-memory-server");
            (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response()
        }
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `cargo build 2>&1 | head -20`
Expected: 编译成功（可能需要添加 `urlencoding` 依赖）

- [ ] **Step 4: 如果需要 `urlencoding`，添加依赖**

Run: `cargo add urlencoding`

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs src/daemon/team_memory_proxy.rs
git commit -m "feat(team-memory): add proxy module with subprocess lifecycle + HTTP forwarding"
```

---

## Task 7: Rust 端 — 路由注册 + 服务启动集成

**Files:**
- Modify: `src/daemon/routes.rs`
- Modify: `src/daemon/server.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: 在 `routes.rs` 注册 team_memory 路由**

在 `routes.rs` 顶部添加 import：

```rust
use super::team_memory_proxy;
```

新增一个函数：

```rust
/// Returns a [`Router`] containing the team memory proxy route.
pub fn team_memory_routes() -> Router<DaemonState> {
    Router::new().route(
        "/api/claude_code/team_memory",
        get(team_memory_proxy::proxy_team_memory)
            .put(team_memory_proxy::proxy_team_memory),
    )
}
```

- [ ] **Step 2: 在 `server.rs` 中 merge team_memory 路由**

修改 `serve_http()` 中的 Router 构建：

```rust
pub async fn serve_http(state: DaemonState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .merge(routes::api_routes())
        .merge(routes::webhook_routes())
        .merge(routes::team_memory_routes())   // 新增
        .route("/health", axum::routing::get(routes::health))
        .route("/events", axum::routing::get(sse::sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // ... 其余不变 ...
}
```

- [ ] **Step 3: 在 `main.rs` daemon 启动流程中 spawn TS 进程**

在 `src/main.rs` 的 daemon 模式块中（`if cli.daemon {` 之后），在创建 `daemon_state` 之后、进入 `tokio::select!` 之前，添加：

```rust
// Spawn team-memory-server if feature is enabled.
let _team_memory_child = if features::enabled(Feature::TeamMemory) {
    match daemon::team_memory_proxy::spawn_team_memory_server(cli.port).await {
        Ok((child, tm_port, tm_secret)) => {
            daemon_state.team_memory_port = Some(tm_port);
            daemon_state.team_memory_secret = Some(tm_secret);
            info!(port = tm_port, "team-memory-server started");
            Some(child)
        }
        Err(e) => {
            warn!(error = %e, "failed to start team-memory-server, feature disabled");
            None
        }
    }
} else {
    None
};
```

注意：`daemon_state` 必须在此处是 `mut`，在 `let daemon_state = ...` 声明时改为 `let mut daemon_state = ...`。

- [ ] **Step 4: 编译验证**

Run: `cargo build 2>&1 | head -20`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add src/daemon/routes.rs src/daemon/server.rs src/main.rs
git commit -m "feat(team-memory): wire proxy routes and subprocess spawn into daemon startup"
```

---

## Task 8: 端到端集成测试

**Files:**
- 无新文件，手动验证

- [ ] **Step 1: 启动 daemon + team memory**

```bash
FEATURE_KAIROS=1 FEATURE_TEAMMEM=1 cargo run -- --daemon --port 19836
```

Expected: 日志显示 `team-memory-server is ready` 和 `daemon HTTP server listening on 127.0.0.1:19836`

- [ ] **Step 2: 测试 PUT（首次写入）**

```bash
curl -s -X PUT "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo" \
  -H "If-Match: *" \
  -H "Content-Type: application/json" \
  -d '{"entries":{"readme.md":"# Hello Team","conventions.md":"## Rules\n1. Use Rust"}}'
```

Expected: `{"checksum":"sha256:..."}`

- [ ] **Step 3: 测试 GET（全量）**

```bash
curl -s "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo"
```

Expected: 返回完整的 `content.entries` 和 `content.entryChecksums`

- [ ] **Step 4: 测试 GET hashes**

```bash
curl -s "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo&view=hashes"
```

Expected: 只返回 `checksum` 和 `entryChecksums`，无 `entries`

- [ ] **Step 5: 测试 304 Not Modified**

取上一步返回的 `checksum` 值：

```bash
curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo" \
  -H "If-None-Match: sha256:<之前返回的值>"
```

Expected: `304`

- [ ] **Step 6: 测试 412 ETag 冲突**

```bash
curl -s -o /dev/null -w "%{http_code}" -X PUT "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo" \
  -H "If-Match: sha256:wrong" \
  -H "Content-Type: application/json" \
  -d '{"entries":{"new.md":"content"}}'
```

Expected: `412`

- [ ] **Step 7: 测试 PUT upsert（增量更新）**

先 GET 拿到当前 checksum，然后：

```bash
curl -s -X PUT "http://127.0.0.1:19836/api/claude_code/team_memory?repo=test/repo" \
  -H "If-Match: <当前checksum>" \
  -H "Content-Type: application/json" \
  -d '{"entries":{"readme.md":"# Updated Hello"}}'
```

Expected: 200 + 新 checksum，GET 验证 readme.md 内容已更新，conventions.md 不变

- [ ] **Step 8: Commit 清理**

```bash
# 删除测试数据库
rm -f ~/.cc-rust/team-memory.db
```

---

## Task 9: 解决 warnings + 最终验证

**Files:**
- 可能涉及所有新增/修改的文件

- [ ] **Step 1: 编译检查 warnings**

Run: `cargo build 2>&1`
检查所有 warning，逐一修复（未使用的 import、变量等）

- [ ] **Step 2: 运行完整测试套件**

Run: `cargo test`
Expected: 所有测试 PASS

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "chore(team-memory): resolve warnings and cleanup"
```
