//! Teammate layout and color management.
//!
//! Corresponds to TypeScript: `utils/swarm/teammateLayoutManager.ts`
//!
//! Manages in-session teammate color assignments through a round-robin
//! cache.  Pane-level layout operations (tmux/iTerm2 window splitting,
//! pane border status, command-to-pane routing) are intentionally not
//! supported in allthecodes — teammates run in-process only.
//!
//! The color cache is session-scoped so the same teammate ID always
//! resolves to the same display color within a session.

use std::collections::HashMap;
use std::sync::Mutex;

use super::constants::AGENT_COLORS;

// ---------------------------------------------------------------------------
// Session-scoped color cache
// ---------------------------------------------------------------------------

/// Map from teammate ID → index into `AGENT_COLORS`.
static COLOR_CACHE: std::sync::LazyLock<Mutex<HashMap<String, usize>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Next round-robin index (append-only — never decremented in normal use).
static NEXT_INDEX: std::sync::LazyLock<Mutex<usize>> = std::sync::LazyLock::new(|| Mutex::new(0));

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Assign a display color to a teammate for this session.
///
/// Returns the same color on repeated calls with the same `teammate_id`,
/// guaranteeing stable coloring across turns.  Colors are handed out in
/// round-robin order from [`AGENT_COLORS`].
///
/// Corresponds to TS: `assignTeammateColor(teammateId)`
pub fn assign_teammate_color(teammate_id: &str) -> &'static str {
    let mut cache = COLOR_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(&idx) = cache.get(teammate_id) {
        return AGENT_COLORS[idx];
    }
    let mut next = NEXT_INDEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let idx = *next % AGENT_COLORS.len();
    *next += 1;
    cache.insert(teammate_id.to_string(), idx);
    AGENT_COLORS[idx]
}

/// Get the previously assigned color for a teammate, if any.
///
/// Corresponds to TS: `getTeammateColor(teammateId)`
pub fn get_teammate_color(teammate_id: &str) -> Option<&'static str> {
    let cache = COLOR_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.get(teammate_id).map(|&idx| AGENT_COLORS[idx])
}

/// Record an already-decided color for a teammate in the session cache.
pub fn remember_teammate_color(teammate_id: &str, color: &str) {
    let Some(idx) = AGENT_COLORS
        .iter()
        .position(|candidate| *candidate == color)
    else {
        return;
    };
    let mut cache = COLOR_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(teammate_id.to_string(), idx);
}

/// Clear all session-local color assignments.
///
/// Called during team cleanup so a subsequent team session starts with a
/// fresh color palette.
///
/// Corresponds to TS: `clearTeammateColors()`
pub fn clear_teammate_colors() {
    let mut cache = COLOR_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.clear();
    let mut next = NEXT_INDEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *next = 0;
}

/// Return the list of available agent display colors.
///
/// Corresponds to TS: `AGENT_COLORS` (imported from `agentColorManager.js`)
pub fn available_colors() -> &'static [&'static str] {
    AGENT_COLORS
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn same_id_returns_same_color() {
        clear_teammate_colors();
        let c1 = assign_teammate_color("alice");
        let c2 = assign_teammate_color("alice");
        assert_eq!(c1, c2);
    }

    #[test]
    #[serial]
    fn different_ids_get_different_colors_when_palette_allows() {
        clear_teammate_colors();
        let c1 = assign_teammate_color("alice");
        let c2 = assign_teammate_color("bob");
        assert_ne!(c1, c2);
    }

    #[test]
    #[serial]
    fn colors_cycle_after_palette_exhausted() {
        clear_teammate_colors();
        // Assign one more than the palette size
        let mut assigned = Vec::new();
        for i in 0..=AGENT_COLORS.len() {
            assigned.push(assign_teammate_color(&format!("agent-{i}")));
        }
        // First and last should both be valid colors (cycle wraps around)
        assert!(AGENT_COLORS.contains(&assigned[0]));
        assert!(AGENT_COLORS.contains(&assigned[AGENT_COLORS.len()]));
        // Wrapped color should match
        assert_eq!(assigned[0], assigned[AGENT_COLORS.len()]);
    }

    #[test]
    #[serial]
    fn get_color_returns_none_for_unassigned() {
        clear_teammate_colors();
        assert!(get_teammate_color("nobody").is_none());
    }

    #[test]
    #[serial]
    fn get_color_returns_assigned() {
        clear_teammate_colors();
        let color = assign_teammate_color("charlie");
        assert_eq!(get_teammate_color("charlie"), Some(color));
    }

    #[test]
    #[serial]
    fn remember_color_updates_cache() {
        clear_teammate_colors();
        remember_teammate_color("eve", "cyan");
        assert_eq!(get_teammate_color("eve"), Some("cyan"));
    }

    #[test]
    #[serial]
    fn clear_resets_all_state() {
        clear_teammate_colors();
        let c1 = assign_teammate_color("dave");
        clear_teammate_colors();
        // After clear, assigning the same ID should restart from index 0
        let c2 = assign_teammate_color("dave");
        assert_eq!(c1, c2);
        assert!(get_teammate_color("dave").is_some());
    }

    #[test]
    fn available_colors_matches_constants() {
        assert_eq!(available_colors().len(), AGENT_COLORS.len());
        assert!(available_colors().contains(&"red"));
        assert!(available_colors().contains(&"cyan"));
    }
}
