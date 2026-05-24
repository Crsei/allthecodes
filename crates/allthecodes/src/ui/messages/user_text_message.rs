//! Rust-side helper for plain user text messages.
//!
//! Routes raw user text to the appropriate rendering path based on content
//! tags (XML-style markers embedded in the text). Handles 15+ subtypes
//! including bash I/O, commands, memory, interrupt, MCP, and more.

use crate::ui::theme::Theme;

/// Routing result for a user text message.
#[derive(Debug, Clone)]
pub enum UserTextRendered {
    /// Fully rendered output string.
    Rendered(String),
    /// This message should be hidden (not displayed).
    Hidden,
    /// Should be delegated to another renderer with a routing key.
    Delegated(&'static str, String),
}

/// Special messages that indicate no content.
const NO_CONTENT_MESSAGE: &str = "[NO_CONTENT]";
const INTERRUPT_MESSAGE: &str = "[Request interrupted by user]";
pub const CONVERSATION_INTERRUPTED_MESSAGE: &str =
    "■ Conversation interrupted - tell the model what to do differently. Something went wrong? Hit `/feedback` to report the issue.";

/// Route a user text message to the appropriate rendering path.
///
/// TS reference: `UserTextMessage.tsx:37-172`
pub fn route_user_text(text: &str) -> UserTextRendered {
    let trimmed = text.trim();

    // 1. Empty / no-content check
    if trimmed.is_empty() || trimmed == NO_CONTENT_MESSAGE {
        return UserTextRendered::Hidden;
    }

    // 2. Interrupt message (check early since it's common)
    if trimmed == CONVERSATION_INTERRUPTED_MESSAGE {
        return UserTextRendered::Rendered(CONVERSATION_INTERRUPTED_MESSAGE.to_string());
    }
    if trimmed.contains(INTERRUPT_MESSAGE) {
        return UserTextRendered::Rendered("[Request interrupted by user]".to_string());
    }

    // 3. LocalCommandCaveat — hidden (system-injected warning caveat)
    if trimmed.contains("<local-command-caveat>") {
        return UserTextRendered::Hidden;
    }

    // 4. Tick tag — hidden (periodic tick messages)
    if trimmed.contains("<tick>") {
        return UserTextRendered::Hidden;
    }

    // 5. Bash tool output
    if trimmed.starts_with("<bash-stdout") || trimmed.starts_with("<bash-stderr") {
        return UserTextRendered::Delegated("bash_output", trimmed.to_string());
    }

    // 6. Local command output
    if trimmed.starts_with("<local-command-stdout") || trimmed.starts_with("<local-command-stderr")
    {
        return UserTextRendered::Delegated("local_command_output", trimmed.to_string());
    }

    // 7. Bash input
    if trimmed.contains("<bash-input>") {
        return UserTextRendered::Delegated("bash_input", trimmed.to_string());
    }

    // 8. Command message
    if trimmed.contains("<command-message>") {
        return UserTextRendered::Delegated("command", trimmed.to_string());
    }

    // 9. User memory input
    if trimmed.contains("<user-memory-input>") {
        return UserTextRendered::Delegated("memory_input", trimmed.to_string());
    }

    // 10. Task notification
    if trimmed.contains("<task-notification>") {
        return UserTextRendered::Delegated("agent_notification", trimmed.to_string());
    }

    // 11. MCP resource update or polling update
    if trimmed.contains("<mcp-resource-update") || trimmed.contains("<mcp-polling-update") {
        return UserTextRendered::Delegated("resource_update", trimmed.to_string());
    }

    // 12. Fork boilerplate
    if trimmed.contains("<fork-boilerplate>") {
        return UserTextRendered::Delegated("fork_boilerplate", trimmed.to_string());
    }

    // 13. Default — treat as a user prompt
    UserTextRendered::Delegated("prompt", trimmed.to_string())
}

/// Render a user text message.
///
/// Routes the text to the appropriate handler and returns the rendered output.
pub fn render_user_text_message(text: &str, _theme: &Theme) -> String {
    match route_user_text(text) {
        UserTextRendered::Hidden => String::new(),
        UserTextRendered::Rendered(s) => s,
        UserTextRendered::Delegated(kind, content) => {
            if kind == "prompt" {
                content
            } else {
                format!("[{kind}]: {content}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_hidden() {
        assert!(matches!(route_user_text(""), UserTextRendered::Hidden));
    }

    #[test]
    fn no_content_is_hidden() {
        assert!(matches!(
            route_user_text("[NO_CONTENT]"),
            UserTextRendered::Hidden
        ));
    }

    #[test]
    fn interrupt_message_rendered() {
        match route_user_text("[Request interrupted by user]") {
            UserTextRendered::Rendered(s) => assert_eq!(s, "[Request interrupted by user]"),
            _ => panic!("expected Rendered"),
        }
    }

    #[test]
    fn conversation_interrupted_message_rendered() {
        match route_user_text(CONVERSATION_INTERRUPTED_MESSAGE) {
            UserTextRendered::Rendered(s) => assert_eq!(s, CONVERSATION_INTERRUPTED_MESSAGE),
            _ => panic!("expected Rendered"),
        }
    }

    #[test]
    fn local_command_caveat_hidden() {
        assert!(matches!(
            route_user_text("<local-command-caveat>some warning"),
            UserTextRendered::Hidden
        ));
    }

    #[test]
    fn tick_hidden() {
        assert!(matches!(
            route_user_text("<tick>heartbeat"),
            UserTextRendered::Hidden
        ));
    }

    #[test]
    fn bash_stdout_routed() {
        match route_user_text("<bash-stdout>output here") {
            UserTextRendered::Delegated("bash_output", _) => {}
            _ => panic!("expected Delegated(bash_output)"),
        }
    }

    #[test]
    fn bash_stderr_routed() {
        match route_user_text("<bash-stderr>error output") {
            UserTextRendered::Delegated("bash_output", _) => {}
            _ => panic!("expected Delegated(bash_output)"),
        }
    }

    #[test]
    fn local_command_stdout_routed() {
        match route_user_text("<local-command-stdout>output") {
            UserTextRendered::Delegated("local_command_output", _) => {}
            _ => panic!("expected Delegated(local_command_output)"),
        }
    }

    #[test]
    fn bash_input_routed() {
        match route_user_text("something <bash-input>cmd</bash-input>") {
            UserTextRendered::Delegated("bash_input", _) => {}
            _ => panic!("expected Delegated(bash_input)"),
        }
    }

    #[test]
    fn command_message_routed() {
        match route_user_text("content <command-message>/help") {
            UserTextRendered::Delegated("command", _) => {}
            _ => panic!("expected Delegated(command)"),
        }
    }

    #[test]
    fn memory_input_routed() {
        match route_user_text("data <user-memory-input>note") {
            UserTextRendered::Delegated("memory_input", _) => {}
            _ => panic!("expected Delegated(memory_input)"),
        }
    }

    #[test]
    fn task_notification_routed() {
        match route_user_text("<task-notification>task completed") {
            UserTextRendered::Delegated("agent_notification", _) => {}
            _ => panic!("expected Delegated(agent_notification)"),
        }
    }

    #[test]
    fn mcp_resource_update_routed() {
        match route_user_text("<mcp-resource-update>resource changed") {
            UserTextRendered::Delegated("resource_update", _) => {}
            _ => panic!("expected Delegated(resource_update)"),
        }
    }

    #[test]
    fn mcp_polling_update_routed() {
        match route_user_text("<mcp-polling-update>poll result") {
            UserTextRendered::Delegated("resource_update", _) => {}
            _ => panic!("expected Delegated(resource_update)"),
        }
    }

    #[test]
    fn fork_boilerplate_routed() {
        match route_user_text("<fork-boilerplate>context") {
            UserTextRendered::Delegated("fork_boilerplate", _) => {}
            _ => panic!("expected Delegated(fork_boilerplate)"),
        }
    }

    #[test]
    fn default_is_prompt() {
        match route_user_text("What is the weather?") {
            UserTextRendered::Delegated("prompt", _) => {}
            other => panic!("expected Delegated(prompt), got {other:?}"),
        }
    }

    #[test]
    fn render_empty_returns_empty_string() {
        assert_eq!(render_user_text_message("", &Theme::default()), "");
    }

    #[test]
    fn render_interrupt_returns_formatted() {
        let result = render_user_text_message("[Request interrupted by user]", &Theme::default());
        assert_eq!(result, "[Request interrupted by user]");
    }

    #[test]
    fn render_conversation_interrupted_returns_formatted() {
        let result = render_user_text_message(CONVERSATION_INTERRUPTED_MESSAGE, &Theme::default());
        assert_eq!(result, CONVERSATION_INTERRUPTED_MESSAGE);
    }
}
