// test infrastructure — JSONL session logging for future IPC adapter integration
//! Optional JSONL session event logging for TUI adapters.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;

static LOG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Debug, Serialize)]
struct SessionLogRecord<'a, T> {
    ts: String,
    kind: &'a str,
    payload: &'a T,
}

pub fn maybe_init(dir: impl AsRef<Path>) -> std::io::Result<()> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;
    *LOG_DIR.lock().expect("session log lock poisoned") = Some(dir.to_path_buf());
    Ok(())
}

pub fn maybe_init_from_env() -> std::io::Result<bool> {
    let Some(dir) = std::env::var_os("ALLTHECODES_TUI_SESSION_LOG_DIR")
        .or_else(|| std::env::var_os("CC_RUST_TUI_SESSION_LOG_DIR"))
    else {
        return Ok(false);
    };
    maybe_init(PathBuf::from(dir))?;
    Ok(true)
}

pub fn log_record<T: Serialize>(kind: &str, payload: &T) {
    let Some(dir) = LOG_DIR.lock().expect("session log lock poisoned").clone() else {
        return;
    };
    let _ = write_record(&dir, kind, payload);
}

pub fn log_session_end() {
    log_record("session_end", &serde_json::json!({}));
}

fn write_record<T: Serialize>(dir: &Path, kind: &str, payload: &T) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("tui-session.jsonl");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let record = SessionLogRecord {
        ts: now_ts(),
        kind,
        payload,
    };
    serde_json::to_writer(&mut file, &record)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn now_ts() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_records_when_initialized() {
        let dir = tempfile::tempdir().expect("tempdir");
        maybe_init(dir.path()).expect("init session log");

        log_record("custom", &serde_json::json!({ "ok": true }));
        log_session_end();

        let body = std::fs::read_to_string(dir.path().join("tui-session.jsonl"))
            .expect("session log body");
        assert!(body.contains("\"kind\":\"custom\""));
        assert!(body.contains("\"kind\":\"session_end\""));
    }

    #[test]
    fn env_init_reports_absent_env() {
        let previous_new = std::env::var_os("ALLTHECODES_TUI_SESSION_LOG_DIR");
        let previous_legacy = std::env::var_os("CC_RUST_TUI_SESSION_LOG_DIR");
        unsafe {
            std::env::remove_var("ALLTHECODES_TUI_SESSION_LOG_DIR");
            std::env::remove_var("CC_RUST_TUI_SESSION_LOG_DIR");
        }
        let result = maybe_init_from_env().expect("env init");
        unsafe {
            match previous_new {
                Some(value) => std::env::set_var("ALLTHECODES_TUI_SESSION_LOG_DIR", value),
                None => std::env::remove_var("ALLTHECODES_TUI_SESSION_LOG_DIR"),
            }
            match previous_legacy {
                Some(value) => std::env::set_var("CC_RUST_TUI_SESSION_LOG_DIR", value),
                None => std::env::remove_var("CC_RUST_TUI_SESSION_LOG_DIR"),
            }
        }
        assert!(!result);
    }
}
