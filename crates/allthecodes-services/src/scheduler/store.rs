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

use super::task::{ScheduledTask, SchedulerKind, TaskId};

/// Schema version for the on-disk JSON so we can evolve the format later
/// without silently deserializing a mismatched layout.
const SCHEMA_VERSION: u32 = 1;

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
    #[error("could not acquire scheduler lock at {path} within {}ms", timeout_ms.as_millis())]
    LockTimeout { path: PathBuf, timeout_ms: Duration },
    #[error("remote-trigger tasks are not supported yet — see issue #60")]
    RemoteTriggerUnsupported,
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
        Self {
            state_path,
            lock_path,
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
        let _file_guard = self.acquire_lock()?;
        self.read_state().map(|s| s.tasks)
    }

    /// Add a new task, persisting immediately.
    pub fn add(&self, task: ScheduledTask) -> Result<ScheduledTask, SchedulerError> {
        if matches!(task.kind, SchedulerKind::RemoteTrigger) {
            return Err(SchedulerError::RemoteTriggerUnsupported);
        }
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        state.tasks.push(task.clone());
        self.write_state(&state)?;
        Ok(task)
    }

    /// Remove a task by id. Returns the removed task or `NotFound`.
    pub fn remove(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let pos = state
            .tasks
            .iter()
            .position(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        let removed = state.tasks.remove(pos);
        self.write_state(&state)?;
        Ok(removed)
    }

    /// Fetch a single task snapshot.
    pub fn get(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        let tasks = self.load()?;
        tasks
            .into_iter()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))
    }

    /// Pause or resume a task by id.
    pub fn set_paused(&self, id: &TaskId, paused: bool) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        task.paused = paused;
        let snapshot = task.clone();
        self.write_state(&state)?;
        Ok(snapshot)
    }

    /// Mark a task as fired (advance its `next_run_at`). This is what the
    /// daemon should call after it successfully dispatches a task.
    pub fn record_fired(&self, id: &TaskId) -> Result<ScheduledTask, SchedulerError> {
        let _guard = self.inner.lock();
        let _file_guard = self.acquire_lock()?;
        let mut state = self.read_state()?;
        let task = state
            .tasks
            .iter_mut()
            .find(|t| t.id == *id)
            .ok_or_else(|| SchedulerError::NotFound(id.to_string()))?;
        task.mark_fired(Utc::now());
        let snapshot = task.clone();
        self.write_state(&state)?;
        Ok(snapshot)
    }

    /// Collect the tasks that are due right now. A passive snapshot — the
    /// caller is responsible for firing them and calling `record_fired`.
    pub fn due_tasks(&self) -> Result<Vec<ScheduledTask>, SchedulerError> {
        let now = Utc::now();
        Ok(self.load()?.into_iter().filter(|t| t.is_due(now)).collect())
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
        let parsed: StateFile = serde_json::from_str(&buf).map_err(|e| SchedulerError::Decode {
            path: self.state_path.clone(),
            source: e,
        })?;
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
}
