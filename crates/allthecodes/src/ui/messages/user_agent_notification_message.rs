//! Rust-side helper for user agent notification messages.

use crate::ui::theme::Theme;

pub fn render_user_agent_notification_message(notification: &str, _theme: &Theme) -> String {
    let notification = notification.trim();
    if notification.is_empty() {
        "User agent notification".to_string()
    } else if let Some(summary) = render_task_notification_summary(notification) {
        summary
    } else {
        format!("Notification: {notification}")
    }
}

fn render_task_notification_summary(notification: &str) -> Option<String> {
    if !notification.contains("<task-notification>") {
        return None;
    }
    let task_id = extract_tag(notification, "task-id")?;
    let status = extract_tag(notification, "status")?;
    let summary = extract_tag(notification, "summary")?;
    Some(format!("Task {task_id} {status}: {summary}"))
}

fn extract_tag(input: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = input.find(&open)? + open.len();
    let end = input[start..].find(&close)? + start;
    Some(unescape_xml(&input[start..end]))
}

fn unescape_xml(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_notification_renders_task_notification_summary() {
        let rendered = render_user_agent_notification_message(
            "<task-notification><task-id>agent-1</task-id><status>completed</status><summary>Agent completed</summary></task-notification>",
            &Theme::default(),
        );

        assert_eq!(rendered, "Task agent-1 completed: Agent completed");
    }

    #[test]
    fn user_agent_notification_falls_back_for_malformed_xml() {
        let rendered = render_user_agent_notification_message(
            "<task-notification><task-id>agent-1</task-id>",
            &Theme::default(),
        );

        assert_eq!(
            rendered,
            "Notification: <task-notification><task-id>agent-1</task-id>"
        );
    }

    #[test]
    fn user_agent_notification_falls_back_for_non_task_xml() {
        let rendered = render_user_agent_notification_message(
            "<other-notification><summary>Agent completed</summary></other-notification>",
            &Theme::default(),
        );

        assert_eq!(
            rendered,
            "Notification: <other-notification><summary>Agent completed</summary></other-notification>"
        );
    }
}
