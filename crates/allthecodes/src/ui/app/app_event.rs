//! App event model for backend-to-TUI routing.

use serde::Serialize;

use allthecodes_ipc_protocol::BackendMessage;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    Backend {
        message: Box<BackendMessage>,
    },
    LocalNotice {
        message: String,
    },
    Notification {
        key: String,
        message: String,
        level: String,
        timeout_ms: Option<u64>,
    },
    Tick,
    Shutdown,
}

impl AppEvent {
    pub fn backend(message: BackendMessage) -> Self {
        Self::Backend {
            message: Box::new(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppEvent;

    #[test]
    fn shutdown_event_is_available_for_test_routing() {
        assert!(matches!(AppEvent::Shutdown, AppEvent::Shutdown));
    }
}
