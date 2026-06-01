# Team Memory 最小实现设计

> 日期: 2026-04-11
> 状态: 已批准

## 一、概述

基于 GitHub 仓库的团队共享记忆系统。复用 Rust daemon 端口 (127.0.0.1:19836) 作为统一入口，转发 `/api/claude_code/team_memory` 请求到独立的 TypeScript (Bun) 服务进程，后端使用 SQLite 存储。

## 二、架构

```
前端 (ink-terminal)
    │
    │  POST/GET /api/claude_code/team_memory
    ▼
Rust Daemon (axum, 127.0.0.1:19836)
    │
    │  reqwest 转发 (X-Team-Memory-Secret header)
    ▼
TS 服务 (Bun, 127.0.0.1:{随机端口})
    │
    │  bun:sqlite
    ▼
~/.cc-rust/team-memory.db (SQLite)
```

### 关键决策

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 入口方式 | Rust daemon 代理转发 (方案 A) | 统一端口，前端无需连两个服务 |
| 存储 | SQLite (`bun:sqlite`) | 原子写入、并发安全、单文件 |
| 认证 | 共享密钥 (UUID) | daemon 和 TS 服务间验证，防止外部直连 TS 端口 |
| 生命周期 | Rust daemon spawn 子进程 | 对用户透明，启动/退出自动管理 |

## 三、生命周期管理

### 3.1 启动流程

1. Rust daemon 启动时：
   - 生成随机 UUID 作为共享密钥
   - 选择一个可用端口（OS 分配或固定偏移如 `daemon_port + 1`）
   - `Command::new("bun").args(["run", "team-memory-server/index.ts", "--port", &port, "--secret", &secret])` 启动子进程
   - 重定向 stdout/stderr 到日志
2. 健康检查：轮询 `GET http://127.0.0.1:{port}/health`，最多等 5 秒（100ms 间隔）
3. 健康检查通过后，记录 TS 服务端口到 `DaemonState`

### 3.2 关闭流程

1. Rust daemon 收到 Ctrl+C / 退出信号
2. 向子进程发送 SIGTERM (Unix) / `kill()` (Windows)
3. 等待最多 3 秒，超时则强制 kill

### 3.3 故障处理

- TS 进程崩溃：转发请求返回 502 Bad Gateway
- 启动超时（5s 内无 /health 200）：日志警告，team memory 功能不可用，其他 daemon 功能正常

## 四、API 设计

### 4.1 路由总览

Rust daemon 新增 3 条路由，全部透传到 TS 服务：

| 路由 | 方法 | 功能 |
|------|------|------|
| `/api/claude_code/team_memory?repo={owner/repo}` | GET | 返回全量 entries + per-entry checksums |
| `/api/claude_code/team_memory?repo={owner/repo}&view=hashes` | GET | 仅返回 checksums（轻量冲突探测） |
| `/api/claude_code/team_memory?repo={owner/repo}` | PUT | Upsert entries，ETag 并发控制 |

### 4.2 GET（全量）

**请求**: `GET /api/claude_code/team_memory?repo=owner/repo`

**响应 200**:
```json
{
  "repo": "owner/repo",
  "version": 1,
  "lastModified": "2026-04-11T10:00:00Z",
  "checksum": "sha256:abc123...",
  "content": {
    "entries": {
      "project_context.md": "# Project Context\n...",
      "team_conventions.md": "## Conventions\n..."
    },
    "entryChecksums": {
      "project_context.md": "sha256:def456...",
      "team_conventions.md": "sha256:789abc..."
    }
  }
}
```

**条件请求**: `If-None-Match: sha256:abc123...` → 304 Not Modified

### 4.3 GET（hashes only）

**请求**: `GET /api/claude_code/team_memory?repo=owner/repo&view=hashes`

**响应 200**:
```json
{
  "repo": "owner/repo",
  "checksum": "sha256:abc123...",
  "entryChecksums": {
    "project_context.md": "sha256:def456...",
    "team_conventions.md": "sha256:789abc..."
  }
}
```

### 4.4 PUT（upsert）

**请求**: `PUT /api/claude_code/team_memory?repo=owner/repo`

**Headers**:
- `If-Match: sha256:abc123...`（必须）
- `Content-Type: application/json`

**Body**:
```json
{
  "entries": {
    "project_context.md": "# Updated content\n..."
  }
}
```

**响应**:
- **200**: 成功，返回 `{ "checksum": "sha256:new..." }`
- **412 Precondition Failed**: ETag 不匹配（并发冲突），返回 `{ "error": "checksum_mismatch", "current_checksum": "sha256:..." }`
- **413 Payload Too Large**: 超过容量限制，返回 `{ "error": "too_many_entries", "extra_details": { "max_entries": 500 } }`

### 4.5 首次 PUT（空仓库）

当 repo 不存在时，`If-Match` 应为空字符串 `""` 或 `*`，服务端创建新记录。

## 五、Rust 端：代理转发

### 5.1 新增文件

`src/daemon/team_memory_proxy.rs`

### 5.2 DaemonState 扩展

```rust
pub struct DaemonState {
    // ... 现有字段 ...
    pub team_memory_port: Option<u16>,
    pub team_memory_secret: Option<String>,
    pub team_memory_process: Option<Arc<Mutex<Child>>>,
}
```

### 5.3 转发逻辑

```rust
// 伪代码
async fn proxy_team_memory(
    State(state): State<DaemonState>,
    method: Method,
    query: Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Option<Bytes>,
) -> Response {
    let port = state.team_memory_port.ok_or(502)?;
    let secret = state.team_memory_secret.as_ref().ok_or(502)?;

    let url = format!("http://127.0.0.1:{}/api/claude_code/team_memory?{}", port, query_string);

    let mut req = reqwest::Client::new()
        .request(method, &url)
        .header("X-Team-Memory-Secret", secret);

    // 转发 If-Match / If-None-Match headers
    if let Some(etag) = headers.get("if-match") {
        req = req.header("If-Match", etag);
    }
    if let Some(etag) = headers.get("if-none-match") {
        req = req.header("If-None-Match", etag);
    }

    if let Some(body) = body {
        req = req.body(body);
        req = req.header("Content-Type", "application/json");
    }

    let resp = req.send().await?;
    // 透传 status code + headers + body
}
```

### 5.4 路由注册

```rust
// routes.rs 中 api_routes() 新增
.route("/api/claude_code/team_memory",
    get(proxy_team_memory).put(proxy_team_memory))
```

## 六、TS 端：Team Memory Server

### 6.1 文件布局

```
ui/team-memory-server/
├── index.ts           -- Bun.serve 入口，CLI 参数解析，中间件
├── db.ts              -- SQLite 初始化 + CRUD 操作
└── routes.ts          -- GET/PUT 端点处理逻辑
```

### 6.2 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS team_memory (
  repo       TEXT    NOT NULL,
  key        TEXT    NOT NULL,    -- 相对路径 (e.g. "project_context.md")
  content    TEXT    NOT NULL,    -- UTF-8 文件内容
  checksum   TEXT    NOT NULL,    -- "sha256:<hex>" 单条目哈希
  updated_at TEXT    NOT NULL,    -- ISO 8601
  PRIMARY KEY (repo, key)
);

CREATE TABLE IF NOT EXISTS repo_meta (
  repo      TEXT    PRIMARY KEY,
  version   INTEGER NOT NULL DEFAULT 1,
  checksum  TEXT    NOT NULL,    -- 全局 ETag (所有 entry checksums 排序拼接后 SHA-256)
  updated_at TEXT   NOT NULL
);
```

数据库路径: `~/.cc-rust/team-memory.db`

### 6.3 ETag 计算

```typescript
function computeGlobalChecksum(entryChecksums: Record<string, string>): string {
  const sorted = Object.entries(entryChecksums)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, hash]) => `${key}:${hash}`)
    .join('\n')
  const hash = createHash('sha256').update(sorted).digest('hex')
  return `sha256:${hash}`
}
```

### 6.4 入口 (index.ts)

```typescript
// CLI: bun run index.ts --port 19837 --secret <uuid>
const port = parseInt(args['--port'])
const secret = args['--secret']

Bun.serve({
  port,
  hostname: '127.0.0.1',
  async fetch(req) {
    // 1. 共享密钥校验
    if (req.headers.get('x-team-memory-secret') !== secret) {
      return new Response('Unauthorized', { status: 401 })
    }
    // 2. 路由分发
    const url = new URL(req.url)
    if (url.pathname === '/health') return new Response('ok')
    if (url.pathname === '/api/claude_code/team_memory') {
      if (req.method === 'GET') return handleGet(url, req)
      if (req.method === 'PUT') return handlePut(url, req)
    }
    return new Response('Not Found', { status: 404 })
  }
})
```

### 6.5 GET 处理 (routes.ts)

```typescript
function handleGet(url: URL, req: Request): Response {
  const repo = url.searchParams.get('repo')
  if (!repo) return new Response('Missing repo', { status: 400 })

  const view = url.searchParams.get('view')
  const meta = db.getRepoMeta(repo)

  // 条件请求: If-None-Match
  const ifNoneMatch = req.headers.get('if-none-match')
  if (meta && ifNoneMatch === meta.checksum) {
    return new Response(null, { status: 304 })
  }

  const entries = db.getEntries(repo)
  const entryChecksums = db.getEntryChecksums(repo)

  if (view === 'hashes') {
    return Response.json({
      repo,
      checksum: meta?.checksum ?? '',
      entryChecksums,
    }, { headers: { 'ETag': meta?.checksum ?? '' } })
  }

  return Response.json({
    repo,
    version: meta?.version ?? 0,
    lastModified: meta?.updated_at ?? '',
    checksum: meta?.checksum ?? '',
    content: { entries, entryChecksums },
  }, { headers: { 'ETag': meta?.checksum ?? '' } })
}
```

### 6.6 PUT 处理 (routes.ts)

```typescript
function handlePut(url: URL, req: Request): Response {
  const repo = url.searchParams.get('repo')
  if (!repo) return new Response('Missing repo', { status: 400 })

  const ifMatch = req.headers.get('if-match')
  const meta = db.getRepoMeta(repo)

  // ETag 校验 (首次允许 "" 或 "*")
  if (meta && ifMatch !== meta.checksum && ifMatch !== '*') {
    return Response.json({
      error: 'checksum_mismatch',
      current_checksum: meta.checksum,
    }, { status: 412 })
  }

  const body = await req.json() as { entries: Record<string, string> }

  // 容量检查
  const currentCount = db.getEntryCount(repo)
  const newKeys = Object.keys(body.entries).filter(k => !db.hasEntry(repo, k))
  if (currentCount + newKeys.length > MAX_ENTRIES) {
    return Response.json({
      error: 'too_many_entries',
      extra_details: { max_entries: MAX_ENTRIES },
    }, { status: 413 })
  }

  // Upsert (事务内)
  db.upsertEntries(repo, body.entries)

  // 重算全局 checksum
  const newChecksum = db.recomputeGlobalChecksum(repo)

  return Response.json({ checksum: newChecksum })
}
```

### 6.7 数据库操作 (db.ts)

关键方法:
- `init()` — 创建表 (IF NOT EXISTS)
- `getRepoMeta(repo)` — 查 repo_meta
- `getEntries(repo)` — 返回 `Record<string, string>`
- `getEntryChecksums(repo)` — 返回 `Record<string, string>`
- `getEntryCount(repo)` — `SELECT COUNT(*)`
- `hasEntry(repo, key)` — 存在性检查
- `upsertEntries(repo, entries)` — `INSERT OR REPLACE` 事务
- `recomputeGlobalChecksum(repo)` — 查所有 checksums → 排序拼接 → SHA-256 → 更新 repo_meta

所有写操作在 `BEGIN EXCLUSIVE` 事务内执行，保证原子性和并发安全。

## 七、常量

```
MAX_ENTRIES         = 500       // 每 repo 最大条目数
MAX_ENTRY_SIZE      = 250_000   // 单条目最大字节 (250KB)
MAX_PUT_BODY        = 512_000   // PUT body 最大字节 (512KB)
HEALTH_CHECK_TIMEOUT = 5_000    // 启动健康检查超时 (ms)
HEALTH_CHECK_INTERVAL = 100     // 健康检查轮询间隔 (ms)
SHUTDOWN_TIMEOUT    = 3_000     // 关闭等待超时 (ms)
```

## 八、错误处理

| 场景 | 行为 |
|------|------|
| TS 服务未启动/崩溃 | Rust 转发返回 502 |
| 共享密钥不匹配 | TS 返回 401 |
| repo 参数缺失 | TS 返回 400 |
| ETag 不匹配 | TS 返回 412 + 当前 checksum |
| 超过 MAX_ENTRIES | TS 返回 413 + max_entries |
| PUT body 超过 MAX_PUT_BODY | TS 返回 413 |
| SQLite 错误 | TS 返回 500 + 错误信息 |

## 九、Feature Gate

```rust
// src/config/features.rs
Feature::TeamMemory  // FEATURE_TEAMMEM=1
```

仅当 `Feature::TeamMemory` 启用时:
- Rust daemon spawn TS 子进程
- 注册 `/api/claude_code/team_memory` 路由

## 十、不在本次实现范围

- 客户端同步逻辑（watcher、auto pull/push）
- 密钥扫描
- 路径穿越防护
- 前端 UI 组件
- 遥测/分析事件
- 删除条目 API (DELETE)

这些留给后续迭代。
