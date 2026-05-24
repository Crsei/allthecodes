//! Rust-side helper for collapsed team memory messages.

#[cfg(test)]
use crate::ui::theme::Theme;

#[cfg(test)]
pub fn render_team_mem_collapsed(team: &str, entries: usize, _theme: &Theme) -> String {
    format!("Team memory for {team} collapsed ({entries} entries)")
}
