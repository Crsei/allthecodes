//! Lossless/best-effort event classification and queue pressure handling.

use std::collections::VecDeque;

use allthecodes_ipc_protocol::BackendMessage;

/// Delivery class for backend events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    Lossless,
    BestEffort,
}

/// Structured queue pressure diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePressureDiagnostic {
    pub event_type: &'static str,
    pub queue_depth: usize,
    pub drop_count: usize,
    pub last_dropped_type: Option<&'static str>,
    pub source_phase: &'static str,
}

impl QueuePressureDiagnostic {
    pub fn into_backend_message(self) -> BackendMessage {
        BackendMessage::Error {
            message: format!(
                "ipc client queue pressure: event_type={} queue_depth={} drop_count={} last_dropped_type={} source_phase={}",
                self.event_type,
                self.queue_depth,
                self.drop_count,
                self.last_dropped_type.unwrap_or("none"),
                self.source_phase
            ),
            recoverable: true,
        }
    }
}

/// Classify a backend event for queueing/backpressure.
pub fn classify_event(msg: &BackendMessage) -> EventClass {
    match msg {
        BackendMessage::ToolProgress { .. }
        | BackendMessage::UsageUpdate { .. }
        | BackendMessage::StatusLineUpdate { .. }
        | BackendMessage::Suggestions { .. }
        | BackendMessage::NotificationSent { .. }
        | BackendMessage::SubsystemStatus { .. }
        | BackendMessage::LspEvent { .. }
        | BackendMessage::McpEvent { .. }
        | BackendMessage::PluginEvent { .. }
        | BackendMessage::SkillEvent { .. }
        | BackendMessage::IdeEvent { .. }
        | BackendMessage::AgentSettingsEvent { .. } => EventClass::BestEffort,
        _ => EventClass::Lossless,
    }
}

/// Stable protocol type name for diagnostics.
pub fn event_type(msg: &BackendMessage) -> &'static str {
    allthecodes_ipc_protocol::legacy_backend_type(msg)
}

/// Small bounded queue that may replace best-effort events but never drops
/// lossless events silently.
#[derive(Debug)]
pub struct ClientEventQueue {
    capacity: usize,
    events: VecDeque<BackendMessage>,
    drop_count: usize,
    last_dropped_type: Option<&'static str>,
}

impl ClientEventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: VecDeque::new(),
            drop_count: 0,
            last_dropped_type: None,
        }
    }

    pub fn push(&mut self, msg: BackendMessage) -> Result<(), QueuePressureDiagnostic> {
        if self.events.len() < self.capacity {
            self.events.push_back(msg);
            return Ok(());
        }

        let incoming_type = event_type(&msg);
        match classify_event(&msg) {
            EventClass::Lossless => Err(QueuePressureDiagnostic {
                event_type: incoming_type,
                queue_depth: self.events.len(),
                drop_count: self.drop_count,
                last_dropped_type: self.last_dropped_type,
                source_phase: "client_event_queue",
            }),
            EventClass::BestEffort => {
                let Some(dropped) = self
                    .events
                    .iter()
                    .position(|queued| classify_event(queued) == EventClass::BestEffort)
                    .and_then(|idx| self.events.remove(idx))
                else {
                    self.drop_count += 1;
                    self.last_dropped_type = Some(incoming_type);
                    return Err(QueuePressureDiagnostic {
                        event_type: incoming_type,
                        queue_depth: self.events.len(),
                        drop_count: self.drop_count,
                        last_dropped_type: self.last_dropped_type,
                        source_phase: "client_event_queue",
                    });
                };
                self.drop_count += 1;
                self.last_dropped_type = Some(event_type(&dropped));
                self.events.push_back(msg);
                Err(QueuePressureDiagnostic {
                    event_type: incoming_type,
                    queue_depth: self.events.len(),
                    drop_count: self.drop_count,
                    last_dropped_type: self.last_dropped_type,
                    source_phase: "client_event_queue",
                })
            }
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = BackendMessage> + '_ {
        self.events.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(text: &str) -> BackendMessage {
        BackendMessage::SystemInfo {
            text: text.to_string(),
            level: "info".to_string(),
        }
    }

    fn progress(id: &str) -> BackendMessage {
        BackendMessage::ToolProgress {
            tool_use_id: id.to_string(),
            tool: "bash".to_string(),
            output: String::new(),
            elapsed_seconds: 1,
            total_lines: None,
            total_bytes: None,
            timeout_ms: None,
            operation: None,
        }
    }

    #[test]
    fn classifies_lossless_and_best_effort_events() {
        assert_eq!(classify_event(&info("ready")), EventClass::Lossless);
        assert_eq!(classify_event(&progress("tool-1")), EventClass::BestEffort);
    }

    #[test]
    fn permission_and_question_requests_are_lossless() {
        let permission = BackendMessage::PermissionRequest {
            tool_use_id: "tool-1".to_string(),
            tool: "Bash".to_string(),
            command: "ls".to_string(),
            input: serde_json::json!({ "command": "ls" }),
            options: vec!["allow".to_string(), "deny".to_string()],
            operation: None,
        };
        let question = BackendMessage::QuestionRequest {
            id: "question-1".to_string(),
            text: "Continue?".to_string(),
            choices: vec!["yes".to_string(), "no".to_string()],
            allow_free_text: false,
        };

        assert_eq!(classify_event(&permission), EventClass::Lossless);
        assert_eq!(classify_event(&question), EventClass::Lossless);
    }

    #[test]
    fn full_queue_rejects_lossless_without_drop() {
        let mut queue = ClientEventQueue::new(1);
        queue.push(info("first")).unwrap();

        let diagnostic = queue.push(info("second")).unwrap_err();

        assert_eq!(diagnostic.event_type, "system_info");
        assert_eq!(diagnostic.queue_depth, 1);
        assert_eq!(diagnostic.drop_count, 0);
        assert_eq!(queue.drain().count(), 1);
    }

    #[test]
    fn best_effort_pressure_drops_with_diagnostic() {
        let mut queue = ClientEventQueue::new(1);
        queue.push(progress("old")).unwrap();

        let diagnostic = queue.push(progress("new")).unwrap_err();

        assert_eq!(diagnostic.event_type, "tool_progress");
        assert_eq!(diagnostic.queue_depth, 1);
        assert_eq!(diagnostic.drop_count, 1);
        assert_eq!(diagnostic.last_dropped_type, Some("tool_progress"));
    }

    #[test]
    fn best_effort_pressure_never_drops_lossless_event() {
        let mut queue = ClientEventQueue::new(1);
        queue.push(info("keep")).unwrap();

        let diagnostic = queue.push(progress("drop-incoming")).unwrap_err();
        let drained: Vec<_> = queue.drain().collect();

        assert_eq!(diagnostic.event_type, "tool_progress");
        assert_eq!(diagnostic.queue_depth, 1);
        assert_eq!(diagnostic.drop_count, 1);
        assert_eq!(diagnostic.last_dropped_type, Some("tool_progress"));
        assert_eq!(drained.len(), 1);
        assert!(matches!(
            &drained[0],
            BackendMessage::SystemInfo { text, .. } if text == "keep"
        ));
    }

    #[test]
    fn zero_capacity_best_effort_is_diagnostic_without_queue_growth() {
        let mut queue = ClientEventQueue::new(0);

        let diagnostic = queue.push(progress("drop-incoming")).unwrap_err();

        assert_eq!(diagnostic.event_type, "tool_progress");
        assert_eq!(diagnostic.queue_depth, 0);
        assert_eq!(diagnostic.drop_count, 1);
        assert_eq!(diagnostic.last_dropped_type, Some("tool_progress"));
        assert_eq!(queue.drain().count(), 0);
    }
}
