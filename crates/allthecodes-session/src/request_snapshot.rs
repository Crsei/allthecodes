//! API request snapshot persistence.
//!
//! Snapshots are written next to session files under `~/.allthecodes/sessions/`
//! as JSONL. They intentionally store the final canonical request shape the
//! engine handed to the provider client, without credentials or headers.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_REQUEST_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiRequestSnapshot {
    pub schema_version: u32,
    pub captured_at: String,
    pub session_id: String,
    pub request_id: String,
    pub sequence: usize,
    pub provider: String,
    pub model: String,
    pub message_count: usize,
    pub system_count: usize,
    pub tool_count: usize,
    pub max_tokens: usize,
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisor_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Sanitized JSON request body. Image bytes are replaced by structured
    /// metadata so exports stay readable and do not duplicate large screenshots.
    pub request: Value,
}

pub fn build_api_request_snapshot(
    session_id: &str,
    provider: &str,
    request: &Value,
) -> ApiRequestSnapshot {
    let message_count = request
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let system_count = request
        .get("system")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let tool_count = request
        .get("tools")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let max_tokens = request
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default()
        .min(usize::MAX as u64) as usize;
    let stream = request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    ApiRequestSnapshot {
        schema_version: API_REQUEST_SNAPSHOT_SCHEMA_VERSION,
        captured_at: Utc::now().to_rfc3339(),
        session_id: session_id.to_string(),
        request_id: format!(
            "req-{}-{}",
            Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_else(|| Utc::now().timestamp_micros() * 1_000),
            std::process::id()
        ),
        sequence: 0,
        provider: provider.to_string(),
        model,
        message_count,
        system_count,
        tool_count,
        max_tokens,
        stream,
        advisor_model: request
            .get("advisor_model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        thinking: request.get("thinking").cloned(),
        tool_choice: request.get("tool_choice").cloned(),
        request: sanitize_request_value(request),
    }
}

pub fn record_api_request_snapshot(
    session_id: &str,
    provider: &str,
    request: &Value,
) -> Result<ApiRequestSnapshot> {
    let mut snapshot = build_api_request_snapshot(session_id, provider, request);
    let mut existing = load_api_request_snapshots(session_id)?;
    snapshot.sequence = existing.len();
    existing.push(snapshot.clone());

    let path = snapshot_path(session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create session directory {}", parent.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open request snapshot log {}", path.display()))?;
    let mut line = serde_json::to_string(&snapshot)?;
    line.push('\n');
    file.write_all(line.as_bytes())
        .with_context(|| format!("Failed to write request snapshot log {}", path.display()))?;
    Ok(snapshot)
}

pub fn load_api_request_snapshots(session_id: &str) -> Result<Vec<ApiRequestSnapshot>> {
    let path = snapshot_path(session_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read request snapshot log {}", path.display()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("Failed to parse API request snapshot"))
        .collect()
}

pub fn snapshot_path(session_id: &str) -> PathBuf {
    crate::storage::get_session_dir().join(format!("{}.requests.ndjson", session_id))
}

pub fn sanitize_request_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(sanitize_request_value).collect()),
        Value::Object(map) => {
            let is_image = map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|ty| ty == "image");
            let mut out = serde_json::Map::with_capacity(map.len() + usize::from(is_image));
            for (key, value) in map {
                if is_image && key == "source" {
                    out.insert(key.clone(), sanitize_image_source(value));
                } else {
                    out.insert(key.clone(), sanitize_request_value(value));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn sanitize_image_source(value: &Value) -> Value {
    let Some(map) = value.as_object() else {
        return sanitize_request_value(value);
    };
    let media_type = map
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    let source_type = map.get("type").and_then(Value::as_str).unwrap_or("base64");
    let base64_len = map.get("data").and_then(Value::as_str).map_or(0, str::len);
    serde_json::json!({
        "type": source_type,
        "media_type": media_type,
        "data": format!("[image omitted: {media_type}, base64 length {base64_len}]"),
        "metadata": {
            "omitted": true,
            "encoding": source_type,
            "base64_length": base64_len,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
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

    #[test]
    fn sanitize_request_value_replaces_image_bytes_with_metadata() {
        let value = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "abcdef"
                    }
                }]
            }]
        });
        let sanitized = sanitize_request_value(&value);
        let source = &sanitized["messages"][0]["content"][0]["source"];
        assert_eq!(source["media_type"], "image/png");
        assert_eq!(source["metadata"]["base64_length"], 6);
        assert_ne!(source["data"], "abcdef");
    }

    #[test]
    #[serial]
    fn record_and_load_api_request_snapshots_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set_path("ALLTHECODES_HOME", temp.path());
        let request = serde_json::json!({
            "model": "claude-sonnet",
            "messages": [{"role": "user", "content": "hello"}],
            "system": [{"type": "text", "text": "sys"}],
            "tools": [{"name": "Read"}],
            "max_tokens": 1024,
            "stream": true
        });

        let snapshot =
            record_api_request_snapshot("session-1", "anthropic", &request).expect("record");
        let loaded = load_api_request_snapshots("session-1").expect("load");

        assert_eq!(snapshot.sequence, 0);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].provider, "anthropic");
        assert_eq!(loaded[0].message_count, 1);
        assert_eq!(loaded[0].system_count, 1);
        assert_eq!(loaded[0].tool_count, 1);
    }
}
