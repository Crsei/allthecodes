#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNotificationStatus {
    Completed,
    Failed,
    Killed,
}

impl TaskNotificationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskNotificationUsage {
    pub total_tokens: Option<u64>,
    pub tool_uses: Option<u64>,
    pub duration_ms: Option<u64>,
    pub retry_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotification {
    pub task_id: String,
    pub status: TaskNotificationStatus,
    pub summary: String,
    pub result: String,
    pub usage: TaskNotificationUsage,
}

impl TaskNotification {
    pub fn to_xml(&self) -> String {
        let mut xml = String::from("<task-notification>");
        push_tag(&mut xml, "task-id", &self.task_id);
        push_tag(&mut xml, "status", self.status.as_str());
        push_tag(&mut xml, "summary", &self.summary);
        push_tag(&mut xml, "result", &self.result);
        if self.usage.total_tokens.is_some()
            || self.usage.tool_uses.is_some()
            || self.usage.duration_ms.is_some()
            || self.usage.retry_count.is_some()
        {
            xml.push_str("<usage>");
            if let Some(total_tokens) = self.usage.total_tokens {
                push_tag(&mut xml, "total_tokens", &total_tokens.to_string());
            }
            if let Some(tool_uses) = self.usage.tool_uses {
                push_tag(&mut xml, "tool_uses", &tool_uses.to_string());
            }
            if let Some(duration_ms) = self.usage.duration_ms {
                push_tag(&mut xml, "duration_ms", &duration_ms.to_string());
            }
            if let Some(retry_count) = self.usage.retry_count {
                push_tag(&mut xml, "retry_count", &retry_count.to_string());
            }
            xml.push_str("</usage>");
        }
        xml.push_str("</task-notification>");
        xml
    }
}

pub fn build_task_notification(
    task_id: &str,
    status: TaskNotificationStatus,
    summary: &str,
    result: &str,
    usage: TaskNotificationUsage,
) -> String {
    TaskNotification {
        task_id: task_id.to_string(),
        status,
        summary: summary.to_string(),
        result: result.to_string(),
        usage,
    }
    .to_xml()
}

fn push_tag(xml: &mut String, tag: &str, value: &str) {
    xml.push('<');
    xml.push_str(tag);
    xml.push('>');
    xml.push_str(&escape_xml(value));
    xml.push_str("</");
    xml.push_str(tag);
    xml.push('>');
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_notification_xml_matches_doc_contract() {
        let xml = TaskNotification {
            task_id: "agent-a1b".into(),
            status: TaskNotificationStatus::Completed,
            summary: "Agent \"Investigate auth bug\" completed".into(),
            result: "Found null pointer in src/auth/validate.ts:42".into(),
            usage: TaskNotificationUsage {
                total_tokens: Some(1200),
                tool_uses: Some(3),
                duration_ms: Some(4500),
                retry_count: None,
            },
        }
        .to_xml();

        assert!(xml.contains("<task-notification>"));
        assert!(xml.contains("<task-id>agent-a1b</task-id>"));
        assert!(xml.contains("<status>completed</status>"));
        assert!(xml.contains("<summary>Agent &quot;Investigate auth bug&quot; completed</summary>"));
        assert!(xml.contains("<total_tokens>1200</total_tokens>"));
        assert!(xml.contains("<tool_uses>3</tool_uses>"));
        assert!(xml.contains("<duration_ms>4500</duration_ms>"));
    }

    #[test]
    fn task_notification_xml_escapes_result_text() {
        let xml = build_task_notification(
            "agent-x",
            TaskNotificationStatus::Failed,
            "failed <fast>",
            "bad & worse",
            TaskNotificationUsage::default(),
        );
        assert!(xml.contains("failed &lt;fast&gt;"));
        assert!(xml.contains("bad &amp; worse"));
    }

    #[test]
    fn task_notification_xml_supports_killed_status() {
        let xml = build_task_notification(
            "agent-x",
            TaskNotificationStatus::Killed,
            "worker killed",
            "cancelled by user",
            TaskNotificationUsage::default(),
        );

        assert!(xml.contains("<status>killed</status>"));
    }

    #[test]
    fn task_notification_xml_includes_retry_count_when_available() {
        let xml = TaskNotification {
            task_id: "agent-retry".into(),
            status: TaskNotificationStatus::Failed,
            summary: "Agent failed".into(),
            result: "blocked".into(),
            usage: TaskNotificationUsage {
                retry_count: Some(1),
                ..TaskNotificationUsage::default()
            },
        }
        .to_xml();

        assert!(xml.contains("<retry_count>1</retry_count>"));
    }
}
