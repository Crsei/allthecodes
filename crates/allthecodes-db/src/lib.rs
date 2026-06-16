//! Shared SQLite infrastructure for allthecodes persistence domains.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

const STATE_DB_FILE: &str = "state_5.sqlite";
const APP_DB_FILE: &str = "app_1.sqlite";
const LOGS_DB_FILE: &str = "logs_2.sqlite";

/// Logical SQLite databases owned by the shared allthecodes data root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseKind {
    State,
    App,
    Logs,
}

/// Resolves and opens the standard SQLite databases under `{data_root}/state`.
#[derive(Debug, Clone)]
pub struct DbPoolManager {
    data_root: PathBuf,
}

impl Default for DbPoolManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DbPoolManager {
    /// Build a manager rooted at [`allthecodes_config::paths::data_root`].
    pub fn new() -> Self {
        Self {
            data_root: allthecodes_config::paths::data_root(),
        }
    }

    /// Build a manager rooted at an explicit data root. Intended for tests and
    /// callers that have already resolved the root through config.
    pub fn from_data_root(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
        }
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn state_dir(&self) -> PathBuf {
        self.data_root.join("state")
    }

    pub fn db_path(&self, kind: DatabaseKind) -> PathBuf {
        let file = match kind {
            DatabaseKind::State => STATE_DB_FILE,
            DatabaseKind::App => APP_DB_FILE,
            DatabaseKind::Logs => LOGS_DB_FILE,
        };
        self.state_dir().join(file)
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.db_path(DatabaseKind::State)
    }

    pub fn app_db_path(&self) -> PathBuf {
        self.db_path(DatabaseKind::App)
    }

    pub fn logs_db_path(&self) -> PathBuf {
        self.db_path(DatabaseKind::Logs)
    }

    pub fn pool(&self, kind: DatabaseKind) -> Result<SqlitePool> {
        pool_for_path(self.db_path(kind))
    }

    pub fn state_pool(&self) -> Result<SqlitePool> {
        self.pool(DatabaseKind::State)
    }

    pub fn app_pool(&self) -> Result<SqlitePool> {
        self.pool(DatabaseKind::App)
    }

    pub fn logs_pool(&self) -> Result<SqlitePool> {
        self.pool(DatabaseKind::Logs)
    }
}

/// Baseline SQLite options used by every allthecodes-owned database.
pub fn base_sqlite_options(db_path: impl AsRef<Path>) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
}

/// Open a lazy pool for an arbitrary SQLite path after creating its parent dir.
pub fn pool_for_path(db_path: impl Into<PathBuf>) -> Result<SqlitePool> {
    let db_path = db_path.into();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create sqlite state directory {}",
                parent.display()
            )
        })?;
    }
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(base_sqlite_options(db_path)))
}

/// One raw SQL migration with a monotonically increasing version.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

impl Migration {
    pub const fn new(version: i64, sql: &'static str) -> Self {
        Self { version, sql }
    }
}

/// Runs idempotent raw SQL migrations and records per-domain versions.
#[derive(Debug, Clone, Copy)]
pub struct MigrationRunner {
    namespace: &'static str,
    migrations: &'static [Migration],
}

impl MigrationRunner {
    pub const fn new(namespace: &'static str, migrations: &'static [Migration]) -> Self {
        Self {
            namespace,
            migrations,
        }
    }

    pub async fn run(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::raw_sql(
            r#"
            CREATE TABLE IF NOT EXISTS _schema_version (
                namespace TEXT PRIMARY KEY NOT NULL,
                version INTEGER NOT NULL,
                applied_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .context("failed to initialize sqlite schema version table")?;

        let current = sqlx::query(
            r#"
            SELECT version
            FROM _schema_version
            WHERE namespace = ?
            "#,
        )
        .bind(self.namespace)
        .fetch_optional(pool)
        .await
        .context("failed to read sqlite schema version")?
        .map(|row| row.try_get::<i64, _>("version"))
        .transpose()
        .context("failed to decode sqlite schema version")?
        .unwrap_or(0);

        for migration in self.migrations {
            if migration.version <= current {
                continue;
            }

            sqlx::raw_sql(migration.sql)
                .execute(pool)
                .await
                .with_context(|| {
                    format!(
                        "failed to run sqlite migration {}:{}",
                        self.namespace, migration.version
                    )
                })?;

            sqlx::query(
                r#"
                INSERT INTO _schema_version (namespace, version, applied_at)
                VALUES (?, ?, ?)
                ON CONFLICT(namespace) DO UPDATE SET
                    version = excluded.version,
                    applied_at = excluded.applied_at
                "#,
            )
            .bind(self.namespace)
            .bind(migration.version)
            .bind(chrono::Utc::now().timestamp())
            .execute(pool)
            .await
            .context("failed to update sqlite schema version")?;
        }

        Ok(())
    }
}

/// Run SQLite async work from synchronous storage APIs.
pub fn run_sqlite_sync<F, T>(thread_name: &'static str, future: F) -> Result<T>
where
    F: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let join = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to create sqlite runtime")?;
            runtime.block_on(future)
        })
        .context("failed to spawn sqlite worker thread")?;
    join.join()
        .map_err(|_| anyhow!("sqlite worker thread panicked"))?
}

/// Return the result of SQLite `PRAGMA integrity_check`.
pub async fn integrity_check(pool: &SqlitePool) -> Result<String> {
    let row = sqlx::query("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .context("failed to run sqlite integrity_check")?;
    row.try_get::<String, _>(0)
        .context("failed to decode sqlite integrity_check result")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MIGRATIONS: &[Migration] = &[
        Migration::new(
            1,
            r#"
            CREATE TABLE IF NOT EXISTS db_test_items (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL
            )
            "#,
        ),
        Migration::new(
            2,
            r#"
            CREATE INDEX IF NOT EXISTS idx_db_test_items_name
                ON db_test_items(name)
            "#,
        ),
    ];

    #[tokio::test]
    async fn pool_paths_resolve_under_state_dir() {
        let temp = tempfile::tempdir().unwrap();
        let manager = DbPoolManager::from_data_root(temp.path());

        assert_eq!(
            manager.state_db_path(),
            temp.path().join("state").join("state_5.sqlite")
        );
        assert_eq!(
            manager.app_db_path(),
            temp.path().join("state").join("app_1.sqlite")
        );
        assert_eq!(
            manager.logs_db_path(),
            temp.path().join("state").join("logs_2.sqlite")
        );

        let pool = manager.state_pool().unwrap();
        sqlx::query("SELECT 1").execute(&pool).await.unwrap();
        assert!(manager.state_db_path().exists());
    }

    #[tokio::test]
    async fn sqlite_pragmas_and_integrity_check_are_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let pool = pool_for_path(temp.path().join("state").join("state_5.sqlite")).unwrap();

        let foreign_keys: i64 = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let journal_mode: String = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let synchronous: i64 = sqlx::query("PRAGMA synchronous")
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();

        assert_eq!(foreign_keys, 1);
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        assert_eq!(synchronous, 1);
        assert_eq!(integrity_check(&pool).await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn migrations_are_idempotent_and_record_version() {
        let temp = tempfile::tempdir().unwrap();
        let pool = pool_for_path(temp.path().join("state.sqlite")).unwrap();
        let runner = MigrationRunner::new("db-test", TEST_MIGRATIONS);

        runner.run(&pool).await.unwrap();
        runner.run(&pool).await.unwrap();

        sqlx::query("INSERT INTO db_test_items (name) VALUES ('first')")
            .execute(&pool)
            .await
            .unwrap();
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM db_test_items")
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_get(0)
            .unwrap();
        let version: i64 =
            sqlx::query("SELECT version FROM _schema_version WHERE namespace = 'db-test'")
                .fetch_one(&pool)
                .await
                .unwrap()
                .try_get(0)
                .unwrap();

        assert_eq!(count, 1);
        assert_eq!(version, 2);
    }
}
