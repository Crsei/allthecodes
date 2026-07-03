//! ACP slash command advertising.
//!
//! Builds ACP `available_commands_update` from the allthecodes command registry
//! and filters hidden commands. Sent after session/new, session/load, and session/resume.

use agent_client_protocol_schema::v2::{AvailableCommand, AvailableCommandsUpdate, SessionUpdate};
use allthecodes_commands::is_hidden_command;

/// Build an ACP `available_commands_update` notification.
///
/// Filters hidden commands. Returns `None` when there are no visible commands
/// (defensive – should not happen in practice).
pub fn build_commands_update() -> Option<AvailableCommandsUpdate> {
    let metadata = allthecodes_commands::runtime::command_metadata_snapshot();
    let visible: Vec<AvailableCommand> = metadata
        .into_iter()
        .filter(|cmd| !is_hidden_command(&cmd.name))
        .map(|cmd| {
            let display_name = if cmd.name.starts_with('/') {
                cmd.name.clone()
            } else {
                format!("/{}", cmd.name)
            };
            let meta = if cmd.aliases.is_empty() {
                None
            } else {
                Some(serde_json::json!({
                    "allthecodes": {
                        "aliases": cmd.aliases,
                    }
                }))
            };
            let mut acp_cmd = AvailableCommand::new(display_name, cmd.description);
            if let Some(m) = meta {
                acp_cmd = serde_json::from_value(serde_json::json!({
                    "name": acp_cmd.name,
                    "description": acp_cmd.description,
                    "_meta": m,
                }))
                .unwrap_or(acp_cmd);
            }
            acp_cmd
        })
        .collect();

    if visible.is_empty() {
        None
    } else {
        Some(AvailableCommandsUpdate::new(visible))
    }
}

/// Wrap an `AvailableCommandsUpdate` into a `SessionUpdate` notification.
pub fn commands_session_update() -> Option<SessionUpdate> {
    build_commands_update().map(SessionUpdate::AvailableCommandsUpdate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_commands_update_contains_visible_commands() {
        let update = build_commands_update();
        assert!(update.is_some());
        let update = update.unwrap();
        assert!(!update.available_commands.is_empty());
        // All commands should start with "/"
        for cmd in &update.available_commands {
            assert!(cmd.name.starts_with('/'), "command '{}' should start with /", cmd.name);
        }
    }

    #[test]
    fn hidden_commands_are_not_advertised() {
        let update = build_commands_update();
        assert!(update.is_some());
        let update = update.unwrap();
        // Verify hidden commands are filtered out
        for cmd in &update.available_commands {
            let bare_name = cmd.name.trim_start_matches('/');
            assert!(!is_hidden_command(bare_name),
                "hidden command '{}' was advertised", bare_name);
        }
    }
}
