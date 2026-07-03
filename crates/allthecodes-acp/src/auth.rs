//! ACP authentication methods: auth/login and auth/logout.
//!
//! Maps ACP auth requests to the existing allthecodes login flow.

use agent_client_protocol_schema::v2;
use agent_client_protocol_schema::v2::{AuthMethod as AcpAuthMethod, AuthMethodAgent};
use allthecodes_auth::AuthMethod as ResolvedAuthMethod;
use allthecodes_config::settings::EffectiveSettings;

const ACP_LOGIN_METHOD_ID: &str = "allthecodes-login";

/// Build the list of ACP auth methods based on current auth state.
///
/// Returns an empty vec when already authenticated, or a single agent-managed
/// login method when credentials are missing or unusable.
pub fn resolve_acp_auth_methods(settings: &EffectiveSettings) -> Vec<AcpAuthMethod> {
    if has_usable_auth(settings) {
        return vec![];
    }

    vec![AcpAuthMethod::Agent(
        AuthMethodAgent::new(ACP_LOGIN_METHOD_ID, "allthecodes login").description(Some(
            "Run allthecodes authentication using existing /login and /login-code flows.".into(),
        )),
    )]
}

/// Backward-compatible wrapper for older call sites.
pub fn build_auth_methods() -> Vec<AcpAuthMethod> {
    resolve_acp_auth_methods(&EffectiveSettings::default())
}

/// ACP auth/login handler.
pub fn handle_login(params: v2::LoginAuthRequest) -> Result<v2::LoginAuthResponse, v2::Error> {
    if params.method_id.to_string() != ACP_LOGIN_METHOD_ID {
        return Err(v2::Error::method_not_found()
            .data(format!("unknown auth method: {}", params.method_id)));
    }

    let mut meta = serde_json::Map::new();
    meta.insert(
        "instructions".to_string(),
        serde_json::Value::String(login_instructions()),
    );

    Ok(v2::LoginAuthResponse::new().meta(meta))
}

/// ACP auth/logout handler.
pub fn handle_logout() -> Result<v2::LogoutAuthResponse, v2::Error> {
    // Attempt OAuth logout; this is best-effort and idempotent.
    let _ = allthecodes_auth::oauth_logout();

    Ok(v2::LogoutAuthResponse::new())
}

fn has_usable_auth(settings: &EffectiveSettings) -> bool {
    settings_has_credentials(settings)
        || native_auth_resolves()
        || codex_auth_resolves()
        || openai_env_present()
}

fn settings_has_credentials(settings: &EffectiveSettings) -> bool {
    non_empty(settings.api_key.as_deref())
        || settings
            .env
            .iter()
            .any(|(key, value)| is_auth_env_key(key) && !value.trim().is_empty())
}

fn native_auth_resolves() -> bool {
    matches!(
        allthecodes_auth::try_resolve_auth(),
        Ok(auth) if !matches!(auth, ResolvedAuthMethod::None)
    )
}

fn codex_auth_resolves() -> bool {
    matches!(
        allthecodes_auth::try_resolve_codex_auth_token(),
        Ok(Some(token)) if !token.trim().is_empty()
    )
}

fn openai_env_present() -> bool {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn is_auth_env_key(key: &str) -> bool {
    matches!(
        key,
        "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "OPENAI_API_KEY" | "OPENAI_CODEX_AUTH_TOKEN"
    )
}

fn non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn login_instructions() -> String {
    [
        "Authenticate allthecodes from the agent session.",
        "Use /login with an API key or provider name.",
        "For OAuth, run /login claude-ai, /login console, or /login codex-oauth, then paste the browser code with /login-code <code>.",
        "After login completes, retry the ACP initialize/auth flow.",
    ]
    .join("\n")
}
