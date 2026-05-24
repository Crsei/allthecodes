//! Phase I: Shutdown and cleanup
//!
//! Corresponds to: LIFECYCLE_STATE_MACHINE.md §10 (Phase I)
//!
//! Handles graceful shutdown:
//! - SIGINT/Ctrl-C handler registration
//! - Abort signal propagation to all running tools
//! - Session persistence flush
//! - Transcript flush
//! - Cursor/terminal reset
//! - Timer/watcher cleanup

use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_session::transcript;

// ---------------------------------------------------------------------------
// Shutdown handler registration
// ---------------------------------------------------------------------------

/// Register a Ctrl-C / SIGINT handler that triggers the cancellation token.
///
/// Returns a `CancellationToken` that the REPL loop watches. When the user
/// presses Ctrl-C, the token is cancelled, which causes the REPL to exit
/// gracefully.
///
/// Corresponds to TypeScript: `registerSigintHandler()` in cli.tsx
pub fn register_shutdown_handler() -> CancellationToken {
    let token = CancellationToken::new();
    let shutdown_token = token.clone();

    // First Ctrl-C: cancel token (graceful shutdown)
    // If the runtime is still alive, this triggers cleanup.
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Ctrl-C received, initiating graceful shutdown");
                shutdown_token.cancel();
            }
            Err(e) => {
                warn!(error = %e, "Failed to install Ctrl-C handler");
            }
        }
    });

    token
}

// ---------------------------------------------------------------------------
// Graceful shutdown sequence
// ---------------------------------------------------------------------------

/// Execute the full graceful shutdown sequence.
///
/// Corresponds to TypeScript: `gracefulShutdown()` + `gracefulShutdownSync()`
///
/// Steps:
/// 1. Abort any running query (propagates to all tool executions)
/// 2. Flush the session transcript to disk
/// 3. Reset terminal state (cursor, raw mode)
/// 4. Log shutdown metrics
pub async fn graceful_shutdown(engine: &QueryEngine) {
    debug!("graceful_shutdown: starting");

    // Step 0: Fire SessionEnd hook (best-effort, fire-and-forget)
    {
        let hooks_map = engine.app_state().hooks;
        let end_configs = allthecodes_types::hooks::load_hook_configs(&hooks_map, "SessionEnd");
        if !end_configs.is_empty() {
            let session_id = engine.current_session_id();
            let payload = serde_json::json!({
                "session_id": session_id.as_str(),
                "exit_reason": "normal",
            });
            let _ = allthecodes_tools::hooks::run_event_hooks("SessionEnd", &payload, &end_configs)
                .await;
        }
    }

    // Step 1: Abort any running query
    if !engine.is_aborted() {
        engine.abort();
        debug!("graceful_shutdown: abort signal sent");
    }

    // Step 2: Cancel supervised background agents and clean transient worktrees.
    let cancelled = allthecodes_engine::agent::supervisor::shutdown_all("graceful shutdown").await;
    if cancelled > 0 {
        debug!(cancelled, "graceful_shutdown: background agents cancelled");
    }

    // Step 3: Flush transcript
    let session_id = engine.current_session_id();
    if let Err(e) = transcript::flush_transcript(session_id.as_str()) {
        warn!(error = %e, "failed to flush transcript during shutdown");
    } else {
        debug!("graceful_shutdown: transcript flushed");
    }

    // Step 4: Persist session (save current messages)
    let messages = engine.messages();
    if !messages.is_empty() {
        let cwd = engine.cwd();
        if let Err(e) =
            allthecodes_session::storage::save_session(session_id.as_str(), &messages, cwd)
        {
            warn!(error = %e, "failed to save session during shutdown");
        } else {
            debug!("graceful_shutdown: session saved");
        }
    }

    let skill_usage_path = allthecodes_config::paths::skill_usage_path();
    if let Err(e) = allthecodes_skills::save_skill_usage(&skill_usage_path) {
        warn!(
            error = %e,
            path = %skill_usage_path.display(),
            "failed to persist skill usage during shutdown"
        );
    }

    // Step 5: Reset terminal state
    graceful_shutdown_sync();

    // Step 6: Emit session.end audit event and sync
    {
        use allthecodes_observability::{AuditLevel, EventKind, Outcome, Stage};
        let ctx = engine.audit_context();
        let usage = engine.usage();
        ctx.emit(
            EventKind::SessionEnd,
            Stage::Session,
            AuditLevel::Info,
            Outcome::Completed,
            None,
            Some(serde_json::json!({
                "api_calls": usage.api_call_count,
                "input_tokens": usage.total_input_tokens,
                "output_tokens": usage.total_output_tokens,
                "cost_usd": usage.total_cost_usd,
            })),
        );
        ctx.sync();
    }

    // Step 7: Log final usage
    let usage = engine.usage();
    if usage.api_call_count > 0 {
        info!(
            api_calls = usage.api_call_count,
            input_tokens = usage.total_input_tokens,
            output_tokens = usage.total_output_tokens,
            cost_usd = format!("{:.4}", usage.total_cost_usd),
            "session usage summary"
        );
    }

    debug!("graceful_shutdown: complete");
}

/// Synchronous shutdown actions (terminal state reset).
///
/// Corresponds to TypeScript: `gracefulShutdownSync()`
fn graceful_shutdown_sync() {
    // Reset cursor visibility (in case we hid it during rendering)
    // This uses crossterm directly to ensure it runs even on panic.
    use std::io::Write;
    let mut stdout = std::io::stdout();

    // Show cursor
    let _ = crossterm::execute!(stdout, crossterm::cursor::Show);

    // Disable raw mode if it was enabled
    let _ = crossterm::terminal::disable_raw_mode();

    // Flush stdout
    let _ = stdout.flush();

    debug!("graceful_shutdown_sync: terminal reset");
}
