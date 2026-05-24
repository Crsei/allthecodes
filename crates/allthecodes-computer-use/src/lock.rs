//! Concurrency lock for Computer Use operations.
//!
//! Prevents multiple tools from simultaneously controlling the mouse/keyboard,
//! which would cause conflicting inputs.

use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use tokio::sync::{Mutex, OwnedMutexGuard};

/// Global lock for all Computer Use operations.
#[derive(Debug, Clone)]
pub struct ComputerUseLock {
    inner: Arc<Mutex<()>>,
    holder: Arc<StdMutex<Option<String>>>,
}

impl ComputerUseLock {
    /// Create a new unlocked Computer Use lock.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(())),
            holder: Arc::new(StdMutex::new(None)),
        }
    }

    /// Acquire the lock, waiting indefinitely until it becomes available.
    pub async fn lock(&self, holder: impl Into<String>) -> LockGuard {
        let guard = self.inner.clone().lock_owned().await;
        self.set_holder(Some(holder.into()));
        LockGuard {
            _guard: Some(guard),
            holder: self.holder.clone(),
        }
    }

    /// Try to acquire the lock without blocking.
    pub fn try_lock(&self, holder: impl Into<String>) -> Option<LockGuard> {
        let guard = self.inner.clone().try_lock_owned().ok()?;
        self.set_holder(Some(holder.into()));
        Some(LockGuard {
            _guard: Some(guard),
            holder: self.holder.clone(),
        })
    }

    /// Try to acquire the lock with a timeout.
    pub async fn try_lock_timeout(
        &self,
        holder: impl Into<String>,
        timeout: Duration,
    ) -> Result<Option<LockGuard>, anyhow::Error> {
        match tokio::time::timeout(timeout, self.inner.clone().lock_owned()).await {
            Ok(guard) => {
                self.set_holder(Some(holder.into()));
                Ok(Some(LockGuard {
                    _guard: Some(guard),
                    holder: self.holder.clone(),
                }))
            }
            Err(_elapsed) => Ok(None),
        }
    }

    /// Check whether the lock is currently held.
    pub fn is_locked(&self) -> bool {
        self.inner.clone().try_lock_owned().is_err()
    }

    /// Number of waiters currently queued. Tokio does not expose this.
    pub fn waiter_count(&self) -> usize {
        0
    }

    fn set_holder(&self, holder: Option<String>) {
        if let Ok(mut current) = self.holder.lock() {
            *current = holder;
        }
    }
}

impl Default for ComputerUseLock {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard that releases the `ComputerUseLock` when dropped.
#[derive(Debug)]
pub struct LockGuard {
    _guard: Option<OwnedMutexGuard<()>>,
    holder: Arc<StdMutex<Option<String>>>,
}

impl LockGuard {
    /// Explicitly release the lock before the guard is dropped.
    pub async fn unlock(mut self) {
        self.clear_holder();
        self._guard.take();
    }

    fn clear_holder(&self) {
        if let Ok(mut holder) = self.holder.lock() {
            *holder = None;
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.clear_holder();
    }
}

/// Global Computer Use lock instance.
pub fn global_lock() -> &'static ComputerUseLock {
    static GLOBAL: OnceLock<ComputerUseLock> = OnceLock::new();
    GLOBAL.get_or_init(ComputerUseLock::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lock_acquire_and_release() {
        let lock = ComputerUseLock::new();
        let guard = lock.lock("test").await;
        assert!(lock.is_locked());
        drop(guard);
        assert!(!lock.is_locked());
        let _guard2 = lock.lock("test2").await;
    }

    #[tokio::test]
    async fn test_try_lock_timeout() {
        let lock = ComputerUseLock::new();
        let _guard = lock.lock("holder").await;

        let result = lock
            .try_lock_timeout("contender", Duration::from_millis(50))
            .await
            .unwrap();
        assert!(result.is_none(), "lock should be held");
    }

    #[tokio::test]
    async fn test_try_lock_nonblocking() {
        let lock = ComputerUseLock::new();
        let _guard = lock.try_lock("holder").expect("first lock");
        assert!(lock.try_lock("contender").is_none());
    }

    #[tokio::test]
    async fn test_global_lock_singleton() {
        let a = global_lock();
        let b = global_lock();
        assert!(std::ptr::eq(a, b));
    }
}
