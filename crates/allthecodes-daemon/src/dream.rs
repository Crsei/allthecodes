//! Daemon-facing KAIROS dream-memory distillation wrappers.

use std::path::PathBuf;

use anyhow::Result;
use chrono::NaiveDate;

pub use allthecodes_services::dream::{distill_daily_log, write_memory, DreamSummary};

pub fn run_daily_dream_once(date: NaiveDate) -> Result<Option<PathBuf>> {
    allthecodes_services::dream::run_daily_dream_once(date)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Local, TimeZone};
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var(key).ok();
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

    fn write_daily_log(date: NaiveDate, body: &str) {
        let now = Local
            .with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0)
            .single()
            .expect("test date should map to local time");
        let path = allthecodes_config::paths::daily_log_path(now);
        std::fs::create_dir_all(path.parent().expect("daily log has parent")).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    #[serial]
    fn daemon_dream_wrapper_writes_memory_summary_once() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        write_daily_log(date, "- Task: daemon dream wrapper.\n");

        let path = run_daily_dream_once(date)
            .unwrap()
            .expect("dream writes memory");
        assert!(path.starts_with(home.path()));
        assert!(path.ends_with("memory/dream/2026-07-04.md"));
        assert!(run_daily_dream_once(date).unwrap().is_none());
    }
}
