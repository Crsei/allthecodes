# Agent Runtime SQLite Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist agent runtime lifecycle and tool execution audit records into SQLite so session, agent, tool, shell digest, retry/fallback, model, and permission decision data are queryable after process exit.

**Architecture:** Keep `session_messages` as the conversation transcript and store runtime audit data in `logs_2.sqlite`. Add a small root-crate persistence module that uses `allthecodes-db::DbPoolManager::logs_pool()`, then wire `RootDashboardEmitter` so existing agent lifecycle and `AgentRuntimeExecutionRecord` emission paths are mirrored to SQLite without changing the engine's normal event flow.

**Tech Stack:** Rust 2021, sqlx 0.8 SQLite, `allthecodes-db`, `allthecodes-types::AgentRuntimeExecutionRecord`, existing `DashboardEmitter` adapter boundary.

## Global Constraints

- Do not write full stdout or stderr to SQLite; persist only `stdout_digest` and `stderr_digest`.
- Do not store runtime audit rows in `session_messages`; transcripts remain user/assistant/tool context only.
- Use `logs_2.sqlite` under `{data_root}/state/` because runtime records are append-heavy audit data.
- Keep dashboard NDJSON behavior compatible; SQLite persistence is additive.
- SQLite write failures must not fail tool execution, agent execution, or user-visible message delivery.
- Use existing `allthecodes-db::MigrationRunner` and `Migration` patterns; do not introduce a new migration framework.
- Use raw SQL and manual row mapping; do not introduce an ORM.
- Preserve nullable execution record fields exactly as `AgentRuntimeExecutionRecord` exposes them.
- Keep npm packaging backend-only; do not touch `../allthecodes-web` or web-ui feature packaging.
- Run Cargo with the repository toolchain environment from `AGENTS.md`, especially `CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target`.

---

## Current State

Already present:

- `allthecodes-session` persists session metadata and `session_messages` into `state_5.sqlite`.
- `AgentRuntimeExecutionRecord` already contains the target audit fields: `session_id`, `agent_id`, `parent_agent_id`, `agent_role`, `tool`, `tool_use_id`, `command`, `cwd`, `exit_code`, `stdout_digest`, `stderr_digest`, `retry_count`, `model`, `fallback_used`, `permission_decision`, `duration_ms`, `had_error`, `schema_version`.
- `crates/allthecodes-engine/src/query/loop_impl.rs` emits `AgentEvent::ExecutionRecord` after each completed tool execution.
- `crates/allthecodes/src/dashboard.rs` can append execution records to `runs/{session_id}/execution-records.ndjson` when `FEATURE_SUBAGENT_DASHBOARD` is enabled.

Gap:

- The runtime record is not persisted into SQLite.
- Agent lifecycle events are not persisted into SQLite.
- `session_messages` may contain tool result messages, but it does not expose structured shell digest, retry/fallback, model, or final permission decision columns.

Decision:

- Persist two SQLite surfaces in `logs_2.sqlite`:
  - `agent_runtime_events`: append-only lifecycle/event audit stream.
  - `agent_runtime_execution_records`: structured one-row-per-tool-execution table.
- Do not persist raw `ToolResult` output beyond the existing transcript/task output paths.

## File Structure

Create:

- `crates/allthecodes/src/runtime_history.rs`
  - Owns SQLite migrations and persistence functions for runtime audit data.
  - Converts root adapter inputs into SQL rows.
  - Keeps persistence best-effort and local to the root app layer.

Modify:

- `crates/allthecodes/src/main.rs`
  - Add `mod runtime_history;`.
- `crates/allthecodes/src/dashboard.rs`
  - Add a small `current_session_id()` accessor for runtime audit rows.
- `crates/allthecodes/src/startup_traits.rs`
  - Mirror `RootDashboardEmitter::emit_subagent_event()` and `emit_execution_record()` to `runtime_history`.
- `crates/allthecodes/Cargo.toml`
  - Add `allthecodes-db = { workspace = true }` and `sqlx = { workspace = true }` if needed by root-crate tests.
- `development/sql/SCHEMA.md`
  - Document the two new `logs_2.sqlite` tables.
- `development/sql/README.md`
  - Mark this plan as the agent runtime SQLite persistence plan.

Do not modify:

- `crates/allthecodes-session/src/storage/sqlite_store.rs`
  - It remains transcript/session storage.
- `crates/allthecodes-types/src/agent_runtime_record.rs`
  - The current type is already the wire contract.
- `crates/allthecodes-engine/src/query/loop_impl.rs`
  - The first implementation should use the existing app-layer adapter path.

## Schema

Add this to `development/sql/SCHEMA.md` under `logs_2.sqlite`:

```sql
CREATE TABLE IF NOT EXISTS agent_runtime_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,
    ts_millis       INTEGER NOT NULL,
    session_id      TEXT,
    agent_id        TEXT NOT NULL,
    parent_agent_id TEXT,
    kind            TEXT NOT NULL,
    description     TEXT,
    model           TEXT,
    depth           INTEGER,
    background      INTEGER NOT NULL DEFAULT 0,
    payload_json    TEXT,
    schema_version  INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_session
    ON agent_runtime_events(session_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_agent
    ON agent_runtime_events(agent_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_kind
    ON agent_runtime_events(kind, timestamp DESC, id DESC);

CREATE TABLE IF NOT EXISTS agent_runtime_execution_records (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp           TEXT NOT NULL,
    ts_millis           INTEGER NOT NULL,
    session_id          TEXT NOT NULL,
    agent_id            TEXT NOT NULL,
    parent_agent_id     TEXT,
    agent_role          TEXT,
    tool                TEXT NOT NULL,
    tool_use_id         TEXT,
    command             TEXT,
    cwd                 TEXT,
    exit_code           INTEGER,
    stdout_digest       TEXT,
    stderr_digest       TEXT,
    retry_count         INTEGER NOT NULL DEFAULT 0,
    model               TEXT,
    fallback_used       INTEGER NOT NULL DEFAULT 0,
    permission_decision TEXT,
    duration_ms         INTEGER,
    had_error           INTEGER NOT NULL DEFAULT 0,
    record_json         TEXT NOT NULL,
    schema_version      INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_session
    ON agent_runtime_execution_records(session_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_agent
    ON agent_runtime_execution_records(agent_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_tool
    ON agent_runtime_execution_records(tool, timestamp DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_permission
    ON agent_runtime_execution_records(permission_decision, timestamp DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_model
    ON agent_runtime_execution_records(model, timestamp DESC, id DESC);
```

Rationale:

- `record_json` preserves the full stable wire payload for forward compatibility.
- Columns expose common query fields without JSON extraction.
- `session_id` is nullable only for generic lifecycle events because early startup can emit before dashboard session id is initialized; execution records require a session id from `QueryDeps`.

## Task 1: Runtime History Store

**Files:**

- Create: `crates/allthecodes/src/runtime_history.rs`
- Modify: `crates/allthecodes/src/main.rs`
- Modify: `crates/allthecodes/src/dashboard.rs`
- Modify: `crates/allthecodes/Cargo.toml`
- Test: `crates/allthecodes/src/runtime_history.rs`

**Interfaces:**

- Produces: `pub(crate) struct RuntimeSubagentEvent<'a>`
- Produces: `pub(crate) fn persist_subagent_event(event: RuntimeSubagentEvent<'_>) -> anyhow::Result<()>`
- Produces: `pub(crate) fn persist_execution_record(record: &AgentRuntimeExecutionRecord) -> anyhow::Result<()>`
- Produces: `pub(crate) fn dashboard::current_session_id() -> Option<&'static str>`
- Consumes: `allthecodes_db::DbPoolManager::logs_pool()`
- Consumes: `allthecodes_db::MigrationRunner`

- [ ] **Step 1: Add root-crate dependencies**

Edit `crates/allthecodes/Cargo.toml` and add these workspace dependencies in the existing workspace crates / persistence dependency area:

```toml
allthecodes-db = { workspace = true }
sqlx = { workspace = true }
```

- [ ] **Step 2: Register the module**

Edit `crates/allthecodes/src/main.rs` near `mod dashboard;`:

```rust
mod dashboard;
mod runtime_history;
```

- [ ] **Step 3: Expose current dashboard session id**

Edit `crates/allthecodes/src/dashboard.rs` below `init_session_id()`:

```rust
pub(crate) fn current_session_id() -> Option<&'static str> {
    SESSION_ID.get().map(String::as_str)
}
```

This is intentionally read-only and does not change dashboard initialization. `runtime_history` uses it for lifecycle event rows; execution records still use the `record.session_id` supplied by the query runtime.

- [ ] **Step 4: Write failing persistence tests**

Create `crates/allthecodes/src/runtime_history.rs` with the module skeleton and tests first. Add these local test helpers inside `#[cfg(test)] mod tests`:

```rust
use allthecodes_types::agent_runtime_record::{
    AgentRuntimeExecutionRecord, AgentRuntimePermissionDecision,
};
use sqlx::Row;

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[derive(Debug)]
struct TestExecutionRecordRow {
    session_id: String,
    tool: String,
    command: Option<String>,
    exit_code: Option<i64>,
    stdout_digest: Option<String>,
    retry_count: i64,
    fallback_used: bool,
    permission_decision: Option<String>,
    record_json: String,
}

#[derive(Debug)]
struct TestRuntimeEventRow {
    kind: String,
    agent_id: String,
    parent_agent_id: Option<String>,
    payload_json: Option<String>,
}

fn query_execution_records_for_test() -> anyhow::Result<Vec<TestExecutionRecordRow>> {
    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-test", async {
        let pool = migrated_pool().await?;
        let rows = sqlx::query(
            r#"
            SELECT session_id, tool, command, exit_code, stdout_digest,
                   retry_count, fallback_used, permission_decision, record_json
            FROM agent_runtime_execution_records
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(TestExecutionRecordRow {
                    session_id: row.try_get("session_id")?,
                    tool: row.try_get("tool")?,
                    command: row.try_get("command")?,
                    exit_code: row.try_get("exit_code")?,
                    stdout_digest: row.try_get("stdout_digest")?,
                    retry_count: row.try_get("retry_count")?,
                    fallback_used: row.try_get::<i64, _>("fallback_used")? != 0,
                    permission_decision: row.try_get("permission_decision")?,
                    record_json: row.try_get("record_json")?,
                })
            })
            .collect()
    })
}

fn query_events_for_test() -> anyhow::Result<Vec<TestRuntimeEventRow>> {
    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-test", async {
        let pool = migrated_pool().await?;
        let rows = sqlx::query(
            r#"
            SELECT kind, agent_id, parent_agent_id, payload_json
            FROM agent_runtime_events
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(TestRuntimeEventRow {
                    kind: row.try_get("kind")?,
                    agent_id: row.try_get("agent_id")?,
                    parent_agent_id: row.try_get("parent_agent_id")?,
                    payload_json: row.try_get("payload_json")?,
                })
            })
            .collect()
    })
}
```

Minimum tests:

```rust
#[test]
#[serial_test::serial]
fn execution_record_persists_queryable_columns() {
    let temp = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

    let record = AgentRuntimeExecutionRecord {
        session_id: "session-1".to_string(),
        agent_id: "agent-1".to_string(),
        parent_agent_id: Some("parent-1".to_string()),
        agent_role: Some("build-agent".to_string()),
        tool: "shell".to_string(),
        tool_use_id: Some("toolu-1".to_string()),
        command: Some("npm test".to_string()),
        cwd: Some(std::path::PathBuf::from("/repo")),
        exit_code: Some(1),
        stdout_digest: Some("a".repeat(64)),
        stderr_digest: Some("b".repeat(64)),
        retry_count: 2,
        model: Some("claude-sonnet-5".to_string()),
        fallback_used: true,
        permission_decision: Some(AgentRuntimePermissionDecision::AllowedByPolicy),
        duration_ms: Some(123),
        had_error: true,
        schema_version: 1,
    };

    persist_execution_record(&record).unwrap();

    let rows = query_execution_records_for_test().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id, "session-1");
    assert_eq!(rows[0].tool, "shell");
    assert_eq!(rows[0].command.as_deref(), Some("npm test"));
    assert_eq!(rows[0].exit_code, Some(1));
    assert_eq!(rows[0].retry_count, 2);
    assert!(rows[0].fallback_used);
    assert_eq!(rows[0].permission_decision.as_deref(), Some("allowed_by_policy"));
    assert!(rows[0].record_json.contains("\"stdout_digest\""));
}

#[test]
#[serial_test::serial]
fn subagent_event_persists_without_dashboard_feature() {
    let temp = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

    persist_subagent_event(RuntimeSubagentEvent {
        kind: "spawned",
        agent_id: "agent-1",
        parent_agent_id: Some("main"),
        description: Some("build checks"),
        model: Some("claude-sonnet-5"),
        depth: 1,
        background: false,
        payload: Some(serde_json::json!({"source": "test"})),
    })
    .unwrap();

    let rows = query_events_for_test().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "spawned");
    assert_eq!(rows[0].agent_id, "agent-1");
    assert_eq!(rows[0].parent_agent_id.as_deref(), Some("main"));
    assert!(rows[0].payload_json.as_ref().unwrap().contains("\"source\""));
}
```

Run:

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"
cargo test -p allthecodes runtime_history
```

Expected: FAIL because `persist_execution_record`, `persist_subagent_event`, `migrated_pool`, and the tables do not exist yet.

- [ ] **Step 5: Implement migrations and insert functions**

Implement the production path in `runtime_history.rs` using this shape:

```rust
use anyhow::{Context, Result};
use allthecodes_db::{Migration, MigrationRunner};
use allthecodes_types::agent_runtime_record::AgentRuntimeExecutionRecord;
use serde_json::Value;
use sqlx::SqlitePool;

const MIGRATIONS: &[Migration] = &[
    Migration::new(1, r#"
        CREATE TABLE IF NOT EXISTS agent_runtime_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            ts_millis INTEGER NOT NULL,
            session_id TEXT,
            agent_id TEXT NOT NULL,
            parent_agent_id TEXT,
            kind TEXT NOT NULL,
            description TEXT,
            model TEXT,
            depth INTEGER,
            background INTEGER NOT NULL DEFAULT 0,
            payload_json TEXT,
            schema_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_session
            ON agent_runtime_events(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_agent
            ON agent_runtime_events(agent_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_kind
            ON agent_runtime_events(kind, timestamp DESC, id DESC);
    "#),
    Migration::new(2, r#"
        CREATE TABLE IF NOT EXISTS agent_runtime_execution_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            ts_millis INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            parent_agent_id TEXT,
            agent_role TEXT,
            tool TEXT NOT NULL,
            tool_use_id TEXT,
            command TEXT,
            cwd TEXT,
            exit_code INTEGER,
            stdout_digest TEXT,
            stderr_digest TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            model TEXT,
            fallback_used INTEGER NOT NULL DEFAULT 0,
            permission_decision TEXT,
            duration_ms INTEGER,
            had_error INTEGER NOT NULL DEFAULT 0,
            record_json TEXT NOT NULL,
            schema_version INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_session
            ON agent_runtime_execution_records(session_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_agent
            ON agent_runtime_execution_records(agent_id, id);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_tool
            ON agent_runtime_execution_records(tool, timestamp DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_permission
            ON agent_runtime_execution_records(permission_decision, timestamp DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_model
            ON agent_runtime_execution_records(model, timestamp DESC, id DESC);
    "#),
];

pub(crate) struct RuntimeSubagentEvent<'a> {
    pub kind: &'a str,
    pub agent_id: &'a str,
    pub parent_agent_id: Option<&'a str>,
    pub description: Option<&'a str>,
    pub model: Option<&'a str>,
    pub depth: usize,
    pub background: bool,
    pub payload: Option<Value>,
}

pub(crate) fn persist_subagent_event(event: RuntimeSubagentEvent<'_>) -> Result<()> {
    let payload_json = event
        .payload
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("failed to serialize agent runtime event payload")?;

    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-sqlite", async move {
        let pool = migrated_pool().await?;
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO agent_runtime_events (
                timestamp, ts_millis, session_id, agent_id, parent_agent_id,
                kind, description, model, depth, background, payload_json,
                schema_version
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.timestamp_millis())
        .bind(crate::dashboard::current_session_id().map(ToOwned::to_owned))
        .bind(event.agent_id)
        .bind(event.parent_agent_id)
        .bind(event.kind)
        .bind(event.description)
        .bind(event.model)
        .bind(event.depth as i64)
        .bind(event.background)
        .bind(payload_json)
        .execute(&pool)
        .await
        .context("failed to insert sqlite agent runtime event")?;
        Ok(())
    })
}

pub(crate) fn persist_execution_record(record: &AgentRuntimeExecutionRecord) -> Result<()> {
    let record = record.clone();
    let record_json =
        serde_json::to_string(&record).context("failed to serialize execution record")?;

    allthecodes_db::run_sqlite_sync("allthecodes-runtime-history-sqlite", async move {
        let pool = migrated_pool().await?;
        let now = chrono::Utc::now();
        sqlx::query(
            r#"
            INSERT INTO agent_runtime_execution_records (
                timestamp, ts_millis, session_id, agent_id, parent_agent_id,
                agent_role, tool, tool_use_id, command, cwd, exit_code,
                stdout_digest, stderr_digest, retry_count, model, fallback_used,
                permission_decision, duration_ms, had_error, record_json,
                schema_version
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.timestamp_millis())
        .bind(&record.session_id)
        .bind(&record.agent_id)
        .bind(&record.parent_agent_id)
        .bind(&record.agent_role)
        .bind(&record.tool)
        .bind(&record.tool_use_id)
        .bind(&record.command)
        .bind(record.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
        .bind(record.exit_code)
        .bind(&record.stdout_digest)
        .bind(&record.stderr_digest)
        .bind(record.retry_count as i64)
        .bind(&record.model)
        .bind(record.fallback_used)
        .bind(record.permission_decision.map(|d| d.as_str().to_string()))
        .bind(record.duration_ms.map(|v| v as i64))
        .bind(record.had_error)
        .bind(record_json)
        .bind(record.schema_version as i64)
        .execute(&pool)
        .await
        .context("failed to insert sqlite agent runtime execution record")?;
        Ok(())
    })
}

async fn migrated_pool() -> Result<SqlitePool> {
    let pool = allthecodes_db::DbPoolManager::new().logs_pool()?;
    MigrationRunner::new("agent-runtime-history", MIGRATIONS)
        .run(&pool)
        .await?;
    Ok(pool)
}
```

- [ ] **Step 6: Make tests pass**

Run:

```bash
cargo test -p allthecodes runtime_history
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A -- crates/allthecodes/Cargo.toml crates/allthecodes/src/main.rs crates/allthecodes/src/dashboard.rs crates/allthecodes/src/runtime_history.rs
git commit -m "Persist runtime history in sqlite"
```

## Task 2: Root Adapter Wiring

**Files:**

- Modify: `crates/allthecodes/src/startup_traits.rs`
- Modify: `crates/allthecodes/src/dashboard.rs`
- Test: `crates/allthecodes/src/runtime_history.rs`

**Interfaces:**

- Consumes: `runtime_history::persist_subagent_event()`
- Consumes: `runtime_history::persist_execution_record()`
- Keeps: `crate::dashboard::emit_subagent_event()`
- Keeps: `crate::dashboard::emit_execution_record()`

- [ ] **Step 1: Write adapter-level tests**

Add tests that call `RootDashboardEmitter` through the `DashboardEmitter` trait. These tests verify persistence works even when `FEATURE_SUBAGENT_DASHBOARD` is not set.

```rust
#[test]
#[serial_test::serial]
fn root_dashboard_emitter_persists_execution_record_when_ndjson_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
    let _dashboard = EnvGuard::remove("FEATURE_SUBAGENT_DASHBOARD");

    let emitter = crate::startup_traits::RootDashboardEmitter;
    emitter.emit_execution_record(&sample_execution_record()).unwrap();

    let rows = query_execution_records_for_test().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_id, "session-1");
}
```

Run:

```bash
cargo test -p allthecodes root_dashboard_emitter_persists
```

Expected: FAIL because `RootDashboardEmitter` still only calls `dashboard`.

- [ ] **Step 2: Wire best-effort SQLite writes**

Modify `crates/allthecodes/src/startup_traits.rs`:

```rust
impl DashboardEmitter for RootDashboardEmitter {
    fn emit_subagent_event(
        &self,
        kind: &str,
        agent_id: &str,
        parent_agent_id: Option<&str>,
        description: Option<&str>,
        model: Option<&str>,
        depth: usize,
        background: bool,
        payload: Option<Value>,
    ) -> anyhow::Result<()> {
        if let Err(error) = crate::runtime_history::persist_subagent_event(
            crate::runtime_history::RuntimeSubagentEvent {
                kind,
                agent_id,
                parent_agent_id,
                description,
                model,
                depth,
                background,
                payload: payload.clone(),
            },
        ) {
            tracing::debug!(%error, agent_id, kind, "failed to persist agent runtime event");
        }

        crate::dashboard::emit_subagent_event(
            kind,
            agent_id,
            parent_agent_id,
            description,
            model,
            depth,
            background,
            payload,
        )
    }

    fn emit_execution_record(&self, record: &AgentRuntimeExecutionRecord) -> anyhow::Result<()> {
        if let Err(error) = crate::runtime_history::persist_execution_record(record) {
            tracing::debug!(
                %error,
                agent_id = %record.agent_id,
                tool = %record.tool,
                "failed to persist agent runtime execution record"
            );
        }

        crate::dashboard::emit_execution_record(record)
    }
}
```

- [ ] **Step 3: Verify adapter tests**

Run:

```bash
cargo test -p allthecodes root_dashboard_emitter_persists
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A -- crates/allthecodes/src/startup_traits.rs crates/allthecodes/src/runtime_history.rs
git commit -m "Mirror runtime events through sqlite adapter"
```

## Task 3: Schema Documentation

**Files:**

- Modify: `development/sql/SCHEMA.md`
- Modify: `development/sql/README.md`

**Interfaces:**

- Consumes: schema from Task 1.
- Produces: documented operator contract for `agent_runtime_events` and `agent_runtime_execution_records`.

- [ ] **Step 1: Update `SCHEMA.md`**

Under `## 2. logs_2.sqlite — 结构化日志`, add a subsection after `log_entries` or after `log_retention`:

````markdown
### 2.3 agent_runtime_events

Append-only agent lifecycle and runtime event stream. This table is for audit
and timeline reconstruction, not prompt reconstruction.

```sql
CREATE TABLE IF NOT EXISTS agent_runtime_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       TEXT NOT NULL,
    ts_millis       INTEGER NOT NULL,
    session_id      TEXT,
    agent_id        TEXT NOT NULL,
    parent_agent_id TEXT,
    kind            TEXT NOT NULL,
    description     TEXT,
    model           TEXT,
    depth           INTEGER,
    background      INTEGER NOT NULL DEFAULT 0,
    payload_json    TEXT,
    schema_version  INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_session
    ON agent_runtime_events(session_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_agent
    ON agent_runtime_events(agent_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_events_kind
    ON agent_runtime_events(kind, timestamp DESC, id DESC);
```

### 2.4 agent_runtime_execution_records

Structured one-row-per-tool-execution records. Shell output is represented only
by SHA-256 digests; full stdout/stderr stays in existing transcript/task output
channels.

```sql
CREATE TABLE IF NOT EXISTS agent_runtime_execution_records (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp           TEXT NOT NULL,
    ts_millis           INTEGER NOT NULL,
    session_id          TEXT NOT NULL,
    agent_id            TEXT NOT NULL,
    parent_agent_id     TEXT,
    agent_role          TEXT,
    tool                TEXT NOT NULL,
    tool_use_id         TEXT,
    command             TEXT,
    cwd                 TEXT,
    exit_code           INTEGER,
    stdout_digest       TEXT,
    stderr_digest       TEXT,
    retry_count         INTEGER NOT NULL DEFAULT 0,
    model               TEXT,
    fallback_used       INTEGER NOT NULL DEFAULT 0,
    permission_decision TEXT,
    duration_ms         INTEGER,
    had_error           INTEGER NOT NULL DEFAULT 0,
    record_json         TEXT NOT NULL,
    schema_version      INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_session
    ON agent_runtime_execution_records(session_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_agent
    ON agent_runtime_execution_records(agent_id, id);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_tool
    ON agent_runtime_execution_records(tool, timestamp DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_permission
    ON agent_runtime_execution_records(permission_decision, timestamp DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_agent_runtime_records_model
    ON agent_runtime_execution_records(model, timestamp DESC, id DESC);
```
````

Use the exact SQL from the `Schema` section above.

- [ ] **Step 2: Update `README.md`**

Add this row to the document index:

```markdown
| [AGENT_RUNTIME_SQLITE_PLAN.md](AGENT_RUNTIME_SQLITE_PLAN.md) | Agent runtime 执行记录和生命周期事件进入 SQLite 的实施计划 | ✅ 草稿 |
```

- [ ] **Step 3: Commit docs**

```bash
git add -A -- development/sql/SCHEMA.md development/sql/README.md
git commit -m "Document agent runtime sqlite schema"
```

## Task 4: Runtime Regression Coverage

**Files:**

- Modify: `crates/allthecodes-engine/src/query/tests/record_tests.rs`
- Modify: `crates/allthecodes/src/runtime_history.rs`

**Interfaces:**

- Consumes: existing `failed_shell_command_emits_execution_record_event` test.
- Produces: confidence that shell digest, retry/fallback, model, and permission decision stay populated before SQLite receives the record.

- [ ] **Step 1: Keep engine event test unchanged unless it fails**

Run:

```bash
cargo test -p allthecodes-engine failed_shell_command_emits_execution_record_event
```

Expected: PASS. If it fails, fix the record construction path before changing SQLite persistence.

- [ ] **Step 2: Add a root persistence test for nullable non-shell fields**

In `runtime_history.rs`, add:

```rust
#[test]
#[serial_test::serial]
fn non_shell_record_persists_null_shell_columns() {
    let temp = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());

    let record = AgentRuntimeExecutionRecord {
        session_id: "session-2".to_string(),
        agent_id: "main".to_string(),
        tool: "read".to_string(),
        tool_use_id: Some("toolu-read".to_string()),
        permission_decision: Some(AgentRuntimePermissionDecision::NotRequired),
        model: Some("claude-sonnet-5".to_string()),
        ..Default::default()
    };

    persist_execution_record(&record).unwrap();

    let rows = query_execution_records_for_test().unwrap();
    assert_eq!(rows[0].tool, "read");
    assert!(rows[0].command.is_none());
    assert!(rows[0].exit_code.is_none());
    assert!(rows[0].stdout_digest.is_none());
    assert_eq!(rows[0].permission_decision.as_deref(), Some("not_required"));
}
```

Run:

```bash
cargo test -p allthecodes runtime_history::tests::non_shell_record_persists_null_shell_columns
```

Expected: PASS.

- [ ] **Step 3: Run focused persistence and record tests**

Run:

```bash
cargo test -p allthecodes runtime_history
cargo test -p allthecodes-engine record_tests
cargo test -p allthecodes-types agent_runtime_record
```

Expected: all PASS.

- [ ] **Step 4: Commit tests**

```bash
git add -A -- crates/allthecodes/src/runtime_history.rs crates/allthecodes-engine/src/query/tests/record_tests.rs
git commit -m "Cover runtime sqlite record persistence"
```

## Task 5: Final Verification

**Files:**

- No planned source edits.

**Interfaces:**

- Verifies the feature across root app, engine, shared types, and workspace build.

- [ ] **Step 1: Run focused tests**

```bash
export CARGO_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/cargo
export RUSTUP_HOME=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.rust/rustup
export CARGO_TARGET_DIR=/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/.tmp/allthecodes-target
export PATH="$CARGO_HOME/bin:$PATH"

cargo test -p allthecodes runtime_history
cargo test -p allthecodes-engine record_tests
cargo test -p allthecodes-types agent_runtime_record
```

Expected: all PASS.

- [ ] **Step 2: Run release build**

```bash
cargo build --workspace --release
```

Expected: build succeeds with no new warnings. If the build reports an unused import or dead code warning from this feature, remove the unused item or add a real test/use site; do not silence it unless the warning is intentionally unavoidable.

- [ ] **Step 3: Inspect changed files**

```bash
git status --short
git diff -- crates/allthecodes/Cargo.toml \
  crates/allthecodes/src/main.rs \
  crates/allthecodes/src/runtime_history.rs \
  crates/allthecodes/src/startup_traits.rs \
  development/sql/SCHEMA.md \
  development/sql/README.md
```

Expected: only the explicit feature and documentation paths are changed.

- [ ] **Step 4: Final commit**

```bash
git add -A -- crates/allthecodes/Cargo.toml \
  crates/allthecodes/src/main.rs \
  crates/allthecodes/src/runtime_history.rs \
  crates/allthecodes/src/startup_traits.rs
git add -A -- development/sql/SCHEMA.md development/sql/README.md
git commit -m "Persist agent runtime audit records in sqlite"
```

## Acceptance Criteria

- A shell tool execution produces one row in `agent_runtime_execution_records` with non-null `session_id`, `agent_id`, `tool = 'shell'`, `command`, `cwd`, `exit_code`, `stdout_digest`, `stderr_digest`, `retry_count`, `model`, `fallback_used`, `permission_decision`, `duration_ms`, `had_error`, and `record_json`.
- A non-shell tool execution produces one row in `agent_runtime_execution_records` with shell-only columns set to `NULL`, not omitted or fabricated.
- An agent spawn/complete lifecycle event produces rows in `agent_runtime_events`.
- SQLite write failure is logged at debug level and does not fail the dashboard NDJSON path or agent runtime.
- `session_messages` remains unchanged by this feature.
- Full stdout/stderr never appears in the new SQLite tables.
- Focused tests and `cargo build --workspace --release` pass.

## Explicit Non-Goals

- No query UI in this implementation.
- No migration of historical `runs/{session_id}/execution-records.ndjson` files into SQLite.
- No full raw tool output persistence.
- No change to model request retry/fallback behavior.
- No change to permission prompts or permission policy evaluation.
- No change to Rust TUI rendering; it may continue ignoring `ExecutionRecord`.

## Self-Review

- Spec coverage: The plan covers session linkage via `session_id`, agent lifecycle via `agent_runtime_events`, tool executions via `agent_runtime_execution_records`, shell digests, retry/fallback, model, and permission decision columns.
- Placeholder scan: No open-ended implementation placeholders remain; all schema and target interfaces are named.
- Type consistency: `AgentRuntimeExecutionRecord` field names match `crates/allthecodes-types/src/agent_runtime_record.rs`; adapter names match `RootDashboardEmitter` and `DashboardEmitter`.
