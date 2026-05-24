//! Shell permission feedback state.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellPermissionFeedback {
    pub command: String,
    pub accepted: bool,
    pub rule_saved: bool,
    pub message: String,
}

pub fn render_shell_permission_feedback(feedback: &ShellPermissionFeedback) -> String {
    let status = if feedback.accepted {
        "accepted"
    } else {
        "denied"
    };
    let saved = if feedback.rule_saved {
        "rule saved"
    } else {
        "one-shot"
    };
    format!(
        "{status}: {}\n{saved}\n{}",
        feedback.command, feedback.message
    )
}
