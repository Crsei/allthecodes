//! Pure helpers for tool-result post-processing.

use serde_json::Value;

/// Enforce a maximum string payload size for tool results.
///
/// Non-string JSON payloads are returned unchanged. String payloads preserve a
/// head and tail sample with an omission marker when they exceed `max_bytes`.
pub fn enforce_result_size(data: Value, max_bytes: usize) -> Value {
    match data {
        Value::String(s) if s.len() > max_bytes => Value::String(truncate_string(&s, max_bytes)),
        other => other,
    }
}

fn truncate_string(value: &str, max_bytes: usize) -> String {
    let head_len = floor_char_boundary(value, max_bytes / 2);
    let tail_len = floor_char_boundary(value, max_bytes / 4);
    let tail_start = ceil_char_boundary(value, value.len().saturating_sub(tail_len));
    let head = &value[..head_len];
    let tail = &value[tail_start..];
    let omitted = value.len().saturating_sub(head.len() + tail.len());

    format!(
        "{}\n\n[... {} characters omitted ...]\n\n{}",
        head, omitted, tail
    )
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index < value.len() && !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn leaves_non_string_payloads_unchanged() {
        let data = json!({"key": "value"});
        assert_eq!(enforce_result_size(data.clone(), 10), data);
    }

    #[test]
    fn leaves_boundary_string_unchanged() {
        let data = json!("a".repeat(1000));
        assert_eq!(enforce_result_size(data.clone(), 1000), data);
    }

    #[test]
    fn truncates_large_string_with_omission_marker() {
        let result = enforce_result_size(json!("x".repeat(10_000)), 1000);
        let rendered = result.as_str().expect("string result");

        assert!(rendered.len() < 10_000);
        assert!(rendered.contains("characters omitted"));
        assert!(rendered.starts_with(&"x".repeat(500)));
        assert!(rendered.ends_with(&"x".repeat(250)));
    }

    #[test]
    fn truncates_at_utf8_boundaries() {
        let result = enforce_result_size(json!("é".repeat(1000)), 999);
        let rendered = result.as_str().expect("string result");

        assert!(rendered.contains("characters omitted"));
        assert!(rendered.is_char_boundary(rendered.len()));
    }
}
