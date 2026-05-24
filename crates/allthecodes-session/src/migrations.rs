//! Session data format migration system.
//!
//! Provides versioned migrations for session data, allowing forward-compatible
//! upgrades when the on-disk session format changes.
//!
//! Each migration is a pure function: `fn(Value) -> Result<Value>` that
//! transforms data from version N to version N+1.

use anyhow::{bail, Context, Result};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Migration version tracking
// ---------------------------------------------------------------------------

/// Current session format version.
pub const CURRENT_VERSION: u32 = 4;

/// Minimum version we can migrate from.
pub const MIN_SUPPORTED_VERSION: u32 = 1;

/// A single migration step.
struct Migration {
    /// Source version (migrates from → from+1).
    from_version: u32,
    /// Human-readable description.
    description: &'static str,
    /// The migration function.
    migrate: fn(Value) -> Result<Value>,
}

/// All registered migrations, in order.
fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            from_version: 1,
            description: "Add session metadata (cwd, created_at)",
            migrate: migrate_v1_to_v2,
        },
        Migration {
            from_version: 2,
            description: "Normalize message content blocks",
            migrate: migrate_v2_to_v3,
        },
        Migration {
            from_version: 3,
            description: "Add optional custom_title field",
            migrate: migrate_v3_to_v4,
        },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Detect the version of a session file.
///
/// Returns `CURRENT_VERSION` if the `version` field is present;
/// infers from structure otherwise.
pub fn detect_version(data: &Value) -> u32 {
    // Explicit version field
    if let Some(v) = data.get("version").and_then(|v| v.as_u64()) {
        return v as u32;
    }

    // Heuristic: v2 added `metadata` object
    if data.get("metadata").is_some() {
        return 2;
    }

    // Heuristic: v1 has `messages` array at top level without `metadata`
    if data.get("messages").is_some() || data.get("session_id").is_some() {
        return 1;
    }

    // Unknown — assume current
    CURRENT_VERSION
}

/// Run all necessary migrations to bring `data` up to `CURRENT_VERSION`.
///
/// Returns the migrated data and a log of applied migrations.
pub fn migrate_to_current(mut data: Value) -> Result<(Value, Vec<String>)> {
    let mut version = detect_version(&data);
    let mut log = Vec::new();

    if version > CURRENT_VERSION {
        bail!(
            "Session version {} is newer than supported version {}. \
             Please update the application.",
            version,
            CURRENT_VERSION
        );
    }

    if version < MIN_SUPPORTED_VERSION {
        bail!(
            "Session version {} is too old to migrate (minimum: {}).",
            version,
            MIN_SUPPORTED_VERSION
        );
    }

    let migrations = all_migrations();

    while version < CURRENT_VERSION {
        let migration = migrations
            .iter()
            .find(|m| m.from_version == version)
            .with_context(|| format!("No migration found for version {}", version))?;

        data = (migration.migrate)(data).with_context(|| {
            format!(
                "Migration v{} → v{} failed: {}",
                version,
                version + 1,
                migration.description
            )
        })?;

        log.push(format!(
            "v{} → v{}: {}",
            version,
            version + 1,
            migration.description
        ));
        version += 1;
    }

    // Stamp the current version
    if let Some(obj) = data.as_object_mut() {
        obj.insert("version".to_string(), Value::from(CURRENT_VERSION));
    }

    Ok((data, log))
}

/// Check if a session needs migration.
pub fn needs_migration(data: &Value) -> bool {
    detect_version(data) < CURRENT_VERSION
}

// ---------------------------------------------------------------------------
// Migration implementations
// ---------------------------------------------------------------------------

/// V1 → V2: Add metadata object with cwd and timestamps.
fn migrate_v1_to_v2(mut data: Value) -> Result<Value> {
    if let Some(obj) = data.as_object_mut() {
        // Move top-level cwd into metadata if present
        let cwd = obj
            .remove("cwd")
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();

        let created_at = obj.get("created_at").and_then(|v| v.as_i64()).unwrap_or(0);

        let metadata = serde_json::json!({
            "cwd": cwd,
            "created_at": created_at,
            "format_version": 2,
        });

        obj.insert("metadata".to_string(), metadata);
        obj.insert("version".to_string(), Value::from(2u32));
    }
    Ok(data)
}

/// V2 → V3: Normalize message content blocks.
///
/// Ensures all messages use the array-of-blocks format for content,
/// converting bare strings to `[{ "type": "text", "text": "..." }]`.
fn migrate_v2_to_v3(mut data: Value) -> Result<Value> {
    if let Some(messages) = data.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            normalize_message_content(msg);
        }
    }

    if let Some(obj) = data.as_object_mut() {
        obj.insert("version".to_string(), Value::from(3u32));
    }
    Ok(data)
}

/// Normalize a single message's content field.
fn normalize_message_content(msg: &mut Value) {
    if let Some(content) = msg.get("content") {
        if content.is_string() {
            let text = content.as_str().unwrap_or("").to_string();
            if let Some(obj) = msg.as_object_mut() {
                obj.insert(
                    "content".to_string(),
                    serde_json::json!([{ "type": "text", "text": text }]),
                );
            }
        }
    }
}

/// V3 → V4: Introduce optional `custom_title` field for user-renamed sessions.
///
/// Sessions written before `/rename` existed did not carry the field; since
/// serde marks it `#[serde(default)]` on load, the on-disk file is already
/// forward-compatible. This migration simply stamps the format version so any
/// consumer inspecting the `version` field knows the file has been surveyed.
fn migrate_v3_to_v4(mut data: Value) -> Result<Value> {
    if let Some(obj) = data.as_object_mut() {
        obj.entry("custom_title".to_string()).or_insert(Value::Null);
        obj.insert("version".to_string(), Value::from(4u32));
    }
    Ok(data)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_version_explicit() {
        let data = json!({ "version": 3, "messages": [] });
        assert_eq!(detect_version(&data), 3);
    }

    #[test]
    fn test_detect_version_v2_heuristic() {
        let data = json!({ "metadata": { "cwd": "/tmp" }, "messages": [] });
        assert_eq!(detect_version(&data), 2);
    }

    #[test]
    fn test_detect_version_v1_heuristic() {
        let data = json!({ "session_id": "abc", "messages": [], "cwd": "/tmp" });
        assert_eq!(detect_version(&data), 1);
    }

    #[test]
    fn test_migrate_v1_to_v2() {
        let v1 = json!({
            "session_id": "test-123",
            "cwd": "/home/user/project",
            "created_at": 1700000000,
            "messages": []
        });

        let v2 = migrate_v1_to_v2(v1).unwrap();
        assert!(v2.get("metadata").is_some());
        assert_eq!(v2["metadata"]["cwd"], "/home/user/project");
        assert_eq!(v2["metadata"]["created_at"], 1700000000);
        // cwd should be removed from top level
        assert!(v2.get("cwd").is_none());
    }

    #[test]
    fn test_migrate_v2_to_v3_normalizes_string_content() {
        let v2 = json!({
            "version": 2,
            "metadata": {},
            "messages": [
                { "role": "user", "content": "hello world" },
                { "role": "assistant", "content": [{ "type": "text", "text": "hi" }] }
            ]
        });

        let v3 = migrate_v2_to_v3(v2).unwrap();
        let msgs = v3["messages"].as_array().unwrap();

        // First message should be normalized to array format
        assert!(msgs[0]["content"].is_array());
        assert_eq!(msgs[0]["content"][0]["text"], "hello world");

        // Second message should remain unchanged
        assert!(msgs[1]["content"].is_array());
        assert_eq!(msgs[1]["content"][0]["text"], "hi");
    }

    #[test]
    fn test_migrate_to_current_from_v1() {
        let v1 = json!({
            "session_id": "test",
            "cwd": "/proj",
            "created_at": 1700000000,
            "messages": [
                { "role": "user", "content": "test input" }
            ]
        });

        let (result, log) = migrate_to_current(v1).unwrap();
        assert_eq!(result["version"], CURRENT_VERSION);
        assert_eq!(log.len(), 3); // v1→v2, v2→v3, v3→v4
        assert!(result["messages"][0]["content"].is_array());
        assert!(result.get("custom_title").is_some());
    }

    #[test]
    fn test_migrate_v3_to_v4_adds_custom_title() {
        let v3 = json!({
            "version": 3,
            "session_id": "xyz",
            "messages": []
        });
        let v4 = migrate_v3_to_v4(v3).unwrap();
        assert_eq!(v4["version"], 4);
        assert!(v4.get("custom_title").is_some());
        assert!(v4["custom_title"].is_null());
    }

    #[test]
    fn test_migrate_v3_to_v4_preserves_existing_custom_title() {
        let v3 = json!({
            "version": 3,
            "messages": [],
            "custom_title": "My cool session"
        });
        let v4 = migrate_v3_to_v4(v3).unwrap();
        assert_eq!(v4["custom_title"], "My cool session");
    }

    #[test]
    fn test_migrate_already_current() {
        let current = json!({ "version": CURRENT_VERSION, "messages": [] });
        let (result, log) = migrate_to_current(current).unwrap();
        assert!(log.is_empty());
        assert_eq!(result["version"], CURRENT_VERSION);
    }

    #[test]
    fn test_migrate_future_version_fails() {
        let future = json!({ "version": 999, "messages": [] });
        assert!(migrate_to_current(future).is_err());
    }

    #[test]
    fn test_needs_migration() {
        assert!(needs_migration(
            &json!({ "session_id": "x", "messages": [] })
        ));
        assert!(!needs_migration(&json!({ "version": CURRENT_VERSION })));
    }
}
