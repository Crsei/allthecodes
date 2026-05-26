//! Provider-related URL helpers.

use super::types::OPENAI_CODEX_PROVIDER_NAME;

/// Build the endpoint URL for an OpenAI-compatible provider.
///
/// For the codex provider the path varies depending on the base URL:
/// - ends with `/responses`      -> no suffix
/// - ends with `/codex`          -> `/responses`
/// - anything else               -> `/codex/responses`
///
/// For all other providers the path is always `/chat/completions`.
pub(crate) fn build_openai_compat_url(base_url: &str, provider_name: &str) -> String {
    if provider_name.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_NAME) {
        let base_url = base_url.trim_end_matches('/');
        let endpoint = if base_url.ends_with("/responses") {
            ""
        } else if base_url.ends_with("/codex") {
            "/responses"
        } else {
            "/codex/responses"
        };
        return format!("{base_url}{endpoint}");
    }

    let endpoint = "/chat/completions";
    format!("{}{}", base_url.trim_end_matches('/'), endpoint)
}

/// Return `true` when `provider_name` is the openai-codex provider.
pub(crate) fn is_openai_codex_provider(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_NAME)
}
