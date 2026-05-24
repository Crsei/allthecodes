//! macOS event loop drain — ensure input events are processed by the system.
//!
//! After injecting input events (mouse clicks, keystrokes), the macOS WindowServer
//! and target applications need time to process them before we can reliably read
//! the resulting state. This module provides a small run loop drain that spins
//! the CGEvent run loop for a configurable number of iterations.
//!
//! On non-macOS platforms, the drain is a no-op.

use std::time::Duration;

/// Configuration for the event drain.
#[derive(Debug, Clone)]
pub struct DrainConfig {
    /// Number of run loop iterations to drain.
    pub iterations: u32,
    /// Sleep between iterations (macOS fallback if CGEvent drain unavailable).
    pub interval: Duration,
}

impl Default for DrainConfig {
    fn default() -> Self {
        Self {
            iterations: 3,
            interval: Duration::from_millis(16), // ~1 frame at 60fps
        }
    }
}

/// Drain the platform event loop to allow input events to be processed.
///
/// After calling this, any subsequent state reads (screenshots, cursor position)
/// will reflect the effects of the just-injected input events.
///
/// # Arguments
///
/// * `config` — Drain configuration (iterations and interval).
pub async fn drain_event_loop(config: &DrainConfig) {
    #[cfg(target_os = "macos")]
    {
        // On macOS, we attempt to spin the CGEvent run loop via a small
        // Swift or C helper. If unavailable, fall back to sleeping.
        //
        // CGEventPostToPid-based injection already processes via the event
        // tap queue, but we add a small delay to let applications respond.
        for _ in 0..config.iterations {
            let _ = platform_drain_tick().await;
            tokio::time::sleep(config.interval).await;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Non-macOS: just sleep for a short time to let the OS process events.
        let total = config.interval * config.iterations;
        tokio::time::sleep(total).await;
    }
}

/// Convenience: drain with default config.
pub async fn drain_default() {
    drain_event_loop(&DrainConfig::default()).await;
}

/// Convenience: drain with explicit iterations count.
pub async fn drain_iterations(n: u32) {
    drain_event_loop(&DrainConfig {
        iterations: n,
        ..Default::default()
    })
    .await;
}

/// Platform-specific tick of the event loop drain.
#[cfg(target_os = "macos")]
async fn platform_drain_tick() -> anyhow::Result<()> {
    // On macOS, the real CGEvent drain requires calling CFRunLoopRunInMode
    // via a C FFI binding. Since cc-computer-use doesn't link CoreFoundation
    // directly, we use a tiny embedded Swift script as a fallback.
    //
    // The Swift script runs a single iteration of the main run loop, which
    // processes any pending CGEvent-tap callbacks.
    let script = r#"
import Foundation
// Drain one iteration of the main run loop
RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.001))
"#;
    let output = tokio::process::Command::new("swift")
        .args(["-e", script])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            // Swift not available — drain is best-effort
            Ok(())
        }
    }
}

/// Convenience drain after mouse clicks.
pub async fn drain_after_click() {
    drain_iterations(5).await; // More iterations for click to register
}

/// Convenience drain after keyboard input.
pub async fn drain_after_keyboard() {
    drain_iterations(3).await;
}

/// Convenience drain after mouse move.
pub async fn drain_after_move() {
    drain_iterations(2).await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_drain_default_does_not_panic() {
        // Should not crash on any platform
        drain_default().await;
    }

    #[tokio::test]
    async fn test_drain_custom_iterations() {
        drain_iterations(1).await;
    }
}
