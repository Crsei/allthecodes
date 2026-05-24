pub use allthecodes_utils::tokens::get_context_window_size;

const AUTO_COMPACT_THRESHOLD_RATIO: f64 = 0.8;
const EXACT_FALLBACK_BAND_RATIO: f64 = 0.05;

/// Return the auto-compaction threshold for a model.
pub fn auto_compact_threshold_tokens(model: &str) -> u64 {
    let context_window = get_context_window_size(model);
    (context_window as f64 * AUTO_COMPACT_THRESHOLD_RATIO) as u64
}

/// Return true when the heuristic estimate is close enough to the threshold
/// that a provider exact count can prevent false-positive or false-negative
/// auto-compaction decisions.
pub fn should_check_exact_for_auto_compact(estimated_tokens: u64, model: &str) -> bool {
    let threshold = auto_compact_threshold_tokens(model);
    let band = ((get_context_window_size(model) as f64 * EXACT_FALLBACK_BAND_RATIO) as u64).max(1);
    estimated_tokens.abs_diff(threshold) <= band
}

/// Check if auto-compaction should be triggered based on token count.
///
/// Returns true when the estimated token usage exceeds 80% of the
/// model's context window, indicating that a compaction pass should
/// be run to free up space.
pub fn should_auto_compact(estimated_tokens: u64, model: &str) -> bool {
    estimated_tokens > auto_compact_threshold_tokens(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_auto_compact_below_threshold() {
        // 80% of 200k = 160k
        assert!(!should_auto_compact(100_000, "claude-sonnet-4-20250514"));
        assert!(!should_auto_compact(159_999, "claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_should_auto_compact_above_threshold() {
        assert!(should_auto_compact(160_001, "claude-sonnet-4-20250514"));
        assert!(should_auto_compact(200_000, "claude-opus-4-20250514"));
    }

    #[test]
    fn test_should_auto_compact_respects_1m_suffix() {
        assert!(!should_auto_compact(
            200_000,
            "claude-sonnet-4-20250514[1m]"
        ));
        assert!(should_auto_compact(800_001, "claude-sonnet-4-20250514[1m]"));
    }

    #[test]
    fn test_should_check_exact_for_auto_compact_near_threshold() {
        // 80% of 200k is 160k; the exact-count band is +/- 5% of the window.
        assert!(should_check_exact_for_auto_compact(
            150_000,
            "claude-sonnet-4-20250514"
        ));
        assert!(should_check_exact_for_auto_compact(
            160_000,
            "claude-sonnet-4-20250514"
        ));
        assert!(should_check_exact_for_auto_compact(
            170_000,
            "claude-sonnet-4-20250514"
        ));
        assert!(!should_check_exact_for_auto_compact(
            149_999,
            "claude-sonnet-4-20250514"
        ));
        assert!(!should_check_exact_for_auto_compact(
            170_001,
            "claude-sonnet-4-20250514"
        ));
    }

    #[test]
    fn test_context_window_sizes() {
        assert_eq!(get_context_window_size("claude-opus-4-20250514"), 200_000);
        assert_eq!(get_context_window_size("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(
            get_context_window_size("claude-haiku-3-5-20241022"),
            200_000
        );
        assert_eq!(
            get_context_window_size("claude-sonnet-4-20250514[1m]"),
            1_000_000
        );
        assert_eq!(get_context_window_size("unknown-model"), 200_000);
    }
}
