//! Rust-side helper for team memory save messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_team_mem_saved(path: &str, _theme: &Theme) -> String {
    if path.trim().is_empty() {
        "Team memory saved".to_string()
    } else {
        format!("Team memory saved to {path}")
    }
}
