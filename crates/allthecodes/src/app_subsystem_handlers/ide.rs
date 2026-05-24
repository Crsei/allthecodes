use allthecodes_ipc_protocol::subsystem_events::{IdeCommand, IdeEvent};
use allthecodes_ipc_protocol::BackendMessage;

use super::snapshot::build_ide_info_list;

/// Handle an IDE subsystem command from the frontend (issue #41).
///
/// - `Detect` / `QueryStatus` re-run detection and return the current list.
/// - `Select` / `Clear` persist the user's selection through `crate::ide`.
/// - `Reconnect` re-triggers a `ConnectionStateChanged` event so the MCP
///   manager notices the selection on its next discovery pass.
pub fn handle_ide_command(cmd: IdeCommand) -> Vec<BackendMessage> {
    match cmd {
        IdeCommand::Detect | IdeCommand::QueryStatus => {
            let ides = build_ide_info_list();
            vec![BackendMessage::IdeEvent {
                event: IdeEvent::IdeList { ides },
            }]
        }
        IdeCommand::Select { ide_id } => {
            tracing::info!(ide_id = %ide_id, "IDE select requested via IPC");
            match allthecodes_lsp_service::ide::select_ide(&ide_id) {
                Ok(()) => {
                    let ides = build_ide_info_list();
                    vec![BackendMessage::IdeEvent {
                        event: IdeEvent::IdeList { ides },
                    }]
                }
                Err(e) => vec![BackendMessage::SystemInfo {
                    text: format!("IDE select failed: {}", e),
                    level: "error".to_string(),
                }],
            }
        }
        IdeCommand::Clear => {
            tracing::info!("IDE selection clear requested via IPC");
            match allthecodes_lsp_service::ide::clear_selection() {
                Ok(()) => {
                    let ides = build_ide_info_list();
                    vec![BackendMessage::IdeEvent {
                        event: IdeEvent::IdeList { ides },
                    }]
                }
                Err(e) => vec![BackendMessage::SystemInfo {
                    text: format!("IDE clear failed: {}", e),
                    level: "error".to_string(),
                }],
            }
        }
        IdeCommand::Reconnect => {
            tracing::info!("IDE reconnect requested via IPC");
            match allthecodes_lsp_service::ide::reconnect_selected() {
                Ok(()) => vec![BackendMessage::SystemInfo {
                    text: "IDE reconnect scheduled".to_string(),
                    level: "info".to_string(),
                }],
                Err(e) => vec![BackendMessage::SystemInfo {
                    text: format!("IDE reconnect failed: {}", e),
                    level: "error".to_string(),
                }],
            }
        }
    }
}
