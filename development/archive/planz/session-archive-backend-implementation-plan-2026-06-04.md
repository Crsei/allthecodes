# Plan: Web Session Archive 后端实现计划

> 日期：2026-06-04
>
> 背景：前端左侧 session 列表已经按新契约调用 `POST /api/sessions/{id}/archive`。当前 `allthecodes-web` OpenAPI 中已有该路径，但 sibling 后端 `crates/allthecodes-web` 尚未实现 route/handler，因此 `openapi:check` 会报告 `OpenAPI has routes not present in Rust: /api/sessions/{id}/archive`。

---

## Context

现有后端 session 管理集中在：

- `crates/allthecodes-web/src/mod.rs`
- `crates/allthecodes-web/src/handlers/sessions.rs`
- `crates/allthecodes-session/src/storage.rs`

当前行为：

- `GET /api/sessions` 通过 `storage::list_sessions()` 读取 `~/.allthecodes/sessions/*.json`。
- `GET /api/sessions/{id}` 和 `POST /api/sessions/{id}/resume` 通过顶层 session 文件加载历史。
- 没有 archive/delete API。

目标行为：

- 增加 `POST /api/sessions/{id}/archive`。
- archive 是持久化隐藏，不是删除；归档后不再出现在默认 `GET /api/sessions` 列表。
- 保留 session JSON，便于未来恢复/查看归档列表。

---

## Public API Contract

### 新 endpoint

```http
POST /api/sessions/{id}/archive
```

### Response

成功：

```json
{
  "ok": true,
  "message": "Session archived"
}
```

失败：

- `404 session_not_found`：顶层活跃 session 文件不存在。
- `409 engine_busy`：当前有 streaming query，不能归档。
- `409 session_owned`：TUI / IPC / chat stream 持有 session 写入权。
- `409 session_active`：目标 session 是当前 engine active session。v1 不归档 active session，避免 engine 后续 autosave 重新创建同名顶层文件。
- `500 session_archive_failed`：移动文件或创建 archive 目录失败。

### OpenAPI

前端仓库已新增：

- `scripts/openapi-contract.mjs`
- `docs/openapi/allthecodes-web.openapi.json`

后端实现后，`allthecodes-web` 的 `npm run openapi:check` 不应再因为 `/api/sessions/{id}/archive` 报 “route not present in Rust”。注意该脚本仍可能报告其他既有 OpenAPI/Rust 差异；本计划只消除 archive route 的新增差异。

---

## Implementation

### 1. Storage 层：新增归档路径与移动操作

在 `crates/allthecodes-session/src/storage.rs` 增加：

```rust
pub fn get_archived_session_dir() -> PathBuf
pub fn get_archived_session_file(session_id: &str) -> PathBuf
pub fn archive_session(session_id: &str) -> Result<()>
```

建议路径：

```text
~/.allthecodes/sessions/archive/{session_id}.json
```

原因：

- `list_sessions()` 只读取 `sessions/` 顶层 `.json` 文件，不递归读取子目录；移动到 `sessions/archive/` 后默认列表自然隐藏。
- 保留原始 JSON 文件，不做破坏性删除。
- 不改现有 `SessionFile` schema。

`archive_session` 行为：

1. `src = get_session_file(session_id)`。
2. 如果 `src` 不存在，返回带上下文的 error，handler 映射为 `404 session_not_found`。
3. 创建 `get_archived_session_dir()`。
4. `dest = get_archived_session_file(session_id)`。
5. 如果 `dest` 已存在，生成不覆盖的备份文件名：
   `archive/{session_id}.archived-{epoch_seconds}.json`。
   这样避免重复归档或历史文件冲突。
6. 使用 `std::fs::rename(&src, &dest)`；如果跨文件系统 rename 失败，再 fallback 为 `copy + remove_file`。
7. 用 `tracing::debug!` 记录 `session_id` 和目标路径。

不在 v1 范围内：

- 不归档 transcript、API request snapshots、rewind backups。
- 不提供 unarchive API。
- 不提供 archived session list API。

### 2. Web handler：新增 mutation response 和 handler

在 `crates/allthecodes-web/src/handlers/sessions.rs` 增加：

```rust
#[derive(Serialize)]
pub struct SessionMutationResponse {
    pub ok: bool,
    pub message: String,
}
```

新增 handler：

```rust
pub async fn session_archive_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> impl IntoResponse
```

处理顺序必须固定：

1. 如果 `state.is_streaming` 为 true，返回 `409 engine_busy`。
2. 如果 `ownership_conflict_response(&state)` 有值，直接返回。
3. 如果 `state.engine().current_session_id().to_string() == id`，返回 `409 session_active`。
4. 先调用 `storage::load_session_info(&id)` 验证存在；失败返回 `404 session_not_found`。
5. 调用 `storage::archive_session(&id)`。
6. 成功返回 `Json(SessionMutationResponse { ok: true, message: "Session archived".into() })`。
7. 失败返回 `500 session_archive_failed`，错误消息带原始 error。

记录：

```rust
info!(session_id = %id, "POST /api/sessions/:id/archive");
warn!(session_id = %id, error = %e, "failed to archive session");
```

### 3. Router：注册 route

在 `crates/allthecodes-web/src/mod.rs` session routes 区域增加：

```rust
.route(
    "/api/sessions/{id}/archive",
    post(handlers::session_archive_handler),
)
```

保持在 `/api/sessions/{id}/resume` 附近，方便 OpenAPI route sync 扫描。

### 4. 测试

#### Storage unit tests

在 `crates/allthecodes-session/src/storage.rs` 测试模块新增：

- `test_archive_session_moves_file_out_of_default_list`
  - 写入一个 fixture session。
  - 调用 `archive_session(id)`。
  - 断言顶层 `{id}.json` 不存在。
  - 断言 `archive/{id}.json` 存在。
  - 断言 `list_sessions()` 不包含该 session。

- `test_archive_session_missing_file_returns_error`
  - 对不存在的 id 调用。
  - 断言 error 包含 session id 或源路径。

- `test_archive_session_does_not_overwrite_existing_archive`
  - 预先创建 `archive/{id}.json`。
  - 再 archive 顶层 `{id}.json`。
  - 断言两个归档文件都存在，其中新文件带 `.archived-{ts}.json`。

#### Web handler tests

在 `crates/allthecodes-web/src/handlers/mod.rs` 或现有 web handler 测试区新增：

- `session_archive_handler_returns_404_for_missing_session`
- `session_archive_handler_rejects_active_session_with_409`
- `session_archive_handler_archives_inactive_session`

如果构造 `WebState` 成本过高，优先覆盖 storage tests，并加一个 router-level smoke test 验证 route 存在。

#### Frontend contract verification

在 `allthecodes-web` 中运行：

```bash
npm run openapi:check
```

预期：

- 不再出现 `OpenAPI has routes not present in Rust: /api/sessions/{id}/archive`。
- 如果还有其他 route 差异，按既有 backlog 处理，不阻塞本功能。

---

## Edge Cases

- **当前 active session**：v1 返回 `409 session_active`，不移动文件。
- **streaming 中**：返回 `409 engine_busy`，保持和 `new/resume` 一致。
- **TUI/IPC 持有 session**：返回现有 `session_owned`。
- **重复归档**：顶层文件不存在时返回 `404`；如果顶层文件存在但 archive 目标冲突，则使用带时间戳的 archive 文件名，不覆盖旧归档。
- **Windows**：不要在 Windows 平台编译后端。Linux/macOS 下运行 Rust 测试即可。

---

## Acceptance Criteria

- `POST /api/sessions/{id}/archive` 在 Rust router 中注册。
- 成功归档后，默认 `GET /api/sessions` 不再返回该 session。
- 归档文件仍保存在 `~/.allthecodes/sessions/archive/`。
- active / streaming / owned session 不会被归档。
- 前端点击归档按钮不再收到 404。
- `cargo test -p allthecodes-session archive_session` 通过。
- `cargo test -p allthecodes-web session_archive` 或对应 web handler tests 通过。
- `allthecodes-web` 的 `npm run openapi:check` 不再报告 archive route 缺失于 Rust。
