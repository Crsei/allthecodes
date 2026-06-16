use std::fmt;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::process_state::{daemon_dir, process_is_alive};

const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_OPERATION_LOCK_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_OPERATION_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonOperationLockMetadata {
    pub schema_version: u32,
    pub pid: u32,
    pub operation: String,
    pub started_at: DateTime<Utc>,
    pub cwd: PathBuf,
}

#[derive(Debug)]
pub struct DaemonOperationLock {
    path: PathBuf,
    metadata: DaemonOperationLockMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLockTimeout {
    pub path: PathBuf,
    pub holder: Option<DaemonOperationLockMetadata>,
    pub age: Option<Duration>,
}

impl fmt::Display for OperationLockTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "daemon operation lock timed out: path={}",
            self.path.display()
        )?;
        match &self.holder {
            Some(holder) => {
                write!(
                    f,
                    " holder_pid={} holder_operation={}",
                    holder.pid, holder.operation
                )?;
                if let Some(age) = self.age {
                    write!(f, " age={age:?}")?;
                }
            }
            None => {
                write!(
                    f,
                    " holder_pid=unknown holder_operation=unknown age=unknown"
                )?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for OperationLockTimeout {}

impl Drop for DaemonOperationLock {
    fn drop(&mut self) {
        let Ok(current) = read_lock_metadata(&self.path) else {
            let _ = fs::remove_file(&self.path);
            return;
        };
        if current.pid == self.metadata.pid && current.operation == self.metadata.operation {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn operation_lock_path() -> PathBuf {
    daemon_dir().join("operation.lock")
}

pub fn with_operation_lock<T>(
    operation: &str,
    cwd: &Path,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _lock = acquire_operation_lock(operation, cwd)?;
    f()
}

pub fn acquire_operation_lock(operation: &str, cwd: &Path) -> Result<DaemonOperationLock> {
    acquire_operation_lock_with(
        operation,
        cwd,
        DEFAULT_OPERATION_LOCK_TIMEOUT,
        DEFAULT_OPERATION_LOCK_RETRY_INTERVAL,
    )
}

pub(crate) fn acquire_operation_lock_with(
    operation: &str,
    cwd: &Path,
    timeout: Duration,
    retry_interval: Duration,
) -> Result<DaemonOperationLock> {
    let path = operation_lock_path();
    let started = Instant::now();

    loop {
        match try_create_lock(&path, operation, cwd) {
            Ok(lock) => return Ok(lock),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let holder = read_lock_metadata(&path).ok();
                if let Some(holder) = &holder {
                    if !process_is_alive(holder.pid) {
                        fs::remove_file(&path).with_context(|| {
                            format!("failed to remove stale daemon lock {}", path.display())
                        })?;
                        continue;
                    }
                }

                if started.elapsed() >= timeout {
                    let age = holder
                        .as_ref()
                        .and_then(|holder| lock_age(holder.started_at));
                    return Err(OperationLockTimeout { path, holder, age }.into());
                }

                let remaining = timeout.saturating_sub(started.elapsed());
                let sleep_for = retry_interval.min(remaining);
                if !sleep_for.is_zero() {
                    std::thread::sleep(sleep_for);
                }
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to create daemon lock {}", path.display()));
            }
        }
    }
}

fn try_create_lock(
    path: &Path,
    operation: &str,
    cwd: &Path,
) -> std::io::Result<DaemonOperationLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let metadata = DaemonOperationLockMetadata {
        schema_version: SCHEMA_VERSION,
        pid: std::process::id(),
        operation: operation.to_string(),
        started_at: Utc::now(),
        cwd: cwd.to_path_buf(),
    };
    let bytes = serde_json::to_vec_pretty(&metadata)
        .map_err(|error| std::io::Error::new(ErrorKind::InvalidData, error))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.write_all(b"\n")) {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    if let Err(error) = file.sync_all() {
        let _ = fs::remove_file(path);
        return Err(error);
    }
    Ok(DaemonOperationLock {
        path: path.to_path_buf(),
        metadata,
    })
}

pub(crate) fn read_lock_metadata(path: &Path) -> Result<DaemonOperationLockMetadata> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read daemon lock {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon lock {}", path.display()))
}

fn lock_age(started_at: DateTime<Utc>) -> Option<Duration> {
    (Utc::now() - started_at).to_std().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::{Arc, Barrier};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
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

    fn dead_test_pid() -> u32 {
        (1_000_000u32..4_194_303u32)
            .rev()
            .find(|pid| !process_is_alive(*pid))
            .expect("dead test pid")
    }

    #[test]
    #[serial]
    fn concurrent_acquire_allows_only_one_holder() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();

        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let cwd = temp.path().to_path_buf();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                acquire_operation_lock_with(
                    "start",
                    &cwd,
                    Duration::from_millis(1),
                    Duration::from_millis(1),
                )
                .map(|_lock| {
                    std::thread::sleep(Duration::from_millis(20));
                })
            }));
        }

        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap().is_ok())
            .filter(|ok| *ok)
            .count();
        assert_eq!(successes, 1);
    }

    #[test]
    #[serial]
    fn timeout_error_includes_holder_information() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let _lock = acquire_operation_lock_with(
            "restart",
            temp.path(),
            Duration::from_millis(1),
            Duration::from_millis(1),
        )
        .unwrap();

        let error = acquire_operation_lock_with(
            "status",
            temp.path(),
            Duration::from_millis(1),
            Duration::from_millis(1),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("operation.lock"));
        assert!(text.contains("holder_pid="));
        assert!(text.contains("holder_operation=restart"));
        assert!(text.contains("age="));
    }

    #[test]
    #[serial]
    fn dead_pid_lock_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let path = operation_lock_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let metadata = DaemonOperationLockMetadata {
            schema_version: SCHEMA_VERSION,
            pid: dead_test_pid(),
            operation: "start".to_string(),
            started_at: Utc::now() - chrono::Duration::seconds(60),
            cwd: temp.path().to_path_buf(),
        };
        fs::write(&path, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();

        let _lock = acquire_operation_lock_with(
            "start",
            temp.path(),
            Duration::from_millis(50),
            Duration::from_millis(1),
        )
        .unwrap();
        let current = read_lock_metadata(&path).unwrap();
        assert_eq!(current.pid, std::process::id());
    }
}
