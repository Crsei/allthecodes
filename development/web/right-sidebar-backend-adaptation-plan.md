# 右侧边栏 — 后端适配计划

> 前端 RightSideBar 所需的 3 个后端 API 端点适配分析。
> 前端计划：`allthecodes-web/development-docs/UI/RightSideBar/00-index.md`
> 后端代码库：`/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/allthecodes/`

---

## 目录

1. [现状分析](#1-现状分析)
2. [需新增的 API 端点](#2-需新增的-api-端点)
3. [端点详细设计](#3-端点详细设计)
   - 3.1 `GET /api/git/log` — 文件提交历史
   - 3.2 `GET /api/git/diff` — 文件差异对比
   - 3.3 `GET /api/proxy` — Web 页面代理预览
4. [Capability 注册](#4-capability-注册)
5. [依赖关系](#5-依赖关系)
6. [实现顺序](#6-实现顺序)

---

## 1. 现状分析

### 1.1 已有基础设施

| 模块 | 路径 | 说明 |
|------|------|------|
| `allthecodes-utils::git` | `crates/allthecodes-utils/src/git.rs` | ✅ 完整的 git2 封装：`get_log()`, `diff_staged()`, `diff_unstaged()`, `diff_between()`, `get_status()`, `current_branch()`, `head_sha()`, `is_git_repo()`, `find_git_root()` |
| `allthecodes-web::handlers` | `crates/allthecodes-web/src/handlers/` | ✅ 路由注册模式已成熟，30+ handler 文件 |
| `WebState` | `crates/allthecodes-web/src/state.rs` | ✅ `state.engine().app_state()` 可获取 `cwd`（当前工作目录） |
| `reqwest` | `Cargo.toml` 工作空间依赖 | ✅ 可用于 web proxy 的 HTTP 请求 |
| Axum 路由 | `crates/allthecodes-web/src/mod.rs` | ✅ 模式：`.route("/api/...", get(handler).post(handler))` |

### 1.2 不需要新增的

| 能力 | 说明 |
|------|------|
| Terminal | 已有 `/api/tui/ws` WebSocket，前端直接复用现有 `XtermPane` |
| Tool Calls | 数据源自 `chat-store` 中的消息，前端直接提取 `tool_use` 和 `tool_result`，无需后端新端点 |

### 1.3 需要新增的

| 端点 | 原因 | 依赖 |
|------|------|------|
| `GET /api/git/log` | 用于 File Commits 面板展示 commit 历史 | `allthecodes-utils::git::get_log()` |
| `GET /api/git/diff` | 用于 File Changes 面板展示 diff | `allthecodes-utils::git::diff_*()` |
| `GET /api/proxy` | 用于 Web Preview 面板加载外部页面 | `reqwest`（已有依赖） |

---

## 2. 需新增的 API 端点

```
// 需注册到 build_router() 中的路由：

GET  /api/git/log      → handlers::git_log_handler
GET  /api/git/diff     → handlers::git_diff_handler
GET  /api/proxy        → handlers::proxy_handler
```

### 添加位置

**路由注册**（`crates/allthecodes-web/src/mod.rs`，约第 267 行，在 `/api/{*path}` catch-all 之前）：

```rust
// === RightSideBar: Git & Proxy ===
.route("/api/git/log", get(handlers::git_log_handler))
.route("/api/git/diff", get(handlers::git_diff_handler))
.route("/api/proxy", get(handlers::proxy_handler))
```

### 新文件

| 文件 | 包含 |
|------|------|
| `crates/allthecodes-web/src/handlers/git.rs` | `git_log_handler`, `git_diff_handler` 及相关 request/response 类型 |
| `crates/allthecodes-web/src/handlers/proxy.rs` | `proxy_handler` 及相关类型 |

### 修改列表

| 文件 | 修改内容 |
|------|----------|
| `crates/allthecodes-web/src/handlers/mod.rs` | 添加 `pub mod git; pub use git::*;` 和 `pub mod proxy; pub use proxy::*;` |
| `crates/allthecodes-web/src/mod.rs` | 在 build_router() 中添加 3 条 `.route()` |
| `crates/allthecodes-web/src/handlers/capabilities.rs` | 在 `capabilities_map()` 中添加 `git` 和 `proxy` 能力 |

---

## 3. 端点详细设计

### 3.1 `GET /api/git/log` — 文件提交历史

**用途**：获取当前 git 仓库中特定文件（或整体）的 commit 历史。

#### 请求

```http
GET /api/git/log?file=src/main.rs&max_count=50&path=/path/to/repo
```

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `file` | string | 否 | 空（全仓库） | 文件路径（相对于仓库根） |
| `max_count` | integer | 否 | 50 | 最多返回 commit 数 |
| `path` | string | 否 | engine.cwd() | git 仓库路径（测试用，生产由 engine 提供） |

#### 响应

```json
{
  "ok": true,
  "commits": [
    {
      "sha": "a1b2c3d4e5f6...",
      "short_sha": "a1b2c3d",
      "summary": "Add login feature",
      "message": "Add login feature\n\n- Add login form\n- Add auth API",
      "author_name": "John Doe",
      "author_email": "john@example.com",
      "timestamp": 1717200000,
      "stats": {
        "additions": 120,
        "deletions": 30
      }
    }
  ],
  "branch": "main",
  "git_root": "/path/to/repo"
}
```

#### 错误响应

```json
// 不在 git 仓库中
{ "ok": false, "error": "Not a git repository", "code": "not_a_git_repo" }

// 文件不存在
{ "ok": false, "error": "File not found in repository", "code": "file_not_found" }

// 其他 git 错误
{ "ok": false, "error": "Git operation failed: ...", "code": "git_error" }
```

#### 实现逻辑

```rust
pub async fn git_log_handler(
    State(state): State<WebState>,
    Query(params): Query<GitLogParams>,
) -> impl IntoResponse {
    let engine = state.engine();
    let cwd = resolve_repo_path(&engine, &params.path);

    if !is_git_repo(&cwd) {
        return (StatusCode::BAD_REQUEST, Json(json!({
            "ok": false, "error": "Not a git repository", "code": "not_a_git_repo"
        })));
    }

    let repo = open_repo(&cwd).unwrap();
    let max_count = params.max_count.unwrap_or(50).min(200);

    let mut commits = get_log(&cwd, max_count)?;

    // 如果指定了 file 参数，过滤出包含该文件的 commit
    if let Some(file) = &params.file {
        commits.retain(|commit| {
            // 使用 git2 检查该 commit 是否涉及目标文件
            commit_contains_file(&repo, &commit.sha, file).unwrap_or(false)
        });
    }

    // 为每个 commit 计算 stats（改动行数统计）
    for commit in &mut commits {
        if let Ok(stats) = get_commit_stats(&repo, &commit.sha) {
            // 注入 stats 信息
        }
    }

    let branch = current_branch(&cwd).unwrap_or_default();
    let git_root = find_git_root(&cwd);

    Json(json!({
        "ok": true,
        "commits": commits,
        "branch": branch,
        "git_root": git_root,
    }))
}
```

#### 关键辅助函数

`commit_contains_file()`：使用 `diff_tree_to_tree()` 检查某个 commit 是否修改了目标文件。

`get_commit_stats()`：使用 `git2::Patch::from_diff()` 计算新增/删除行数。

> **注意**：`allthecodes-utils::git::get_log()` 已返回 `Vec<LogEntry>`，无需新增输出类型。但若需 `stats` 字段，需要在 `LogEntry` 结构体中添加 `stats: Option<CommitStats>` 或直接在 handler 中单独计算。

### 3.2 `GET /api/git/diff` — 文件差异对比

**用途**：获取文件或 commit 的 diff 内容（包含行级别的改动详情）。

#### 请求

**模式 A — 查看工作区文件的改动**：
```http
GET /api/git/diff?file=src/main.rs
```

**模式 B — 查看特定 commit 的改动**：
```http
GET /api/git/diff?commit=a1b2c3d
```

**模式 C — 查看某文件中某 commit 的改动**：
```http
GET /api/git/diff?file=src/main.rs&commit=a1b2c3d
```

**模式 D — 对比两个 commit**：
```http
GET /api/git/diff?from=abc123&to=def456
```

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `file` | string | 否 | — | 过滤特定文件 |
| `commit` | string | 否 | — | 查看该 commit 引入的改动（相对于 parent） |
| `from` | string | 否 | — | 起始 commit（与 `to` 配对使用） |
| `to` | string | 否 | — | 结束 commit（与 `from` 配对使用） |
| `path` | string | 否 | engine.cwd() | git 仓库路径 |

**约束**：`commit` 与 `(from, to)` 二选一；如均不提供，默认显示工作区的 unstaged 改动。

#### 响应

```json
{
  "ok": true,
  "files": [
    {
      "path": "src/main.rs",
      "old_path": null,
      "delta": "modified",
      "additions": 15,
      "deletions": 3,
      "hunks": [
        {
          "header": "@@ -42,7 +42,15 @@",
          "old_start": 42,
          "old_lines": 7,
          "new_start": 42,
          "new_lines": 15,
          "lines": [
            { "type": "context",  "old_no": 42, "new_no": 42, "content": "fn main() {" },
            { "type": "deletion", "old_no": 43, "new_no": null, "content": "  let old = 1;" },
            { "type": "addition", "old_no": null, "new_no": 43, "content": "  let new = 2;" },
            { "type": "context",  "old_no": 44, "new_no": 44, "content": "}" }
          ]
        }
      ]
    }
  ],
  "branch": "main"
}
```

#### 实现逻辑

```rust
pub async fn git_diff_handler(
    State(state): State<WebState>,
    Query(params): Query<GitDiffParams>,
) -> impl IntoResponse {
    let engine = state.engine();
    let cwd = resolve_repo_path(&engine, &params.path);

    if !is_git_repo(&cwd) {
        return error_response("not_a_git_repo", "Not a git repository");
    }

    let repo = open_repo(&cwd).unwrap();
    let files_diff = match (params.commit, params.from.as_ref(), params.to.as_ref()) {
        // 工作区 unstaged diff
        (None, None, None) => diff_unstaged_with_hunks(&repo, params.file.as_deref()),
        // 特定 commit 相对其 parent 的 diff
        (Some(commit), None, None) => diff_commit(&repo, &commit, params.file.as_deref()),
        // 两个 commit 间的 diff
        (None, Some(from), Some(to)) => diff_between_with_hunks(&repo, from, to, params.file.as_deref()),
        // 无效参数组合
        _ => return error_response("invalid_params", "Provide commit= or from= and to="),
    };

    match files_diff {
        Ok(files) => Json(json!({ "ok": true, "files": files, "branch": current_branch(&cwd).ok() })),
        Err(e) => error_response("git_error", &e.to_string()),
    }
}
```

**关键**：需要从 `git2::Diff` 的 `patch` 中提取行级别的信息（每行的类型、行号、内容）。现有 `DiffEntry` 只包含文件级别的统计信息，不包含行级数据。

**新增类型**：

```rust
#[derive(Serialize)]
struct FileDiff {
    path: String,
    old_path: Option<String>,
    delta: String,
    additions: usize,
    deletions: usize,
    hunks: Vec<Hunk>,
}

#[derive(Serialize)]
struct Hunk {
    header: String,
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    lines: Vec<DiffLine>,
}

#[derive(Serialize)]
struct DiffLine {
    #[serde(rename = "type")]
    line_type: String,  // "context" | "addition" | "deletion"
    old_no: Option<u32>,
    new_no: Option<u32>,
    content: String,
}
```

> **位置**：这些类型可以定义在 `handlers/git.rs` 中，或在 `allthecodes-utils::git` 中添加行级 diff 功能。

### 3.3 `GET /api/proxy` — Web 页面代理预览

**用途**：为 Web Preview 面板提供 iframe 嵌入代理，绕过 X-Frame-Options 和 CORS 限制。

#### 请求

```http
GET /api/proxy?url=https://example.com
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 要代理的目标 URL（需 URL 编码或 base64 编码） |

#### 响应

直接返回目标 URL 的内容（透传响应 body + Content-Type），状态码为 200（即使目标返回 4xx/5xx）。

#### 实现逻辑

```rust
pub async fn proxy_handler(
    Query(params): Query<ProxyParams>,
) -> impl IntoResponse {
    let url = match sanitize_url(&params.url) {
        Some(u) => u,
        None => return (StatusCode::BAD_REQUEST, "Invalid URL").into_response(),
    };

    // 安全性检查：仅允许 http/https
    if url.scheme() != "https" && url.scheme() != "http" {
        return (StatusCode::BAD_REQUEST, "Only http/https URLs allowed").into_response();
    }

    // DNS 解析 + 请求
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| ...)?;

    let response = client.get(url).send().await.map_err(|e| ...)?;

    // 构建响应：透传状态码、Content-Type 和 body
    let content_type = response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("text/html"));

    let body = response.bytes().await.map_err(|e| ...)?;

    // 可选的 HTML 内容重写：替换相对路径为绝对路径
    // let rewritten = rewrite_html_links(&body, &origin_url);

    ([(axum::http::header::CONTENT_TYPE, content_type)], body)
}
```

#### 安全措施

| 措施 | 实现 |
|------|------|
| URL 白名单 | 仅允许 `https://` 和 `http://` 协议 |
| 服务端请求伪造 (SSRF) | 禁止内网 IP（127.0.0.1, 10.x.x.x, 172.16-31.x.x, 192.168.x.x） |
| 超时 | 15 秒请求超时 |
| 大小限制 | 最大响应体 10MB，超过则截断或拒绝 |
| 重定向 | 最多跟随 10 次重定向 |

```rust
fn is_private_ip(host: &str) -> bool {
    if let Ok(addr) = host.parse::<std::net::IpAddr>() {
        return addr.is_loopback()
            || addr.is_private()
            || addr.is_link_local()
            || addr.is_unspecified();
    }
    // 域名：先解析 DNS 再检查
    false
}
```

#### 可选的 HTML 重写

对于 HTML 响应，重写页面中的相对链接为绝对链接，以确保页面在 iframe 中正确渲染：

```rust
fn rewrite_html_links(body: &str, origin_url: &Url) -> String {
    // 替换 href="/path" → href="https://origin.com/path"
    // 替换 src="/assets/..." → src="https://origin.com/assets/..."
    // 使用正则或 html5ever 解析
    body
}
```

> **可选实现**：初期可以不重写 HTML 直接透传，用户自己处理链接问题。

---

## 4. Capability 注册

在 `capabilities_map()` 中添加：

```rust
// handlers/capabilities.rs
caps.insert("git".into(), true);      // git log / git diff
caps.insert("proxy".into(), true);    // web preview proxy
```

前端可以通过 `GET /api/capabilities` 检查后端是否支持这些功能，并据此显示/隐藏右侧边栏的对应 Tab。

---

## 5. 依赖关系

```
RightSideBar 后端适配
│
├── git_log_handler ────────────────────────────┐
│   ├── allthecodes-utils::git::get_log()       │  ✅ 已有
│   ├── allthecodes-utils::git::current_branch() │  ✅ 已有
│   └── allthecodes-utils::git::find_git_root()  │  ✅ 已有
│   └── [NEW] 扩展 LogEntry 加 stats 字段        │  ⚠️ 可选
│
├── git_diff_handler ───────────────────────────┐
│   ├── allthecodes-utils::git::diff_staged()    │  ✅ 已有（文件级统计）
│   ├── allthecodes-utils::git::diff_unstaged()  │  ✅ 已有
│   ├── allthecodes-utils::git::diff_between()   │  ✅ 已有
│   └── [NEW] 行级 diff（hunks + lines）提取     │  **需要新增**
│
├── proxy_handler ──────────────────────────────┐
│   ├── reqwest (HTTP Client)                    │  ✅ 已有工作空间依赖
│   ├── [NEW] SSRF 防护（内网 IP 检查）           │  需要实现
│   └── [NEW] HTML 链接重写（可选）               │  需要实现
│
└── 注册与整合 ──────────────────────────────────┐
    ├── handlers/mod.rs (模块注册)               │  新增
    ├── mod.rs (路由注册)                        │  新增
    └── capabilities.rs (能力注册)              │  新增
```

### 需要新增的类型/函数

| 新增内容 | 位置 | 原因 |
|----------|------|------|
| `struct FileDiff`, `struct Hunk`, `struct DiffLine` | `handlers/git.rs` 或 `allthecodes-utils::git` | 行级 diff 数据需要新的结构体 |
| `fn diff_commit()` | `allthecodes-utils::git` | 获取某 commit 相对于其 parent 的 diff |
| `fn diff_with_hunks()` | `allthecodes-utils::git` | 获取包含行级详情的 diff |
| `fn sanitize_url()`, `fn is_private_ip()`, `fn rewrite_html_links()` | `handlers/proxy.rs` | proxy 的安全处理和 HTML 重写 |

### LogEntry 扩展（可选）

若需要为每个 commit 显示改动统计（+X/-Y），需扩展 `LogEntry`：

```rust
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    // ... 现有字段 ...
    pub stats: Option<CommitStats>,  // 新增可选字段
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitStats {
    pub additions: usize,
    pub deletions: usize,
}
```

---

## 6. 实现顺序

### Phase 1 — Git 基础设施（P0）

| 步骤 | 内容 | 文件 |
|------|------|------|
| 1.1 | 在 `allthecodes-utils::git` 中添加行级 diff 函数 | `crates/allthecodes-utils/src/git.rs` |
| 1.2 | 可选：扩展 `LogEntry` 添加 `stats` 字段 | `crates/allthecodes-utils/src/git.rs` |
| 1.3 | 创建 `handlers/git.rs`，实现 `git_log_handler` 和 `git_diff_handler` | `crates/allthecodes-web/src/handlers/git.rs` |
| 1.4 | 在 `mod.rs` 和 `handlers/mod.rs` 注册 | 模块注册 + 路由注册 |
| 1.5 | 在 `capabilities.rs` 注册 `git` 能力 | `crates/allthecodes-web/src/handlers/capabilities.rs` |

### Phase 2 — Web Proxy（P1）

| 步骤 | 内容 | 文件 |
|------|------|------|
| 2.1 | 创建 `handlers/proxy.rs`，实现 `proxy_handler` | `crates/allthecodes-web/src/handlers/proxy.rs` |
| 2.2 | 实现 SSRF 防护（`is_private_ip` + DNS 预检查） | `crates/allthecodes-web/src/handlers/proxy.rs` |
| 2.3 | 可选：HTML 链接重写 | `crates/allthecodes-web/src/handlers/proxy.rs` |
| 2.4 | 在 `mod.rs` 和 `handlers/mod.rs` 注册 | 模块注册 + 路由注册 |
| 2.5 | 在 `capabilities.rs` 注册 `proxy` 能力 | `crates/allthecodes-web/src/handlers/capabilities.rs` |

### 不涉及后端改动的部分

| 前端功能 | 原因 |
|----------|------|
| Terminal (XtermPane) | 已有 `/api/tui/ws` WebSocket，前端直接复用 |
| Tool Calls | 数据来源于 `chat-store` 中的消息列表，在客户端提取 |
| 路由注册 (mod.rs) | 静态路由，在 build_router() 添加 3 行 |

---

## 附录 A：参考 handler 模式 (activity_recorder)

```rust
// handlers/activity_recorder.rs 的结构可参考
#[derive(Serialize)]
pub struct XxxResponse {
    pub ok: bool,
    // ...
}

pub async fn xxx_handler(
    State(state): State<WebState>,
) -> impl IntoResponse {
    // 1. 获取 engine / state
    let engine = state.engine();
    // 2. 执行业务逻辑
    // 3. 返回 JSON
    Json(json!({ "ok": true, ... }))
}
```

## 附录 B：路由注册位置

在 `crates/allthecodes-web/src/mod.rs` 的 `build_router()` 函数中，建议在 Phase 4/5 路由之后、`/api/{*path}` catch-all 之前插入：

```rust
// 现有：Phase 5: IPC WebSocket
.route("/api/ipc/ws", any(ws::ipc::ipc_ws_handler))

// === NEW: RightSideBar: Git & Web Proxy ===
.route("/api/git/log", get(handlers::git_log_handler))
.route("/api/git/diff", get(handlers::git_diff_handler))
.route("/api/proxy", get(handlers::proxy_handler))

// 现有：API catch-all
.route("/api/{*path}", get(handlers::api_fallback_handler).post(handlers::api_fallback_handler))
```

---

> 对应前端计划：`allthecodes-web/development-docs/UI/RightSideBar/00-index.md`
