//! 响应式信号原语 — 基于 tokio::sync::watch
//!
//! 对应 TypeScript: bootstrap/src/utils/signal.ts
//!
//! 与 `utils/abort.rs` 的 AbortController 的区别:
//! - AbortController 只做 bool 信号 (aborted or not)
//! - Signal<T> 承载任意类型的值变更通知

use tokio::sync::watch;

/// 响应式信号 — 持有一个值，变更时通知所有订阅者。
pub struct Signal<T: Clone + Send + Sync + 'static> {
    tx: watch::Sender<T>,
    rx: watch::Receiver<T>,
}

impl<T: Clone + Send + Sync + 'static> Signal<T> {
    /// 创建一个新信号，初始值为 `initial`。
    pub fn new(initial: T) -> Self {
        let (tx, rx) = watch::channel(initial);
        Self { tx, rx }
    }

    /// 读取当前值 (克隆)。
    pub fn get(&self) -> T {
        self.rx.borrow().clone()
    }

    /// 设置新值，通知所有订阅者。
    pub fn set(&self, value: T) {
        let _ = self.tx.send(value);
    }

    /// 仅在值不同时设置 (需要 PartialEq)。
    pub fn set_if_changed(&self, value: T)
    where
        T: PartialEq,
    {
        self.tx.send_if_modified(|current| {
            if *current != value {
                *current = value;
                true
            } else {
                false
            }
        });
    }

    /// 创建一个新的订阅者 — 用于异步等待变更。
    ///
    /// ```ignore
    /// let mut rx = signal.subscribe();
    /// rx.changed().await; // 等待下一次变更
    /// let new_value = rx.borrow().clone();
    /// ```
    pub fn subscribe(&self) -> watch::Receiver<T> {
        self.rx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_value() {
        let sig = Signal::new(42);
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn set_and_get() {
        let sig = Signal::new(0);
        sig.set(99);
        assert_eq!(sig.get(), 99);
    }

    #[test]
    fn subscriber_sees_update() {
        let sig = Signal::new("hello".to_string());
        let rx = sig.subscribe();

        sig.set("world".to_string());
        assert_eq!(*rx.borrow(), "world");
    }

    #[test]
    fn set_if_changed_skips_equal() {
        let sig = Signal::new(10);
        let mut rx = sig.subscribe();

        // Mark current value as seen
        rx.borrow_and_update();

        // Set same value — should not trigger change
        sig.set_if_changed(10);
        assert!(!rx.has_changed().unwrap_or(false));

        // Set different value — should trigger
        sig.set_if_changed(20);
        assert!(rx.has_changed().unwrap_or(false));
    }

    #[tokio::test]
    async fn async_subscribe_await() {
        let sig = Signal::new(0);
        let mut rx = sig.subscribe();

        // Mark initial as seen
        rx.borrow_and_update();

        let sig_clone_tx = sig.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let _ = sig_clone_tx.send(42);
        });

        let _ = rx.changed().await;
        assert_eq!(*rx.borrow(), 42);
    }
}
