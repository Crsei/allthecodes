//! OAuth endpoint configuration and authorization URL construction.

use anyhow::{bail, Result};

/// OAuth login method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthMethod {
    /// Claude.ai subscriber (Bearer token mode)
    ClaudeAi,
    /// Console user (API Key creation mode)
    Console,
    /// OpenAI Codex (ChatGPT OAuth)
    OpenAiCodex,
}

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTH_URL: &str = "https://platform.claude.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const CREATE_API_KEY_URL: &str =
    "https://api.anthropic.com/api/oauth/claude_cli/create_api_key";
pub const MANUAL_REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";

pub const OPENAI_CODEX_OAUTH_CLIENT_ID_ENV: &str = "OPENAI_CODEX_OAUTH_CLIENT_ID";
pub const OPENAI_CODEX_OAUTH_AUTH_URL_ENV: &str = "OPENAI_CODEX_OAUTH_AUTH_URL";
pub const OPENAI_CODEX_OAUTH_TOKEN_URL_ENV: &str = "OPENAI_CODEX_OAUTH_TOKEN_URL";
pub const OPENAI_CODEX_OAUTH_REDIRECT_URI_ENV: &str = "OPENAI_CODEX_OAUTH_REDIRECT_URI";
pub const OPENAI_CODEX_OAUTH_SCOPES_ENV: &str = "OPENAI_CODEX_OAUTH_SCOPES";

pub const OPENAI_CODEX_OAUTH_AUTH_URL_DEFAULT: &str = "https://auth.openai.com/authorize";
pub const OPENAI_CODEX_OAUTH_TOKEN_URL_DEFAULT: &str = "https://auth.openai.com/oauth/token";
pub const OPENAI_CODEX_OAUTH_REDIRECT_URI_DEFAULT: &str = "http://localhost:1455/callback";

pub const CLAUDE_AI_SCOPES: &[&str] = &[
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
];

pub const CONSOLE_SCOPES: &[&str] = &["org:create_api_key", "user:profile"];
pub const OPENAI_CODEX_SCOPES: &[&str] = &["openid", "profile", "offline_access"];

/// Return the display name for an OAuth method.
pub fn display_name(method: OAuthMethod) -> &'static str {
    match method {
        OAuthMethod::ClaudeAi => "Claude.ai",
        OAuthMethod::Console => "Console",
        OAuthMethod::OpenAiCodex => "OpenAI Codex",
    }
}

/// Return the persisted method name used in credentials.
pub fn method_storage_name(method: OAuthMethod) -> &'static str {
    match method {
        OAuthMethod::ClaudeAi => "claude_ai",
        OAuthMethod::Console => "console",
        OAuthMethod::OpenAiCodex => "openai_codex",
    }
}

/// Parse a persisted credentials method name.
pub fn method_from_storage_name(name: &str) -> Option<OAuthMethod> {
    if name.eq_ignore_ascii_case("claude_ai") {
        Some(OAuthMethod::ClaudeAi)
    } else if name.eq_ignore_ascii_case("console") {
        Some(OAuthMethod::Console)
    } else if name.eq_ignore_ascii_case("openai_codex") {
        Some(OAuthMethod::OpenAiCodex)
    } else {
        None
    }
}

/// Return the default scopes for the given login method.
pub fn scopes_for(method: OAuthMethod) -> &'static [&'static str] {
    match method {
        OAuthMethod::ClaudeAi => CLAUDE_AI_SCOPES,
        OAuthMethod::Console => CONSOLE_SCOPES,
        OAuthMethod::OpenAiCodex => OPENAI_CODEX_SCOPES,
    }
}

/// Resolve scopes, allowing OpenAI Codex override via env var.
pub fn resolved_scopes_for(method: OAuthMethod) -> Vec<String> {
    if method != OAuthMethod::OpenAiCodex {
        return scopes_for(method).iter().map(|s| s.to_string()).collect();
    }

    if let Ok(value) = std::env::var(OPENAI_CODEX_OAUTH_SCOPES_ENV) {
        let parsed: Vec<String> = value
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }

    scopes_for(method).iter().map(|s| s.to_string()).collect()
}

/// Resolve OAuth client id by method.
pub fn client_id_for(method: OAuthMethod) -> Result<String> {
    match method {
        OAuthMethod::ClaudeAi | OAuthMethod::Console => Ok(CLIENT_ID.to_string()),
        OAuthMethod::OpenAiCodex => {
            let value = std::env::var(OPENAI_CODEX_OAUTH_CLIENT_ID_ENV)
                .unwrap_or_default()
                .trim()
                .to_string();
            if value.is_empty() {
                bail!(
                    "Missing {}. Set your OpenAI Codex OAuth client id before /login 4.",
                    OPENAI_CODEX_OAUTH_CLIENT_ID_ENV
                );
            }
            Ok(value)
        }
    }
}

/// Resolve OAuth authorization URL by method.
pub fn auth_url_for(method: OAuthMethod) -> String {
    match method {
        OAuthMethod::ClaudeAi | OAuthMethod::Console => AUTH_URL.to_string(),
        OAuthMethod::OpenAiCodex => std::env::var(OPENAI_CODEX_OAUTH_AUTH_URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| OPENAI_CODEX_OAUTH_AUTH_URL_DEFAULT.to_string()),
    }
}

/// Resolve OAuth token URL by method.
pub fn token_url_for(method: OAuthMethod) -> String {
    match method {
        OAuthMethod::ClaudeAi | OAuthMethod::Console => TOKEN_URL.to_string(),
        OAuthMethod::OpenAiCodex => std::env::var(OPENAI_CODEX_OAUTH_TOKEN_URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| OPENAI_CODEX_OAUTH_TOKEN_URL_DEFAULT.to_string()),
    }
}

/// Resolve OAuth redirect URI by method.
pub fn redirect_uri_for(method: OAuthMethod) -> String {
    match method {
        OAuthMethod::ClaudeAi | OAuthMethod::Console => MANUAL_REDIRECT_URL.to_string(),
        OAuthMethod::OpenAiCodex => std::env::var(OPENAI_CODEX_OAUTH_REDIRECT_URI_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| OPENAI_CODEX_OAUTH_REDIRECT_URI_DEFAULT.to_string()),
    }
}

/// Build the full authorization URL with PKCE parameters.
pub fn authorization_url(method: OAuthMethod, code_challenge: &str, state: &str) -> Result<String> {
    let auth_url = auth_url_for(method);
    let client_id = client_id_for(method)?;
    let redirect_uri = redirect_uri_for(method);
    let scopes = resolved_scopes_for(method).join(" ");
    Ok(format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        auth_url,
        urlencoding::encode(&client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&scopes),
        code_challenge,
        state,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_ai_scopes() {
        let scopes = scopes_for(OAuthMethod::ClaudeAi);
        assert!(scopes.contains(&"user:inference"));
        assert!(!scopes.contains(&"org:create_api_key"));
    }

    #[test]
    fn test_console_scopes() {
        let scopes = scopes_for(OAuthMethod::Console);
        assert!(scopes.contains(&"org:create_api_key"));
        assert!(!scopes.contains(&"user:inference"));
    }

    #[test]
    fn test_openai_codex_default_scopes() {
        let scopes = scopes_for(OAuthMethod::OpenAiCodex);
        assert!(scopes.contains(&"openid"));
        assert!(scopes.contains(&"offline_access"));
    }

    #[test]
    fn test_authorization_url_contains_required_params() {
        let url = authorization_url(OAuthMethod::ClaudeAi, "test_challenge", "test_state").unwrap();
        assert!(url.starts_with(AUTH_URL));
        assert!(url.contains("client_id=9d1c250a"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=test_state"));
        assert!(url.contains("redirect_uri="));
    }

    #[test]
    fn test_openai_codex_requires_client_id() {
        let saved = std::env::var(OPENAI_CODEX_OAUTH_CLIENT_ID_ENV).ok();
        std::env::remove_var(OPENAI_CODEX_OAUTH_CLIENT_ID_ENV);
        let res = authorization_url(OAuthMethod::OpenAiCodex, "c", "s");
        assert!(res.is_err());
        if let Some(value) = saved {
            std::env::set_var(OPENAI_CODEX_OAUTH_CLIENT_ID_ENV, value);
        }
    }

    #[test]
    fn test_method_storage_roundtrip() {
        let method = OAuthMethod::OpenAiCodex;
        let stored = method_storage_name(method);
        assert_eq!(method_from_storage_name(stored), Some(method));
    }
}
