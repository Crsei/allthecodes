# 逐 crate 迁移指南

本文档描述如何将各 crate 从 JSON 文件存储迁移到 SQLite。每个 crate 的迁移步骤、回滚方案、以及双写期间的行为。

---

## 已确认范围

- 执行顺序：先迁移 `allthecodes-tasks`，再迁移 `allthecodes-session`
- Task output 大文本继续存文件，SQLite 只记录 `output_file`、摘要、大小和截断状态等元数据
- `tasks` 与 `sessions` 保留 feature flag / JSON fallback
- 初期不包含旧 JSON 数据 backfill；旧数据通过 fallback 读取，后续再单独实现导入命令

---

## 通用迁移模式

### 双写策略

所有迁移遵循一个模式：

1. **Phase A** — 新增 SQLite store，JSON 文件写入不变，新增 SQLite 并行写入
2. **Phase B** — 写入全走 SQLite，JSON 文件继续写入作为备份
3. **Phase C** — 读取优先从 SQLite，JSON 文件只作为 fallback
4. **Phase D** — 移除 JSON 文件读写，纯 SQLite

```
写路径： JSON file ──→ JSON file + SQLite ──→ SQLite only
读路径： JSON file ──→ SQLite (fallback JSON) ──→ SQLite only
```

### 连接传递

`allthecodes-db::DbPoolManager` 在 `main.rs` / `StateRuntime` 初始化时创建，通过以下方式传递：

```rust
// 方式 A（推荐）：Arc<DbPoolManager> 挂在全局 AppState
pub struct AppState {
    pub db: Arc<DbPoolManager>,
}

// 方式 B：各 Store 单独注入
let session_store = SqliteSessionStore::new(pool_manager.state_pool());
session_store.ensure_ready().await?;
```

---

## 1. allthecodes-session

### 当前代码

- 文件：`session/src/storage.rs`
- 核心类型：`SessionFile`, `SessionInfo`, `SerializableMessage`
- 操作：`save_session`, `load_session`, `list_sessions`, `truncate_session`, 等
- 存储路径：`~/.allthecodes/sessions/{uuid}.json`

### 替换范围

| 函数 | 替换方式 | 备注 |
|------|----------|------|
| `save_session` | `SessionStore::upsert_session()` + 批量 `insert_messages()` | 事务内完成 |
| `load_session` | `SessionStore::get_session()` + `get_messages()` | 分两步 |
| `list_sessions` | `SessionStore::list_sessions()` | 带过滤 + keyset 分页 |
| `truncate_session` | `DELETE FROM session_messages WHERE session_id = ? AND position > ?` | |
| `archive_session` | `UPDATE sessions SET archived = 1` | |
| `set_session_title` | `UPDATE sessions SET title = ?, updated_at = ?` | |

### 不替换的操作

- `export_session` → 仍导出为文件
- `fork_session` → 逻辑不变，底层改用 SQLite

### 迁移步骤

```
Phase A: storage.rs 中新增 SqliteSessionStore 引用（feature-gated）
        原有 JSON save/load 不变
        在 save_session 末尾追加 SQLite 写入

Phase B: 读取优先从 SQLite，未命中时 fallback 到 JSON
        list_sessions 改为 SQLite 查询（更快）

Phase C: 删除 JSON 写入逻辑
        删除 fallback 读取

Phase D: 可选：后续单独运行全量迁移脚本，将旧 JSON session 导入 SQLite
```

### 回滚方案

Phase A/B 均可通过 feature flag 切换：`--features json-storage` 回退到纯 JSON 模式。

---

## 2. allthecodes-tasks

### 当前代码

- 文件：`tasks/src/store.rs`, `tasks/src/repository.rs`
- 核心类型：`PersistedTaskFile`, `PersistedTaskRecord`
- 存储路径：`~/.allthecodes/tasks/{id}.json` + `{id}.output.log`
- 并发控制：`HighWatermarkLock`, `TaskListLock`

### 替换范围

| 操作 | 替换方式 | 备注 |
|------|----------|------|
| `create_task` | `INSERT INTO tasks` | ID 生成不变（UUID v4） |
| `get_task` | `SELECT * FROM tasks WHERE id = ?` | |
| `update_task` | `UPDATE tasks SET ... WHERE id = ?` | |
| `list_tasks` | `SELECT * FROM tasks WHERE status IN (?) ORDER BY created_at` | SQL 层面过滤 |
| `delete_task` | `DELETE FROM tasks WHERE id = ?` | CASCADE 删除依赖 |
| 依赖查询 | `SELECT * FROM task_dependencies WHERE task_id = ?` | JOIN 替代全量加载 |

### 不需要替换的操作

- TaskOutput 大文本仍存文件（`output_file` 列记录路径）
- 不创建 `task_output_logs` 表，避免把大文本 output 搬进 SQLite
- 文件锁机制不再需要，SQLite WAL + 事务保证原子性

### 迁移步骤

```
Phase A: 在 TaskStore 中添加 sqlx 成员（feature-gated）
        JSON 文件写入并行写入 SQLite
        TaskListLock 在 SQLite 模式下跳过

Phase B: TaskRepository 读取优先从 SQLite
        文件只用于 output log 的读写

Phase C: 移除文件写入
        移除 TaskListLock / HighWatermarkLock

Phase D: 可选：后续单独运行全量迁移脚本，将旧 JSON task 导入 SQLite
```

### 注意点

- `TaskListLock` 和 `HighWatermarkLock` 是文件锁。SQLite 模式下不再需要——WAL 事务提供足够隔离。
- 但 migrate 期间可能需要兼容旧代码创建的 `.lock` 文件。建议 Phase A 先检查 `.lock` 是否存在并清理。

---

## 3. allthecodes-session/memdir (Memory)

### 当前代码

- 文件：`session/src/memdir/`
- 存储路径：`~/.allthecodes/memory/{key}.json` 或 `.allthecodes/memory/{key}.json`
- 类型：`MemoryType` (User/Feedback/Project/Reference)

### 替换范围

| 操作 | 替换方式 |
|------|----------|
| `add_memory` | `INSERT INTO memories (id, name, content, ...)` |
| `get_memory` | `SELECT * FROM memories WHERE name = ?` |
| `list_memories` | `SELECT * FROM memories WHERE scope = ? AND (project_cwd = ? OR project_cwd IS NULL)` |
| `search_memories` | `SELECT * FROM memories WHERE content LIKE ? OR search_terms LIKE ?` |

### 优化机会

- SQLite 的 `LIKE` 搜索比当前的全量加载 + 正则匹配快
- Phase 4 可加 FTS5 全文索引

---

## 4. allthecodes-services/scheduler

### 当前代码

- 文件：`services/src/scheduler/store.rs`
- 存储路径：`~/.allthecodes/scheduled_tasks.json`
- 并发：文件锁 `create_new(true)` + `parking_lot::Mutex`

### 替换范围

| 操作 | 替换方式 |
|------|----------|
| `list_scheduled_tasks` | `SELECT * FROM scheduled_tasks WHERE paused = 0` |
| `get_next_due` | `SELECT * FROM scheduled_tasks WHERE next_run_at <= ? AND paused = 0 ORDER BY next_run_at LIMIT 1` |
| `schedule_task` | `INSERT INTO scheduled_tasks` |
| `cancel_task` | `UPDATE scheduled_tasks SET paused = 1` |
| `update_next_run` | `UPDATE scheduled_tasks SET last_run_at = ?, next_run_at = ?, run_count = run_count + 1` |

### 迁移步骤

直接切换：`scheduled_tasks.json` 数据量小，不考虑双写。启动时检测 SQLite 模式是否启用，启用则从 SQLite 读，否则从 JSON 读。

---

## 5. allthecodes-daemon

### 当前代码

- 文件：`daemon/src/process_state.rs`, `daemon/src/protocol.rs`
- 存储路径：`~/.allthecodes/daemon/*.json`

### 替换范围

| 文件 | 替换表 |
|------|--------|
| `supervisor.json` | `daemon_state` WHERE key = 'supervisor' |
| `shutdown-request.json` | `daemon_state` WHERE key = 'shutdown-request' |
| `control-token.json` | `daemon_state` WHERE key = 'control-token' |
| `sleep-state.json` | `daemon_state` WHERE key = 'sleep-state' |
| `workers/{worker_id}.json` | `daemon_workers` 表 |

### 不需要替换的操作

- `logs/{worker_id}.log` → 保持文件存储（非结构化日志，适合 tail/查看）

---

## 6. allthecodes-gateway

### 当前代码

- 文件：`gateway/src/store.rs`
- 存储路径：`~/.allthecodes/gateway/runs/{run_id}/meta.json` + `events.ndjson`

### 替换范围

| 操作 | 替换方式 |
|------|----------|
| `create_run` | `INSERT INTO gateway_runs` |
| `append_event` | `INSERT INTO gateway_events` |
| `get_run` | `SELECT * FROM gateway_runs WHERE id = ?` |
| `list_runs` | `SELECT * FROM gateway_runs WHERE adapter_id = ? ORDER BY created_at` |
| `list_events` | `SELECT * FROM gateway_events WHERE run_id = ? ORDER BY sequence` |

### 设计考虑

`events.ndjson` 在当前实现中是纯 appended，没有查询需求。迁移后 gateway event 查询更方便（按 type 过滤、按 adapter 聚合等），但写入性能可能略降。

建议：**gateway 最后迁移**，低优先级。

---

## 7. 并行写入与数据一致性

### 多进程场景

当前多个 `allthecodes` 进程（daemon + CLI + web）可能写入同一 DB：
- daemon 写入 daemon_state / scheduled_tasks
- CLI 写入 sessions / tasks / memories
- web 写入 sessions

SQLite WAL 模式允许多个 reader + 一个 writer 并发。5 连接池 + 5s busy timeout 足够应对：

```
Process A (writer): BEGIN → UPDATE → COMMIT
Process B (writer): wait (busy timeout 5s) → retry → BEGIN → ...
Process C (reader): 同时读，不受影响
```

### 事务使用规范

```rust
// 需要原子性的操作使用事务
let mut tx = pool.begin().await?;

sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
    .bind(&now)
    .bind(&session_id)
    .execute(&mut *tx)
    .await?;

sqlx::query("INSERT INTO session_messages (id, session_id, position, ...) VALUES (?, ?, ?, ...)")
    .bind(&msg_id)
    .bind(&session_id)
    .bind(&position)
    .bind(&content)
    .execute(&mut *tx)
    .await?;

tx.commit().await?;
```

### 超时与重试

```rust
// 一次性查询超时
let session = sqlx::query_as::<_, SessionRow>(...)
    .fetch_optional(&self.pool)
    .timeout(Duration::from_secs(10))  // tokio::time::timeout
    .await??;

// 事务超时
let result: sqlx::Result<_> = tokio::time::timeout(
    Duration::from_secs(15),
    async {
        let mut tx = self.pool.begin().await?;
        // ... operations ...
        tx.commit().await
    }
).await.map_err(|_| sqlx::Error::Protocol("transaction timed out".into()))??;
```

---

## 8. 恢复与修复

### 完整备份

```rust
// 热备份：VACUUM INTO（SQLite 3.27+）
sqlx::raw_sql(&format!("VACUUM INTO '{}'", backup_path))
    .execute(&pool)
    .await?;
```

### 完整性检查

```rust
// 参考 codex 的 sqlite_integrity_check()
pub async fn integrity_check(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as("PRAGMA integrity_check")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
    // 返回 ["ok"] 表示正常
}
```

### 恢复流程

```rust
pub async fn recover_database(path: &Path) -> Result<()> {
    // 1. 备份损坏文件
    let backup = path.with_extension("sqlite.corrupted");
    fs::rename(path, &backup)?;

    // 2. 备份 WAL 文件
    if path.with_extension("sqlite-wal").exists() {
        fs::rename(path.with_extension("sqlite-wal"),
                   backup.with_extension("sqlite-wal"))?;
    }
    if path.with_extension("sqlite-shm").exists() {
        fs::rename(path.with_extension("sqlite-shm"),
                   backup.with_extension("sqlite-shm"))?;
    }

    // 3. 创建新 DB + 运行迁移
    let pool = create_pool(path);
    run_all_migrations(&pool).await?;

    // 4. 可选：从旧备份恢复数据
    // ... (看情况)
    Ok(())
}
```

参考 codex `state_db_recovery.rs`：备份所有 `.sqlite`、`.sqlite-wal`、`.sqlite-shm` 文件后再创建新 DB。

---

## 9. Feature Flag 设计

```toml
# allthecodes-db/Cargo.toml
[features]
default = ["sqlite-storage"]
sqlite-storage = ["sqlx"]
```

各 consumer crate 的条件编译：

```rust
// session/src/storage.rs
#[cfg(feature = "sqlite-storage")]
pub use sqlite_session_store::SqliteSessionStore;

#[cfg(not(feature = "sqlite-storage"))]
pub use json_session_store::JsonSessionStore;
```

可删除的 JSON 存储代码用 `#[cfg(feature = "json-storage")]` 保护，方便彻底迁移后移除。

---

## 10. 优先级排序

| 优先级 | Crate | 原因 | 难度 |
|--------|-------|------|------|
| P0 | **allthecodes-db** | 基础设施，其他迁移依赖它 | 中 |
| P1 | **allthecodes-tasks** | 并发锁问题最严重，收益最直接 | 中 |
| P1 | **allthecodes-session** | 数据量大，列表查询频繁 | 高 |
| P2 | **allthecodes-services/scheduler** | 单文件锁问题 | 低 |
| P2 | **allthecodes-daemon/state** | 数据量小，迁移简单 | 低 |
| P3 | **allthecodes-session/memdir** | 当前性能可接受 | 中 |
| P4 | **allthecodes-gateway** | 查询需求不迫切 | 低 |
