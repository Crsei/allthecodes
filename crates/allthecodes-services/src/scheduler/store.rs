//! Persistence for scheduled tasks — JSON file guarded by a sibling
//! lockfile so concurrent sessions serialize their writes.
//!
//! The store does *not* poll or fire tasks. It offers CRUD and `due_tasks`
//! snapshots; the daemon or any other scheduling host can wire a timer on
//! top. Keeping the store passive makes it trivially testable and decouples
//! `/loop` (which just wants to insert a task) from any tick loop.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(feature = "sqlite-storage")]
use tracing::warn;

#[cfg(feature = "sqlite-storage")]
use super::task::ScheduleKind;
use super::task::{ScheduledTask, SchedulerKind, TaskId};

/// Schema version for the on-disk JSON so we can evolve the format later
/// without silently deserializing a mismatched layout.
const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("I/O error touching {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to encode scheduler state: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("task '{0}' not found")]
    NotFound(String),
    #[error("task '{0}' already exists")]
    AlreadyExists(String),
    #[error("task revision conflict: expected {expected}, current {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("could not acquire scheduler lock at {path} within {}ms", timeout_ms.as_millis())]
    LockTimeout { path: PathBuf, timeout_ms: Duration },
    #[error("remote-trigger tasks are not supported yet — see issue #60")]
    RemoteTriggerUnsupported,
    #[cfg(feature = "sqlite-storage")]
    #[error("SQLite scheduler storage failed: {0:#}")]
    Sqlite(#[source] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    tasks: Vec<ScheduledTask>,
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            tasks: Vec::new(),
        }
    }
}

/// File-backed scheduler store.
///
/// Multiple `SchedulerStore` instances can point at the same JSON file —
/// they'll serialize through the on-disk lockfile even across processes.
/// Within one process the in-process `parking_lot::Mutex` keeps concurrent
/// calls from the same host cheap.
pub struct SchedulerStore {
    state_path: PathBuf,
    lock_path: PathBuf,
    #[cfg(feature = "sqlite-storage")]
    sqlite_data_root: PathBuf,
    inner: Mutex<()>,
}

impl SchedulerStore {
    /// Default on-disk location under the allthecodes data root.
    pub fn default_path() -> PathBuf {
        allthecodes_config::paths::data_root().join("scheduled_tasks.json")
    }

    /// Open (or prepare to open) a store at `state_path`. Does not create
    /// the file — the first `save` call does that.
    pub fn new(state_path: impl Into<PathBuf>) -> Self {
        let state_path = state_path.into();
        let lock_path = state_path.with_extension("json.lock");
        #[cfg(feature = "sqlite-storage")]
        let sqlite_data_root = state_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(allthecodes_config::paths::data_root);
        Self {
            state_path,
            lock_path,
            #[cfg(feature = "sqlite-storage")]
            sqlite_data_root,
            inner: Mutex::new(()),
        }
    }

    pub fn open_default() -> Self {
        Self::new(Self::default_path())
    }

    pub fn path(&self) -> &Path {
        &self.state_path
    }

    /// Load all tasks. Missing file → empty list.
    pub fn load(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_load() {
            Ok(tasks) => return Ok(tasks),
            Err(err) => self.warn_sqlite_fallback("load scheduler tasks", &err),
        }

        let _file_guard = self.acquire_lock()?;
        self.read_state().map(|s| s.tasks)
    }

    /// Add a new task, persisting immediately.
    pub fn add(&self, mut task: ScheduledTask) -> Result<ScheduledTask, SchedulerError> {
        if matches!(task.kind, SchedulerKind::RemoteTrigger) {
            return Err(SchedulerError::RemoteTriggerUnsupported);
        }
        task.revision = task.revision.max(1);
        task.updated_at.get_or_insert(task.created_at);
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_add(task.clone()) {
            Ok(task) => {
                self.write_json_backup_from_sqlite();
                return Ok(task);
            }
            Err(err @ SchedulerError::AlreadyExists(_)) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("add scheduler task", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        if state.tasks.iter().any(|existing| existing.id == task.id) {
            return Err(SchedulerError::AlreadyExists(task.id.to_string()));
        }
        state.tasks.push(task.clone());
        self.write_state(&state)?;
        Ok(task)
    }

    /// Remove a task by id. Returns the removed task or `NotFound`.
    pub fn remove(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        self.remove_if_revision(id, None)
    }

    /// Remove a task after an optional optimistic revision check.
    pub fn remove_if_revision(
        &self,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_remove(id, expected_revision) {
            Ok(task) => {
                self.write_json_backup_from_sqlite();
                return Ok(task);
            }
            Err(err @ SchedulerError::NotFound(_))
            | Err(err @ SchedulerError::RevisionConflict { .. }) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("remove scheduler task", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let pos = state
            .tasks
            .iter()
            .position(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        check_revision(&state.tasks[pos], expected_revision)?;
        let removed = state.tasks.remove(pos);
        self.write_state(&state)?;
        Ok(removed)
    }

    /// Atomically replace a task definition and increment its revision.
    pub fn replace(
        &self,
        id: &TaskId,
        expected_revision: u64,
        mut replacement: ScheduledTask,
    ) -> Result<ScheduledTask, SchedulerError> {
        if matches!(replacement.kind, SchedulerKind::RemoteTrigger) {
            return Err(SchedulerError::RemoteTriggerUnsupported);
        }
        replacement.id = id.clone();
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_replace(id, expected_revision, replacement.clone()) {
            Ok(task) => {
                self.write_json_backup_from_sqlite();
                return Ok(task);
            }
            Err(err @ SchedulerError::NotFound(_))
            | Err(err @ SchedulerError::RevisionConflict { .. }) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("replace scheduler task", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let task = state
            .tasks
            .iter_mut()
            .find(|task| task.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        check_revision(task, Some(expected_revision))?;
        replacement.created_at = task.created_at;
        replacement.last_run_at = task.last_run_at;
        replacement.revision = task.revision.saturating_add(1);
        replacement.updated_at = Some(Utc::now());
        *task = replacement.clone();
        self.write_state(&state)?;
        Ok(replacement)
    }

    /// Fetch a single task snapshot.
    pub fn get(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_get(id) {
            Ok(task) => return Ok(task),
            Err(err @ SchedulerError::NotFound(_))
            | Err(err @ SchedulerError::RevisionConflict { .. }) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("get scheduler task", &err),
        }

        let _file_guard = self.acquire_lock()?;
        self.read_state()?
            .tasks
            .into_iter()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    /// Pause or resume a task by id.
    pub fn set_paused(&self, id: &TaskId, paused: bool) -> Result<ScheduledTask, SchedulerError> {
        self.set_paused_if_revision(id, paused, None)
    }

    pub fn set_paused_if_revision(
        &self,
        id: &TaskId,
        paused: bool,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_set_paused(id, paused, expected_revision) {
            Ok(task) => {
                self.write_json_backup_from_sqlite();
                return Ok(task);
            }
            Err(err @ SchedulerError::NotFound(_))
            | Err(err @ SchedulerError::RevisionConflict { .. }) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("set scheduler pause state", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        check_revision(task, expected_revision)?;
        task.paused = paused;
        task.revision = task.revision.saturating_add(1);
        task.updated_at = Some(Utc::now());
        let snapshot = task.clone();
        self.write_state(&state)?;
        Ok(snapshot)
    }

    /// Mark a task as fired (advance its `next_run_at`). This is what the
    /// daemon should call after it successfully dispatches a task.
    pub fn record_fired(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        self.record_fired_if_revision(id, None)
    }

    pub fn record_fired_if_revision(
        &self,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_record_fired(id, expected_revision) {
            Ok(task) => {
                self.write_json_backup_from_sqlite();
                return Ok(task);
            }
            Err(err @ SchedulerError::NotFound(_))
            | Err(err @ SchedulerError::RevisionConflict { .. }) => return Err(err),
            Err(err) => self.warn_sqlite_fallback("record scheduler task fired", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        check_revision(task, expected_revision)?;
        task.mark_fired(Utc::now());
        let snapshot = task.clone();
        self.write_state(&state)?;
        Ok(snapshot)
    }

    /// Collect the tasks that are due right now. A passive snapshot — the
    /// caller is responsible for firing them and calling `record_fired`.
    pub fn due_tasks(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
        let _guard = self.inner.lock();
        #[cfg(feature = "sqlite-storage")]
        match self.sqlite_due_tasks() {
            Ok(tasks) => return Ok(tasks),
            Err(err) => self.warn_sqlite_fallback("list due scheduler tasks", &err),
        }

        let _file_guard = self.acquire_lock()?;
        let now = Utc::now();
        Ok(self
            .read_state()?
            .tasks
            .into_iter()
            .filter(|t| t.is_due(now))
            .collect())
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    fn read_state(&self) -> Result<StateFile, SchedulerError> {
        if !self.state_path.exists() {
            return Ok(StateFile::default());
        }
        let mut file = File::open(&self.state_path).map_err(|e| SchedulerError::Io {
            path: self.state_path.clone(),
            source: e,
        })?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(|e| SchedulerError::Io {
                path: self.state_path.clone(),
                source: e,
            })?;
        if buf.trim().is_empty() {
            return Ok(StateFile::default());
        }
        let mut parsed: StateFile =
            serde_json::from_str(&buf).map_err(|e| SchedulerError::Decode {
                path: self.state_path.clone(),
                source: e,
            })?;
        parsed.version = SCHEMA_VERSION;
        for task in &mut parsed.tasks {
            task.revision = task.revision.max(1);
        }
        Ok(parsed)
    }

    fn write_state(&self, state: &StateFile) -> Result<(), SchedulerError> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent).map_err(|e| SchedulerError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let bytes = serde_json::to_vec_pretty(state).map_err(SchedulerError::Encode)?;
        let tmp_path = self.state_path.with_extension("json.tmp");
        // Atomic-write: write to tmp then rename. Avoids corrupting the
        // scheduled_tasks.json if the process is killed mid-write.
        {
            let mut tmp = File::create(&tmp_path).map_err(|e| SchedulerError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
            tmp.write_all(&bytes).map_err(|e| SchedulerError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
            tmp.flush().map_err(|e| SchedulerError::Io {
                path: tmp_path.clone(),
                source: e,
            })?;
        }
        fs::rename(&tmp_path, &self.state_path).map_err(|e| SchedulerError::Io {
            path: self.state_path.clone(),
            source: e,
        })?;
        Ok(())
    }

    fn acquire_lock(&self) -> Result<FileLockGuard<'_>, SchedulerError> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent).map_err(|e| SchedulerError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let start = Instant::now();
        let timeout = Duration::from_millis(2_000);
        loop {
            let result = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.lock_path);
            match result {
                Ok(file) => {
                    return Ok(FileLockGuard {
                        file: Some(file),
                        path: &self.lock_path,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if start.elapsed() >= timeout {
                        return Err(SchedulerError::LockTimeout {
                            path: self.lock_path.clone(),
                            timeout_ms: timeout,
                        });
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => {
                    return Err(SchedulerError::Io {
                        path: self.lock_path.clone(),
                        source: e,
                    });
                }
            }
        }
    }

    #[cfg(feature = "sqlite-storage")]
    fn warn_sqlite_fallback(&self, operation: &'static str, error: &SchedulerError) {
        warn!(
            operation,
            path = %self.state_path.display(),
            error = %error,
            "falling back to JSON scheduler storage"
        );
    }

    #[cfg(feature = "sqlite-storage")]
    fn write_json_backup_from_sqlite(&self) {
        match sqlite_store::load_current(&self.sqlite_data_root).map_err(sqlite_error) {
            Ok(tasks) => {
                if let Err(err) = self.write_state(&StateFile {
                    version: SCHEMA_VERSION,
                    tasks,
                }) {
                    warn!(
                        path = %self.state_path.display(),
                        error = %err,
                        "failed to write scheduler JSON backup after SQLite update"
                    );
                }
            }
            Err(err) => {
                warn!(
                    path = %self.state_path.display(),
                    error = %err,
                    "failed to read scheduler SQLite state for JSON backup"
                );
            }
        }
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_load(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
        sqlite_store::load(&self.sqlite_data_root, &self.state_path).map_err(sqlite_error)
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_add(&self, task: ScheduledTask) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::add(&self.sqlite_data_root, &self.state_path, task).map_err(sqlite_error)
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_remove(
        &self,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::remove(
            &self.sqlite_data_root,
            &self.state_path,
            id,
            expected_revision,
        )
        .map_err(sqlite_error)?
        .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_replace(
        &self,
        id: &TaskId,
        expected_revision: u64,
        replacement: ScheduledTask,
    ) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::replace(
            &self.sqlite_data_root,
            &self.state_path,
            id,
            expected_revision,
            replacement,
        )
        .map_err(sqlite_error)
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_get(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::get(&self.sqlite_data_root, &self.state_path, id)
            .map_err(sqlite_error)?
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_set_paused(
        &self,
        id: &TaskId,
        paused: bool,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::set_paused(
            &self.sqlite_data_root,
            &self.state_path,
            id,
            paused,
            expected_revision,
        )
        .map_err(sqlite_error)?
        .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_record_fired(
        &self,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<ScheduledTask, SchedulerError> {
        sqlite_store::record_fired(
            &self.sqlite_data_root,
            &self.state_path,
            id,
            expected_revision,
        )
        .map_err(sqlite_error)?
        .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    #[cfg(feature = "sqlite-storage")]
    fn sqlite_due_tasks(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
        sqlite_store::due_tasks(&self.sqlite_data_root, &self.state_path).map_err(sqlite_error)
    }
}

#[cfg(feature = "sqlite-storage")]
fn sqlite_error(error: anyhow::Error) -> SchedulerError {
    match error.downcast::<SchedulerError>() {
        Ok(error) => error,
        Err(error) => SchedulerError::Sqlite(error),
    }
}

fn check_revision(
    task: &ScheduledTask,
    expected_revision: Option<u64>,
) -> Result<(), SchedulerError> {
    if let Some(expected) = expected_revision {
        if task.revision != expected {
            return Err(SchedulerError::RevisionConflict {
                expected,
                actual: task.revision,
            });
        }
    }
    Ok(())
}

#[cfg(feature = "sqlite-storage")]
mod sqlite_store {
    use super::*;
    use allthecodes_db::{Migration, MigrationRunner};
    use anyhow::{Context, Result};
    use chrono::{DateTime, TimeZone};
    use sqlx::{Row, SqlitePool};

    const MIGRATIONS: &[Migration] = &[
        Migration::new(
            1,
            r#"
            CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id TEXT PRIMARY KEY NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                schedule TEXT NOT NULL,
                schedule_kind TEXT NOT NULL,
                timezone TEXT,
                interval_seconds INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_run_at INTEGER,
                next_run_at INTEGER NOT NULL,
                paused INTEGER NOT NULL DEFAULT 0
            )
            "#,
        ),
        Migration::new(
            2,
            r#"
            CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_due
                ON scheduled_tasks(paused, next_run_at)
            "#,
        ),
        Migration::new(
            3,
            r#"
            ALTER TABLE scheduled_tasks
                ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
            ALTER TABLE scheduled_tasks
                ADD COLUMN updated_at INTEGER;
            ALTER TABLE scheduled_tasks
                ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}';
            "#,
        ),
    ];

    pub(super) fn load(data_root: &Path, json_path: &Path) -> Result<Vec<ScheduledTask>> {
        run(data_root, json_path, move |pool| async move {
            load_from_pool(&pool).await
        })
    }

    pub(super) fn load_current(data_root: &Path) -> Result<Vec<ScheduledTask>> {
        let data_root = data_root.to_path_buf();
        allthecodes_db::run_sqlite_sync("allthecodes-scheduler-sqlite", async move {
            let pool = migrated_pool(&data_root).await?;
            load_from_pool(&pool).await
        })
    }

    pub(super) fn add(
        data_root: &Path,
        json_path: &Path,
        task: ScheduledTask,
    ) -> Result<ScheduledTask> {
        run(data_root, json_path, move |pool| async move {
            insert_task_new(&pool, &task).await?;
            Ok(task)
        })
    }

    pub(super) fn remove(
        data_root: &Path,
        json_path: &Path,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<Option<ScheduledTask>> {
        let id = id.clone();
        run(data_root, json_path, move |pool| async move {
            let Some(task) = get_from_pool(&pool, &id).await? else {
                return Ok(None);
            };
            check_revision(&task, expected_revision).map_err(anyhow::Error::new)?;
            let result = sqlx::query("DELETE FROM scheduled_tasks WHERE id = ? AND revision = ?")
                .bind(id.as_str())
                .bind(u64_to_i64(task.revision))
                .execute(&pool)
                .await
                .context("failed to delete sqlite scheduled task")?;
            if result.rows_affected() == 0 {
                return Err(current_revision_conflict(&pool, &id, task.revision).await);
            }
            Ok(Some(task))
        })
    }

    pub(super) fn replace(
        data_root: &Path,
        json_path: &Path,
        id: &TaskId,
        expected_revision: u64,
        mut replacement: ScheduledTask,
    ) -> Result<ScheduledTask> {
        let id = id.clone();
        run(data_root, json_path, move |pool| async move {
            let Some(current) = get_from_pool(&pool, &id).await? else {
                return Err(anyhow::Error::new(SchedulerError::NotFound(id.to_string())));
            };
            check_revision(&current, Some(expected_revision)).map_err(anyhow::Error::new)?;
            replacement.id = id.clone();
            replacement.created_at = current.created_at;
            replacement.last_run_at = current.last_run_at;
            replacement.revision = current.revision.saturating_add(1);
            replacement.updated_at = Some(Utc::now());
            update_task_cas(&pool, current.revision, &replacement).await?;
            Ok(replacement)
        })
    }

    pub(super) fn get(
        data_root: &Path,
        json_path: &Path,
        id: &TaskId,
    ) -> Result<Option<ScheduledTask>> {
        let id = id.clone();
        run(data_root, json_path, |pool| async move {
            get_from_pool(&pool, &id).await
        })
    }

    pub(super) fn set_paused(
        data_root: &Path,
        json_path: &Path,
        id: &TaskId,
        paused: bool,
        expected_revision: Option<u64>,
    ) -> Result<Option<ScheduledTask>> {
        let id = id.clone();
        run(data_root, json_path, move |pool| async move {
            let Some(mut task) = get_from_pool(&pool, &id).await? else {
                return Ok(None);
            };
            check_revision(&task, expected_revision).map_err(anyhow::Error::new)?;
            let previous_revision = task.revision;
            task.paused = paused;
            task.revision = task.revision.saturating_add(1);
            task.updated_at = Some(Utc::now());
            update_task_cas(&pool, previous_revision, &task).await?;
            Ok(Some(task))
        })
    }

    pub(super) fn record_fired(
        data_root: &Path,
        json_path: &Path,
        id: &TaskId,
        expected_revision: Option<u64>,
    ) -> Result<Option<ScheduledTask>> {
        let id = id.clone();
        run(data_root, json_path, move |pool| async move {
            let Some(mut task) = get_from_pool(&pool, &id).await? else {
                return Ok(None);
            };
            check_revision(&task, expected_revision).map_err(anyhow::Error::new)?;
            let previous_revision = task.revision;
            task.mark_fired(Utc::now());
            update_task_cas(&pool, previous_revision, &task).await?;
            Ok(Some(task))
        })
    }

    pub(super) fn due_tasks(data_root: &Path, json_path: &Path) -> Result<Vec<ScheduledTask>> {
        run(data_root, json_path, |pool| async move {
            let now = datetime_to_ns(Utc::now())?;
            let rows = sqlx::query(
                r#"
                SELECT *
                FROM scheduled_tasks
                WHERE paused = 0 AND next_run_at <= ?
                ORDER BY next_run_at ASC, id ASC
                "#,
            )
            .bind(now)
            .fetch_all(&pool)
            .await
            .context("failed to query due sqlite scheduled tasks")?;
            rows.into_iter().map(task_from_row).collect()
        })
    }

    fn run<F, Fut, T>(data_root: &Path, json_path: &Path, op: F) -> Result<T>
    where
        F: FnOnce(SqlitePool) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let data_root = data_root.to_path_buf();
        let json_path = json_path.to_path_buf();
        allthecodes_db::run_sqlite_sync("allthecodes-scheduler-sqlite", async move {
            let pool = migrated_pool(&data_root).await?;
            import_legacy_json(&pool, &json_path).await?;
            op(pool).await
        })
    }

    async fn migrated_pool(data_root: &Path) -> Result<SqlitePool> {
        let pool = allthecodes_db::DbPoolManager::from_data_root(data_root).state_pool()?;
        MigrationRunner::new("scheduled_tasks", MIGRATIONS)
            .run(&pool)
            .await?;
        Ok(pool)
    }

    async fn import_legacy_json(pool: &SqlitePool, json_path: &Path) -> Result<()> {
        let state = read_json_state(json_path)?;
        for task in state.tasks {
            let exists = sqlx::query("SELECT 1 FROM scheduled_tasks WHERE id = ?")
                .bind(task.id.as_str())
                .fetch_optional(pool)
                .await
                .context("failed to check sqlite scheduled task before JSON import")?
                .is_some();
            if !exists {
                insert_task(pool, &task).await?;
            }
        }
        Ok(())
    }

    fn read_json_state(path: &Path) -> Result<StateFile> {
        if !path.exists() {
            return Ok(StateFile::default());
        }
        let mut file = File::open(path)
            .with_context(|| format!("failed to open scheduler JSON {}", path.display()))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .with_context(|| format!("failed to read scheduler JSON {}", path.display()))?;
        if buf.trim().is_empty() {
            return Ok(StateFile::default());
        }
        serde_json::from_str(&buf)
            .with_context(|| format!("failed to parse scheduler JSON {}", path.display()))
    }

    async fn load_from_pool(pool: &SqlitePool) -> Result<Vec<ScheduledTask>> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM scheduled_tasks
            ORDER BY next_run_at ASC, id ASC
            "#,
        )
        .fetch_all(pool)
        .await
        .context("failed to load sqlite scheduled tasks")?;
        rows.into_iter().map(task_from_row).collect()
    }

    async fn get_from_pool(pool: &SqlitePool, id: &TaskId) -> Result<Option<ScheduledTask>> {
        sqlx::query("SELECT * FROM scheduled_tasks WHERE id = ?")
            .bind(id.as_str())
            .fetch_optional(pool)
            .await
            .context("failed to get sqlite scheduled task")?
            .map(task_from_row)
            .transpose()
    }

    async fn insert_task(pool: &SqlitePool, task: &ScheduledTask) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO scheduled_tasks (
                id,
                kind,
                name,
                schedule,
                schedule_kind,
                timezone,
                interval_seconds,
                payload_json,
                created_at,
                last_run_at,
                next_run_at,
                paused,
                revision,
                updated_at,
                metadata_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                name = excluded.name,
                schedule = excluded.schedule,
                schedule_kind = excluded.schedule_kind,
                timezone = excluded.timezone,
                interval_seconds = excluded.interval_seconds,
                payload_json = excluded.payload_json,
                created_at = excluded.created_at,
                last_run_at = excluded.last_run_at,
                next_run_at = excluded.next_run_at,
                paused = excluded.paused,
                revision = excluded.revision,
                updated_at = excluded.updated_at,
                metadata_json = excluded.metadata_json
            "#,
        )
        .bind(task.id.as_str())
        .bind(kind_to_str(task.kind))
        .bind(&task.name)
        .bind(&task.schedule)
        .bind(schedule_kind_to_str(task.schedule_kind))
        .bind(&task.timezone)
        .bind(u64_to_i64(task.interval_seconds))
        .bind(serde_json::to_string(&task.payload).context("failed to encode task payload")?)
        .bind(datetime_to_ns(task.created_at)?)
        .bind(task.last_run_at.map(datetime_to_ns).transpose()?)
        .bind(datetime_to_ns(task.next_run_at)?)
        .bind(if task.paused { 1_i64 } else { 0_i64 })
        .bind(u64_to_i64(task.revision))
        .bind(task.updated_at.map(datetime_to_ns).transpose()?)
        .bind(serde_json::to_string(&task.metadata).context("failed to encode task metadata")?)
        .execute(pool)
        .await
        .context("failed to upsert sqlite scheduled task")?;
        Ok(())
    }

    async fn insert_task_new(pool: &SqlitePool, task: &ScheduledTask) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO scheduled_tasks (
                id, kind, name, schedule, schedule_kind, timezone,
                interval_seconds, payload_json, created_at, last_run_at,
                next_run_at, paused, revision, updated_at, metadata_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(task.id.as_str())
        .bind(kind_to_str(task.kind))
        .bind(&task.name)
        .bind(&task.schedule)
        .bind(schedule_kind_to_str(task.schedule_kind))
        .bind(&task.timezone)
        .bind(u64_to_i64(task.interval_seconds))
        .bind(serde_json::to_string(&task.payload).context("failed to encode task payload")?)
        .bind(datetime_to_ns(task.created_at)?)
        .bind(task.last_run_at.map(datetime_to_ns).transpose()?)
        .bind(datetime_to_ns(task.next_run_at)?)
        .bind(if task.paused { 1_i64 } else { 0_i64 })
        .bind(u64_to_i64(task.revision))
        .bind(task.updated_at.map(datetime_to_ns).transpose()?)
        .bind(serde_json::to_string(&task.metadata).context("failed to encode task metadata")?)
        .execute(pool)
        .await
        .context("failed to insert sqlite scheduled task")?;
        if result.rows_affected() == 0 {
            return Err(anyhow::Error::new(SchedulerError::AlreadyExists(
                task.id.to_string(),
            )));
        }
        Ok(())
    }

    async fn update_task_cas(
        pool: &SqlitePool,
        previous_revision: u64,
        task: &ScheduledTask,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE scheduled_tasks SET
                kind = ?, name = ?, schedule = ?, schedule_kind = ?,
                timezone = ?, interval_seconds = ?, payload_json = ?,
                created_at = ?, last_run_at = ?, next_run_at = ?, paused = ?,
                revision = ?, updated_at = ?, metadata_json = ?
            WHERE id = ? AND revision = ?
            "#,
        )
        .bind(kind_to_str(task.kind))
        .bind(&task.name)
        .bind(&task.schedule)
        .bind(schedule_kind_to_str(task.schedule_kind))
        .bind(&task.timezone)
        .bind(u64_to_i64(task.interval_seconds))
        .bind(serde_json::to_string(&task.payload).context("failed to encode task payload")?)
        .bind(datetime_to_ns(task.created_at)?)
        .bind(task.last_run_at.map(datetime_to_ns).transpose()?)
        .bind(datetime_to_ns(task.next_run_at)?)
        .bind(if task.paused { 1_i64 } else { 0_i64 })
        .bind(u64_to_i64(task.revision))
        .bind(task.updated_at.map(datetime_to_ns).transpose()?)
        .bind(serde_json::to_string(&task.metadata).context("failed to encode task metadata")?)
        .bind(task.id.as_str())
        .bind(u64_to_i64(previous_revision))
        .execute(pool)
        .await
        .context("failed to update sqlite scheduled task")?;
        if result.rows_affected() == 0 {
            return Err(current_revision_conflict(pool, &task.id, previous_revision).await);
        }
        Ok(())
    }

    async fn current_revision_conflict(
        pool: &SqlitePool,
        id: &TaskId,
        expected: u64,
    ) -> anyhow::Error {
        match get_from_pool(pool, id).await {
            Ok(Some(current)) => anyhow::Error::new(SchedulerError::RevisionConflict {
                expected,
                actual: current.revision,
            }),
            Ok(None) => anyhow::Error::new(SchedulerError::NotFound(id.to_string())),
            Err(error) => error,
        }
    }

    fn task_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ScheduledTask> {
        let kind: String = row.try_get("kind")?;
        let schedule_kind: String = row.try_get("schedule_kind")?;
        let payload_json: String = row.try_get("payload_json")?;
        let interval_seconds: i64 = row.try_get("interval_seconds")?;
        let last_run_at: Option<i64> = row.try_get("last_run_at")?;
        let updated_at: Option<i64> = row.try_get("updated_at")?;
        let metadata_json: String = row.try_get("metadata_json")?;
        Ok(ScheduledTask {
            id: TaskId(row.try_get("id")?),
            kind: kind_from_str(&kind)?,
            name: row.try_get("name")?,
            schedule: row.try_get("schedule")?,
            schedule_kind: schedule_kind_from_str(&schedule_kind)?,
            timezone: row.try_get("timezone")?,
            interval_seconds: i64_to_u64(interval_seconds),
            payload: serde_json::from_str(&payload_json)
                .context("failed to decode task payload")?,
            created_at: ns_to_datetime(row.try_get("created_at")?)?,
            last_run_at: last_run_at.map(ns_to_datetime).transpose()?,
            next_run_at: ns_to_datetime(row.try_get("next_run_at")?)?,
            paused: row.try_get::<i64, _>("paused")? != 0,
            revision: i64_to_u64(row.try_get("revision")?),
            updated_at: updated_at.map(ns_to_datetime).transpose()?,
            metadata: serde_json::from_str(&metadata_json)
                .context("failed to decode task metadata")?,
        })
    }

    fn kind_to_str(kind: SchedulerKind) -> &'static str {
        match kind {
            SchedulerKind::LocalCron => "local_cron",
            SchedulerKind::RemoteTrigger => "remote_trigger",
        }
    }

    fn kind_from_str(value: &str) -> Result<SchedulerKind> {
        match value {
            "local_cron" => Ok(SchedulerKind::LocalCron),
            "remote_trigger" => Ok(SchedulerKind::RemoteTrigger),
            other => anyhow::bail!("unknown scheduler kind {other}"),
        }
    }

    fn schedule_kind_to_str(kind: ScheduleKind) -> &'static str {
        match kind {
            ScheduleKind::Interval => "interval",
            ScheduleKind::Cron => "cron",
        }
    }

    fn schedule_kind_from_str(value: &str) -> Result<ScheduleKind> {
        match value {
            "interval" => Ok(ScheduleKind::Interval),
            "cron" => Ok(ScheduleKind::Cron),
            other => anyhow::bail!("unknown schedule kind {other}"),
        }
    }

    fn datetime_to_ns(value: DateTime<Utc>) -> Result<i64> {
        value
            .timestamp_nanos_opt()
            .context("datetime is outside SQLite nanosecond range")
    }

    fn ns_to_datetime(value: i64) -> Result<DateTime<Utc>> {
        let secs = value.div_euclid(1_000_000_000);
        let nanos = value.rem_euclid(1_000_000_000) as u32;
        Utc.timestamp_opt(secs, nanos)
            .single()
            .context("invalid scheduler timestamp in SQLite")
    }

    fn u64_to_i64(value: u64) -> i64 {
        i64::try_from(value).unwrap_or(i64::MAX)
    }

    fn i64_to_u64(value: i64) -> u64 {
        u64::try_from(value.max(0)).unwrap_or(u64::MAX)
    }
}

/// Drop-guard that removes the lock file when it goes out of scope.
struct FileLockGuard<'a> {
    file: Option<File>,
    path: &'a Path,
}

impl Drop for FileLockGuard<'_> {
    fn drop(&mut self) {
        // Close the file handle before unlinking — on Windows we'd otherwise
        // hit sharing-violation errors while trying to remove an open file.
        self.file.take();
        let _ = fs::remove_file(self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{parse_interval, TaskPayload};
    use tempfile::tempdir;

    fn fresh_store() -> (tempfile::TempDir, SchedulerStore) {
        let dir = tempdir().unwrap();
        let store = SchedulerStore::new(dir.path().join("scheduled_tasks.json"));
        (dir, store)
    }

    fn make_task(name: &str, secs: u64) -> ScheduledTask {
        let now = Utc::now();
        let interval = parse_interval(&format!("{}s", secs)).unwrap();
        ScheduledTask::new(
            SchedulerKind::LocalCron,
            name,
            format!("{}s", secs),
            interval,
            TaskPayload::Prompt(format!("prompt-{}", name)),
            now,
        )
    }

    #[test]
    fn add_list_remove_roundtrip() {
        let (_dir, store) = fresh_store();
        assert!(store.load().unwrap().is_empty());

        let created = store.add(make_task("one", 60)).unwrap();
        let list = store.load().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);

        let removed = store.remove(&created.id).unwrap();
        assert_eq!(removed.id, created.id);
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn duplicate_add_is_rejected_without_overwriting_definition() {
        let (_dir, store) = fresh_store();
        let original = store.add(make_task("one", 60)).unwrap();
        let mut duplicate = make_task("replacement", 120);
        duplicate.id = original.id.clone();

        let err = store.add(duplicate).unwrap_err();
        assert!(matches!(err, SchedulerError::AlreadyExists(_)));
        assert_eq!(store.get(&original.id).unwrap().name, "one");
    }

    #[test]
    fn replace_uses_compare_and_swap_and_preserves_runtime_history() {
        let (_dir, store) = fresh_store();
        let original = store.add(make_task("one", 60)).unwrap();
        let fired = store
            .record_fired_if_revision(&original.id, Some(original.revision))
            .unwrap();
        let last_run_at = fired.last_run_at;

        let mut replacement = make_task("renamed", 120);
        replacement.metadata.description = Some("canonical metadata".to_string());
        let replaced = store
            .replace(&original.id, fired.revision, replacement)
            .unwrap();

        assert_eq!(replaced.id, original.id);
        assert_eq!(replaced.name, "renamed");
        assert_eq!(replaced.created_at, original.created_at);
        assert_eq!(replaced.last_run_at, last_run_at);
        assert_eq!(replaced.revision, fired.revision + 1);
        assert_eq!(
            replaced.metadata.description.as_deref(),
            Some("canonical metadata")
        );

        let err = store
            .replace(&original.id, fired.revision, make_task("stale", 30))
            .unwrap_err();
        assert!(matches!(
            err,
            SchedulerError::RevisionConflict {
                expected,
                actual
            } if expected == fired.revision && actual == replaced.revision
        ));
        assert_eq!(store.get(&original.id).unwrap().name, "renamed");
    }

    #[test]
    fn pause_fire_and_delete_reject_stale_revisions() {
        let (_dir, store) = fresh_store();
        let original = store.add(make_task("one", 60)).unwrap();
        let paused = store
            .set_paused_if_revision(&original.id, true, Some(original.revision))
            .unwrap();
        assert_eq!(paused.revision, original.revision + 1);

        let stale_fire = store
            .record_fired_if_revision(&original.id, Some(original.revision))
            .unwrap_err();
        assert!(matches!(
            stale_fire,
            SchedulerError::RevisionConflict { .. }
        ));

        let stale_delete = store
            .remove_if_revision(&original.id, Some(original.revision))
            .unwrap_err();
        assert!(matches!(
            stale_delete,
            SchedulerError::RevisionConflict { .. }
        ));

        let fired = store
            .record_fired_if_revision(&original.id, Some(paused.revision))
            .unwrap();
        assert_eq!(fired.revision, paused.revision + 1);
        store
            .remove_if_revision(&original.id, Some(fired.revision))
            .unwrap();
        assert!(matches!(
            store.get(&original.id),
            Err(SchedulerError::NotFound(_))
        ));
    }

    #[test]
    fn legacy_json_defaults_revision_without_rewriting_on_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scheduled_tasks.json");
        let task = make_task("legacy", 60);
        let mut value = serde_json::to_value(StateFile {
            version: 1,
            tasks: vec![task.clone()],
        })
        .unwrap();
        let object = value["tasks"][0].as_object_mut().unwrap();
        object.remove("revision");
        object.remove("updated_at");
        object.remove("metadata");
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let store = SchedulerStore::new(&path);
        let loaded = store.get(&task.id).unwrap();
        assert_eq!(loaded.revision, 1);
        assert_eq!(loaded.updated_at, None);
        assert!(loaded.metadata.artifact_links.is_empty());

        let paused = store
            .set_paused_if_revision(&task.id, true, Some(1))
            .unwrap();
        assert_eq!(paused.revision, 2);
        assert!(paused.updated_at.is_some());
    }

    #[test]
    fn remove_missing_returns_not_found() {
        let (_dir, store) = fresh_store();
        let err = store.remove(&TaskId::new()).unwrap_err();
        assert!(matches!(err, SchedulerError::NotFound(_)));
    }

    #[test]
    fn remote_trigger_rejected() {
        let (_dir, store) = fresh_store();
        let mut task = make_task("remote", 60);
        task.kind = SchedulerKind::RemoteTrigger;
        let err = store.add(task).unwrap_err();
        assert!(matches!(err, SchedulerError::RemoteTriggerUnsupported));
    }

    #[test]
    fn record_fired_advances_next_run() {
        let (_dir, store) = fresh_store();
        let task = store.add(make_task("t", 60)).unwrap();
        let before = task.next_run_at;
        // Force "now" to be ahead of the initial next_run_at by mutating
        // last_run_at through the public API.
        std::thread::sleep(Duration::from_millis(10));
        let after = store.record_fired(&task.id).unwrap();
        assert!(after.last_run_at.is_some());
        assert!(after.next_run_at >= before);
    }

    #[test]
    fn pause_skips_due_reporting() {
        let (_dir, store) = fresh_store();
        let mut task = make_task("t", 1);
        task.next_run_at = Utc::now() - chrono::Duration::seconds(5);
        let added = store.add(task).unwrap();
        assert_eq!(store.due_tasks().unwrap().len(), 1);
        store.set_paused(&added.id, true).unwrap();
        assert_eq!(store.due_tasks().unwrap().len(), 0);
        store.set_paused(&added.id, false).unwrap();
        assert_eq!(store.due_tasks().unwrap().len(), 1);
    }

    #[test]
    fn persists_across_instances() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scheduled_tasks.json");
        {
            let store = SchedulerStore::new(&path);
            store.add(make_task("persist", 120)).unwrap();
        }
        let store2 = SchedulerStore::new(&path);
        assert_eq!(store2.load().unwrap().len(), 1);
    }

    #[test]
    fn get_by_id() {
        let (_dir, store) = fresh_store();
        let task = store.add(make_task("g", 60)).unwrap();
        let fetched = store.get(&task.id).unwrap();
        assert_eq!(fetched.id, task.id);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    fn imports_legacy_json_and_refreshes_backup_after_delete() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scheduled_tasks.json");
        let legacy = make_task("legacy", 60);
        let json = serde_json::to_string_pretty(&StateFile {
            version: SCHEMA_VERSION,
            tasks: vec![legacy.clone()],
        })
        .unwrap();
        std::fs::write(&path, json).unwrap();

        let store = SchedulerStore::new(&path);
        assert_eq!(store.load().unwrap()[0].id, legacy.id);

        let added = store.add(make_task("added", 120)).unwrap();
        assert_eq!(store.load().unwrap().len(), 2);
        store.remove(&legacy.id).unwrap();

        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, added.id);

        let backup: StateFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(backup.tasks.len(), 1);
        assert_eq!(backup.tasks[0].id, added.id);
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    fn falls_back_to_json_when_sqlite_path_is_blocked() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("state").join("state_5.sqlite")).unwrap();
        let path = dir.path().join("scheduled_tasks.json");
        let store = SchedulerStore::new(&path);

        let task = store.add(make_task("json", 60)).unwrap();
        assert_eq!(store.load().unwrap()[0].id, task.id);
        assert!(path.exists());
    }
}
