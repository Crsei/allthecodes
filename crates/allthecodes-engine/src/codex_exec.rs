pub const NATIVE_BACKEND_NAME: &str = "native";
pub const CODEX_BACKEND_NAME: &str = "codex";
pub const DEFAULT_CODEX_MODEL: &str = "gpt-5.6-sol";

pub fn normalize_backend(value: Option<&str>) -> String {
    let raw = value.unwrap_or(NATIVE_BACKEND_NAME).trim();
    if raw.eq_ignore_ascii_case(CODEX_BACKEND_NAME) {
        CODEX_BACKEND_NAME.to_string()
    } else {
        NATIVE_BACKEND_NAME.to_string()
    }
}

pub fn is_codex_backend(value: &str) -> bool {
    value.eq_ignore_ascii_case(CODEX_BACKEND_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_backend_name() {
        assert_eq!(normalize_backend(None), NATIVE_BACKEND_NAME);
        assert_eq!(normalize_backend(Some("codex")), CODEX_BACKEND_NAME);
        assert_eq!(normalize_backend(Some("CODEX")), CODEX_BACKEND_NAME);
        assert_eq!(normalize_backend(Some("other")), NATIVE_BACKEND_NAME);
    }

    #[test]
    fn detects_codex_backend_case_insensitive() {
        assert!(is_codex_backend("codex"));
        assert!(is_codex_backend("CoDeX"));
        assert!(!is_codex_backend("native"));
    }
}
