# SQLite 集成总体架构

## Thesis

**allthecodes 现阶段用 JSON 文件做所有持久化——session、task、memory、daemon state、gateway events、scheduled tasks 各自有一套文件读写逻辑。文件系统作为唯一持久层在数据量小时够用，但随着 session 数增长、跨实体查询需求出现、以及多进程并发场景（daemon + CLI + web），文件系统的局限开始显性化：没有原子跨实体操作、没有高效的过滤/排序/分页、文件锁并发度低、没有统一的备份/恢复策略。**

**正确模型是：一个共享的 `allthecodes-db` 基础设施 crate + 按领域拆分的 SQLite 数据库文件 + 统一的连接池/迁移/查询模式，取代分散的 JSON 文件存储。**

SQLite 不是要完全取代文件系统——settings、export artifacts、transcripts 仍然是文件系统更合适——而是接管所有**结构化记录型数据**的持久化。

---

## 当前状态评估

### 已有基础（可以直接用）

```
# workspace Cargo.toml 中已声明
sqlx = { version = "0.8", default-features = false, features = [
    "runtime-tokio-rustls", "sqlite", "chrono", "uuid",
] }
```

- **sqlx 0.8 已经是 workspace 依赖**，SQLite feature 已开启
- **`allthecodes-web-state`** 已是一个可工作的参考实现：WAL 模式连接池 + 懒初始化 + inline 迁移
- **`allthecodes_config::paths`** 提供了统一的数据根目录解析

### 需要替换的 JSON 文件存储

| 领域 | 当前存储 | 文件数增长 | 痛点 |
|------|----------|-----------|------|
| **Sessions** | `sessions/{uuid}.json` | 每 session 一个文件 | 列表需读取所有文件；无法按字段过滤 |
| **Tasks** | `tasks/{id}.json` | 每 task 一个文件 | 依赖关系需全量加载；跨进程锁复杂 |
| **Memories** | `memory/{key}.json` | 每 memory 一个文件 | 搜索需全量读取 + 正则匹配 |
| **Scheduled Tasks** | `scheduled_tasks.json` | 单文件 | 多进程写冲突；锁粒度粗 |
| **Daemon State** | `daemon/{worker_id}.json` | 每 worker 一个文件 | 无原子更新多个 state |
| **Gateway Events** | `runs/{run_id}/events.ndjson` | 每运行一个目录 | 无查询能力；清理需扫目录 |
| **Goals** | `goals/{id}.json` | 每 goal 一个文件 | 列表/过滤开销大 |
| **Workflows** | `workflows/{id}.json` | 每 workflow 一个文件 | 同上 |

### sqlx 0.8 关键 API 说明

sqlx 0.8 的 `sqlite` feature 提供的是编译时绑定的 SQLite3 C 库（类似 `sqlite-bundled`），但注意 sqlx 0.8 **没有** `migrate!` 宏的 SQLite 支持（这需要 `migrate` feature + 编译时 SQLite 支持）。替代方案：

- **方案 A（推荐）**：用 `sqlx::raw_sql()` + 运行时执行迁移 SQL，类似 `allthecodes-web-state` 的 `ensure_ready()` 模式——读取 embed 的 `.sql` 文件或硬编码字符串
- **方案 B**：升级到 sqlx 0.9+ 以获得完整的 `migrate!` 宏支持

当前建议走**方案 A**以最小化变更范围，未来可迁移到方案 B。

---

## 目标架构

```
┌─────────────────────────────────────────────────┐
│                 应用层（现有 crate）                │
│  allthecodes-session                             │
│  allthecodes-tasks                               │
│  allthecodes-daemon                              │
│  allthecodes-gateway                             │
│  allthecodes-services/scheduler                  │
│  allthecodes-commands/memory                     │
│  ───────────────────────────────                 │
│  改为通过 Store trait 调用 DB，不再直接读写 JSON    │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│             allthecodes-db（新建基础设施 crate）      │
│                                                   │
│  DbPoolManager                                    │
│  ├─ open_state_db()       → state_5.sqlite        │
│  ├─ open_logs_db()        → logs_2.sqlite         │
│  └─ open_app_db()         → app_1.sqlite          │
│                                                   │
│  MigrationRunner                                  │
│  ├─ run_pending("state")                          │
│  └─ run_pending("app")                            │
│                                                   │
│  Domain Stores (trait-based)                      │
│  ├─ SessionStore     [state DB]                   │
│  ├─ TaskStore        [state DB]                   │
│  ├─ MemoryStore      [state DB]                   │
│  ├─ SchedulerStore   [state DB]                   │
│  ├─ DaemonStateStore [state DB]                   │
│  ├─ GatewayStore     [app DB]                     │
│  ├─ GoalStore        [app DB]                     │
│  └─ WorkflowStore    [app DB]                     │
│                                                   │
│  Common Utilities                                 │
│  ├─ base_sqlite_options()                         │
│  ├─ try_from_row helpers                          │
│  ├─ keyset_pagination                             │
│  └─ integrity_check                               │
└─────────────────────────────────────────────────┘
```

### 分库设计

| 数据库 | 路径 | 包含的域 | 说明 |
|--------|------|----------|------|
| `state_5.sqlite` | `{data_root}/state/state_5.sqlite` | sessions, tasks, memories, scheduled_tasks, daemon_state | 核心运行时状态，读写频繁 |
| `logs_2.sqlite` | `{data_root}/state/logs_2.sqlite` | 结构化日志 | 追加写入为主，定期裁剪 |
| `app_1.sqlite` | `{data_root}/state/app_1.sqlite` | goals, workflows, gateway_events | 低频写入，查询/分析为主 |

版本号后缀（`_5`, `_2`, `_1`）允许在不兼容 migration 时创建新数据库文件，旧文件可安全删除。

### 连接池配置

```rust
pub fn base_sqlite_options() -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .create_if_missing(true)
        .auto_vacuum(SqliteAutoVacuum::Incremental)
}

pub fn create_pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(base_sqlite_options().filename(path))
}
```

所有参考 codex 的模式（WAL + Normal synchronous + 5s busy timeout），已在 `allthecodes-web-state` 验证。

---

## Migration 策略

### 运行时迁移（方案 A，已选）

每个 Store 的 `ensure_ready()` 方法用 `OnceCell<sqlx::Result<()>>` 保护，首次调用时运行迁移 SQL：

```rust
impl SessionStore {
    pub async fn ensure_ready(&self) -> sqlx::Result<()> {
        self.ready.get_or_try_init(|| async {
            sqlx::raw_sql(include_str!("../../migrations/state/0001_create_sessions.sql"))
                .execute(&self.pool)
                .await?;
            sqlx::raw_sql(include_str!("../../migrations/state/0002_create_tasks.sql"))
                .execute(&self.pool)
                .await?;
            Ok(())
        }).await.map(|_| ())
    }
}
```

这样做的原因：
1. sqlx 0.8 的 `migrate!` 宏在 `sqlite` feature 下不支持编译时嵌入
2. inline 迁移文件对所有 crate 可见，无需额外 build script
3. 每次 `raw_sql` 用 `CREATE TABLE IF NOT EXISTS`，幂等执行
4. 新增 migration 只需追加文件，版本号递增

### 版本追踪

用 SQLite 的 `PRAGMA user_version` 追踪当前 schema 版本：

```sql
-- 在每个 migrate 函数的开头
SELECT PRAGMA user_version;
-- 如果 < N，运行对应 SQL 并更新
PRAGMA user_version = N;
```

或者创建一张 `_schema_version` 表：

```sql
CREATE TABLE IF NOT EXISTS _schema_version (
    version     INTEGER NOT NULL,
    applied_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    description TEXT
);
```

---

## Store 设计模式

```rust
#[async_trait]
pub trait SessionStoreTrait: Send + Sync {
    async fn get_session(&self, id: &str) -> sqlx::Result<Option<Session>>;
    async fn list_sessions(&self, filter: SessionFilter) -> sqlx::Result<Vec<SessionSummary>>;
    async fn upsert_session(&self, session: &Session) -> sqlx::Result<()>;
    async fn delete_session(&self, id: &str) -> sqlx::Result<()>;
}

pub struct SqliteSessionStore {
    pool: SqlitePool,
    ready: OnceCell<sqlx::Result<()>>,
}

#[async_trait]
impl SessionStoreTrait for SqliteSessionStore {
    // 每个方法先 ensure_ready() 再执行查询
    async fn get_session(&self, id: &str) -> sqlx::Result<Option<Session>> {
        self.ensure_ready().await?;
        sqlx::query_as::<_, SessionRow>("SELECT * FROM sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(|r| r.map(Session::from_row))
    }
}
```

---

## 查询模式

### 静态查询

```rust
let sessions = sqlx::query_as::<_, SessionRow>(
    "SELECT id, title, created_at, updated_at, cwd
     FROM sessions
     WHERE archived = 0
     ORDER BY updated_at DESC
     LIMIT ? OFFSET ?"
)
.bind(limit)
.bind(offset)
.fetch_all(&self.pool)
.await?;
```

### 动态查询（QueryBuilder）

```rust
let mut builder = sqlx::QueryBuilder::<Sqlite>::new(
    "SELECT id, title, created_at, cwd FROM sessions WHERE 1=1"
);
if let Some(cwd) = filter.cwd {
    builder.push(" AND cwd = ").push_bind(cwd);
}
if let Some(source) = filter.source {
    builder.push(" AND source = ").push_bind(source.to_string());
}
builder.push(" ORDER BY created_at DESC LIMIT ").push_bind(filter.limit);
```

### Keyset 分页

```rust
let sessions = sqlx::query_as::<_, SessionRow>(
    "SELECT id, title, created_at, updated_at, cwd
     FROM sessions
     WHERE archived = 0
       AND (updated_at, id) < (?, ?)   -- keyset cursor
     ORDER BY updated_at DESC, id DESC
     LIMIT ?"
)
.bind(cursor.updated_at)
.bind(cursor.id)
.bind(limit)
.fetch_all(&self.pool)
.await?;
```

### JSON 列

复杂/可选字段存为 JSON TEXT：

```rust
// 建表
"CREATE TABLE IF NOT EXISTS sessions (
    id       TEXT PRIMARY KEY,
    title    TEXT,
    metadata TEXT  -- JSON blob, serde_json::Value
)"

// 写
sqlx::query("UPDATE sessions SET metadata = ? WHERE id = ?")
    .bind(serde_json::to_string(&metadata).unwrap())
    .bind(id)
    .execute(&self.pool)
    .await?;

// 读：sqlx 不支持 SELECT -> struct 自动 JSON 解析
// 需要手动 from_row
let metadata: serde_json::Value = serde_json::from_str(row.get("metadata"))?;
```

---

## 阶段计划

### Phase 0 — 基础设施（估算：3–5 天）

新建 `allthecodes-db` crate，包含：
- `lib.rs`: re-export 所有 store trait 和连接管理
- `pool.rs`: `DbPoolManager`，管理 3 个连接池
- `migration.rs`: `MigrationRunner`，版本追踪
- `types.rs`: 共享类型（分页参数、游标、过滤条件）
- `store/mod.rs`: Store trait 定义
- 为每个 domain 定义 trait（SessionStoreTrait, TaskStoreTrait, ...）

**不创建具体实现**，只定义接口。

### Phase 1 — Task 迁移（估算：3–5 天）

- 实现 `SqliteTaskStore`
- 表：`tasks`（替代 `{id}.json`）+ `task_dependencies`
- 支持依赖关系查询：`SELECT * FROM tasks WHERE id IN (SELECT depends_on FROM task_dependencies WHERE task_id = ?)`
- task output 仍可以文件存储（大文本），SQLite 存路径和摘要
- 保留 feature flag 与 JSON fallback，初期不做旧 JSON task 数据 backfill

### Phase 2 — Session 迁移（估算：3–5 天）

- ✅ 已在 `allthecodes-session::storage` 中落地 SQLite 优先读写。
- ✅ 表：`sessions`, `session_messages`。
- ✅ `save/load/list/archive/rename/chat_mode/truncate` 保持现有 public API，并在 SQLite 不可用时 fallback 到 JSON 文件。
- ✅ 新增 `list_sessions_page(limit, cursor)` 与 `list_workspace_sessions_page(cwd, limit, cursor)`，排序固定为 `last_modified DESC, created_at DESC, session_id ASC`。
- ✅ 分页/list 前幂等导入顶层 legacy `sessions/*.json`；损坏 JSON 跳过并记录 warning。
- ✅ JSON 文件继续作为兼容备份，不删除旧数据。

### Phase 3 — 其余 Domain 迁移（估算：5–7 天）

- MemoryStore（替换 memdir 文件）
- ✅ SchedulerStore（SQLite 优先，`scheduled_tasks.json` 作为导入源与 backup/fallback）
- ✅ DaemonStateStore（SQLite 优先，`daemon/*.json` 与 worker JSON 作为导入源与 backup/fallback）
- GoalStore, WorkflowStore（替换 `goals/*.json`, `workflows/*.json`）

### Phase 4 — 增强功能（估算：3–5 天）

- 全文搜索：session messages / memories 的 FTS5 全文索引
- 备份/恢复：`VACUUM INTO` 热备份命令
- 健康检查：`PRAGMA integrity_check` 定时运行
- 日志 DB 的定期裁剪（按大小/时间）

---

## 对比 codex 的异同

| 维度 | codex | allthecodes（计划） | 原因 |
|------|-------|-------------------|------|
| 库 | sqlx 0.9 | sqlx 0.8 | allthecodes 0.8 已存在 |
| 分库数 | 4 | 3 | codex 的 logs DB 必要性待验证 |
| 迁移方式 | 编译时 `migrate!` | 运行时 inline + file | sqlx 0.8 限制 |
| Backfill/repair | 独立模块 | 初期不做 | 先用 `integrity_check` |
| `FromRow` derive | 极少（仅 LogRow） | 初期使用少量 derive，逐步手写 | 更灵活的错误处理 |
| QueryBuilder | 动态过滤 + 批量插入 | 相同模式 | 已验证的模式 |
| JSON 列 | 少量 | 同 | 保持简单列设计 |

---

## 不做的事

1. **不用 ORM** — 维持 raw SQL + 手动 row 映射
2. **不完全替换文件存储** — settings、export artifacts、transcripts 保持文件系统
3. **不用 `migrate!` 宏** — 等 sqlx 0.9+ 升级时再考虑
4. **不用连接池动态扩容** — 5 connections per DB 足够
5. **不引入异步 write-ahead 队列** — 同步写入足够，批处理在未来

---

## Confidence

Medium-high。codex 已经用同样模式（sqlx + raw SQL + WAL + 多库分离）在生产运行。allthecodes 已有的 `allthecodes-web-state` 是更小规模的验证。主要的风险点是 session 迁移——session 数据量大、格式复杂、需要来回文件/DB 双写。建议 Phase 0 先建好 DB crate，在 Phase 1 先迁移 tasks 并验证双写/fallback 模式，确认无问题后再推进到 sessions 和其他 domain。
