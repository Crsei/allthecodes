//! Typed sender wrapper for app events.

use tokio::sync::mpsc;

use super::app_event::AppEvent;

#[derive(Debug, Clone)]
pub struct AppEventSender {
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl AppEventSender {
    pub fn new(tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        Self { tx }
    }

    pub fn send(&self, event: AppEvent) -> Result<(), mpsc::error::SendError<AppEvent>> {
        self.tx.send(event)
    }

    pub fn notice(
        &self,
        message: impl Into<String>,
    ) -> Result<(), mpsc::error::SendError<AppEvent>> {
        self.send(AppEvent::LocalNotice {
            message: message.into(),
        })
    }

    pub fn notification(
        &self,
        key: impl Into<String>,
        message: impl Into<String>,
        level: impl Into<String>,
        timeout_ms: Option<u64>,
    ) -> Result<(), mpsc::error::SendError<AppEvent>> {
        self.send(AppEvent::Notification {
            key: key.into(),
            message: message.into(),
            level: level.into(),
            timeout_ms,
        })
    }
}

pub fn channel() -> (AppEventSender, mpsc::UnboundedReceiver<AppEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (AppEventSender::new(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_typed_events_in_order() {
        let (sender, mut rx) = channel();

        sender.notice("notice").expect("send notice");
        sender
            .notification("key", "message", "warning", Some(123))
            .expect("send notification");
        sender.send(AppEvent::Tick).expect("send tick");

        assert!(matches!(
            rx.try_recv(),
            Ok(AppEvent::LocalNotice { message }) if message == "notice"
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(AppEvent::Notification {
                key,
                message,
                level,
                timeout_ms
            }) if key == "key"
                && message == "message"
                && level == "warning"
                && timeout_ms == Some(123)
        ));
        assert!(matches!(rx.try_recv(), Ok(AppEvent::Tick)));
    }
}
