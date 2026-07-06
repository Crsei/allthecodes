use std::fs;
use std::path::Path;

use allthecodes_db::{Migration, MigrationRunner};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{de::DeserializeOwned, Serialize};
use sqlx::{Row, SqlitePool};
use tracing::warn;

use super::paths::{
    control_token_path, proactive_state_path, shutdown_request_path, sleep_state_path, state_path,
    workers_dir,
};
use super::storage::read_worker_state_file;
use super::types::{
    DaemonControlToken, DaemonProactiveState, DaemonProcessState, DaemonShutdownRequest,
    DaemonSleepState, DaemonWorkerState,
};

const MIGRATIONS: &[Migration] = &[
    Migration::new(
        1,
        r#"
            CREATE TABLE IF NOT EXISTS daemon_state (
                key TEXT PRIMARY KEY NOT NULL,
                value_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )
            "#,
    ),
    Migration::new(
        2,
        r#"
            CREATE TABLE IF NOT EXISTS daemon_workers (
                worker_id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL,
                pid INTEGER,
                status TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                value_json TEXT NOT NULL
            )
            "#,
    ),
    Migration::new(
        3,
        r#"
            CREATE INDEX IF NOT EXISTS idx_daemon_workers_status
                ON daemon_workers(status, updated_at)
            "#,
    ),
];

pub(super) fn write_state_value<T>(key: &'static str, value: &T) -> Result<()>
where
    T: Serialize,
{
    let value_json = serde_json::to_string(value)?;
    run(move |pool| async move {
        sqlx::query(
            r#"
                INSERT INTO daemon_state (key, value_json, updated_at)
                VALUES (?, ?, ?)
                ON CONFLICT(key) DO UPDATE SET
                    value_json = excluded.value_json,
                    updated_at = excluded.updated_at
                "#,
        )
        .bind(key)
        .bind(value_json)
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .context("failed to upsert daemon state")?;
        Ok(())
    })
}

pub(super) fn read_state_value<T>(key: &'static str) -> Result<Option<T>>
where
    T: DeserializeOwned + Send + 'static,
{
    run(move |pool| async move {
        import_legacy_json(&pool).await?;
        let row = sqlx::query("SELECT value_json FROM daemon_state WHERE key = ?")
            .bind(key)
            .fetch_optional(&pool)
            .await
            .context("failed to read daemon state")?;
        row.map(|row| {
            let value_json: String = row.try_get("value_json")?;
            serde_json::from_str(&value_json).context("failed to decode daemon state JSON")
        })
        .transpose()
    })
}

pub(super) fn state_value_exists(key: &'static str) -> Result<bool> {
    run(move |pool| async move {
        import_legacy_json(&pool).await?;
        Ok(sqlx::query("SELECT 1 FROM daemon_state WHERE key = ?")
            .bind(key)
            .fetch_optional(&pool)
            .await
            .context("failed to check daemon state")?
            .is_some())
    })
}

pub(super) fn remove_state_value(key: &'static str) -> Result<bool> {
    run(move |pool| async move {
        let result = sqlx::query("DELETE FROM daemon_state WHERE key = ?")
            .bind(key)
            .execute(&pool)
            .await
            .context("failed to delete daemon state")?;
        Ok(result.rows_affected() > 0)
    })
}

pub(super) fn write_worker(state: &DaemonWorkerState) -> Result<()> {
    let state = state.clone();
    run(move |pool| async move { upsert_worker(&pool, &state).await })
}

pub(super) fn read_worker(worker_id: &str) -> Result<Option<DaemonWorkerState>> {
    let worker_id = worker_id.to_string();
    run(move |pool| async move {
        import_legacy_json(&pool).await?;
        read_worker_from_pool(&pool, &worker_id).await
    })
}

pub(super) fn read_workers() -> Result<Vec<DaemonWorkerState>> {
    run(move |pool| async move {
        import_legacy_json(&pool).await?;
        let rows = sqlx::query(
            r#"
                SELECT value_json
                FROM daemon_workers
                ORDER BY worker_id ASC
                "#,
        )
        .fetch_all(&pool)
        .await
        .context("failed to read daemon worker rows")?;
        rows.into_iter()
            .map(|row| {
                let value_json: String = row.try_get("value_json")?;
                serde_json::from_str(&value_json).context("failed to decode daemon worker JSON")
            })
            .collect()
    })
}

pub(super) fn remove_worker(worker_id: &str) -> Result<bool> {
    let worker_id = worker_id.to_string();
    run(move |pool| async move {
        let result = sqlx::query("DELETE FROM daemon_workers WHERE worker_id = ?")
            .bind(&worker_id)
            .execute(&pool)
            .await
            .context("failed to delete daemon worker")?;
        Ok(result.rows_affected() > 0)
    })
}

fn run<F, Fut, T>(op: F) -> Result<T>
where
    F: FnOnce(SqlitePool) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    allthecodes_db::run_sqlite_sync("allthecodes-daemon-sqlite", async move {
        let pool = migrated_pool().await?;
        op(pool).await
    })
}

async fn migrated_pool() -> Result<SqlitePool> {
    let pool = allthecodes_db::DbPoolManager::new().state_pool()?;
    MigrationRunner::new("daemon", MIGRATIONS)
        .run(&pool)
        .await?;
    Ok(pool)
}

async fn import_legacy_json(pool: &SqlitePool) -> Result<()> {
    import_state_file::<DaemonProcessState>(pool, "supervisor", &state_path()).await?;
    import_state_file::<DaemonShutdownRequest>(pool, "shutdown-request", &shutdown_request_path())
        .await?;
    import_state_file::<DaemonControlToken>(pool, "control-token", &control_token_path()).await?;
    import_state_file::<DaemonSleepState>(pool, "sleep-state", &sleep_state_path()).await?;
    import_state_file::<DaemonProactiveState>(pool, "proactive-state", &proactive_state_path())
        .await?;
    import_worker_files(pool).await?;
    Ok(())
}

async fn import_state_file<T>(pool: &SqlitePool, key: &'static str, path: &Path) -> Result<()>
where
    T: DeserializeOwned + Serialize,
{
    if !path.exists()
        || sqlx::query("SELECT 1 FROM daemon_state WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .context("failed to check daemon state before import")?
            .is_some()
    {
        return Ok(());
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon JSON {}", path.display()))?;
    let value: T = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon JSON {}", path.display()))?;
    let value_json = serde_json::to_string(&value)?;
    sqlx::query(
        r#"
            INSERT OR IGNORE INTO daemon_state (key, value_json, updated_at)
            VALUES (?, ?, ?)
            "#,
    )
    .bind(key)
    .bind(value_json)
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await
    .context("failed to import daemon state JSON")?;
    Ok(())
}

async fn import_worker_files(pool: &SqlitePool) -> Result<()> {
    let dir = workers_dir();
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let state = match read_worker_state_file(&path) {
            Ok(state) => state,
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "skipping invalid daemon worker JSON during sqlite import"
                );
                continue;
            }
        };
        if read_worker_from_pool(pool, &state.worker_id)
            .await?
            .is_none()
        {
            upsert_worker(pool, &state).await?;
        }
    }

    Ok(())
}

async fn read_worker_from_pool(
    pool: &SqlitePool,
    worker_id: &str,
) -> Result<Option<DaemonWorkerState>> {
    sqlx::query("SELECT value_json FROM daemon_workers WHERE worker_id = ?")
        .bind(worker_id)
        .fetch_optional(pool)
        .await
        .context("failed to read daemon worker")?
        .map(|row| {
            let value_json: String = row.try_get("value_json")?;
            serde_json::from_str(&value_json).context("failed to decode daemon worker JSON")
        })
        .transpose()
}

async fn upsert_worker(pool: &SqlitePool, state: &DaemonWorkerState) -> Result<()> {
    let value_json = serde_json::to_string(state)?;
    sqlx::query(
        r#"
            INSERT INTO daemon_workers (
                worker_id,
                kind,
                pid,
                status,
                updated_at,
                value_json
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(worker_id) DO UPDATE SET
                kind = excluded.kind,
                pid = excluded.pid,
                status = excluded.status,
                updated_at = excluded.updated_at,
                value_json = excluded.value_json
            "#,
    )
    .bind(&state.worker_id)
    .bind(&state.kind)
    .bind(state.pid.map(i64::from))
    .bind(state.status.as_str())
    .bind(state.updated_at.timestamp())
    .bind(value_json)
    .execute(pool)
    .await
    .context("failed to upsert daemon worker")?;
    Ok(())
}
