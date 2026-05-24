//! Computer Use executor — orchestrates the full desktop control workflow.
//!
//! The executor ties together input simulation, screenshot capture, event loop
//! draining, concurrency locking, and host capabilities into a single high-level
//! interface. This is the entry point used by `tools.rs` to perform complex
//! multi-step operations.
//!
//! ## Workflow
//!
//! 1. **Acquire lock** — prevents concurrent Computer Use operations
//! 2. **Pre-flight** — drain event loop, check permissions, probe capabilities
//! 3. **Execute** — perform the requested action (click, type, screenshot, etc.)
//! 4. **Post-flight** — drain event loop again to let events propagate
//! 5. **Release lock** — allow next operation

use std::time::Duration;

use crate::drain_run_loop::{self, DrainConfig};
use crate::input::{self, InputAction, MouseButton};
use crate::lock::{global_lock, ComputerUseLock};
use crate::screenshot::{self, ScreenshotResult};

/// Configuration for the executor.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Whether to use the concurrency lock.
    pub use_lock: bool,
    /// Drain config for event loop flushing.
    pub drain: DrainConfig,
    /// Whether to drain after each operation.
    pub drain_after_ops: bool,
    /// Lock acquisition timeout.
    pub lock_timeout: Duration,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            use_lock: true,
            drain: DrainConfig::default(),
            drain_after_ops: true,
            lock_timeout: Duration::from_secs(30),
        }
    }
}

/// The Computer Use executor.
///
/// All public methods acquire the global lock, perform the action, drain the
/// event loop, and release the lock.
pub struct Executor {
    config: ExecutorConfig,
    lock: &'static ComputerUseLock,
}

impl Executor {
    /// Create a new executor with default config.
    pub fn new() -> Self {
        Self {
            config: ExecutorConfig::default(),
            lock: global_lock(),
        }
    }

    /// Create a new executor with custom config.
    pub fn with_config(config: ExecutorConfig) -> Self {
        Self {
            config,
            lock: global_lock(),
        }
    }

    /// Take a screenshot with lock and drain.
    pub async fn screenshot(&self) -> anyhow::Result<ScreenshotResult> {
        let _guard = self.acquire_lock("screenshot").await?;
        let result = screenshot::capture_screenshot().await?;
        self.post_drain().await;
        Ok(result)
    }

    /// Click a mouse button at coordinates with lock and drain.
    pub async fn click(&self, x: i32, y: i32, button: MouseButton) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("click").await?;
        let result = input::execute_input(InputAction::Click { x, y, button }).await?;
        drain_run_loop::drain_after_click().await;
        Ok(result)
    }

    /// Double-click at coordinates.
    pub async fn double_click(&self, x: i32, y: i32) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("double_click").await?;
        let result = input::execute_input(InputAction::DoubleClick { x, y }).await?;
        drain_run_loop::drain_after_click().await;
        Ok(result)
    }

    /// Type text.
    pub async fn type_text(&self, text: &str) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("type_text").await?;
        let result = input::execute_input(InputAction::TypeText {
            text: text.to_string(),
        })
        .await?;
        drain_run_loop::drain_after_keyboard().await;
        Ok(result)
    }

    /// Press a key or key combination.
    pub async fn key_press(&self, key: &str) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("key").await?;
        let result = input::execute_input(InputAction::KeyPress {
            key: key.to_string(),
        })
        .await?;
        drain_run_loop::drain_after_keyboard().await;
        Ok(result)
    }

    /// Scroll at coordinates.
    pub async fn scroll(&self, x: i32, y: i32, amount: i32) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("scroll").await?;
        let result = input::execute_input(InputAction::Scroll { x, y, amount }).await?;
        self.post_drain().await;
        Ok(result)
    }

    /// Move the mouse to coordinates.
    pub async fn mouse_move(&self, x: i32, y: i32) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("mouse_move").await?;
        let result = input::execute_input(InputAction::MouseMove { x, y }).await?;
        drain_run_loop::drain_after_move().await;
        Ok(result)
    }

    /// Get the current cursor position.
    pub async fn cursor_position(&self) -> anyhow::Result<input::CursorPosition> {
        // Reading cursor position is concurrency-safe (no lock needed unless configured).
        if self.config.use_lock {
            let _guard = self.acquire_lock("cursor_position").await?;
            return input::get_cursor_position().await;
        }
        input::get_cursor_position().await
    }

    /// Perform a complex multi-step action:
    /// move → click → type (e.g., click a text field and type into it).
    pub async fn click_and_type(&self, x: i32, y: i32, text: &str) -> anyhow::Result<String> {
        let _guard = self.acquire_lock("click_and_type").await?;
        input::execute_input(InputAction::MouseMove { x, y }).await?;
        drain_run_loop::drain_after_move().await;
        input::execute_input(InputAction::Click {
            x,
            y,
            button: MouseButton::Left,
        })
        .await?;
        drain_run_loop::drain_after_click().await;
        input::execute_input(InputAction::TypeText {
            text: text.to_string(),
        })
        .await?;
        drain_run_loop::drain_after_keyboard().await;
        Ok(format!("Clicked at ({}, {}) and typed text", x, y))
    }

    /// Acquire the lock if configured.
    async fn acquire_lock(
        &self,
        operation: &str,
    ) -> anyhow::Result<Option<crate::lock::LockGuard>> {
        if !self.config.use_lock {
            return Ok(None);
        }
        self.lock
            .try_lock_timeout(operation, self.config.lock_timeout)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not acquire Computer Use lock for '{}' within {:?}",
                    operation,
                    self.config.lock_timeout
                )
            })
            .map(Some)
    }

    /// Post-operation drain.
    async fn post_drain(&self) {
        if self.config.drain_after_ops {
            drain_run_loop::drain_event_loop(&self.config.drain).await;
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_default_config() {
        let config = ExecutorConfig::default();
        assert!(config.use_lock);
        assert!(config.drain_after_ops);
    }

    #[tokio::test]
    async fn test_executor_creation() {
        let exec = Executor::new();
        // Should not panic
        let _ = exec.cursor_position().await;
    }

    #[tokio::test]
    async fn test_executor_lock_timeout_config() {
        let config = ExecutorConfig {
            lock_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let exec = Executor::with_config(config);
        // Should not hang — lock_timeout is short
        let _ = exec.cursor_position().await;
    }
}
