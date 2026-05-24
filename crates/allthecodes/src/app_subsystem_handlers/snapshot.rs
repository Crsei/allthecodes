use std::path::Path;

use allthecodes_ipc_protocol::subsystem_types::*;
use allthecodes_mcp::discovery::DiscoveryScope;

use super::lsp::lsp_server_info_to_ipc;

// ===========================================================================
// Status snapshot builders
// ===========================================================================

/// Build a list of LSP server info from the default server configurations.
pub fn build_lsp_server_info_list() -> Vec<LspServerInfo> {
    allthecodes_lsp_service::server_info_snapshot()
        .into_iter()
        .map(lsp_server_info_to_ipc)
        .collect()
}

/// Build a list of MCP server status info from discovered configurations.
pub fn build_mcp_server_info_list() -> Vec<McpServerStatusInfo> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    build_mcp_server_info_list_for_cwd(&cwd)
}

pub fn build_mcp_server_info_list_for_cwd(cwd: &Path) -> Vec<McpServerStatusInfo> {
    let (configs, mut diagnostics) = match discover_mcp_runtime_configs_with_diagnostics(cwd) {
        Ok(discovery) => discovery,
        Err(err) => return vec![mcp_discovery_error_status(err)],
    };
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        if let Ok(manager) = manager.try_lock() {
            let mut rows = build_mcp_server_info_list_from_configs(configs, Some(&manager));
            rows.append(&mut diagnostics);
            return rows;
        }
    }

    let mut rows = build_mcp_server_info_list_from_configs(configs, None);
    rows.append(&mut diagnostics);
    rows
}

pub async fn build_mcp_server_info_list_for_cwd_async(cwd: &Path) -> Vec<McpServerStatusInfo> {
    let (configs, mut diagnostics) = match discover_mcp_runtime_configs_with_diagnostics(cwd) {
        Ok(discovery) => discovery,
        Err(err) => return vec![mcp_discovery_error_status(err)],
    };
    if let Some(manager) = allthecodes_mcp::runtime::current_manager() {
        let manager = manager.lock().await;
        let mut rows = build_mcp_server_info_list_from_configs(configs, Some(&manager));
        rows.append(&mut diagnostics);
        return rows;
    }

    let mut rows = build_mcp_server_info_list_from_configs(configs, None);
    rows.append(&mut diagnostics);
    rows
}

fn mcp_discovery_error_status(err: anyhow::Error) -> McpServerStatusInfo {
    McpServerStatusInfo {
        name: "discovery".to_string(),
        state: "error".to_string(),
        transport: "settings".to_string(),
        tools_count: 0,
        resources_count: 0,
        server_info: None,
        instructions: None,
        error: Some(format!("Failed to discover MCP servers: {err:#}")),
    }
}

fn discover_mcp_runtime_configs_with_diagnostics(
    cwd: &Path,
) -> anyhow::Result<(
    Vec<allthecodes_mcp::McpServerConfig>,
    Vec<McpServerStatusInfo>,
)> {
    let scoped = allthecodes_mcp::discovery::discover_mcp_servers_scoped(cwd)?;
    let mut configs: Vec<allthecodes_mcp::McpServerConfig> = Vec::new();
    let mut diagnostics = Vec::new();

    for entry in scoped {
        if let Some(error) = entry.error {
            diagnostics.push(McpServerStatusInfo {
                name: entry.config.name,
                state: "error".to_string(),
                transport: entry.config.transport,
                tools_count: 0,
                resources_count: 0,
                server_info: None,
                instructions: None,
                error: Some(format!(
                    "{} scope: {}",
                    scope_from_discovery(&entry.scope).label(),
                    error
                )),
            });
            continue;
        }

        if let Some(existing) = configs
            .iter_mut()
            .find(|config| config.name == entry.config.name)
        {
            *existing = entry.config;
        } else {
            configs.push(entry.config);
        }
    }

    Ok((configs, diagnostics))
}

fn build_mcp_server_info_list_from_configs(
    configs: Vec<allthecodes_mcp::McpServerConfig>,
    manager: Option<&allthecodes_mcp::manager::McpManager>,
) -> Vec<McpServerStatusInfo> {
    configs
        .into_iter()
        .map(|cfg| build_mcp_server_info(cfg, manager))
        .collect()
}

fn build_mcp_server_info(
    cfg: allthecodes_mcp::McpServerConfig,
    manager: Option<&allthecodes_mcp::manager::McpManager>,
) -> McpServerStatusInfo {
    if cfg.disabled.unwrap_or(false) {
        return McpServerStatusInfo {
            name: cfg.name,
            state: "disabled".to_string(),
            transport: cfg.transport,
            tools_count: 0,
            resources_count: 0,
            server_info: None,
            instructions: None,
            error: None,
        };
    }

    if let Some(client) = manager.and_then(|manager| manager.clients.get(&cfg.name)) {
        let (state, error) = match &client.state {
            allthecodes_mcp::McpConnectionState::Pending => ("pending".to_string(), None),
            allthecodes_mcp::McpConnectionState::Connected => ("connected".to_string(), None),
            allthecodes_mcp::McpConnectionState::Disconnected => ("disconnected".to_string(), None),
            allthecodes_mcp::McpConnectionState::Error(error) => {
                ("error".to_string(), Some(error.clone()))
            }
        };
        let server_info = (!client.server_info.name.is_empty()).then(|| McpServerInfoBrief {
            name: client.server_info.name.clone(),
            version: client.server_info.version.clone(),
        });
        return McpServerStatusInfo {
            name: cfg.name,
            state,
            transport: cfg.transport,
            tools_count: client.tools.len(),
            resources_count: client.resources.len(),
            server_info,
            instructions: client.instructions.clone(),
            error,
        };
    }

    let remembered = allthecodes_mcp::runtime::server_state(&cfg.name);
    McpServerStatusInfo {
        name: cfg.name,
        state: remembered
            .as_ref()
            .map(|state| state.state.clone())
            .unwrap_or_else(|| "pending".to_string()),
        transport: cfg.transport,
        tools_count: 0,
        resources_count: 0,
        server_info: None,
        instructions: None,
        error: remembered.and_then(|state| state.error),
    }
}

/// Build a list of editable config entries (issue #44) from scope-aware
/// discovery. Unlike [`build_mcp_server_info_list`] this preserves one row
/// per scope so the same logical server can appear in multiple scopes (e.g.
/// "same name in user + project").
pub fn build_mcp_server_config_entries(cwd: &std::path::Path) -> Vec<McpServerConfigEntry> {
    let scoped = match allthecodes_mcp::discovery::discover_mcp_servers_scoped(cwd) {
        Ok(scoped) => scoped,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to discover scoped MCP server configs");
            return vec![McpServerConfigEntry {
                name: "discovery".to_string(),
                transport: "settings".to_string(),
                command: None,
                args: None,
                url: None,
                headers: None,
                oauth: None,
                env: None,
                browser_mcp: None,
                disabled: Some(true),
                scope: ConfigScope::User,
            }];
        }
    };
    scoped
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
        })
        .collect()
}

/// Map the discovery-layer `DiscoveryScope` onto the IPC `ConfigScope`.
fn scope_from_discovery(scope: &DiscoveryScope) -> ConfigScope {
    match scope {
        DiscoveryScope::User => ConfigScope::User,
        DiscoveryScope::Project => ConfigScope::Project,
        DiscoveryScope::Plugin(id) => ConfigScope::Plugin { id: id.clone() },
        DiscoveryScope::Ide(id) => ConfigScope::Ide { id: id.clone() },
    }
}

/// Build a list of plugin info from the in-memory plugin registry.
pub fn build_plugin_info_list() -> Vec<PluginInfo> {
    use allthecodes_plugins::PluginStatus;

    allthecodes_plugins::get_all_plugins()
        .into_iter()
        .map(|p| {
            let (status_str, error) = match &p.status {
                PluginStatus::NotInstalled => ("not_installed".to_string(), None),
                PluginStatus::Installed => ("installed".to_string(), None),
                PluginStatus::Disabled => ("disabled".to_string(), None),
                PluginStatus::Error(e) => ("error".to_string(), Some(e.clone())),
            };
            PluginInfo {
                id: p.id,
                name: p.name,
                version: p.version,
                status: status_str,
                contributed_tools: p.tools,
                contributed_skills: p.skills,
                contributed_mcp_servers: p.mcp_servers,
                error,
            }
        })
        .collect()
}

/// Discover plugin-contributed skill definitions for skill reload.
pub(super) fn discover_plugin_skills_for_handlers() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();

    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                tracing::warn!(
                    plugin = %contributed.plugin_id,
                    path = %contributed.path.display(),
                    "Plugin: failed to load contributed skill file"
                );
                continue;
            }
        };

        skill.name = contributed.name;
        if let Some(desc) = contributed.description {
            if !desc.trim().is_empty() {
                skill.frontmatter.description = desc;
            }
        }
        out.push(skill);
    }

    out
}

/// Build a list of skill info from the global skill registry.
pub fn build_skill_info_list() -> Vec<SkillInfo> {
    use allthecodes_skills::SkillSource;

    allthecodes_skills::get_all_skills()
        .into_iter()
        .map(|s| {
            let source_str = match &s.source {
                SkillSource::Bundled => "bundled".to_string(),
                SkillSource::User => "user".to_string(),
                SkillSource::Project => "project".to_string(),
                SkillSource::Plugin(_) => "plugin".to_string(),
                SkillSource::Mcp(_) => "mcp".to_string(),
            };
            SkillInfo {
                name: s.display_name().to_string(),
                source: source_str,
                description: s.frontmatter.description.clone(),
                user_invocable: s.is_user_invocable(),
                model_invocable: s.is_model_invocable(),
            }
        })
        .collect()
}

/// Build the list of detected IDE integrations (issue #41).
///
/// Thin wrapper around [`allthecodes_lsp_service::ide::detect_ides`] that exists primarily
/// so the IPC layer has a stable entry point we can hook from other
/// places (e.g. the future `/ide` TUI view) without reaching into the
/// `ide` module.
pub fn build_ide_info_list() -> Vec<IdeInfo> {
    allthecodes_lsp_service::ide::detect_ides()
}

/// Build a complete subsystem status snapshot combining all subsystems.
pub fn build_subsystem_status_snapshot() -> SubsystemStatusSnapshot {
    SubsystemStatusSnapshot {
        lsp: build_lsp_server_info_list(),
        mcp: build_mcp_server_info_list(),
        plugins: build_plugin_info_list(),
        skills: build_skill_info_list(),
        ides: build_ide_info_list(),
        timestamp: chrono::Utc::now().timestamp(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_subsystem_status_snapshot_has_timestamp() {
        let snapshot = build_subsystem_status_snapshot();
        assert!(snapshot.timestamp > 0, "timestamp should be positive");
        assert!(snapshot.lsp.len() >= 6);
    }
}
