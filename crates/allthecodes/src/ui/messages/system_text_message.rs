//! Rust-side helper for system text messages.
//!
//! Classifies raw `(tag, message)` pairs into structured subtypes
//! and renders each with appropriate formatting. Supports 11+ subtypes
//! matching the TS `SystemTextMessage.tsx` dispatch.

use crate::ui::theme::Theme;

/// Parsed subtypes for system messages, derived from the `tag` string.
///
/// Mirrors the TS `SystemTextMessage.tsx:44-149` subtype switch.
#[derive(Debug, Clone)]
pub enum SystemTagKind {
    /// Turn duration summary with optional budget tracking.
    TurnDuration {
        duration_ms: u64,
        budget_limit: Option<u64>,
        budget_tokens: Option<u64>,
        budget_nudges: Option<u32>,
    },
    /// Memory write confirmation with list of written paths.
    MemorySaved {
        written_paths: Vec<String>,
        verb: Option<String>,
    },
    /// Away/takeover summary from another session.
    AwaySummary(String),
    /// All background agents have been killed.
    AgentsKilled,
    /// Thinking content from extended thinking.
    Thinking(String),
    /// Bridge/remote-control status.
    BridgeStatus {
        url: String,
        upgrade_nudge: Option<String>,
    },
    /// A scheduled task fired.
    ScheduledTaskFire(String),
    /// Permission retry notification listing granted commands.
    PermissionRetry(Vec<String>),
    /// API error with retry information.
    ApiError {
        retry_attempt: u32,
        error: String,
        retry_in_ms: u64,
        max_retries: u32,
    },
    /// Stop-hook summary with hook details and possible prevention.
    StopHookSummary {
        hook_count: usize,
        hook_infos: Vec<String>,
        hook_errors: Vec<String>,
        prevented_continuation: bool,
        stop_reason: Option<String>,
        total_duration_ms: u64,
    },
    /// Generic informational message with severity level.
    Generic { content: String, level: String },
}

/// Parse a turn duration message string.
///
/// Expected format (JSON or simple):
/// `{"duration_ms": 12345, "budget_limit": 60000, ...}`
fn parse_turn_duration(message: &str) -> SystemTagKind {
    let default = SystemTagKind::TurnDuration {
        duration_ms: message.parse().unwrap_or(0),
        budget_limit: None,
        budget_tokens: None,
        budget_nudges: None,
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(message) {
        if let Some(obj) = value.as_object() {
            return SystemTagKind::TurnDuration {
                duration_ms: obj.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                budget_limit: obj.get("budget_limit").and_then(|v| v.as_u64()),
                budget_tokens: obj.get("budget_tokens").and_then(|v| v.as_u64()),
                budget_nudges: obj
                    .get("budget_nudges")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
            };
        }
    }
    default
}

/// Parse a memory saved message string.
fn parse_memory_saved(message: &str) -> SystemTagKind {
    let parts: Vec<&str> = message.splitn(2, '\n').collect();
    let first_line = parts.first().copied().unwrap_or("");
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(message) {
        if let Some(obj) = value.as_object() {
            let paths: Vec<String> = obj
                .get("written_paths")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let verb = obj
                .get("verb")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned);
            return SystemTagKind::MemorySaved {
                written_paths: paths,
                verb,
            };
        }
    }

    SystemTagKind::MemorySaved {
        written_paths: first_line.lines().map(ToOwned::to_owned).collect(),
        verb: parts.get(1).map(|value| (*value).to_string()),
    }
}

/// Parse a permission retry message.
fn parse_permission_retry(message: &str) -> SystemTagKind {
    let commands: Vec<String> = message
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    SystemTagKind::PermissionRetry(commands)
}

/// Parse an API error message.
fn parse_api_error(message: &str) -> SystemTagKind {
    let (retry_attempt, rest) = if let Some(rest) = message.strip_prefix("retry_attempt=") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let attempt: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
        (attempt, parts.get(1).copied().unwrap_or(""))
    } else {
        (1, message)
    };

    SystemTagKind::ApiError {
        retry_attempt,
        error: rest.trim().to_string(),
        retry_in_ms: 0,
        max_retries: 3,
    }
}

/// Parse a stop hook summary message.
fn parse_stop_hook_summary(_message: &str) -> SystemTagKind {
    SystemTagKind::StopHookSummary {
        hook_count: 0,
        hook_infos: Vec::new(),
        hook_errors: Vec::new(),
        prevented_continuation: false,
        stop_reason: None,
        total_duration_ms: 0,
    }
}

/// Parse a bridge status message.
fn parse_bridge_status(message: &str) -> SystemTagKind {
    let parts: Vec<&str> = message.splitn(2, '\n').collect();
    let url = parts.first().copied().unwrap_or("").to_string();
    let upgrade_nudge = parts.get(1).map(|value| (*value).to_string());
    SystemTagKind::BridgeStatus { url, upgrade_nudge }
}

/// Classify a raw `(tag, message)` pair into a structured subtype.
///
/// TS reference: `SystemTextMessage.tsx:44-149`
pub fn classify_system_message(tag: &str, message: &str) -> SystemTagKind {
    let tag = tag.trim();
    match tag {
        "turn_duration" => parse_turn_duration(message),
        "memory_saved" => parse_memory_saved(message),
        "away_summary" => SystemTagKind::AwaySummary(message.trim().to_string()),
        "agents_killed" => SystemTagKind::AgentsKilled,
        "thinking" => SystemTagKind::Thinking(message.trim().to_string()),
        "bridge_status" => parse_bridge_status(message),
        "scheduled_task_fire" => SystemTagKind::ScheduledTaskFire(message.trim().to_string()),
        "permission_retry" => parse_permission_retry(message),
        "api_error" => parse_api_error(message),
        "stop_hook_summary" => parse_stop_hook_summary(message),
        _ => SystemTagKind::Generic {
            content: message.trim().to_string(),
            level: tag.to_string(),
        },
    }
}

/// Render a turn duration message.
///
/// TS reference: `SystemTextMessage.tsx:288-339`
fn render_turn_duration(kind: &SystemTagKind) -> String {
    match kind {
        SystemTagKind::TurnDuration {
            duration_ms,
            budget_limit,
            budget_tokens,
            budget_nudges,
        } => {
            let mut parts = vec![format!("∗ Worked for {}ms", duration_ms)];
            if let Some(limit) = budget_limit {
                parts.push(format!("budget: {limit}ms"));
            }
            if let Some(tokens) = budget_tokens {
                parts.push(format!("tokens: {tokens}"));
            }
            if let Some(nudges) = budget_nudges {
                if *nudges > 0 {
                    parts.push(format!("{nudges} nudges"));
                }
            }
            parts.join(" · ")
        }
        _ => String::new(),
    }
}

/// Render a memory saved confirmation.
///
/// TS reference: `SystemTextMessage.tsx:341-384`
fn render_memory_saved(kind: &SystemTagKind) -> String {
    match kind {
        SystemTagKind::MemorySaved {
            written_paths,
            verb,
        } => {
            if written_paths.is_empty() {
                return "● Saved memories".to_string();
            }
            let verb_display = verb.as_deref().unwrap_or("Saved");
            let mut lines = vec![format!("● {verb_display} {} memories", written_paths.len())];
            for path in written_paths {
                let basename = path.rsplit('/').next().unwrap_or(path);
                lines.push(format!("  {basename}"));
            }
            lines.join("\n")
        }
        _ => String::new(),
    }
}

/// Render a stop hook summary.
///
/// TS reference: `SystemTextMessage.tsx:151-252`
fn render_stop_hook_summary(kind: &SystemTagKind) -> String {
    match kind {
        SystemTagKind::StopHookSummary {
            hook_count,
            hook_infos,
            hook_errors,
            prevented_continuation,
            stop_reason,
            total_duration_ms,
        } => {
            let mut lines = vec![format!(
                "● Ran {} hooks ({}ms)",
                hook_count, total_duration_ms
            )];
            for info in hook_infos {
                lines.push(format!("  ⎿ {info}"));
            }
            if let Some(reason) = stop_reason {
                lines.push(format!("  ⎿ stopped: {reason}"));
            }
            if *prevented_continuation {
                lines.push("  ⎿ prevented continuation".to_string());
            }
            for err in hook_errors {
                lines.push(format!("  ⎿ hook error: {err}"));
            }
            lines.join("\n")
        }
        _ => String::new(),
    }
}

/// Render an API error message.
///
/// TS reference: `SystemAPIErrorMessage.tsx:42-53`
fn render_api_error_msg(kind: &SystemTagKind) -> String {
    match kind {
        SystemTagKind::ApiError {
            retry_attempt,
            error,
            retry_in_ms,
            max_retries,
        } => {
            let error_preview = if error.len() > 1000 {
                format!("{}...", &error[..997])
            } else {
                error.to_string()
            };
            if *retry_in_ms > 0 {
                format!(
                    "Error occurred: {} · Retrying in {}ms (attempt {}/{})",
                    error_preview, retry_in_ms, retry_attempt, max_retries
                )
            } else {
                format!("Error occurred: {error_preview}")
            }
        }
        _ => String::new(),
    }
}

/// Render a system text message.
///
/// Classifies the `(tag, message)` pair and renders the appropriate
/// formatted output. Unknown tags fall through to generic rendering
/// based on tag-as-level.
pub fn render_system_text_message(tag: &str, message: &str, _theme: &Theme) -> String {
    let tag = tag.trim();
    let message = message.trim();

    if message.is_empty() && tag.is_empty() {
        return "System message".to_string();
    }

    let kind = classify_system_message(tag, message);

    match &kind {
        SystemTagKind::TurnDuration { .. } => render_turn_duration(&kind),
        SystemTagKind::MemorySaved { .. } => render_memory_saved(&kind),
        SystemTagKind::StopHookSummary { .. } => render_stop_hook_summary(&kind),
        SystemTagKind::ApiError { .. } => render_api_error_msg(&kind),
        SystemTagKind::AwaySummary(content) => format!("※ {content}"),
        SystemTagKind::AgentsKilled => "● All background agents stopped".to_string(),
        SystemTagKind::Thinking(content) => format!("∗ {content}"),
        SystemTagKind::BridgeStatus { url, upgrade_nudge } => {
            let mut msg = format!("/remote-control is active. Code in CLI or at {url}");
            if let Some(nudge) = upgrade_nudge {
                msg.push_str(&format!(" · {nudge}"));
            }
            msg
        }
        SystemTagKind::ScheduledTaskFire(content) => format!("⁭ {content}"),
        SystemTagKind::PermissionRetry(commands) => {
            format!("⁭ Allowed: {}", commands.join(", "))
        }
        SystemTagKind::Generic { content, level } => match level.as_str() {
            "" | "info" => content.to_string(),
            "warning" => format!("● {content}"),
            "error" => format!("● {content}"),
            _ => format!("System({level}): {content}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tag_and_message() {
        assert_eq!(
            render_system_text_message("", "", &Theme::default()),
            "System message"
        );
    }

    #[test]
    fn unknown_tag_falls_through() {
        let result = render_system_text_message("info", "hello world", &Theme::default());
        assert_eq!(result, "hello world");
    }

    #[test]
    fn away_summary_renders() {
        let result =
            render_system_text_message("away_summary", "Session taken over", &Theme::default());
        assert_eq!(result, "※ Session taken over");
    }

    #[test]
    fn agents_killed_renders() {
        let result = render_system_text_message("agents_killed", "", &Theme::default());
        assert_eq!(result, "● All background agents stopped");
    }

    #[test]
    fn thinking_renders() {
        let result = render_system_text_message("thinking", "Processing deeply", &Theme::default());
        assert_eq!(result, "∗ Processing deeply");
    }

    #[test]
    fn bridge_status_renders() {
        let result = render_system_text_message(
            "bridge_status",
            "https://claude.ai/code",
            &Theme::default(),
        );
        assert!(result.contains("/remote-control is active"));
        assert!(result.contains("https://claude.ai/code"));
    }

    #[test]
    fn scheduled_task_fire_renders() {
        let result =
            render_system_text_message("scheduled_task_fire", "Review PR #42", &Theme::default());
        assert_eq!(result, "⁭ Review PR #42");
    }

    #[test]
    fn permission_retry_renders() {
        let result =
            render_system_text_message("permission_retry", "Bash, Read", &Theme::default());
        assert_eq!(result, "⁭ Allowed: Bash, Read");
    }

    #[test]
    fn turn_duration_renders() {
        let result = render_system_text_message(
            "turn_duration",
            r#"{"duration_ms":12345}"#,
            &Theme::default(),
        );
        assert!(result.contains("Worked for 12345ms"));
    }

    #[test]
    fn turn_duration_with_budget() {
        let result = render_system_text_message(
            "turn_duration",
            r#"{"duration_ms":5000,"budget_limit":60000,"budget_nudges":2}"#,
            &Theme::default(),
        );
        assert!(result.contains("5000ms"));
        assert!(result.contains("60000ms"));
        assert!(result.contains("2 nudges"));
    }

    #[test]
    fn memory_saved_renders() {
        let result = render_system_text_message(
            "memory_saved",
            r#"{"written_paths":["docs/api.md","src/lib.rs"],"verb":"Saved"}"#,
            &Theme::default(),
        );
        assert!(result.contains("Saved 2 memories"));
        assert!(result.contains("api.md"));
        assert!(result.contains("lib.rs"));
    }

    #[test]
    fn stop_hook_summary_empty() {
        let result = render_system_text_message("stop_hook_summary", "", &Theme::default());
        assert!(result.contains("Ran 0 hooks"));
    }

    #[test]
    fn api_error_renders() {
        let result = render_system_text_message("api_error", "API Error 500", &Theme::default());
        assert!(result.contains("API Error 500"));
    }

    #[test]
    fn generic_warning_renders() {
        let result = render_system_text_message("warning", "Disk space low", &Theme::default());
        assert_eq!(result, "● Disk space low");
    }

    #[test]
    fn generic_error_renders() {
        let result = render_system_text_message("error", "Connection failed", &Theme::default());
        assert_eq!(result, "● Connection failed");
    }

    #[test]
    fn classify_turn_duration() {
        match classify_system_message("turn_duration", r#"{"duration_ms":999}"#) {
            SystemTagKind::TurnDuration { duration_ms, .. } => assert_eq!(duration_ms, 999),
            _ => panic!("expected TurnDuration"),
        }
    }

    #[test]
    fn classify_agents_killed() {
        match classify_system_message("agents_killed", "") {
            SystemTagKind::AgentsKilled => {}
            _ => panic!("expected AgentsKilled"),
        }
    }

    #[test]
    fn classify_generic() {
        match classify_system_message("custom_tag", "some message") {
            SystemTagKind::Generic { content, level } => {
                assert_eq!(content, "some message");
                assert_eq!(level, "custom_tag");
            }
            _ => panic!("expected Generic"),
        }
    }
}
