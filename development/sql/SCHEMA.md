# 分库设计与表结构

## 库结构总览

```
{data_root}/state/
├── state_5.sqlite      # 核心运行时状态
├── logs_2.sqlite       # 结构化日志
├── app_1.sqlite        # 应用级数据
└── *.sqlite-wal        # WAL 侧车文件（自动管理）
    *.sqlite-shm
```

---

## 1. state_5.sqlite — 核心运行时状态

### 1.1 sessions

当前文件存储：`~/.allthecodes/sessions/{uuid}.json`

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id                TEXT PRIMARY KEY,
    title             TEXT,
    cwd               TEXT NOT NULL,
    created_at        TEXT NOT NULL,  -- ISO 8601
    updated_at        TEXT NOT NULL,  -- ISO 8601
    archived          INTEGER NOT NULL DEFAULT 0,
    archived_at       TEXT,
    chat_mode_override TEXT,
    message_count     INTEGER NOT NULL DEFAULT 0,
    token_count       INTEGER NOT NULL DEFAULT 0,
    source            TEXT,           -- "cli" | "web" | "daemon"
    model_provider    TEXT,
    model             TEXT,
    cli_version       TEXT,
    git_sha           TEXT,
    git_branch        TEXT,
    git_origin_url    TEXT,
    preview           TEXT,           -- session preview text
    metadata_json     TEXT            -- extra JSON blob
);

CREATE INDEX idx_sessions_updated_at ON sessions(updated_at DESC);
CREATE INDEX idx_sessions_created_at ON sessions(created_at DESC);
CREATE INDEX idx_sessions_archived_cwd ON sessions(archived, cwd);
-- keyset pagination index
CREATE INDEX idx_sessions_keyset ON sessions(updated_at DESC, id DESC);
```

### 1.2 session_messages

当前存储：嵌在 `sessions/{uuid}.json` 的 `messages` 数组。

```sql
CREATE TABLE IF NOT EXISTS session_messages (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    role        TEXT NOT NULL,      -- "user" | "assistant" | "tool"
    content     TEXT NOT NULL,      -- JSON serialized content
    created_at  TEXT NOT NULL,
    token_count INTEGER,
    parent_id   TEXT,               -- for branching/resume
    UNIQUE(session_id, position)
);

CREATE INDEX idx_session_messages_session ON session_messages(session_id, position);
```

设计考虑：
- 当前 session JSON 文件包含完整消息列表。迁移后 messages 在单独表中，支持按 session 高效加载/分页
- `content` 存为 JSON TEXT 而非多表 EAV，保持与 session export 格式一致
- `token_count` 列允许按 token 数排序/过滤，支持 context budgeting

### 1.3 tasks

当前文件存储：`~/.allthecodes/tasks/{id}.json`

```sql
CREATE TABLE IF NOT EXISTS tasks (
    id                TEXT PRIMARY KEY,
    kind              TEXT NOT NULL,     -- "bash" | "agent" | "remote" | ...
    status            TEXT NOT NULL,     -- "pending" | "in_progress" | "completed" | "failed" | "cancelled"
    subject           TEXT,
    description       TEXT,
    owner             TEXT,
    parent_id         TEXT,
    active_form       TEXT,
    metadata_json     TEXT,
    output_summary    TEXT,
    output_file       TEXT,              -- path to {id}.output.log
    output_bytes      INTEGER DEFAULT 0,
    output_truncated  INTEGER DEFAULT 0,
    cancel_requested_at TEXT,
    recovered_at      TEXT,
    previous_status   TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    started_at        TEXT,
    completed_at      TEXT
);

CREATE INDEX idx_tasks_status ON tasks(status, updated_at DESC);
CREATE INDEX idx_tasks_owner ON tasks(owner, status);
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
```

### 1.4 task_dependencies

当前存储：嵌在 task JSON 的 `depends_on` 字段。

```sql
CREATE TABLE IF NOT EXISTS task_dependencies (
    task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on   TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on)
);
```

### 1.5 memories

当前文件存储：`~/.allthecodes/memory/{key}.json` 或 `.allthecodes/memory/{key}.json`

```sql
CREATE TABLE IF NOT EXISTS memories (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    content     TEXT NOT NULL,
    category    TEXT,             -- "user" | "feedback" | "project" | "reference"
    search_terms TEXT,
    scope       TEXT NOT NULL DEFAULT 'global',  -- "global" | "project" | "team"
    project_cwd TEXT,            -- non-NULL when scope = "project" | "team"
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_usage  TEXT
);

CREATE INDEX idx_memories_name ON memories(name);
CREATE INDEX idx_memories_scope ON memories(scope, project_cwd);
CREATE INDEX idx_memories_category ON memories(category);
```

### 1.6 scheduled_tasks

当前文件存储：`~/.allthecodes/scheduled_tasks.json`

```sql
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,       -- "cron" | "once" | "interval"
    name        TEXT NOT NULL,
    schedule    TEXT,                 -- cron expression "*/5 * * * *"
    interval_ms INTEGER,
    payload_json TEXT,
    next_run_at TEXT,
    last_run_at TEXT,
    paused      INTEGER NOT NULL DEFAULT 0,
    run_count   INTEGER DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_scheduled_tasks_next_run ON scheduled_tasks(next_run_at)
    WHERE paused = 0 AND next_run_at IS NOT NULL;
```

### 1.7 daemon_state

当前文件存储：`~/.allthecodes/daemon/` 下多个 JSON 文件

```sql
CREATE TABLE IF NOT EXISTS daemon_state (
    key         TEXT PRIMARY KEY,    -- "supervisor" | "shutdown-request" | "control-token" | "sleep-state"
    value_json  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
```

键值对模式 —— supervisor 进程状态、shutdown 信号、control token 等都存为 JSON 值。简单、灵活，无需每新增一个 daemon state 类型就改 schema。

### 1.8 daemon_workers

当前文件存储：`~/.allthecodes/daemon/workers/{worker_id}.json`

```sql
CREATE TABLE IF NOT EXISTS daemon_workers (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL,    -- "supervisor" | "worker" | "webhook"
    status          TEXT NOT NULL,    -- "running" | "stopped" | "crashed"
    pid             INTEGER,
    port            INTEGER,
    metadata_json   TEXT,
    started_at      TEXT,
    stopped_at      TEXT,
    last_heartbeat  TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_daemon_workers_status ON daemon_workers(status);
```

---

## 2. logs_2.sqlite — 结构化日志

### 2.1 log_entries

```sql
CREATE TABLE IF NOT EXISTS log_entries (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,        -- ISO 8601
    ts_nanos        INTEGER NOT NULL,     -- sub-second precision
    level           TEXT NOT NULL,        -- "trace" | "debug" | "info" | "warn" | "error"
    target          TEXT,
    message         TEXT NOT NULL,
    module_path     TEXT,
    file            TEXT,
    line            INTEGER,
    thread_id       TEXT,
    process_uuid    TEXT,
    fields_json     TEXT,                 -- structured log fields (serde_json)
    estimated_bytes INTEGER
);

CREATE INDEX idx_log_entries_ts ON log_entries(timestamp DESC, id DESC);
CREATE INDEX idx_log_entries_thread ON log_entries(thread_id, timestamp DESC);
CREATE INDEX idx_log_entries_level ON log_entries(level, timestamp DESC);
CREATE INDEX idx_log_entries_process ON log_entries(process_uuid, timestamp DESC)
    WHERE thread_id IS NULL;
```

索引参考 codex：`(ts DESC, ts_nanos DESC, id DESC)`、`(thread_id)`、`(thread_id, ts DESC, ts_nanos DESC, id DESC)`、partial index `(process_uuid, ts DESC, ts_nanos DESC, id DESC) WHERE thread_id IS NULL`。

### 2.2 log_retention

日志裁剪策略（代码级，非表）：

```sql
-- 按条数裁剪（保留最近 N 条）
DELETE FROM log_entries WHERE id NOT IN (
    SELECT id FROM log_entries ORDER BY id DESC LIMIT ?
);

-- 按时间裁剪（保留最近 N 天）
DELETE FROM log_entries WHERE timestamp < datetime('now', ?);

-- 按大小裁剪（保留最近 N MB）
DELETE FROM log_entries WHERE id NOT IN (
    SELECT id FROM log_entries ORDER BY id DESC
    LIMIT (SELECT count(*) FROM log_entries WHERE (
        SELECT sum(estimated_bytes) FROM (
            SELECT estimated_bytes FROM log_entries ORDER BY id DESC LIMIT ?
        )
    ) <= ? * 1024 * 1024)
);
```

---

## 3. app_1.sqlite — 应用级数据

### 3.1 goals

当前文件存储：`~/.allthecodes/goals/{id}.json`

```sql
CREATE TABLE IF NOT EXISTS goals (
    id              TEXT PRIMARY KEY,
    thread_id       TEXT,
    objective       TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active', 'paused', 'blocked', 'complete', 'cancelled')),
    token_budget    INTEGER,
    tokens_used     INTEGER DEFAULT 0,
    time_budget_sec INTEGER,
    time_used_sec   INTEGER DEFAULT 0,
    metadata_json   TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_goals_thread ON goals(thread_id);
CREATE INDEX idx_goals_status ON goals(status);
```

### 3.2 workflows

当前文件存储：`~/.allthecodes/workflows/{id}.json`

```sql
CREATE TABLE IF NOT EXISTS workflows (
    id              TEXT PRIMARY KEY,
    name            TEXT,
    description     TEXT,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
    script          TEXT,            -- workflow script content
    result_summary  TEXT,
    run_count       INTEGER DEFAULT 0,
    metadata_json   TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_workflows_status ON workflows(status, updated_at DESC);
```

### 3.3 gateway_runs

当前文件存储：`~/.allthecodes/gateway/runs/{run_id}/meta.json`

```sql
CREATE TABLE IF NOT EXISTS gateway_runs (
    id              TEXT PRIMARY KEY,
    adapter_id      TEXT NOT NULL,
    status          TEXT NOT NULL,     -- "active" | "completed" | "failed"
    request_json    TEXT,
    response_json   TEXT,
    duration_ms     INTEGER,
    error           TEXT,
    parent_run_id   TEXT,              -- for nested/spawned runs
    created_at      TEXT NOT NULL,
    completed_at    TEXT
);

CREATE INDEX idx_gateway_runs_adapter ON gateway_runs(adapter_id, created_at DESC);
CREATE INDEX idx_gateway_runs_status ON gateway_runs(status);
```

### 3.4 gateway_events

当前文件存储：`~/.allthecodes/gateway/runs/{run_id}/events.ndjson`

```sql
CREATE TABLE IF NOT EXISTS gateway_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          TEXT NOT NULL REFERENCES gateway_runs(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,     -- "request" | "response" | "error" | "heartbeat"
    payload_json    TEXT NOT NULL,
    sequence        INTEGER NOT NULL,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_gateway_events_run ON gateway_events(run_id, sequence);
```

---

## 4. 跨库操作注意事项

### 不支持跨库 JOIN

SQLite 不支持跨 `.sqlite` 文件的 JOIN。以下查询不能在 SQL 层面完成：

- "查某个 session 关联的所有 task" → application 层分两次查询
- "查某个 thread 的 goal + 相关 workflow" → application 层分两次查询

### 外部键约束

SQLite 默认关闭 `PRAGMA foreign_keys = ON`，需要在每次连接时设置：

```rust
pub fn base_sqlite_options() -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .create_if_missing(true)
        .foreign_keys(true)        // <-- 启用外键约束
}
```

### FTS5 全文搜索（Phase 4）

```sql
-- FTS5 虚拟表（Phase 4 添加）
CREATE VIRTUAL TABLE IF NOT EXISTS sessions_fts USING fts5(
    title, preview,
    content='sessions',
    content_rowid='rowid'
);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    name, content, search_terms,
    content='memories',
    content_rowid='rowid'
);
```
