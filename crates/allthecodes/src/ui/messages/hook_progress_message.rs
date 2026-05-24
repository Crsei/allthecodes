//! Rust-side helper for hook progress messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_hook_progress_message(hook: &str, stage: &str, _theme: &Theme) -> String {
    let status = if stage.trim().is_empty() {
        "running"
    } else {
        stage
    };
    format!("Hook [{hook}] {status}")
}
