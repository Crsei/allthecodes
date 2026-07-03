//! ACP authentication methods: auth/login and auth/logout.
//!
//! Maps ACP auth requests to the existing allthecodes login flow.

use agent_client_protocol_schema::v2;
use agent_client_protocol_schema::v2::{AuthMethod, AuthMethodAgent};

/// Build the list of ACP auth methods based on current auth state.
/// Returns empty vec when already authenticated, or a single agent-managed
/// login method when credentials are missing.
pub fn build_auth_methods() -> Vec<AuthMethod> {
    // Check if we have usable credentials
    let has_env_creds = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        "OPENAI_CODEX_AUTH_TOKEN",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some());
    let has_creds = has_env_creds || allthecodes_auth::resolve_codex_auth_token().is_some();

    if has_creds {
        return vec![];
    }

    vec![AuthMethod::Agent(AuthMethodAgent::new(
        "allthecodes-login",
        "allthecodes login",
    )
    .description(Some(
        "Run allthecodes authentication using existing /login and /login-code flows.".into(),
    )))]
}

/// ACP auth/login handler.
pub fn handle_login(
    params: v2::LoginAuthRequest,
) -> Result<v2::LoginAuthResponse, v2::Error> {
    if params.method_id.to_string() != "allthecodes-login" {
        return Err(v2::Error::method_not_found().data(format!(
            "unknown auth method: {}",
            params.method_id
        )));
    }

    Ok(v2::LoginAuthResponse::new())
}

/// ACP auth/logout handler.
pub fn handle_logout() -> Result<v2::LogoutAuthResponse, v2::Error> {
    // Attempt OAuth logout; this is best-effort
    let _ = allthecodes_auth::oauth_logout();

    Ok(v2::LogoutAuthResponse::new())
}
