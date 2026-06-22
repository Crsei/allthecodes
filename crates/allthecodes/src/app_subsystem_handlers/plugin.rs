use allthecodes_ipc_protocol::subsystem_events::{PluginCommand, PluginEvent};
use allthecodes_ipc_protocol::BackendMessage;

use super::snapshot::build_plugin_info_list;

/// Handle a plugin subsystem command from the frontend.
///
/// Enable/disable are deferred to the `/plugin` slash command.
/// `QueryStatus` returns the full plugin list.
pub fn handle_plugin_command(cmd: PluginCommand) -> Vec<BackendMessage> {
    match cmd {
        PluginCommand::Enable { plugin_id } => {
            tracing::info!(plugin_id = %plugin_id, "Plugin enable requested via IPC");
            vec![BackendMessage::SystemInfo {
                text: format!(
                    "Use /plugin to manage plugins. To enable {}, run: /plugin enable {}",
                    plugin_id, plugin_id
                ),
                level: "info".to_string(),
            }]
        }
        PluginCommand::Disable { plugin_id } => {
            tracing::info!(plugin_id = %plugin_id, "Plugin disable requested via IPC");
            vec![BackendMessage::SystemInfo {
                text: format!(
                    "Use /plugin to manage plugins. To disable {}, run: /plugin disable {}",
                    plugin_id, plugin_id
                ),
                level: "info".to_string(),
            }]
        }
        PluginCommand::QueryStatus => {
            let plugins = build_plugin_info_list();
            vec![BackendMessage::PluginEvent {
                event: PluginEvent::PluginList { plugins },
            }]
        }
        PluginCommand::Reload => {
            tracing::info!("Plugin reload requested via IPC");
            let report = allthecodes_plugins::reload_plugins();
            vec![BackendMessage::PluginEvent {
                event: PluginEvent::Reloaded {
                    count: report.count,
                    had_error: report.had_error(),
                },
            }]
        }
        PluginCommand::Uninstall {
            plugin_id,
            purge_cache,
        } => {
            tracing::info!(
                plugin_id = %plugin_id,
                purge_cache,
                "Plugin uninstall requested via IPC"
            );
            match allthecodes_plugins::uninstall_plugin(&plugin_id, purge_cache) {
                Ok(Some(entry)) => vec![BackendMessage::PluginEvent {
                    event: PluginEvent::StatusChanged {
                        plugin_id: entry.id.clone(),
                        name: entry.name.clone(),
                        status: "not_installed".to_string(),
                        error: None,
                    },
                }],
                Ok(None) => vec![BackendMessage::SystemInfo {
                    text: format!("Plugin '{}' is not installed.", plugin_id),
                    level: "warn".to_string(),
                }],
                Err(e) => vec![BackendMessage::SystemInfo {
                    text: format!("Failed to uninstall '{}': {}", plugin_id, e),
                    level: "error".to_string(),
                }],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_plugin_query_status_returns_plugin_list() {
        let msgs = handle_plugin_command(PluginCommand::QueryStatus);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], BackendMessage::PluginEvent { .. }));
    }

    #[test]
    fn build_plugin_info_list_maps_status() {
        use super::super::snapshot::build_plugin_info_list;

        allthecodes_plugins::clear_plugins();
        allthecodes_plugins::register_plugin(allthecodes_plugins::PluginEntry {
            id: "test-plugin-handlers".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "For testing".to_string(),
            source: allthecodes_plugins::PluginSource::Local {
                path: "/tmp/test".to_string(),
            },
            status: allthecodes_plugins::PluginStatus::Installed,
            marketplace: None,
            cache_path: None,
            installed_version: Some("1.0.0".to_string()),
            official: false,
            download_url: None,
            homepage: None,
            sha256: None,
            tools: vec!["tool_a".to_string()],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        });
        allthecodes_plugins::register_plugin(allthecodes_plugins::PluginEntry {
            id: "err-plugin-handlers".to_string(),
            name: "Error Plugin".to_string(),
            version: "0.1.0".to_string(),
            description: "Broken".to_string(),
            source: allthecodes_plugins::PluginSource::Local {
                path: "/tmp/err".to_string(),
            },
            status: allthecodes_plugins::PluginStatus::Error("load failed".to_string()),
            marketplace: None,
            cache_path: None,
            installed_version: Some("0.1.0".to_string()),
            official: false,
            download_url: None,
            homepage: None,
            sha256: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
            installed_at: None,
            updated_at: None,
        });

        let infos = build_plugin_info_list();
        let test_p = infos.iter().find(|p| p.id == "test-plugin-handlers");
        assert!(test_p.is_some());
        assert_eq!(test_p.unwrap().status, "installed");
        assert!(test_p.unwrap().error.is_none());

        let err_p = infos.iter().find(|p| p.id == "err-plugin-handlers");
        assert!(err_p.is_some());
        assert_eq!(err_p.unwrap().status, "error");
        assert_eq!(err_p.unwrap().error.as_deref(), Some("load failed"));

        allthecodes_plugins::clear_plugins();
    }
}
