use std::time::{Duration, Instant};

use ratatui::text::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
    Immediate,
    High,
    Medium,
    Low,
}

impl NotificationPriority {
    fn rank(self) -> u8 {
        match self {
            Self::Immediate => 0,
            Self::High => 1,
            Self::Medium => 2,
            Self::Low => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationTone {
    Info,
    Warning,
    Error,
    Dim,
}

#[derive(Debug, Clone)]
pub struct InAppNotification {
    pub key: String,
    pub priority: NotificationPriority,
    pub timeout: Option<Duration>,
    pub tone: NotificationTone,
    pub text: String,
    pub rendered: Option<Vec<Span<'static>>>,
    pub fold: bool,
    pub invalidates: Vec<String>,
}

impl InAppNotification {
    pub fn new(
        key: impl Into<String>,
        priority: NotificationPriority,
        text: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            priority,
            timeout: None,
            tone: NotificationTone::Info,
            text: text.into(),
            rendered: None,
            fold: false,
            invalidates: Vec::new(),
        }
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout = Some(Duration::from_millis(timeout_ms));
        self
    }

    pub fn with_tone(mut self, tone: NotificationTone) -> Self {
        self.tone = tone;
        self
    }
    pub fn with_rendered(mut self, spans: Vec<Span<'static>>) -> Self {
        self.rendered = Some(spans);
        self
    }

    pub fn with_fold(mut self, fold: bool) -> Self {
        self.fold = fold;
        self
    }

    pub fn with_invalidates(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.invalidates = keys.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone)]
struct ActiveNotification {
    notification: InAppNotification,
    expires_at: Option<Instant>,
}

impl ActiveNotification {
    fn new(notification: InAppNotification, now: Instant) -> Self {
        Self {
            expires_at: notification.timeout.map(|timeout| now + timeout),
            notification,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        self.expires_at.is_some_and(|expires_at| now >= expires_at)
    }
}

#[derive(Debug, Default)]
pub struct NotificationState {
    current: Option<ActiveNotification>,
    queue: Vec<InAppNotification>,
}

impl NotificationState {
    pub fn current(&self) -> Option<&InAppNotification> {
        self.current.as_ref().map(|active| &active.notification)
    }

    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    pub fn add_notification(&mut self, notification: InAppNotification) -> bool {
        let now = Instant::now();
        self.add_notification_at(notification, now)
    }

    pub fn remove_notification(&mut self, key: &str) -> bool {
        let mut changed = false;
        if self
            .current
            .as_ref()
            .is_some_and(|active| active.notification.key == key)
        {
            self.current = None;
            changed = true;
        }
        let before = self.queue.len();
        self.queue.retain(|notification| notification.key != key);
        changed || before != self.queue.len()
    }

    pub fn process_queue(&mut self) -> bool {
        self.process_queue_at(Instant::now())
    }

    fn add_notification_at(&mut self, notification: InAppNotification, now: Instant) -> bool {
        self.remove_keys(notification.invalidates.iter().map(String::as_str));

        if notification.fold {
            if self.try_update_folded_current(&notification, now) {
                return true;
            }
            if self.try_update_folded_queue(&notification) {
                return true;
            }
        }

        if self.should_preempt_current(notification.priority) {
            let previous = self.current.take();
            self.current = Some(ActiveNotification::new(notification, now));
            if let Some(previous) = previous {
                self.queue.push(previous.notification);
            }
            return true;
        }

        if self.current.is_none() {
            self.current = Some(ActiveNotification::new(notification, now));
            return true;
        }

        self.queue.push(notification);
        true
    }

    fn should_preempt_current(&self, priority: NotificationPriority) -> bool {
        self.current
            .as_ref()
            .is_none_or(|active| priority.rank() < active.notification.priority.rank())
    }

    fn process_queue_at(&mut self, now: Instant) -> bool {
        let was_expired = self
            .current
            .as_ref()
            .is_some_and(|active| active.is_expired(now));
        if was_expired {
            self.current = None;
        }

        if self.current.is_none() {
            if let Some(next) = self.pop_next() {
                self.current = Some(ActiveNotification::new(next, now));
                return true;
            }
        }

        was_expired
    }

    fn try_update_folded_current(
        &mut self,
        notification: &InAppNotification,
        now: Instant,
    ) -> bool {
        let Some(current) = self.current.as_mut() else {
            return false;
        };
        if current.notification.key != notification.key {
            return false;
        }
        current.notification = notification.clone();
        current.expires_at = notification.timeout.map(|timeout| now + timeout);
        true
    }

    fn try_update_folded_queue(&mut self, notification: &InAppNotification) -> bool {
        let Some(existing) = self
            .queue
            .iter_mut()
            .find(|queued| queued.key == notification.key)
        else {
            return false;
        };
        *existing = notification.clone();
        true
    }

    fn remove_keys<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        let keys: Vec<&str> = keys.into_iter().collect();
        if keys.is_empty() {
            return;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|active| keys.iter().any(|key| active.notification.key == *key))
        {
            self.current = None;
        }
        self.queue
            .retain(|queued| !keys.iter().any(|key| queued.key == *key));
    }

    fn pop_next(&mut self) -> Option<InAppNotification> {
        let best_idx = self
            .queue
            .iter()
            .enumerate()
            .min_by_key(|(_, notification)| notification.priority.rank())
            .map(|(idx, _)| idx)?;
        Some(self.queue.remove(best_idx))
    }
}

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    let notification = InAppNotification::new("anchor", NotificationPriority::Low, "ready")
        .with_rendered(vec![Span::raw("ready")])
        .with_invalidates(["anchor"]);
    let mut state = NotificationState::default();
    let _ = state.add_notification(notification);
    let _ = state.queued_len();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_preempts_current() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        let low = InAppNotification::new("low", NotificationPriority::Low, "low");
        let immediate = InAppNotification::new("imm", NotificationPriority::Immediate, "imm");

        assert!(state.add_notification_at(low, now));
        assert!(state.add_notification_at(immediate.clone(), now));

        assert_eq!(state.current().map(|n| n.key.as_str()), Some("imm"));
        assert_eq!(state.queued_len(), 1);
    }

    #[test]
    fn higher_priority_preempts_persistent_low_priority_current() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        let low = InAppNotification::new("verbose", NotificationPriority::Low, "verbose");
        let high = InAppNotification::new("warning", NotificationPriority::High, "warning");

        assert!(state.add_notification_at(low, now));
        assert!(state.add_notification_at(high, now));

        assert_eq!(state.current().map(|n| n.key.as_str()), Some("warning"));
        assert_eq!(state.queued_len(), 1);
    }

    #[test]
    fn lower_priority_does_not_preempt_current() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        let high = InAppNotification::new("warning", NotificationPriority::High, "warning");
        let low = InAppNotification::new("verbose", NotificationPriority::Low, "verbose");

        assert!(state.add_notification_at(high, now));
        assert!(state.add_notification_at(low, now));

        assert_eq!(state.current().map(|n| n.key.as_str()), Some("warning"));
        assert_eq!(state.queued_len(), 1);
    }

    #[test]
    fn fold_updates_existing_key() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        let first =
            InAppNotification::new("memory", NotificationPriority::Medium, "80%").with_fold(true);
        let second =
            InAppNotification::new("memory", NotificationPriority::Medium, "90%").with_fold(true);

        state.add_notification_at(first, now);
        state.add_notification_at(second, now);

        assert_eq!(state.current().map(|n| n.text.as_str()), Some("90%"));
        assert_eq!(state.queued_len(), 0);
    }

    #[test]
    fn invalidates_removes_current_and_queue() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        state.add_notification_at(
            InAppNotification::new("token-warning", NotificationPriority::High, "warn"),
            now,
        );
        state.add_notification_at(
            InAppNotification::new("memory", NotificationPriority::Low, "low"),
            now,
        );

        state.add_notification_at(
            InAppNotification::new("all-good", NotificationPriority::Medium, "ok")
                .with_invalidates(["token-warning", "memory"]),
            now,
        );

        assert_eq!(state.current().map(|n| n.key.as_str()), Some("all-good"));
        assert_eq!(state.queued_len(), 0);
    }

    #[test]
    fn expired_current_promotes_highest_priority() {
        let now = Instant::now();
        let mut state = NotificationState::default();
        state.add_notification_at(
            InAppNotification::new("old", NotificationPriority::Low, "old").with_timeout_ms(10),
            now,
        );
        state.add_notification_at(
            InAppNotification::new("medium", NotificationPriority::Medium, "medium"),
            now,
        );
        state.add_notification_at(
            InAppNotification::new("high", NotificationPriority::High, "high"),
            now,
        );

        state.process_queue_at(now + Duration::from_millis(20));

        assert_eq!(state.current().map(|n| n.key.as_str()), Some("high"));
    }

    #[test]
    fn rendered_spans_are_preserved() {
        let notification = InAppNotification::new("rendered", NotificationPriority::Medium, "")
            .with_rendered(vec![Span::raw("custom")]);

        assert_eq!(
            notification.rendered.expect("rendered")[0].content.as_ref(),
            "custom"
        );
    }
}
