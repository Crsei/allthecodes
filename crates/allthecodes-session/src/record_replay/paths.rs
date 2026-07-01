use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

pub fn rollouts_dir() -> PathBuf {
    allthecodes_config::paths::rollouts_dir()
}

pub fn rollout_day_dir(created_at: DateTime<Utc>) -> PathBuf {
    rollouts_dir()
        .join(created_at.format("%Y").to_string())
        .join(created_at.format("%m").to_string())
        .join(created_at.format("%d").to_string())
}

pub fn new_rollout_file(session_id: &str, created_at: DateTime<Utc>) -> PathBuf {
    let timestamp = created_at.format("%Y%m%dT%H%M%SZ");
    rollout_day_dir(created_at).join(format!(
        "rollout-{}-{}.jsonl",
        timestamp,
        sanitize_session_id(session_id)
    ))
}

pub fn is_rollout_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        && path.starts_with(rollouts_dir())
}

fn sanitize_session_id(session_id: &str) -> String {
    let sanitized: String = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.trim_matches('_').is_empty() {
        "session".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serial_test::serial;

    struct EnvGuard {
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(value: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", value);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("ALLTHECODES_HOME", value),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn rollout_path_uses_day_partition_under_allthecodes_home() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set(temp.path());
        let created_at = Utc.with_ymd_and_hms(2026, 7, 2, 13, 14, 55).unwrap();

        let path = new_rollout_file("session/with spaces", created_at);
        let display = path.to_string_lossy().replace('\\', "/");

        assert!(display
            .ends_with("rollouts/2026/07/02/rollout-20260702T131455Z-session_with_spaces.jsonl"));
        assert!(is_rollout_path(&path));
    }
}
