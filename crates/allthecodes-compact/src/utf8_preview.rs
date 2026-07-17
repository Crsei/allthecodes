//! UTF-8-safe character-budget helpers shared by compact previews.
//!
//! The compact pipeline uses Unicode scalar counts for all public `*_CHARS`
//! budgets. Byte lengths remain available to callers that explicitly need
//! them, but no preview may use an arbitrary byte offset as a string boundary.

/// Count Unicode scalar values in `text`.
pub(crate) fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Return the first `limit` Unicode scalar values as an owned string.
pub(crate) fn head_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Return the last `limit` Unicode scalar values as an owned string.
pub(crate) fn tail_chars(text: &str, limit: usize) -> String {
    text.chars()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Split a string into non-overlapping head/tail previews using scalar counts.
/// The returned omitted count uses the same scalar-count unit.
pub(crate) fn head_tail_chars(
    text: &str,
    head_limit: usize,
    tail_limit: usize,
) -> (String, String, usize) {
    let total = char_count(text);
    let head_count = head_limit.min(total);
    let tail_count = tail_limit.min(total.saturating_sub(head_count));
    let omitted = total.saturating_sub(head_count.saturating_add(tail_count));
    (
        head_chars(text, head_count),
        tail_chars(text, tail_count),
        omitted,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_and_tail_never_split_utf8() {
        let text = format!("{}中🙂e\u{301}{}", "a".repeat(3), "z".repeat(3));
        let (head, tail, omitted) = head_tail_chars(&text, 4, 4);

        assert_eq!(head.chars().count(), 4);
        assert_eq!(tail.chars().count(), 4);
        assert_eq!(omitted, text.chars().count() - 8);
        assert!(std::str::from_utf8(head.as_bytes()).is_ok());
        assert!(std::str::from_utf8(tail.as_bytes()).is_ok());
    }

    #[test]
    fn tail_budget_does_not_overlap_head() {
        let (head, tail, omitted) = head_tail_chars("abcdef", 5, 5);
        assert_eq!(head, "abcde");
        assert_eq!(tail, "f");
        assert_eq!(omitted, 0);
    }
}
