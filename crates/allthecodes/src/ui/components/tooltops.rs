//! Tooltip text provider.
//!
//! The filename intentionally follows the existing allthecodes placeholder
//! (`tooltops.rs`). It maps the Codex `tooltips.rs` responsibility into this
//! crate without requiring the upstream `tooltips.txt` resource.

const DEFAULT_TOOLTIPS: &[&str] = &[
    "Use /help to list available commands.",
    "Use /status to inspect the current session.",
    "Use /model to view or switch the active model.",
    "Use /diff to review local changes before asking for edits.",
];

pub(crate) fn get_tooltip(plan: Option<&str>, fast_mode_enabled: bool) -> Option<String> {
    if fast_mode_enabled {
        return Some("Fast mode is enabled for lower-latency responses.".to_string());
    }
    if let Some(plan) = plan.filter(|value| !value.trim().is_empty()) {
        return Some(format!("Current plan: {plan}"));
    }
    DEFAULT_TOOLTIPS.first().map(|tip| (*tip).to_string())
}
pub(crate) fn all_tooltips() -> Vec<&'static str> {
    DEFAULT_TOOLTIPS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_mode_tooltip_takes_priority() {
        assert!(get_tooltip(Some("pro"), true)
            .unwrap()
            .contains("Fast mode"));
    }

    #[test]
    fn all_tooltips_exposes_default_rotation() {
        let tips = all_tooltips();
        assert!(tips.len() >= 4);
        assert!(tips.iter().any(|tip| tip.contains("/status")));
    }
}
