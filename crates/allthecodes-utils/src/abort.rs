//! AbortController — pure utility, kept in `cc-utils`.
//!
//! Provides cooperative cancellation for async operations via `tokio::sync::watch`.
//! Corresponds to Bun `src/utils/abortController.ts`.
//!
//! **Decision**: Keep in cc-utils. Pure utility with no domain dependencies.
//! Currently has no external callers in the workspace, but is available as a
//! reusable primitive for any crate that needs async cancellation.

use parking_lot::Mutex;
use tokio::sync::watch;

/// AbortController equivalent for Rust async tasks.
///
/// Provides cooperative cancellation for async operations, analogous to
/// JavaScript's `AbortController` / `AbortSignal`. Uses a `tokio::sync::watch`
/// channel to broadcast the abort signal to all listeners.
///
/// # Usage
///
/// ```ignore
/// let controller = AbortController::new();
/// let mut rx = controller.subscribe();
///
/// // In an async task:
/// tokio::select! {
///     _ = rx.changed() => {
///         // Aborted
///     }
///     result = do_work() => {
///         // Completed normally
///     }
/// }
///
/// // To abort from another context:
/// controller.abort("user cancelled");
/// ```
pub struct AbortController {
    /// Sender side of the watch channel. Sends `true` when aborted.
    tx: watch::Sender<bool>,
    /// Receiver template for creating new subscribers.
    rx: watch::Receiver<bool>,
    /// Human-readable reason for the abort.
    reason: Mutex<Option<String>>,
}

impl AbortController {
    /// Create a new AbortController in the non-aborted state.
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            tx,
            rx,
            reason: Mutex::new(None),
        }
    }

    /// Signal an abort with the given reason.
    ///
    /// All current and future subscribers will see the aborted state.
    /// Calling `abort` multiple times is safe; only the first reason is kept.
    pub fn abort(&self, reason: &str) {
        // Set the reason (only if not already set)
        {
            let mut guard = self.reason.lock();
            if guard.is_none() {
                *guard = Some(reason.to_string());
            }
        }
        // Broadcast the abort signal (ignore error if no receivers)
        let _ = self.tx.send(true);
    }

    /// Check whether this controller has been aborted.
    pub fn is_aborted(&self) -> bool {
        *self.rx.borrow()
    }

    /// Get the abort reason, if aborted.
    pub fn reason(&self) -> Option<String> {
        self.reason.lock().clone()
    }

    /// Subscribe to this controller's abort signal.
    ///
    /// Returns a `watch::Receiver<bool>` that will receive `true` when
    /// the controller is aborted. Use `rx.changed().await` to wait for
    /// the abort, or `rx.borrow()` to check the current state.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.rx.clone()
    }

    /// Create a child abort controller that is automatically aborted
    /// when this (parent) controller is aborted.
    ///
    /// Useful for propagating cancellation down a task tree.
    pub fn child(&self) -> AbortController {
        let child = AbortController::new();
        let child_tx = child.tx.clone();
        let mut parent_rx = self.rx.clone();

        tokio::spawn(async move {
            // Wait for parent to be aborted
            loop {
                if parent_rx.changed().await.is_err() {
                    // Parent dropped, abort child
                    let _ = child_tx.send(true);
                    break;
                }
                if *parent_rx.borrow() {
                    let _ = child_tx.send(true);
                    break;
                }
            }
        });

        child
    }
}

impl Default for AbortController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let ctrl = AbortController::new();
        assert!(!ctrl.is_aborted());
        assert!(ctrl.reason().is_none());
    }

    #[test]
    fn test_abort() {
        let ctrl = AbortController::new();
        ctrl.abort("user pressed Ctrl+C");
        assert!(ctrl.is_aborted());
        assert_eq!(ctrl.reason().unwrap(), "user pressed Ctrl+C");
    }

    #[test]
    fn test_double_abort_keeps_first_reason() {
        let ctrl = AbortController::new();
        ctrl.abort("first reason");
        ctrl.abort("second reason");
        assert_eq!(ctrl.reason().unwrap(), "first reason");
    }

    #[test]
    fn test_subscriber_sees_abort() {
        let ctrl = AbortController::new();
        let rx = ctrl.subscribe();

        assert!(!*rx.borrow());
        ctrl.abort("test");
        assert!(*rx.borrow());
    }

    #[tokio::test]
    async fn test_subscriber_await_abort() {
        let ctrl = AbortController::new();
        let mut rx = ctrl.subscribe();

        // Spawn a task that aborts after a short delay
        let ctrl_clone_tx = ctrl.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = ctrl_clone_tx.send(true);
        });

        // Wait for the abort
        let _ = rx.changed().await;
        assert!(*rx.borrow());
    }
}
