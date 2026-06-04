use std::future::Future;

use allthecodes_ipc_protocol::subsystem_events::{LspCommand, LspEvent};
use allthecodes_ipc_protocol::subsystem_types::*;
use allthecodes_ipc_protocol::BackendMessage;

fn lsp_diagnostic_to_ipc(diagnostic: allthecodes_lsp_service::LspDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: DiagnosticRange {
            start_line: diagnostic.range.start_line,
            start_character: diagnostic.range.start_character,
            end_line: diagnostic.range.end_line,
            end_character: diagnostic.range.end_character,
        },
        severity: diagnostic.severity,
        message: diagnostic.message,
        source: diagnostic.source,
        code: diagnostic.code,
    }
}

fn lsp_document_change_from_ipc(
    change: allthecodes_ipc_protocol::DocumentChange,
) -> allthecodes_lsp_service::DocumentChange {
    allthecodes_lsp_service::DocumentChange {
        range: allthecodes_lsp_service::SourceRange {
            start_line: change.range.start_line,
            start_character: change.range.start_character,
            end_line: change.range.end_line,
            end_character: change.range.end_character,
        },
        range_length: change.range_length,
        text: change.text,
    }
}

// Used by snapshot.rs — kept pub(super) to avoid code duplication.
pub(super) fn lsp_server_info_to_ipc(
    info: allthecodes_lsp_service::LspServerInfo,
) -> LspServerInfo {
    LspServerInfo {
        language_id: info.language_id,
        state: info.state,
        extensions: info.extensions,
        open_files_count: info.open_files_count,
        error: info.error,
    }
}

/// Handle an LSP subsystem command from the frontend.
///
/// Read-only snapshots are returned immediately. Live editor operations are
/// scheduled onto the active Tokio runtime and report results through the LSP
/// subsystem event bus, so the headless loop can keep processing stdin while
/// language servers start, sync documents, or compute completions.
pub fn handle_lsp_command(cmd: LspCommand) -> Vec<BackendMessage> {
    match cmd {
        LspCommand::StartServer { language_id } => {
            tracing::info!(language_id = %language_id, "LSP start requested via IPC");
            spawn_lsp_task("start_server", None, async move {
                allthecodes_lsp_service::start_server(&language_id).await
            })
        }
        LspCommand::StopServer { language_id } => {
            tracing::info!(language_id = %language_id, "LSP stop requested via IPC");
            spawn_lsp_task("stop_server", None, async move {
                allthecodes_lsp_service::stop_server(&language_id).await
            })
        }
        LspCommand::RestartServer { language_id } => {
            tracing::info!(language_id = %language_id, "LSP restart requested via IPC");
            spawn_lsp_task("restart_server", None, async move {
                allthecodes_lsp_service::restart_server(&language_id).await
            })
        }
        LspCommand::QueryStatus => {
            let servers = super::snapshot::build_lsp_server_info_list();
            vec![BackendMessage::LspEvent {
                event: LspEvent::ServerList { servers },
            }]
        }
        LspCommand::QueryDiagnostics { uri } => {
            let entries = allthecodes_lsp_service::diagnostics_snapshot(uri.as_deref())
                .into_iter()
                .map(|(uri, diagnostics)| LspDiagnosticSnapshotEntry {
                    uri,
                    diagnostics: diagnostics.into_iter().map(lsp_diagnostic_to_ipc).collect(),
                })
                .collect();
            vec![BackendMessage::LspEvent {
                event: LspEvent::DiagnosticsSnapshot { entries },
            }]
        }
        LspCommand::OpenDocument {
            uri,
            language_id,
            text,
        } => spawn_lsp_task("open_document", None, async move {
            allthecodes_lsp_service::open_document(&uri, language_id, text)
                .await
                .map(|_| ())
        }),
        LspCommand::ChangeDocument {
            uri,
            version,
            text,
            changes,
        } => spawn_lsp_task("change_document", None, async move {
            let changes = changes
                .into_iter()
                .map(lsp_document_change_from_ipc)
                .collect();
            allthecodes_lsp_service::change_document(&uri, text, changes, version)
                .await
                .map(|_| ())
        }),
        LspCommand::SaveDocument { uri, text } => {
            spawn_lsp_task("save_document", None, async move {
                allthecodes_lsp_service::save_document(&uri, text)
                    .await
                    .map(|_| ())
            })
        }
        LspCommand::CloseDocument { uri } => spawn_lsp_task("close_document", None, async move {
            allthecodes_lsp_service::close_document(&uri)
                .await
                .map(|_| ())
        }),
        LspCommand::Completion {
            request_id,
            uri,
            line,
            character,
            trigger_character,
        } => {
            let request_id_for_error = request_id.clone();
            spawn_lsp_task("completion", Some(request_id_for_error), async move {
                let items = allthecodes_lsp_service::completion(
                    &uri,
                    line.saturating_sub(1),
                    character.saturating_sub(1),
                    trigger_character,
                )
                .await?;
                allthecodes_lsp_service::emit_event(
                    allthecodes_lsp_service::LspEvent::CompletionResults {
                        request_id,
                        uri,
                        items,
                    },
                );
                Ok(())
            })
        }
        LspCommand::QuerySettings => {
            let settings = load_lsp_recommendation_settings();
            vec![BackendMessage::LspEvent {
                event: LspEvent::SettingsSnapshot { settings },
            }]
        }
        LspCommand::RecommendationResponse {
            request_id,
            plugin_name,
            decision,
        } => {
            tracing::info!(
                request_id = %request_id,
                plugin_name = %plugin_name,
                decision = %decision,
                "LSP recommendation response"
            );
            let (settings, info_text) = apply_recommendation_decision(&plugin_name, &decision);
            let mut msgs = Vec::with_capacity(2);
            msgs.push(BackendMessage::LspEvent {
                event: LspEvent::SettingsSnapshot { settings },
            });
            if let Some(text) = info_text {
                msgs.push(BackendMessage::SystemInfo {
                    text,
                    level: "info".to_string(),
                });
            }
            msgs
        }
        LspCommand::UnmutePlugin { plugin_name } => {
            let settings = unmute_lsp_plugin(&plugin_name);
            vec![BackendMessage::LspEvent {
                event: LspEvent::SettingsSnapshot { settings },
            }]
        }
        LspCommand::SetRecommendationsDisabled { disabled } => {
            let settings = set_lsp_recommendations_disabled(disabled);
            vec![BackendMessage::LspEvent {
                event: LspEvent::SettingsSnapshot { settings },
            }]
        }
    }
}

fn spawn_lsp_task<F>(
    operation: &'static str,
    request_id: Option<String>,
    future: F,
) -> Vec<BackendMessage>
where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(err) => {
            return vec![BackendMessage::SystemInfo {
                text: format!("LSP {operation} could not run without an async runtime: {err}"),
                level: "error".to_string(),
            }];
        }
    };

    handle.spawn(async move {
        if let Err(err) = future.await {
            tracing::warn!(operation, error = %err, "LSP IPC command failed");
            allthecodes_lsp_service::emit_event(allthecodes_lsp_service::LspEvent::CommandError {
                request_id,
                message: format!("{operation}: {err}"),
            });
        }
    });

    vec![BackendMessage::SystemInfo {
        text: format!("Queued LSP {operation}."),
        level: "info".to_string(),
    }]
}

// ---------------------------------------------------------------------------
// LSP recommendation settings persistence
// ---------------------------------------------------------------------------

/// Key inside the user-level `settings.json` object that holds the
/// `LspRecommendationSettings` payload.
const LSP_RECOMMENDATIONS_KEY: &str = "lspRecommendations";

/// Read the `lspRecommendations` block from the user settings file.
///
/// Returns a `Default` value when the file is missing, the key is
/// absent, or the stored value fails to deserialize — a corrupt entry
/// should never brick the prompt pipeline.
pub fn load_lsp_recommendation_settings() -> LspRecommendationSettings {
    let path = allthecodes_config::settings::user_settings_path();
    let Ok(value) = super::mcp_config::read_settings_value(&path) else {
        return LspRecommendationSettings::default();
    };
    value
        .get(LSP_RECOMMENDATIONS_KEY)
        .and_then(|v| serde_json::from_value::<LspRecommendationSettings>(v.clone()).ok())
        .unwrap_or_default()
}

/// Persist `settings` under the `lspRecommendations` key, preserving every
/// other field in the user settings file.
fn save_lsp_recommendation_settings(settings: &LspRecommendationSettings) {
    let path = allthecodes_config::settings::user_settings_path();
    let mut value = super::mcp_config::read_settings_value(&path).unwrap_or_else(|err| {
        tracing::warn!(error = %err, "LSP recommendations: read user settings failed; overwriting with fresh object");
        serde_json::Value::Object(serde_json::Map::new())
    });
    if !value.is_object() {
        value = serde_json::Value::Object(serde_json::Map::new());
    }
    let encoded = match serde_json::to_value(settings) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(error = %err, "LSP recommendations: serialize failed");
            return;
        }
    };
    if let Some(object) = value.as_object_mut() {
        object.insert(LSP_RECOMMENDATIONS_KEY.to_string(), encoded);
    }
    if let Err(err) = super::mcp_config::write_settings_value(&path, &value) {
        tracing::warn!(error = %err, "LSP recommendations: write user settings failed");
    }
}

/// Apply a user decision from an [`LspEvent::RecommendationRequest`] prompt.
///
/// Returns the updated settings snapshot plus an optional info-level
/// message the frontend can surface in its system log. `yes` attempts the
/// plugin install immediately; `no` is transient, while `never` and `disable`
/// are sticky.
fn apply_recommendation_decision(
    plugin_name: &str,
    decision: &str,
) -> (LspRecommendationSettings, Option<String>) {
    let mut settings = load_lsp_recommendation_settings();
    let info = match decision {
        "yes" => Some(match install_lsp_recommendation_plugin(plugin_name) {
            Ok(summary) => summary,
            Err(error) => format!("Failed to install LSP plugin '{}': {}", plugin_name, error),
        }),
        "no" => None,
        "never" => {
            if !settings.muted_plugins.iter().any(|p| p == plugin_name) {
                settings.muted_plugins.push(plugin_name.to_string());
                save_lsp_recommendation_settings(&settings);
            }
            Some(format!(
                "Muted LSP recommendation for '{}'. Run /lsp to undo.",
                plugin_name
            ))
        }
        "disable" => {
            if !settings.disabled {
                settings.disabled = true;
                save_lsp_recommendation_settings(&settings);
            }
            Some("Disabled all LSP plugin recommendations. Run /lsp to re-enable.".to_string())
        }
        other => {
            tracing::warn!(decision = %other, "LSP recommendations: unknown decision value");
            None
        }
    };
    (settings, info)
}

fn install_lsp_recommendation_plugin(plugin_name: &str) -> anyhow::Result<String> {
    let source = plugin_name.to_string();
    let policy = crate::command_runtime_bridge::managed_policy_for_commands();
    let (available_plugins, all_manifests) =
        crate::command_runtime_bridge::plugin_dependency_context();
    let result = crate::command_runtime_bridge::block_on_in_worker(async move {
        allthecodes_plugins::installation::install_plugin(
            &source,
            None,
            Some(env!("CARGO_PKG_VERSION")),
            policy.as_ref(),
            &available_plugins,
            &all_manifests,
        )
        .await
        .map_err(|error| anyhow::anyhow!("{}", error))
    })?;
    Ok(format!(
        "Installed LSP plugin '{}' v{}.",
        result.plugin.name, result.plugin.version
    ))
}

/// Remove `plugin_name` from the muted list and persist the result.
fn unmute_lsp_plugin(plugin_name: &str) -> LspRecommendationSettings {
    let mut settings = load_lsp_recommendation_settings();
    let before = settings.muted_plugins.len();
    settings.muted_plugins.retain(|p| p != plugin_name);
    if settings.muted_plugins.len() != before {
        save_lsp_recommendation_settings(&settings);
    }
    settings
}

/// Flip the global "disable all recommendations" switch.
fn set_lsp_recommendations_disabled(disabled: bool) -> LspRecommendationSettings {
    let mut settings = load_lsp_recommendation_settings();
    if settings.disabled != disabled {
        settings.disabled = disabled;
        save_lsp_recommendation_settings(&settings);
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_lsp_server_info_list_returns_configured_servers() {
        let infos = super::super::snapshot::build_lsp_server_info_list();
        assert!(infos.len() >= 6);
        let rust = infos.iter().find(|i| i.language_id == "rust");
        assert!(rust.is_some());
        assert_eq!(rust.unwrap().state, "not_started");
    }

    #[test]
    fn build_lsp_server_info_list_has_dotted_extensions() {
        let infos = super::super::snapshot::build_lsp_server_info_list();
        let rust = infos.iter().find(|i| i.language_id == "rust").unwrap();
        assert!(
            rust.extensions.contains(&".rs".to_string()),
            "extensions should be dot-prefixed"
        );
    }

    #[test]
    fn handle_lsp_query_status_returns_server_list() {
        let msgs = handle_lsp_command(LspCommand::QueryStatus);
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], BackendMessage::LspEvent { .. }));
    }

    #[test]
    fn handle_lsp_start_returns_info() {
        let msgs = handle_lsp_command(LspCommand::StartServer {
            language_id: "rust".into(),
        });
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], BackendMessage::SystemInfo { .. }));
    }

    #[test]
    fn apply_recommendation_decision_no_is_noop() {
        let (_settings, info) = apply_recommendation_decision("foo-ls", "no");
        assert!(info.is_none(), "'no' should not produce an info message");
    }

    #[test]
    fn apply_recommendation_decision_unknown_is_warned_but_silent() {
        let (_settings, info) = apply_recommendation_decision("foo-ls", "banana");
        assert!(info.is_none());
    }

    #[test]
    fn apply_recommendation_decision_yes_attempts_install() {
        let (_settings, info) = apply_recommendation_decision("rust-analyzer", "yes");
        assert!(info.is_some());
        let text = info.unwrap();
        assert!(text.contains("rust-analyzer"));
        assert!(
            text.contains("Installed LSP plugin") || text.contains("Failed to install LSP plugin")
        );
    }
}
