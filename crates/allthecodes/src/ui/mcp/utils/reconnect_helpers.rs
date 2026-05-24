//! Helpers for presenting MCP reconnect attempts.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectAttempt {
    pub server_name: String,
    pub attempt: usize,
    pub max_attempts: usize,
    pub last_error: Option<String>,
}

pub fn reconnect_label(attempt: &ReconnectAttempt) -> String {
    let mut label = format!(
        "Reconnecting {} ({}/{})",
        attempt.server_name, attempt.attempt, attempt.max_attempts
    );
    if let Some(error) = &attempt.last_error {
        label.push_str(&format!(" - {error}"));
    }
    label
}
