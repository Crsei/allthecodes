use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const SLEEP_STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SleepState {
    pub schema_version: u32,
    pub sleeping_until: DateTime<Utc>,
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

pub fn write_sleep_state(duration_seconds: u64, reason: &str) -> Result<SleepState> {
    anyhow::ensure!(
        (1..=3600).contains(&duration_seconds),
        "\"duration_seconds\" must be between 1 and 3600, got {}",
        duration_seconds
    );

    let sleeping_until = Utc::now() + Duration::seconds(duration_seconds as i64);
    write_sleep_state_until(sleeping_until, reason)
}

pub fn write_sleep_state_until(sleeping_until: DateTime<Utc>, reason: &str) -> Result<SleepState> {
    let state = SleepState {
        schema_version: SLEEP_STATE_SCHEMA_VERSION,
        sleeping_until,
        reason: normalized_text(reason),
        updated_at: Utc::now(),
    };
    write_sleep_state_file(&state)?;
    Ok(state)
}

pub fn read_sleep_state() -> Result<Option<SleepState>> {
    let path = sleep_state_path();
    read_sleep_state_from_path(&path, |path| fs::read_to_string(path))
}

pub fn active_sleep_state() -> Result<Option<SleepState>> {
    let Some(state) = read_sleep_state()? else {
        return Ok(None);
    };

    if state.sleeping_until <= Utc::now() {
        clear_sleep_state("expired")?;
        return Ok(None);
    }

    Ok(Some(state))
}

pub fn clear_sleep_state(reason: &str) -> Result<bool> {
    let path = sleep_state_path();
    match fs::remove_file(&path) {
        Ok(()) => {
            tracing::debug!(
                reason = %reason,
                path = %path.display(),
                "cleared proactive sleep state"
            );
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to clear daemon sleep state {}", path.display())),
    }
}

fn sleep_state_path() -> PathBuf {
    crate::paths::daemon_dir().join("sleep-state.json")
}

fn write_sleep_state_file(state: &SleepState) -> Result<()> {
    let path = sleep_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create daemon directory {}", parent.display()))?;
    }

    let tmp = path.with_file_name(format!(
        "sleep-state.json.{}.{}.tmp",
        std::process::id(),
        Utc::now().timestamp_micros()
    ));
    fs::write(&tmp, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("failed to write daemon sleep state {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "failed to replace daemon sleep state {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

fn read_sleep_state_from_path<F>(path: &Path, read_to_string: F) -> Result<Option<SleepState>>
where
    F: FnOnce(&Path) -> std::io::Result<String>,
{
    let text = match read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read daemon sleep state {}", path.display()))
        }
    };
    let state = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon sleep state {}", path.display()))?;
    Ok(Some(state))
}

fn normalized_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serial_test::serial;
    use std::fs;

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value.as_ref());
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial]
    fn proactive_sleep_writes_reads_and_clears_daemon_file() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let path = home.path().join("daemon").join("sleep-state.json");

        let state = write_sleep_state(60, " waiting for ci ").unwrap();
        assert_eq!(state.schema_version, SLEEP_STATE_SCHEMA_VERSION);
        assert_eq!(state.reason.as_deref(), Some("waiting for ci"));
        assert!(state.sleeping_until > Utc::now());
        assert!(path.exists());

        let stored: SleepState = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(stored.schema_version, SLEEP_STATE_SCHEMA_VERSION);
        assert_eq!(stored.reason.as_deref(), Some("waiting for ci"));

        let raw = read_sleep_state().unwrap().unwrap();
        assert_eq!(raw.reason.as_deref(), Some("waiting for ci"));

        let active = active_sleep_state().unwrap().unwrap();
        assert_eq!(active.reason.as_deref(), Some("waiting for ci"));

        assert!(clear_sleep_state("test cleanup").unwrap());
        assert!(!path.exists());
        assert!(!clear_sleep_state("second cleanup").unwrap());
    }

    #[test]
    #[serial]
    fn proactive_sleep_active_state_clears_expired_state() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let path = home.path().join("daemon").join("sleep-state.json");

        write_sleep_state_until(Utc::now() - chrono::Duration::seconds(1), "expired").unwrap();

        assert!(active_sleep_state().unwrap().is_none());
        assert!(!path.exists());
    }

    #[test]
    #[serial]
    fn proactive_sleep_treats_missing_state_as_none() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

        assert!(read_sleep_state().unwrap().is_none());
        assert!(active_sleep_state().unwrap().is_none());
        assert!(!clear_sleep_state("missing").unwrap());
    }

    #[test]
    #[serial]
    fn proactive_sleep_validates_duration_range() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let low = write_sleep_state(0, "too low").unwrap_err().to_string();
        assert!(low.contains("between 1 and 3600"));

        let high = write_sleep_state(3601, "too high").unwrap_err().to_string();
        assert!(high.contains("between 1 and 3600"));
    }
}
