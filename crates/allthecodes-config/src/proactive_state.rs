use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const PROACTIVE_STATE_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_PROACTIVE_TICK_INTERVAL_MS: u64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableProactiveState {
    pub schema_version: u32,
    pub active: bool,
    pub next_tick_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub context_blocked: bool,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

pub fn write_proactive_state(
    active: bool,
    next_tick_at: Option<DateTime<Utc>>,
    context_blocked: bool,
    blocked_reason: Option<String>,
) -> Result<DurableProactiveState> {
    let state = DurableProactiveState {
        schema_version: PROACTIVE_STATE_SCHEMA_VERSION,
        active,
        next_tick_at,
        context_blocked,
        blocked_reason: blocked_reason.and_then(|reason| normalized_text(&reason)),
        updated_at: Utc::now(),
    };
    write_proactive_state_file(&state)?;
    Ok(state)
}

pub fn write_proactive_context_blocked(
    blocked: bool,
    reason: &str,
) -> Result<DurableProactiveState> {
    let current = read_proactive_state()?;
    let active = current.as_ref().map(|state| state.active).unwrap_or(false);
    let next_tick_at = if blocked || !active {
        None
    } else {
        current.and_then(|state| state.next_tick_at).or_else(|| {
            Some(Utc::now() + Duration::milliseconds(DEFAULT_PROACTIVE_TICK_INTERVAL_MS as i64))
        })
    };
    write_proactive_state(
        active,
        next_tick_at,
        blocked,
        blocked.then(|| reason.to_string()),
    )
}

pub fn read_proactive_state() -> Result<Option<DurableProactiveState>> {
    let path = proactive_state_path();
    read_proactive_state_from_path(&path, |path| fs::read_to_string(path))
}

pub fn clear_proactive_state(reason: &str) -> Result<bool> {
    let path = proactive_state_path();
    match fs::remove_file(&path) {
        Ok(()) => {
            tracing::debug!(
                reason = %reason,
                path = %path.display(),
                "cleared durable proactive state"
            );
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("failed to clear daemon proactive state {}", path.display())),
    }
}

pub fn proactive_state_path() -> PathBuf {
    crate::paths::daemon_dir().join("proactive-state.json")
}

fn write_proactive_state_file(state: &DurableProactiveState) -> Result<()> {
    let path = proactive_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create daemon directory {}", parent.display()))?;
    }

    let tmp = path.with_file_name(format!(
        "proactive-state.json.{}.{}.tmp",
        std::process::id(),
        Utc::now().timestamp_micros()
    ));
    fs::write(&tmp, serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("failed to write daemon proactive state {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "failed to replace daemon proactive state {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}

fn read_proactive_state_from_path<F>(
    path: &Path,
    read_to_string: F,
) -> Result<Option<DurableProactiveState>>
where
    F: FnOnce(&Path) -> std::io::Result<String>,
{
    let text = match read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read daemon proactive state {}", path.display())
            })
        }
    };
    let state = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse daemon proactive state {}", path.display()))?;
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
    use chrono::{Duration, Utc};
    use serial_test::serial;

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
    fn proactive_context_blocked_write_preserves_active_and_uses_schema() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let next_tick_at = Utc::now() + Duration::seconds(60);
        super::write_proactive_state(true, Some(next_tick_at), false, None).unwrap();

        let blocked = super::write_proactive_context_blocked(true, " context_limit ").unwrap();

        assert_eq!(
            blocked.schema_version,
            super::PROACTIVE_STATE_SCHEMA_VERSION
        );
        assert!(blocked.active);
        assert!(blocked.next_tick_at.is_none());
        assert!(blocked.context_blocked);
        assert_eq!(blocked.blocked_reason.as_deref(), Some("context_limit"));

        let read_back = super::read_proactive_state().unwrap().unwrap();
        assert_eq!(read_back, blocked);
    }

    #[test]
    #[serial]
    fn proactive_context_blocked_write_without_existing_state_does_not_activate() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());

        let blocked = super::write_proactive_context_blocked(true, "context_limit").unwrap();

        assert!(!blocked.active);
        assert!(blocked.next_tick_at.is_none());
        assert!(blocked.context_blocked);
        assert_eq!(blocked.blocked_reason.as_deref(), Some("context_limit"));

        let unblocked = super::write_proactive_context_blocked(false, "context_ready").unwrap();

        assert!(!unblocked.active);
        assert!(unblocked.next_tick_at.is_none());
        assert!(!unblocked.context_blocked);
        assert!(unblocked.blocked_reason.is_none());

        let read_back = super::read_proactive_state().unwrap().unwrap();
        assert_eq!(read_back, unblocked);
    }
}
