use super::*;

use allthecodes_db::{Migration, MigrationRunner};
use anyhow::{Context, Result};
use sqlx::{Row, SqlitePool};
use std::sync::{Arc, OnceLock};

const MIGRATIONS: &[Migration] = &[
    Migration::new(
        1,
        r#"
    CREATE TABLE IF NOT EXISTS tasks (
        task_list_id TEXT NOT NULL,
        id TEXT NOT NULL,
        kind TEXT NOT NULL,
        subject TEXT NOT NULL,
        description TEXT NOT NULL,
        status TEXT NOT NULL,
        output_file TEXT,
        output_summary TEXT NOT NULL DEFAULT '',
        output_bytes INTEGER NOT NULL DEFAULT 0,
        output_truncated INTEGER NOT NULL DEFAULT 0,
        parent_id TEXT,
        owner TEXT,
        active_form TEXT,
        metadata_json TEXT,
        tool_use_id TEXT,
        agent_id TEXT,
        supervisor_id TEXT,
        isolation TEXT,
        worktree_path TEXT,
        worktree_branch TEXT,
        remote_task_type TEXT,
        remote_session_id TEXT,
        remote_task_metadata_json TEXT,
        poll_started_at INTEGER,
        cancel_requested_at INTEGER,
        recovered_at INTEGER,
        previous_status TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (task_list_id, id)
    )
    "#,
    ),
    Migration::new(
        2,
        r#"
    CREATE TABLE IF NOT EXISTS task_dependencies (
        task_list_id TEXT NOT NULL,
        task_id TEXT NOT NULL,
        depends_on TEXT NOT NULL,
        position INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (task_list_id, task_id, depends_on),
        FOREIGN KEY (task_list_id, task_id)
            REFERENCES tasks(task_list_id, id)
            ON DELETE CASCADE
    )
    "#,
    ),
    Migration::new(
        3,
        r#"
    CREATE TABLE IF NOT EXISTS task_id_counters (
        task_list_id TEXT PRIMARY KEY NOT NULL,
        high_watermark INTEGER NOT NULL DEFAULT 0
    )
    "#,
    ),
    Migration::new(
        4,
        r#"
    CREATE INDEX IF NOT EXISTS idx_tasks_status
        ON tasks(task_list_id, status, updated_at DESC)
    "#,
    ),
    Migration::new(
        5,
        r#"
    CREATE INDEX IF NOT EXISTS idx_tasks_owner
        ON tasks(task_list_id, owner, status)
    "#,
    ),
    Migration::new(
        6,
        r#"
    CREATE INDEX IF NOT EXISTS idx_tasks_parent
        ON tasks(task_list_id, parent_id)
    "#,
    ),
    Migration::new(
        7,
        r#"
    CREATE INDEX IF NOT EXISTS idx_task_dependencies_task
        ON task_dependencies(task_list_id, task_id, position)
    "#,
    ),
    Migration::new(
        8,
        r#"
    ALTER TABLE tasks ADD COLUMN runtime_activity_json TEXT
    "#,
    ),
];

#[derive(Debug, Clone)]
pub(crate) struct SqliteTaskRepository {
    db_path: PathBuf,
    task_list_id: String,
    ready: Arc<OnceLock<std::result::Result<(), String>>>,
}

impl SqliteTaskRepository {
    pub(crate) fn new(dir: PathBuf, _output_limit_bytes: usize) -> Self {
        let db_path = db_path_for_task_dir(&dir);
        let task_list_id = task_list_id_for_dir(&dir);
        Self {
            db_path,
            task_list_id,
            ready: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn load_records(&self) -> Result<Vec<PersistedTaskRecord>> {
        let db_path = self.db_path.clone();
        let task_list_id = self.task_list_id.clone();
        self.ensure_ready()?;
        run_sqlite(async move {
            let pool = open_pool(db_path)?;
            let rows = sqlx::query(
                r#"
                SELECT
                    id,
                    kind,
                    subject,
                    description,
                    status,
                    output_file,
                    output_summary,
                    output_bytes,
                    output_truncated,
                    parent_id,
                    owner,
                    active_form,
                    metadata_json,
                    tool_use_id,
                    agent_id,
                    supervisor_id,
                    isolation,
                    worktree_path,
                    worktree_branch,
                    remote_task_type,
                    remote_session_id,
                    remote_task_metadata_json,
                    poll_started_at,
                    cancel_requested_at,
                    recovered_at,
                    previous_status,
                    runtime_activity_json,
                    created_at,
                    updated_at
                FROM tasks
                WHERE task_list_id = ?
                ORDER BY created_at ASC, id ASC
                "#,
            )
            .bind(&task_list_id)
            .fetch_all(&pool)
            .await
            .context("failed to query sqlite tasks")?;

            let dependency_rows = sqlx::query(
                r#"
                SELECT task_id, depends_on
                FROM task_dependencies
                WHERE task_list_id = ?
                ORDER BY task_id ASC, position ASC, depends_on ASC
                "#,
            )
            .bind(&task_list_id)
            .fetch_all(&pool)
            .await
            .context("failed to query sqlite task dependencies")?;

            let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
            for row in dependency_rows {
                let task_id: String = row.try_get("task_id")?;
                let depends_on: String = row.try_get("depends_on")?;
                dependencies.entry(task_id).or_default().push(depends_on);
            }

            let mut records = Vec::with_capacity(rows.len());
            for row in rows {
                let id: String = row.try_get("id")?;
                records.push(PersistedTaskRecord {
                    depends_on: dependencies.remove(&id).unwrap_or_default(),
                    id,
                    kind: row.try_get("kind")?,
                    subject: row.try_get("subject")?,
                    description: row.try_get("description")?,
                    status: row.try_get("status")?,
                    output_file: row.try_get("output_file")?,
                    output_summary: row.try_get("output_summary")?,
                    output_bytes: i64_to_usize(row.try_get("output_bytes")?),
                    output_truncated: row.try_get::<i64, _>("output_truncated")? != 0,
                    parent_id: row.try_get("parent_id")?,
                    owner: row.try_get("owner")?,
                    active_form: row.try_get("active_form")?,
                    metadata: parse_json_value(row.try_get("metadata_json")?)?,
                    tool_use_id: row.try_get("tool_use_id")?,
                    agent_id: row.try_get("agent_id")?,
                    supervisor_id: row.try_get("supervisor_id")?,
                    isolation: row.try_get("isolation")?,
                    worktree_path: row.try_get("worktree_path")?,
                    worktree_branch: row.try_get("worktree_branch")?,
                    remote_task_type: row.try_get("remote_task_type")?,
                    remote_session_id: row.try_get("remote_session_id")?,
                    remote_task_metadata: parse_json_value(
                        row.try_get("remote_task_metadata_json")?,
                    )?,
                    poll_started_at: row.try_get("poll_started_at")?,
                    cancel_requested_at: row.try_get("cancel_requested_at")?,
                    recovered_at: row.try_get("recovered_at")?,
                    previous_status: row.try_get("previous_status")?,
                    runtime_activity: parse_runtime_activity(
                        row.try_get("runtime_activity_json")?,
                    )?,
                    created_at: row.try_get("created_at")?,
                    updated_at: row.try_get("updated_at")?,
                    legacy_inline_output: None,
                });
            }
            Ok(records)
        })
    }

    pub(crate) fn reserve_next_task_id(&self) -> Result<String> {
        let db_path = self.db_path.clone();
        let task_list_id = self.task_list_id.clone();
        self.ensure_ready()?;
        run_sqlite(async move {
            let pool = open_pool(db_path)?;
            let mut tx = pool
                .begin()
                .await
                .context("failed to begin sqlite task id transaction")?;
            sqlx::query(
                r#"
                INSERT INTO task_id_counters (task_list_id, high_watermark)
                VALUES (
                    ?,
                    COALESCE((
                        SELECT MAX(CAST(id AS INTEGER))
                        FROM tasks
                        WHERE task_list_id = ?
                          AND id NOT GLOB '*[^0-9]*'
                          AND id != ''
                    ), 0)
                )
                ON CONFLICT(task_list_id) DO NOTHING
                "#,
            )
            .bind(&task_list_id)
            .bind(&task_list_id)
            .execute(&mut *tx)
            .await
            .context("failed to initialize sqlite task id counter")?;

            sqlx::query(
                r#"
                UPDATE task_id_counters
                SET high_watermark = high_watermark + 1
                WHERE task_list_id = ?
                "#,
            )
            .bind(&task_list_id)
            .execute(&mut *tx)
            .await
            .context("failed to reserve sqlite task id")?;

            let row = sqlx::query(
                r#"
                SELECT high_watermark
                FROM task_id_counters
                WHERE task_list_id = ?
                "#,
            )
            .bind(&task_list_id)
            .fetch_one(&mut *tx)
            .await
            .context("failed to read sqlite task id counter")?;
            tx.commit()
                .await
                .context("failed to commit sqlite task id transaction")?;
            let id: i64 = row.try_get("high_watermark")?;
            Ok(id.max(0).to_string())
        })
    }

    pub(crate) fn persist_record(&self, record: &PersistedTaskRecord) -> Result<()> {
        let db_path = self.db_path.clone();
        let task_list_id = self.task_list_id.clone();
        let record = record.clone();
        self.ensure_ready()?;
        run_sqlite(async move {
            let pool = open_pool(db_path)?;
            let mut tx = pool
                .begin()
                .await
                .context("failed to begin sqlite task persist transaction")?;
            sqlx::query(
                r#"
                INSERT INTO tasks (
                    task_list_id,
                    id,
                    kind,
                    subject,
                    description,
                    status,
                    output_file,
                    output_summary,
                    output_bytes,
                    output_truncated,
                    parent_id,
                    owner,
                    active_form,
                    metadata_json,
                    tool_use_id,
                    agent_id,
                    supervisor_id,
                    isolation,
                    worktree_path,
                    worktree_branch,
                    remote_task_type,
                    remote_session_id,
                    remote_task_metadata_json,
                    poll_started_at,
                    cancel_requested_at,
                    recovered_at,
                    previous_status,
                    runtime_activity_json,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(task_list_id, id) DO UPDATE SET
                    kind = excluded.kind,
                    subject = excluded.subject,
                    description = excluded.description,
                    status = excluded.status,
                    output_file = excluded.output_file,
                    output_summary = excluded.output_summary,
                    output_bytes = excluded.output_bytes,
                    output_truncated = excluded.output_truncated,
                    parent_id = excluded.parent_id,
                    owner = excluded.owner,
                    active_form = excluded.active_form,
                    metadata_json = excluded.metadata_json,
                    tool_use_id = excluded.tool_use_id,
                    agent_id = excluded.agent_id,
                    supervisor_id = excluded.supervisor_id,
                    isolation = excluded.isolation,
                    worktree_path = excluded.worktree_path,
                    worktree_branch = excluded.worktree_branch,
                    remote_task_type = excluded.remote_task_type,
                    remote_session_id = excluded.remote_session_id,
                    remote_task_metadata_json = excluded.remote_task_metadata_json,
                    poll_started_at = excluded.poll_started_at,
                    cancel_requested_at = excluded.cancel_requested_at,
                    recovered_at = excluded.recovered_at,
                    previous_status = excluded.previous_status,
                    runtime_activity_json = excluded.runtime_activity_json,
                    created_at = excluded.created_at,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(&task_list_id)
            .bind(&record.id)
            .bind(&record.kind)
            .bind(&record.subject)
            .bind(&record.description)
            .bind(&record.status)
            .bind(&record.output_file)
            .bind(&record.output_summary)
            .bind(usize_to_i64(record.output_bytes))
            .bind(if record.output_truncated { 1_i64 } else { 0_i64 })
            .bind(&record.parent_id)
            .bind(&record.owner)
            .bind(&record.active_form)
            .bind(json_to_string(&record.metadata)?)
            .bind(&record.tool_use_id)
            .bind(&record.agent_id)
            .bind(&record.supervisor_id)
            .bind(&record.isolation)
            .bind(&record.worktree_path)
            .bind(&record.worktree_branch)
            .bind(&record.remote_task_type)
            .bind(&record.remote_session_id)
            .bind(json_to_string(&record.remote_task_metadata)?)
            .bind(record.poll_started_at)
            .bind(record.cancel_requested_at)
            .bind(record.recovered_at)
            .bind(&record.previous_status)
            .bind(
                record
                    .runtime_activity
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
            )
            .bind(record.created_at)
            .bind(record.updated_at)
            .execute(&mut *tx)
            .await
            .context("failed to upsert sqlite task")?;

            sqlx::query(
                r#"
                DELETE FROM task_dependencies
                WHERE task_list_id = ? AND task_id = ?
                "#,
            )
            .bind(&task_list_id)
            .bind(&record.id)
            .execute(&mut *tx)
            .await
            .context("failed to clear sqlite task dependencies")?;

            for (position, depends_on) in record.depends_on.iter().enumerate() {
                sqlx::query(
                    r#"
                    INSERT INTO task_dependencies
                        (task_list_id, task_id, depends_on, position)
                    VALUES (?, ?, ?, ?)
                    "#,
                )
                .bind(&task_list_id)
                .bind(&record.id)
                .bind(depends_on)
                .bind(usize_to_i64(position))
                .execute(&mut *tx)
                .await
                .context("failed to insert sqlite task dependency")?;
            }

            tx.commit()
                .await
                .context("failed to commit sqlite task persist transaction")?;
            Ok(())
        })
    }

    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        let db_path = self.db_path.clone();
        let task_list_id = self.task_list_id.clone();
        let id = id.to_string();
        self.ensure_ready()?;
        run_sqlite(async move {
            let pool = open_pool(db_path)?;
            let mut tx = pool
                .begin()
                .await
                .context("failed to begin sqlite task delete transaction")?;
            sqlx::query(
                r#"
                DELETE FROM task_dependencies
                WHERE task_list_id = ? AND (task_id = ? OR depends_on = ?)
                "#,
            )
            .bind(&task_list_id)
            .bind(&id)
            .bind(&id)
            .execute(&mut *tx)
            .await
            .context("failed to delete sqlite task dependencies")?;
            sqlx::query(
                r#"
                DELETE FROM tasks
                WHERE task_list_id = ? AND id = ?
                "#,
            )
            .bind(&task_list_id)
            .bind(&id)
            .execute(&mut *tx)
            .await
            .context("failed to delete sqlite task")?;
            tx.commit()
                .await
                .context("failed to commit sqlite task delete transaction")?;
            Ok(())
        })
    }

    fn ensure_ready(&self) -> Result<()> {
        if let Some(result) = self.ready.get() {
            return result.clone().map_err(anyhow::Error::msg);
        }

        let db_path = self.db_path.clone();
        let result = run_sqlite(async move {
            let pool = open_pool(db_path)?;
            MigrationRunner::new("tasks", MIGRATIONS).run(&pool).await?;
            Ok(())
        })
        .map_err(|err| err.to_string());
        let _ = self.ready.set(result.clone());
        result.map_err(anyhow::Error::msg)
    }
}

fn open_pool(db_path: PathBuf) -> Result<SqlitePool> {
    allthecodes_db::pool_for_path(db_path)
}

fn db_path_for_task_dir(dir: &Path) -> PathBuf {
    if let Ok(path) = std::env::var("ALLTHECODES_TASK_SQLITE_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("tasks")
    {
        if let Some(root) = dir.parent().and_then(Path::parent) {
            return root.join("state").join("state_5.sqlite");
        }
    }

    dir.join("state_5.sqlite")
}

fn task_list_id_for_dir(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_task_list_id)
        .unwrap_or_else(|| DEFAULT_TASK_LIST_ID.to_string())
}

fn run_sqlite<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    allthecodes_db::run_sqlite_sync("allthecodes-task-sqlite", future)
}

fn parse_json_value(raw: Option<String>) -> Result<Option<Value>> {
    raw.map(|value| serde_json::from_str(&value).context("failed to parse sqlite JSON field"))
        .transpose()
}

fn parse_runtime_activity(raw: Option<String>) -> Result<Option<AgentRuntimeActivity>> {
    raw.map(|value| serde_json::from_str(&value).context("invalid runtime activity JSON"))
        .transpose()
}

fn json_to_string(value: &Option<Value>) -> Result<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("failed to serialize sqlite JSON field")
}

fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
