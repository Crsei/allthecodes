//! Rust-side helper for assistant text messages.
//!
//! Classifies assistant text into API error categories and renders
//! human-readable messages for each error type, or passes through
//! ordinary text content unchanged.

#[cfg(test)]
use crate::ui::theme::Theme;

/// API error categories derived from assistant response text.
///
/// Mirrors the TypeScript switch in `AssistantTextMessage.tsx:73-173`
/// and the error constants in `services/api/errors.ts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantApiError<'a> {
    /// Rate-limited (exact match or `isRateLimitErrorMessage`)
    RateLimit(&'a str),
    /// Prompt exceeded context window
    PromptTooLong(&'a str),
    /// Insufficient credit balance
    CreditBalanceTooLow(&'a str),
    /// Invalid API Key (OAuth session)
    InvalidApiKey(&'a str),
    /// Invalid API Key (external source, e.g. env var)
    InvalidApiKeyExternal(&'a str),
    /// Organization disabled (env key)
    OrgDisabledEnvKey(&'a str),
    /// Organization disabled (env key + OAuth coexist)
    OrgDisabledEnvKeyWithOAuth(&'a str),
    /// OAuth token revoked
    TokenRevoked(&'a str),
    /// API request timed out
    ApiTimeout(&'a str),
    /// Opus capacity exhausted
    CustomOffSwitch(&'a str),
    /// User-initiated abort
    UserAbort,
    /// Catch-all for any message starting with "API Error"
    ApiErrorPrefix(&'a str),
}

/// Error message constants matching TS `services/api/errors.ts`.
const RATE_LIMIT_MESSAGES: &[&str] =
    &["rate_limit", "rate limit", "Rate limit", "rate_limit_error"];

const PROMPT_TOO_LONG_MESSAGES: &[&str] = &[
    "prompt too long",
    "prompt is too long",
    "too long for context window",
    "context_length_exceeded",
];

const CREDIT_BALANCE_MESSAGES: &[&str] = &[
    "credit balance is too low",
    "insufficient credit",
    "billing credit",
    "billing: credit",
];

const API_TIMEOUT_MESSAGES: &[&str] = &["request timed out", "timeout error", "timed out"];

/// Token strings used in TS to identify specific error messages.
const TOKEN_REVOKED_TOKENS: &[&str] = &["token revoked", "oauth token", "token has been revoked"];

/// Classify assistant response text into an API error category.
///
/// Priority order matches the TS switch statement in `AssistantTextMessage.tsx:73-173`.
pub fn classify_assistant_text(text: &str) -> Option<AssistantApiError<'_>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();

    // 1. Rate limit
    if RATE_LIMIT_MESSAGES.iter().any(|m| lower.contains(m)) {
        return Some(AssistantApiError::RateLimit(trimmed));
    }

    // 2. Prompt too long / context length
    if PROMPT_TOO_LONG_MESSAGES.iter().any(|m| lower.contains(m)) {
        return Some(AssistantApiError::PromptTooLong(trimmed));
    }

    // 3. Credit balance
    if CREDIT_BALANCE_MESSAGES.iter().any(|m| lower.contains(m)) {
        return Some(AssistantApiError::CreditBalanceTooLow(trimmed));
    }

    // 4. Token revoked (check before generic API key check)
    if TOKEN_REVOKED_TOKENS.iter().any(|m| lower.contains(m)) {
        return Some(AssistantApiError::TokenRevoked(trimmed));
    }

    // 5. Custom off switch (Opus capacity)
    if lower.contains("custom_off_switch")
        || lower.contains("high demand for opus")
        || (lower.contains("opus") && lower.contains("capacity"))
    {
        return Some(AssistantApiError::CustomOffSwitch(trimmed));
    }

    // 6. Timeout
    if API_TIMEOUT_MESSAGES.iter().any(|m| lower.contains(m)) {
        return Some(AssistantApiError::ApiTimeout(trimmed));
    }

    // 7. Invalid API key (OAuth context)
    if lower.contains("not logged in") {
        return Some(AssistantApiError::InvalidApiKey(trimmed));
    }

    // 8. Invalid API key (external env var)
    if lower.contains("invalid api key") || (lower.contains("api key") && lower.contains("invalid"))
    {
        // Closer inspection: if "token" appears nearby it's revoked, already caught above
        return Some(AssistantApiError::InvalidApiKeyExternal(trimmed));
    }

    // 9. Org disabled
    if lower.contains("organization disabled") || lower.contains("org disabled") {
        if lower.contains("oauth") || lower.contains("login") {
            return Some(AssistantApiError::OrgDisabledEnvKeyWithOAuth(trimmed));
        }
        return Some(AssistantApiError::OrgDisabledEnvKey(trimmed));
    }

    // 10. User abort
    if lower.contains("request interrupted by user")
        || lower.contains("user interrupted")
        || trimmed.contains("[Request interrupted by user]")
    {
        return Some(AssistantApiError::UserAbort);
    }

    // 11. Generic API error prefix
    if lower.starts_with("api error") {
        return Some(AssistantApiError::ApiErrorPrefix(trimmed));
    }

    None
}

/// Render a classified API error as a human-readable string.
pub fn render_api_error(error: &AssistantApiError) -> String {
    match *error {
        AssistantApiError::RateLimit(msg) => {
            format!("Error occurred: {msg}")
        }
        AssistantApiError::PromptTooLong(text)
        | AssistantApiError::CreditBalanceTooLow(text)
        | AssistantApiError::InvalidApiKey(text)
        | AssistantApiError::InvalidApiKeyExternal(text)
        | AssistantApiError::TokenRevoked(text)
        | AssistantApiError::ApiTimeout(text)
        | AssistantApiError::CustomOffSwitch(text) => format!("Error occurred: {text}"),
        AssistantApiError::OrgDisabledEnvKey(text) => {
            format!("Error occurred: {text}")
        }
        AssistantApiError::OrgDisabledEnvKeyWithOAuth(text) => {
            format!("Error occurred: {text}")
        }
        AssistantApiError::UserAbort => "[Request interrupted by user]".to_string(),
        AssistantApiError::ApiErrorPrefix(s) => {
            let truncated = if s.len() > 1000 {
                format!("{}...", &s[..997])
            } else {
                s.to_string()
            };
            format!("Error occurred: {truncated}")
        }
    }
}

/// Render assistant text message, performing error classification first.
///
/// If the text matches an API error pattern, the classified error message
/// is rendered; otherwise the plain text is returned.
#[cfg(test)]
pub fn render_assistant_text_message(text: &str, _theme: &Theme) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "<empty text>".to_string();
    }

    if let Some(error) = classify_assistant_text(trimmed) {
        return render_api_error(&error);
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_placeholder() {
        let result = render_assistant_text_message("", &Theme::default());
        assert_eq!(result, "<empty text>");
    }

    #[test]
    fn whitespace_text_returns_placeholder() {
        let result = render_assistant_text_message("   ", &Theme::default());
        assert_eq!(result, "<empty text>");
    }

    #[test]
    fn plain_text_passes_through() {
        let result =
            render_assistant_text_message("Hello, I can help with that.", &Theme::default());
        assert_eq!(result, "Hello, I can help with that.");
    }

    #[test]
    fn rate_limit_detected() {
        let result =
            render_assistant_text_message("rate_limit_error: too many requests", &Theme::default());
        assert_eq!(
            result,
            "Error occurred: rate_limit_error: too many requests"
        );
    }

    #[test]
    fn prompt_too_long_detected() {
        let result = render_assistant_text_message(
            "prompt is too long for context window",
            &Theme::default(),
        );
        assert_eq!(
            result,
            "Error occurred: prompt is too long for context window"
        );
    }

    #[test]
    fn credit_balance_too_low_detected() {
        let result = render_assistant_text_message("credit balance is too low", &Theme::default());
        assert_eq!(result, "Error occurred: credit balance is too low");
    }

    #[test]
    fn invalid_api_key_oauth_detected() {
        let result =
            render_assistant_text_message("Not logged in · Please run /login", &Theme::default());
        assert_eq!(result, "Error occurred: Not logged in · Please run /login");
    }

    #[test]
    fn invalid_api_key_external_detected() {
        let result = render_assistant_text_message("Invalid API key provided", &Theme::default());
        assert_eq!(result, "Error occurred: Invalid API key provided");
    }

    #[test]
    fn token_revoked_detected() {
        let result = render_assistant_text_message("OAuth token revoked", &Theme::default());
        assert_eq!(result, "Error occurred: OAuth token revoked");
    }

    #[test]
    fn custom_off_switch_detected() {
        let result = render_assistant_text_message("high demand for Opus", &Theme::default());
        assert!(result.contains("high demand for Opus"));
    }

    #[test]
    fn timeout_detected() {
        let result =
            render_assistant_text_message("request timed out after 120s", &Theme::default());
        assert_eq!(result, "Error occurred: request timed out after 120s");
    }

    #[test]
    fn user_abort_detected() {
        let result =
            render_assistant_text_message("[Request interrupted by user]", &Theme::default());
        assert_eq!(result, "[Request interrupted by user]");
    }

    #[test]
    fn api_error_prefix_detected() {
        let result =
            render_assistant_text_message("API Error: something went wrong", &Theme::default());
        assert!(result.contains("Error occurred: API Error: something went wrong"));
    }

    #[test]
    fn api_error_prefix_truncated() {
        let long = format!("API Error: {}", "x".repeat(2000));
        let result = render_assistant_text_message(&long, &Theme::default());
        // 3 for "API Error: " prefix + 997 truncated content + "..." = ~1000 chars total + "Error occurred: "
        assert!(result.len() < "Error occurred: ".len() + 1000 + 10);
    }

    #[test]
    fn org_disabled_env_key() {
        let result = render_assistant_text_message("organization disabled", &Theme::default());
        assert!(result.contains("Error occurred: organization disabled"));
    }

    #[test]
    fn classify_empty_returns_none() {
        assert_eq!(classify_assistant_text(""), None);
        assert_eq!(classify_assistant_text("  "), None);
    }

    #[test]
    fn classify_normal_text_returns_none() {
        assert_eq!(
            classify_assistant_text("Here is the code you requested"),
            None
        );
    }

    #[test]
    fn classify_rate_limit() {
        assert_eq!(
            classify_assistant_text("rate_limit_error"),
            Some(AssistantApiError::RateLimit("rate_limit_error"))
        );
    }

    #[test]
    fn classify_prompt_too_long() {
        assert_eq!(
            classify_assistant_text("context_length_exceeded"),
            Some(AssistantApiError::PromptTooLong("context_length_exceeded"))
        );
    }

    #[test]
    fn classify_user_abort() {
        assert_eq!(
            classify_assistant_text("[Request interrupted by user]"),
            Some(AssistantApiError::UserAbort)
        );
    }
}
