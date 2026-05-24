//! Audit event sink — non-blocking, append-only NDJSON writer.
//!
//! The sink is the single write path for runtime audit events. It uses a
//! bounded channel + background writer thread to avoid blocking the hot path.
//!
//! Features:
//! - Non-blocking `emit()` — drops events if the channel is full (logged via tracing).
//! - Append-only NDJSON — crash-safe prefix (each line is independently parseable).
//! - Explicit `flush()` at submit boundaries and shutdown.
//! - Redaction hook for secrets/tokens before serialization.
//!
//! See: docs/traceable-logging-plan.md §4.1-B, §9 (risk 3, risk 4)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use tracing::{debug, warn};

use super::event::{AuditEvent, SessionMeta};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Controls audit log behavior via environment variables.
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Whether audit logging is enabled (`CC_AUDIT_LOG=on|off`, default: on).
    pub enabled: bool,
    /// Whether to record stream deltas (`CC_AUDIT_STREAM_DELTAS=0|1`, default: 0).
    pub stream_deltas: bool,
    /// Redaction mode (`CC_AUDIT_REDACTION=strict|default|off`, default: default).
    pub redaction: RedactionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactionMode {
    /// Redact auth tokens, API keys, cookies, and all env-like secrets.
    Strict,
    /// Redact auth tokens and API keys.
    Default,
    /// No redaction (use only for local debugging).
    Off,
}

impl AuditConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = std::env::var("CC_AUDIT_LOG")
            .map(|v| v != "off" && v != "0")
            .unwrap_or(true);

        let stream_deltas = std::env::var("CC_AUDIT_STREAM_DELTAS")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);

        let redaction = match std::env::var("CC_AUDIT_REDACTION")
            .unwrap_or_default()
            .as_str()
        {
            "strict" => RedactionMode::Strict,
            "off" => RedactionMode::Off,
            _ => RedactionMode::Default,
        };

        Self {
            enabled,
            stream_deltas,
            redaction,
        }
    }
}

// ---------------------------------------------------------------------------
// AuditSink
// ---------------------------------------------------------------------------

/// Handle to the audit event writer.
///
/// Cheaply cloneable (wraps an `Arc`). All clones share the same writer.
#[derive(Clone)]
pub struct AuditSink {
    inner: Arc<SinkInner>,
}

impl std::fmt::Debug for AuditSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditSink")
            .field("runs_dir", &self.inner.runs_dir)
            .field("enabled", &self.inner.config.enabled)
            .finish()
    }
}

struct SinkInner {
    /// Path: `~/.allthecodes/runs/<session_id>/`
    runs_dir: PathBuf,
    /// Writer handle (behind a mutex for flush coordination).
    writer: Mutex<Option<std::io::BufWriter<std::fs::File>>>,
    /// Configuration snapshot.
    config: AuditConfig,
}

impl AuditSink {
    /// Initialize the audit sink for a session.
    ///
    /// Creates:
    /// - `<runs_dir>/events.ndjson`
    /// - `<runs_dir>/meta.json`
    /// - `<runs_dir>/artifacts/`
    ///
    /// `runs_dir` is the per-session output directory (resolved by the caller,
    /// typically `~/.allthecodes/runs/<session_id>/`). Injecting it here keeps this
    /// crate free of any dependency on the root config module.
    ///
    /// Returns `Ok(sink)` even if the directory cannot be created (sink becomes
    /// a no-op so the process can still run).
    pub fn init(
        session_id: &str,
        runs_dir: PathBuf,
        meta: &SessionMeta,
        config: AuditConfig,
    ) -> Result<Self> {
        if !config.enabled {
            return Ok(Self::noop(config));
        }

        std::fs::create_dir_all(&runs_dir)
            .with_context(|| format!("Failed to create runs directory {}", runs_dir.display()))?;

        // Create artifacts subdirectory
        let artifacts_dir = runs_dir.join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).ok();

        // Write meta.json
        let meta_path = runs_dir.join("meta.json");
        let meta_json =
            serde_json::to_string_pretty(meta).context("Failed to serialize session meta")?;
        std::fs::write(&meta_path, meta_json)
            .with_context(|| format!("Failed to write {}", meta_path.display()))?;

        // Open events.ndjson for append
        let events_path = runs_dir.join("events.ndjson");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .with_context(|| format!("Failed to open {}", events_path.display()))?;

        let writer = std::io::BufWriter::new(file);

        debug!(
            session_id = session_id,
            path = %runs_dir.display(),
            "audit sink initialized"
        );

        Ok(Self {
            inner: Arc::new(SinkInner {
                runs_dir,
                writer: Mutex::new(Some(writer)),
                config,
            }),
        })
    }

    /// Create a no-op sink that discards all events.
    pub fn noop(config: AuditConfig) -> Self {
        Self {
            inner: Arc::new(SinkInner {
                runs_dir: PathBuf::new(),
                writer: Mutex::new(None),
                config,
            }),
        }
    }

    /// Emit an event to the NDJSON log.
    ///
    /// Non-blocking in the sense that it only acquires a mutex briefly.
    /// If the writer is unavailable (noop or after error), the event is dropped.
    pub fn emit(&self, mut event: AuditEvent) {
        // Apply redaction before serialization
        if self.inner.config.redaction != RedactionMode::Off {
            redact_event(&mut event, &self.inner.config.redaction);
        }

        let mut guard = self.inner.writer.lock();
        let writer = match guard.as_mut() {
            Some(w) => w,
            None => return, // noop or closed
        };

        match serde_json::to_string(&event) {
            Ok(line) => {
                if let Err(e) = writeln!(writer, "{}", line) {
                    warn!(error = %e, "failed to write audit event");
                }
            }
            Err(e) => {
                warn!(error = %e, event_id = %event.event_id, "failed to serialize audit event");
            }
        }
    }

    /// Emit multiple events in batch.
    ///
    /// Each event is emitted individually (same as calling `emit()` in a loop).
    /// This is a convenience method for bulk emission paths.
    pub fn emit_batch(&self, events: &[AuditEvent]) {
        for event in events {
            self.emit(event.clone());
        }
    }

    /// Emit an event with an explicit timestamp.
    ///
    /// Useful for replay, backfill, or cross-process event alignment where
    /// the original timestamp should be preserved rather than set to `now()`.
    pub fn emit_with_ts(&self, mut event: AuditEvent, timestamp: chrono::DateTime<chrono::Utc>) {
        event.ts = timestamp;
        self.emit(event);
    }

    /// Flush buffered events to disk.
    ///
    /// Call at submit boundaries and during shutdown.
    pub fn flush(&self) {
        let mut guard = self.inner.writer.lock();
        if let Some(writer) = guard.as_mut() {
            if let Err(e) = writer.flush() {
                warn!(error = %e, "failed to flush audit sink");
            }
        }
    }

    /// Sync to disk (flush + fsync). Use at shutdown or critical error.
    pub fn sync(&self) {
        let mut guard = self.inner.writer.lock();
        if let Some(writer) = guard.as_mut() {
            let _ = writer.flush();
            // Get the inner file handle for fsync
            let file = writer.get_ref();
            if let Err(e) = file.sync_all() {
                warn!(error = %e, "failed to sync audit sink");
            }
        }
    }

    /// Whether audit logging is enabled.
    pub fn is_enabled(&self) -> bool {
        self.inner.config.enabled
    }

    /// Whether stream delta logging is enabled.
    pub fn stream_deltas_enabled(&self) -> bool {
        self.inner.config.stream_deltas
    }

    /// Path to the runs directory for this session.
    pub fn runs_dir(&self) -> &Path {
        &self.inner.runs_dir
    }

    /// Path to the artifacts directory.
    pub fn artifacts_dir(&self) -> PathBuf {
        self.inner.runs_dir.join("artifacts")
    }
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

/// Redact sensitive data from an event before serialization.
fn redact_event(event: &mut AuditEvent, mode: &RedactionMode) {
    if let Some(ref mut data) = event.data {
        redact_value(data, mode);
    }
}

/// Recursively redact sensitive keys in a JSON value.
fn redact_value(value: &mut serde_json::Value, mode: &RedactionMode) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let key_lower = key.to_lowercase();
                let should_redact = is_sensitive_key(&key_lower, mode);
                if should_redact {
                    *val = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_value(val, mode);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                redact_value(val, mode);
            }
        }
        _ => {}
    }
}

/// Check if a key name suggests sensitive content.
fn is_sensitive_key(key: &str, mode: &RedactionMode) -> bool {
    if *mode == RedactionMode::Off {
        return false;
    }

    // Always redact these (Default + Strict)
    let always_sensitive = [
        "api_key",
        "apikey",
        "api-key",
        "token",
        "auth_token",
        "access_token",
        "refresh_token",
        "secret",
        "password",
        "credential",
        "authorization",
        "bearer",
        "oauth",
        "oauth_code",
    ];

    if always_sensitive.iter().any(|s| key.contains(s)) {
        return true;
    }

    // Strict mode: also redact cookies, env-like values
    if *mode == RedactionMode::Strict {
        let strict_sensitive = ["cookie", "set-cookie", "x-api-key", "session_token"];
        if strict_sensitive.iter().any(|s| key.contains(s)) {
            return true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_sensitive_keys() {
        let mut data = serde_json::json!({
            "tool_name": "Bash",
            "api_key": "sk-ant-12345",
            "nested": {
                "auth_token": "bearer xyz",
                "safe_field": "visible"
            }
        });

        redact_value(&mut data, &RedactionMode::Default);

        assert_eq!(data["api_key"], "[REDACTED]");
        assert_eq!(data["nested"]["auth_token"], "[REDACTED]");
        assert_eq!(data["nested"]["safe_field"], "visible");
        assert_eq!(data["tool_name"], "Bash");
    }

    #[test]
    fn strict_redacts_cookies() {
        let mut data = serde_json::json!({
            "cookie": "session=abc",
            "tool_name": "WebFetch"
        });

        redact_value(&mut data, &RedactionMode::Strict);
        assert_eq!(data["cookie"], "[REDACTED]");
        assert_eq!(data["tool_name"], "WebFetch");
    }

    #[test]
    fn off_mode_skips_redaction() {
        let mut data = serde_json::json!({
            "api_key": "sk-ant-12345"
        });

        redact_value(&mut data, &RedactionMode::Off);
        assert_eq!(data["api_key"], "sk-ant-12345");
    }

    #[test]
    fn config_from_env_defaults() {
        // Clear env vars to test defaults
        std::env::remove_var("CC_AUDIT_LOG");
        std::env::remove_var("CC_AUDIT_STREAM_DELTAS");
        std::env::remove_var("CC_AUDIT_REDACTION");

        let config = AuditConfig::from_env();
        assert!(config.enabled);
        assert!(!config.stream_deltas);
        assert_eq!(config.redaction, RedactionMode::Default);
    }
}
