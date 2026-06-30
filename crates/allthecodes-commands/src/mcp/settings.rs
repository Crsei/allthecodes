use std::path::PathBuf;

use anyhow::Result;

use allthecodes_ipc_protocol::subsystem_types::{ConfigScope, McpServerConfigEntry};
use allthecodes_mcp::McpServerConfig;

// ---------------------------------------------------------------------------
// Misc helpers — shared between subcommands
// ---------------------------------------------------------------------------

pub(super) fn describe_entry(entry: &McpServerConfigEntry) -> String {
    let mut parts = Vec::new();
    parts.push(format!("transport={}", entry.transport));
    if let Some(cmd) = &entry.command {
        if let Some(args) = &entry.args {
            parts.push(format!("command=\"{} {}\"", cmd, args.join(" ")));
        } else {
            parts.push(format!("command=\"{}\"", cmd));
        }
    }
    if let Some(url) = &entry.url {
        parts.push(format!("url=\"{}\"", url));
    }
    if entry.oauth.is_some() {
        parts.push("oauth=configured".to_string());
    }
    if entry.bearer_token_env_var.is_some() {
        parts.push("bearerTokenEnvVar=set".to_string());
    }
    if entry.env_http_headers.is_some() {
        parts.push("envHttpHeaders=set".to_string());
    }
    if let Some(auth) = &entry.auth {
        parts.push(format!("auth={}", auth));
    }
    if let Some(env) = &entry.env {
        if !env.is_empty() {
            let mut keys: Vec<&String> = env.keys().collect();
            keys.sort();
            parts.push(format!(
                "env=[{}]",
                keys.into_iter().cloned().collect::<Vec<_>>().join(",")
            ));
        }
    }
    parts.join(" ")
}

pub(super) fn discover_config_entries(cwd: &std::path::Path) -> Vec<McpServerConfigEntry> {
    match allthecodes_mcp::discovery::discover_mcp_servers_scoped(cwd) {
        Ok(scoped) => scoped
            .into_iter()
            .map(|s| McpServerConfigEntry {
                name: s.config.name,
                scope: scope_from_discovery(&s.scope),
                transport: s.config.transport,
                command: s.config.command,
                args: s.config.args,
                url: s.config.url,
                headers: s.config.headers,
                oauth: s.config.oauth,
                env: s.config.env,
                browser_mcp: s.config.browser_mcp,
                disabled: s.config.disabled,
                bearer_token_env_var: s.config.bearer_token_env_var,
                env_http_headers: s.config.env_http_headers,
                auth: s.config.auth,
            })
            .collect(),
        Err(err) => {
            tracing::warn!(error = %err, "Failed to discover scoped MCP server configs");
            vec![McpServerConfigEntry {
                name: "discovery".to_string(),
                scope: ConfigScope::User,
                transport: "settings".to_string(),
                command: None,
                args: None,
                url: None,
                headers: None,
                oauth: None,
                env: None,
                browser_mcp: None,
                disabled: Some(true),
                bearer_token_env_var: None,
                env_http_headers: None,
                auth: None,
            }]
        }
    }
}

fn scope_from_discovery(scope: &allthecodes_mcp::discovery::DiscoveryScope) -> ConfigScope {
    match scope {
        allthecodes_mcp::discovery::DiscoveryScope::User => ConfigScope::User,
        allthecodes_mcp::discovery::DiscoveryScope::Project => ConfigScope::Project,
        allthecodes_mcp::discovery::DiscoveryScope::Plugin(id) => {
            ConfigScope::Plugin { id: id.clone() }
        }
        allthecodes_mcp::discovery::DiscoveryScope::Ide(id) => ConfigScope::Ide { id: id.clone() },
    }
}

pub(super) fn read_settings_value(path: &std::path::Path) -> Result<serde_json::Value> {
    if !path.exists() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    Ok(serde_json::from_str(&content)?)
}

pub(super) fn write_settings_value(
    path: &std::path::Path,
    value: &serde_json::Value,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pretty = serde_json::to_string_pretty(value)?;
    let tmp: PathBuf = path.with_extension("json.tmp");
    std::fs::write(&tmp, pretty)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(super) fn persist_mcpjson_decision(
    cwd: &std::path::Path,
    names: &[String],
    approve: bool,
    all_project: bool,
) -> Result<String> {
    let path = cwd.join(".allthecodes").join("settings.json");
    let mut value = read_settings_value(&path)?;
    let obj = match value.as_object_mut() {
        Some(obj) => obj,
        None => {
            return Ok(format!("{} is not a JSON object", path.display()));
        }
    };

    if approve {
        add_string_array_entries(obj, "enabledMcpjsonServers", names);
        remove_string_array_entries(obj, "disabledMcpjsonServers", names);
        if all_project {
            obj.insert(
                "enableAllProjectMcpServers".to_string(),
                serde_json::Value::Bool(true),
            );
        }
    } else {
        add_string_array_entries(obj, "disabledMcpjsonServers", names);
        remove_string_array_entries(obj, "enabledMcpjsonServers", names);
    }

    write_settings_value(&path, &value)?;

    let verb = if approve { "Approved" } else { "Rejected" };
    let mut suffix = String::new();
    if approve && all_project {
        suffix.push_str("\n  Future project MCP servers will be approved automatically.");
    }
    Ok(format!(
        "{} .mcp.json server(s) `{}` in project scope (at {}).{}",
        verb,
        names.join("`, `"),
        path.display(),
        suffix
    ))
}

fn add_string_array_entries(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    names: &[String],
) {
    let value = obj
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if !value.is_array() {
        *value = serde_json::Value::Array(Vec::new());
    }
    let Some(array) = value.as_array_mut() else {
        return;
    };
    for name in names {
        if !array.iter().any(|item| item.as_str() == Some(name)) {
            array.push(serde_json::Value::String(name.clone()));
        }
    }
}

fn remove_string_array_entries(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    names: &[String],
) {
    let Some(array) = obj.get_mut(key).and_then(|value| value.as_array_mut()) else {
        return;
    };
    array.retain(|item| {
        item.as_str()
            .map(|name| !names.iter().any(|blocked| blocked == name))
            .unwrap_or(true)
    });
}

pub(super) fn entry_to_settings_value(entry: &McpServerConfigEntry) -> serde_json::Value {
    let cfg = McpServerConfig {
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
    let mut v = serde_json::to_value(&cfg).unwrap_or(serde_json::Value::Null);
    if let Some(obj) = v.as_object_mut() {
        obj.remove("name");
    }
    v
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
