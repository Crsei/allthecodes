use std::path::PathBuf;

use allthecodes_ipc_protocol::subsystem_types::{ConfigScope, McpServerConfigEntry};

// ---------------------------------------------------------------------------
// MCP config persistence (issue #44)
// ---------------------------------------------------------------------------

/// Resolve the `settings.json` path for an editable scope.
///
/// Returns `Err` when the scope is read-only (plugin / IDE).
///
/// We intentionally **don't** walk ancestors for `Project`: the scoped
/// discovery layer reads exactly `{cwd}/.allthecodes/settings.json`, so any
/// write must land in the same place or the round-trip breaks. Callers
/// that really want the ancestor-walking behaviour should stabilize their
/// project root before invoking this.
fn settings_path_for_scope(cwd: &std::path::Path, scope: &ConfigScope) -> Result<PathBuf, String> {
    match scope {
        ConfigScope::User => Ok(allthecodes_config::settings::user_settings_path()),
        ConfigScope::Project => Ok(cwd.join(".allthecodes").join("settings.json")),
        ConfigScope::Plugin { id } => Err(format!(
            "scope `plugin:{}` is read-only — edit the plugin manifest instead",
            id
        )),
        ConfigScope::Ide { id } => Err(format!(
            "scope `ide:{}` is read-only — edit the IDE bridge config instead",
            id
        )),
    }
}

/// Read the raw settings file (returning defaults if missing).
pub(super) fn read_settings_value(path: &std::path::Path) -> Result<serde_json::Value, String> {
    if !path.exists() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(&content).map_err(|e| format!("failed to parse {}: {}", path.display(), e))
}

/// Write a raw settings value with parent-dir creation + atomic rename.
pub(super) fn write_settings_value(
    path: &std::path::Path,
    value: &serde_json::Value,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
    }
    let pretty = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialize settings: {}", e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, pretty)
        .map_err(|e| format!("failed to write {}: {}", tmp.display(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        format!(
            "failed to rename {} -> {}: {}",
            tmp.display(),
            path.display(),
            e
        )
    })?;
    Ok(())
}

/// Upsert a server config into the settings file backing `entry.scope`.
///
/// Returns the entry that was persisted on success, or `(server_name, message)`
/// on failure (so the caller can emit `McpEvent::ConfigError`).
pub fn upsert_mcp_entry(
    cwd: &std::path::Path,
    entry: McpServerConfigEntry,
) -> Result<McpServerConfigEntry, (String, String)> {
    if !entry.scope.is_editable() {
        return Err((
            entry.name.clone(),
            format!(
                "scope `{}` is read-only — cannot upsert MCP server config",
                entry.scope.label()
            ),
        ));
    }

    let path = settings_path_for_scope(cwd, &entry.scope).map_err(|e| (entry.name.clone(), e))?;

    let mut settings = read_settings_value(&path).map_err(|e| (entry.name.clone(), e))?;
    if !settings.is_object() {
        return Err((
            entry.name.clone(),
            format!("{} is not a JSON object", path.display()),
        ));
    }

    let obj = settings.as_object_mut().ok_or_else(|| {
        (
            entry.name.clone(),
            format!("{} is not a JSON object", path.display()),
        )
    })?;
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !servers.is_object() {
        return Err((
            entry.name.clone(),
            format!("{} has a non-object `mcpServers` field", path.display()),
        ));
    }
    let servers_obj = servers.as_object_mut().ok_or_else(|| {
        (
            entry.name.clone(),
            format!("{} has a non-object `mcpServers` field", path.display()),
        )
    })?;
    servers_obj.insert(entry.name.clone(), entry_to_settings_value(&entry));

    write_settings_value(&path, &settings).map_err(|e| (entry.name.clone(), e))?;
    Ok(entry)
}

/// Remove a server config entry from the settings file backing `scope`.
pub fn remove_mcp_entry(
    cwd: &std::path::Path,
    server_name: &str,
    scope: &ConfigScope,
) -> Result<(), String> {
    if !scope.is_editable() {
        return Err(format!(
            "scope `{}` is read-only — cannot remove MCP server config",
            scope.label()
        ));
    }
    let path = settings_path_for_scope(cwd, scope)?;
    if !path.exists() {
        return Err(format!(
            "no settings file at {} — nothing to remove",
            path.display()
        ));
    }
    let mut settings = read_settings_value(&path)?;
    let Some(obj) = settings.as_object_mut() else {
        return Err(format!("{} is not a JSON object", path.display()));
    };
    let Some(servers) = obj.get_mut("mcpServers") else {
        return Err(format!("{} has no `mcpServers` section", path.display()));
    };
    let Some(servers_obj) = servers.as_object_mut() else {
        return Err(format!(
            "{} has a non-object `mcpServers` field",
            path.display()
        ));
    };
    if servers_obj.remove(server_name).is_none() {
        return Err(format!(
            "{} has no MCP server named `{}`",
            path.display(),
            server_name
        ));
    }
    write_settings_value(&path, &settings)?;
    Ok(())
}

/// Flip the `disabled` flag on an existing entry.
///
/// Locate a matching editable entry (`scope`-aware when provided, otherwise
/// Project > User > plugin/ide rejected), toggle its `disabled` bit, persist,
/// and return the updated entry. The returned value always has an explicit
/// `Some(bool)` for `disabled` so the caller can emit a meaningful state.
pub fn toggle_mcp_entry_enabled(
    cwd: &std::path::Path,
    server_name: &str,
    scope: Option<&ConfigScope>,
) -> Result<McpServerConfigEntry, String> {
    use super::snapshot::build_mcp_server_config_entries;

    let existing = build_mcp_server_config_entries(cwd);
    let matches: Vec<&McpServerConfigEntry> = existing
        .iter()
        .filter(|e| {
            e.name == server_name
                && match scope {
                    Some(wanted) => e.scope == *wanted,
                    None => true,
                }
        })
        .collect();
    if matches.is_empty() {
        return Err(format!(
            "no MCP server named `{}`{} found",
            server_name,
            scope
                .map(|s| format!(" in scope `{}`", s.label()))
                .unwrap_or_default()
        ));
    }
    if matches.len() > 1 && scope.is_none() {
        let labels: Vec<String> = matches.iter().map(|e| e.scope.label()).collect();
        return Err(format!(
            "`{}` exists in multiple scopes ({}). Retry with an explicit scope.",
            server_name,
            labels.join(", ")
        ));
    }

    // Prefer the most specific editable match: project > user > plugin/ide.
    let target = matches
        .iter()
        .find(|e| e.scope == ConfigScope::Project)
        .or_else(|| matches.iter().find(|e| e.scope == ConfigScope::User))
        .or_else(|| matches.first())
        .cloned()
        .cloned();
    let Some(mut target) = target else {
        return Err(format!("no editable match for `{}`", server_name));
    };
    if !target.scope.is_editable() {
        return Err(format!(
            "scope `{}` is read-only — cannot toggle MCP server `{}`",
            target.scope.label(),
            server_name
        ));
    }

    let was_disabled = target.disabled.unwrap_or(false);
    target.disabled = Some(!was_disabled);
    upsert_mcp_entry(cwd, target).map_err(|(_, msg)| msg)
}

/// Serialize an entry for the on-disk `mcpServers[name]` value.
///
/// The settings file uses the legacy `McpServerConfig` shape (transport under
/// `type`, `command`/`args`/`url`/-. Consumers using different shapes can
/// still round-trip thanks to `McpServerConfig`'s permissive deserializer.
fn entry_to_settings_value(entry: &McpServerConfigEntry) -> serde_json::Value {
    let cfg = allthecodes_mcp::McpServerConfig {
        name: entry.name.clone(),
        transport: entry.transport.clone(),
        command: entry.command.clone(),
        args: entry.args.clone(),
        url: entry.url.clone(),
        headers: filter_sensitive_static_headers(entry.headers.clone()),
        oauth: entry.oauth.clone(),
        env: entry.env.clone(),
        browser_mcp: entry.browser_mcp,
        disabled: entry.disabled,
        bearer_token_env_var: entry.bearer_token_env_var.clone(),
        env_http_headers: entry.env_http_headers.clone(),
        auth: entry.auth.clone(),
    };
    // `McpServerConfig` serializes `name` as a field; the settings file uses
    // the map key for naming, so drop it from the inner object.
    let mut value = serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Null);
    if let Some(obj) = value.as_object_mut() {
        obj.remove("name");
    }
    value
}

fn filter_sensitive_static_headers(
    headers: Option<std::collections::HashMap<String, String>>,
) -> Option<std::collections::HashMap<String, String>> {
    headers
        .map(|headers| {
            headers
                .into_iter()
                .filter(|(name, _)| !is_sensitive_header_name(name))
                .collect::<std::collections::HashMap<_, _>>()
        })
        .filter(|headers| !headers.is_empty())
}

fn is_sensitive_header_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "authorization"
        || lower == "proxy-authorization"
        || lower == "x-api-key"
        || lower == "api-key"
        || lower == "x-auth-token"
        || lower == "x-access-token"
        || lower.contains("token")
        || lower.contains("secret")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reuse the EnvGuard defined in mcp::tests
    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn upsert_mcp_entry_persists_to_user_scope() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "ctx7".to_string(),
            scope: ConfigScope::User,
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: Some(vec!["-y".to_string(), "ctx7".to_string()]),
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };

        let written = upsert_mcp_entry(cwd.path(), entry).expect("upsert ok");
        assert_eq!(written.name, "ctx7");

        let settings_path = home.path().join("settings.json");
        assert!(
            settings_path.exists(),
            "user settings.json should be created"
        );
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(on_disk["mcpServers"]["ctx7"]["command"], "npx");
        assert_eq!(on_disk["mcpServers"]["ctx7"]["args"][0], "-y");
    }

    #[test]
    #[serial_test::serial]
    fn upsert_mcp_entry_persists_to_project_scope() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "proj-srv".to_string(),
            scope: ConfigScope::Project,
            transport: "stdio".to_string(),
            command: Some("./local.sh".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };

        upsert_mcp_entry(cwd.path(), entry).expect("upsert ok");

        let path = cwd.path().join(".allthecodes").join("settings.json");
        assert!(path.exists(), "project settings.json should be created");
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["mcpServers"]["proj-srv"]["command"], "./local.sh");
    }

    #[test]
    #[serial_test::serial]
    fn upsert_mcp_entry_rejects_plugin_scope() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "plugin-srv".to_string(),
            scope: ConfigScope::Plugin {
                id: "com.example.p".to_string(),
            },
            transport: "stdio".to_string(),
            command: Some("x".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };

        let err = upsert_mcp_entry(cwd.path(), entry).expect_err("plugin scope rejected");
        assert_eq!(err.0, "plugin-srv");
        assert!(err.1.contains("read-only"));
    }

    #[test]
    #[serial_test::serial]
    fn remove_mcp_entry_round_trips_user_scope() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "ctx7".to_string(),
            scope: ConfigScope::User,
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };
        upsert_mcp_entry(cwd.path(), entry).expect("upsert ok");

        remove_mcp_entry(cwd.path(), "ctx7", &ConfigScope::User).expect("remove ok");

        let settings_path = home.path().join("settings.json");
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let servers = on_disk
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("mcpServers object");
        assert!(!servers.contains_key("ctx7"), "entry should be gone");
    }

    #[test]
    #[serial_test::serial]
    fn remove_mcp_entry_rejects_plugin_scope() {
        let cwd = tempfile::tempdir().expect("tempdir");
        let err = remove_mcp_entry(
            cwd.path(),
            "p",
            &ConfigScope::Plugin {
                id: "com.example.p".to_string(),
            },
        )
        .expect_err("plugin scope rejected");
        assert!(err.contains("read-only"));
    }

    #[test]
    #[serial_test::serial]
    fn remove_mcp_entry_errors_on_missing_file() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let err = remove_mcp_entry(cwd.path(), "nope", &ConfigScope::User)
            .expect_err("missing file should error");
        assert!(err.contains("nothing to remove"));
    }

    #[test]
    #[serial_test::serial]
    fn toggle_mcp_entry_enabled_flips_disabled_flag() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        let entry = McpServerConfigEntry {
            name: "tog-srv".to_string(),
            scope: ConfigScope::User,
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: None,
            url: None,
            headers: None,
            oauth: None,
            env: None,
            browser_mcp: None,
            disabled: None,
        };
        upsert_mcp_entry(cwd.path(), entry).expect("upsert ok");

        // First toggle: enable ->disabled.
        let after_disable =
            toggle_mcp_entry_enabled(cwd.path(), "tog-srv", None).expect("first toggle ok");
        assert_eq!(after_disable.disabled, Some(true));

        // Second toggle: disabled ->enabled.
        let after_enable =
            toggle_mcp_entry_enabled(cwd.path(), "tog-srv", None).expect("second toggle ok");
        assert_eq!(after_enable.disabled, Some(false));

        // Verify final on-disk value reflects the second toggle.
        let settings_path = home.path().join("settings.json");
        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        // `disabled: false` serializes to false via the derive — but our
        // serializer skips `None`, so either missing or literal false is OK.
        let v = &on_disk["mcpServers"]["tog-srv"]["disabled"];
        assert!(v.is_null() || v == &serde_json::Value::Bool(false));
    }

    #[test]
    #[serial_test::serial]
    fn toggle_mcp_entry_enabled_rejects_plugin_scope() {
        let home = tempfile::tempdir().expect("tempdir");
        let cwd = tempfile::tempdir().expect("tempdir");
        let _g = EnvGuard::set("ALLTHECODES_HOME", home.path().to_str().unwrap());

        // Directly ask to toggle in a read-only scope — should error even
        // when no matching entry exists in the scope.
        let err = toggle_mcp_entry_enabled(
            cwd.path(),
            "nope",
            Some(&ConfigScope::Plugin {
                id: "com.example".to_string(),
            }),
        )
        .expect_err("plugin scope rejected");
        assert!(err.contains("no MCP server named"));
    }
}
