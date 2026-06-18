//! `/login-code` command — complete OAuth login by exchanging an authorization code.
//!
//! Usage:
//!   /login-code <authorization-code>
//!
//! This is the second step of the OAuth flow started by `/login claude-ai`,
//! `/login console`, or `/login codex-oauth`.

use allthecodes_config::settings::{self, RawSettings};
use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_auth::oauth::{client, config, pkce};
use allthecodes_auth::{api_key, token};

/// Pending OAuth state (PKCE verifier, state, method).
static PENDING_OAUTH: parking_lot::Mutex<Option<PendingOAuth>> = parking_lot::Mutex::new(None);

struct PendingOAuth {
    method: config::OAuthMethod,
    verifier: String,
    state: String,
}

/// Start a pending OAuth flow. Called from the named `/login ...` OAuth commands.
///
/// Generates PKCE params, stores them, and returns the message with the auth URL.
pub fn start_pending(method: config::OAuthMethod) -> String {
    let verifier = pkce::generate_code_verifier();
    let challenge = pkce::generate_code_challenge(&verifier);
    let state = pkce::generate_state();
    let url = match config::authorization_url(method, &challenge, &state) {
        Ok(url) => url,
        Err(e) => return format!("Cannot start OAuth flow: {}", e),
    };

    *PENDING_OAUTH.lock() = Some(PendingOAuth {
        method,
        verifier,
        state,
    });

    let method_name = config::display_name(method);

    format!(
        "Opening {} authorization...\n\n\
         Please visit this URL to authorize:\n\n  {}\n\n\
         After authorizing, paste the code:\n  /login-code <paste-code-here>",
        method_name, url
    )
}

pub struct LoginCodeHandler;

#[async_trait]
impl CommandHandler for LoginCodeHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let code = args.trim();
        if code.is_empty() {
            return Ok(CommandResult::Output(
                "Usage: /login-code <authorization-code>\n\
                 Start the OAuth flow first with /login claude-ai, /login console, or /login codex-oauth"
                    .to_string(),
            ));
        }

        let pending = PENDING_OAUTH.lock().take();
        let pending = match pending {
            Some(p) => p,
            None => {
                return Ok(CommandResult::Output(
                    "No pending OAuth flow. Start one with /login claude-ai, /login console, or /login codex-oauth"
                        .to_string(),
                ));
            }
        };

        let code = extract_authorization_code(code);

        // Exchange code for tokens
        let token_resp = match client::exchange_code(
            pending.method,
            &code,
            &pending.verifier,
            &pending.state,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(CommandResult::Output(format!(
                    "Token exchange failed: {}\n\nPlease retry with /login claude-ai, /login console, or /login codex-oauth",
                    e
                )));
            }
        };

        // Store tokens
        let expires_at = chrono::Utc::now().timestamp() + token_resp.expires_in as i64;
        let method_str = config::method_storage_name(pending.method);
        let scopes: Vec<String> = token_resp
            .scope
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let stored = token::StoredToken {
            access_token: token_resp.access_token.clone(),
            refresh_token: token_resp.refresh_token.clone(),
            expires_at: Some(expires_at),
            token_type: "bearer".into(),
            scopes,
            oauth_method: Some(method_str.to_string()),
        };

        if let Err(e) = token::save_token(&stored) {
            return Ok(CommandResult::Output(format!(
                "Failed to save OAuth tokens: {}",
                e
            )));
        }

        if pending.method == config::OAuthMethod::OpenAiCodex {
            let mut msg = "Logged in successfully (OpenAI Codex OAuth).".to_string();
            append_provider_selection_message(
                &mut msg,
                settings::API_PROVIDER_OPENAI_CODEX,
                Some("codex"),
                ctx,
            );
            return Ok(CommandResult::Output(msg));
        }

        // Console mode: create API key
        if pending.method == config::OAuthMethod::Console {
            match client::create_api_key(&token_resp.access_token).await {
                Ok(raw_key) => {
                    if let Err(e) = api_key::store_api_key(&raw_key) {
                        return Ok(CommandResult::Output(format!(
                            "OAuth tokens saved, but keychain storage failed: {}",
                            e
                        )));
                    }
                    let mut msg =
                        "Logged in successfully (Console). API key stored to keychain.".to_string();
                    append_provider_selection_message(
                        &mut msg,
                        settings::API_PROVIDER_ANTHROPIC,
                        Some("native"),
                        ctx,
                    );
                    return Ok(CommandResult::Output(msg));
                }
                Err(e) => {
                    return Ok(CommandResult::Output(format!(
                        "OAuth tokens saved, but API key creation failed: {}\n\
                         You can retry with /login console",
                        e
                    )));
                }
            }
        }

        let mut msg = "Logged in successfully (Claude.ai).".to_string();
        append_provider_selection_message(
            &mut msg,
            settings::API_PROVIDER_ANTHROPIC,
            Some("native"),
            ctx,
        );
        Ok(CommandResult::Output(msg))
    }
}

fn extract_authorization_code(input: &str) -> String {
    let trimmed = input.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return trimmed.to_string();
    }

    let parsed = match url::Url::parse(trimmed) {
        Ok(url) => url,
        Err(_) => return trimmed.to_string(),
    };
    for (key, value) in parsed.query_pairs() {
        if key == "code" && !value.is_empty() {
            return value.into_owned();
        }
    }
    trimmed.to_string()
}

fn append_provider_selection_message(
    msg: &mut String,
    api_provider: &str,
    backend: Option<&str>,
    ctx: &mut CommandContext,
) {
    let provider_msg = persist_provider_selection(api_provider, backend, ctx);
    msg.push_str("\n\n");
    msg.push_str(&provider_msg);
}

fn persist_provider_selection(
    api_provider: &str,
    backend: Option<&str>,
    ctx: &mut CommandContext,
) -> String {
    let path = settings::user_settings_path();
    let mut raw = if path.exists() {
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|txt| serde_json::from_str::<RawSettings>(&txt).map_err(anyhow::Error::from))
        {
            Ok(raw) => raw,
            Err(error) => {
                return format!(
                    "Provider selected for this session, but user settings were not updated: {}",
                    error
                );
            }
        }
    } else {
        RawSettings::default()
    };

    let profile_name = settings::auth_profile_name_for_provider(api_provider);
    let mut profile = raw
        .auth_profiles
        .as_ref()
        .and_then(|profiles| settings::get_auth_profile_for_provider(profiles, api_provider))
        .cloned()
        .unwrap_or_default();
    profile.api_provider = Some(api_provider.to_string());
    ctx.app_state.settings.api_provider = Some(api_provider.to_string());
    ctx.app_state.settings.active_auth_profile = Some(profile_name.to_string());
    ctx.app_state
        .settings
        .sources
        .insert("apiProvider".to_string(), settings::SettingsSource::User);
    ctx.app_state.settings.sources.insert(
        "activeAuthProfile".to_string(),
        settings::SettingsSource::User,
    );
    if let Some(backend) = backend {
        profile.backend = Some(backend.to_string());
        ctx.app_state.main_loop_backend = backend.to_string();
        ctx.app_state.settings.backend = Some(backend.to_string());
        ctx.app_state
            .settings
            .sources
            .insert("backend".to_string(), settings::SettingsSource::User);
    }
    let selected_model = if api_provider == settings::API_PROVIDER_OPENAI_CODEX {
        let model = resolve_codex_default_model(ctx, &raw);
        let available_models = settings::codex_model_ids();
        let model_capabilities = settings::codex_model_capabilities();
        profile.model = Some(model.clone());
        profile.available_models = Some(available_models.clone());
        profile.model_capabilities = Some(model_capabilities.clone());
        ctx.app_state.main_loop_model = model.clone();
        ctx.app_state.settings.model = Some(model);
        ctx.app_state.settings.available_models = available_models;
        ctx.app_state.settings.model_capabilities = model_capabilities;
        ctx.app_state
            .settings
            .sources
            .insert("model".to_string(), settings::SettingsSource::User);
        ctx.app_state.settings.sources.insert(
            "availableModels".to_string(),
            settings::SettingsSource::User,
        );
        ctx.app_state.settings.sources.insert(
            "modelCapabilities".to_string(),
            settings::SettingsSource::User,
        );
        ctx.app_state.settings.model.clone()
    } else {
        None
    };
    settings::upsert_auth_profile(&mut raw, profile_name, profile, true);
    ctx.app_state.settings.auth_profiles = raw.auth_profiles.clone().unwrap_or_default();
    ctx.app_state
        .settings
        .sources
        .insert("authProfiles".to_string(), settings::SettingsSource::User);

    match settings::write_user_settings(&raw) {
        Ok(path) => format!(
            "Selected apiProvider={}{}{} (persisted to {}).",
            api_provider,
            backend
                .map(|value| format!(", backend={value}"))
                .unwrap_or_default(),
            selected_model
                .as_deref()
                .map(|value| format!(", model={value}"))
                .unwrap_or_default(),
            path.display()
        ),
        Err(error) => format!(
            "Provider selected for this session, but user settings were not updated: {}",
            error
        ),
    }
}

fn resolve_codex_default_model(ctx: &CommandContext, raw: &RawSettings) -> String {
    std::env::var(allthecodes_api::api::client::OPENAI_CODEX_MODEL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .and_then(|value| resolve_codex_model_candidate(&value, raw, ctx))
        .or_else(|| {
            raw.env
                .as_ref()
                .and_then(|env| env.get(allthecodes_api::api::client::OPENAI_CODEX_MODEL_ENV))
                .map(|value| value.trim().to_string())
                .and_then(|value| resolve_codex_model_candidate(&value, raw, ctx))
        })
        .or_else(|| {
            raw.auth_profiles
                .as_ref()
                .and_then(|profiles| profiles.get("codex"))
                .and_then(|profile| {
                    profile
                        .env
                        .as_ref()
                        .and_then(|env| {
                            env.get(allthecodes_api::api::client::OPENAI_CODEX_MODEL_ENV)
                        })
                        .or(profile.model.as_ref())
                })
                .map(|value| value.trim().to_string())
                .and_then(|value| resolve_codex_model_candidate(&value, raw, ctx))
        })
        .or_else(|| {
            raw.model
                .as_deref()
                .map(|value| value.trim().to_string())
                .and_then(|value| resolve_codex_model_candidate(&value, raw, ctx))
        })
        .or_else(|| {
            ctx.app_state
                .settings
                .model
                .as_deref()
                .map(|value| value.trim().to_string())
                .and_then(|value| resolve_codex_model_candidate(&value, raw, ctx))
        })
        .or_else(|| {
            allthecodes_api::api::providers::get_provider(settings::API_PROVIDER_OPENAI_CODEX)
                .map(|provider| provider.default_model.to_string())
        })
        .unwrap_or_else(allthecodes_types::models::default_model_id)
}

fn resolve_codex_model_candidate(
    model: &str,
    raw: &RawSettings,
    ctx: &CommandContext,
) -> Option<String> {
    let trimmed = model.trim();
    if trimmed.is_empty() || !is_codex_model_choice(trimmed) {
        return None;
    }
    Some(
        codex_alias_model(trimmed, raw, ctx)
            .unwrap_or_else(|| allthecodes_types::models::resolve_model_alias(trimmed)),
    )
}

fn codex_alias_model(alias: &str, raw: &RawSettings, ctx: &CommandContext) -> Option<String> {
    let value = match alias.trim().to_ascii_uppercase().as_str() {
        "SOTA" => raw
            .sota_model
            .as_deref()
            .or(ctx.app_state.settings.sota_model.as_deref()),
        "MOTA" => raw
            .mota_model
            .as_deref()
            .or(ctx.app_state.settings.mota_model.as_deref()),
        "FOTA" => raw
            .fota_model
            .as_deref()
            .or(ctx.app_state.settings.fota_model.as_deref()),
        _ => None,
    }?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn is_codex_model_choice(model: &str) -> bool {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.eq_ignore_ascii_case("SOTA")
        || trimmed.eq_ignore_ascii_case("MOTA")
        || trimmed.eq_ignore_ascii_case("FOTA")
    {
        return true;
    }

    settings::codex_model_ids()
        .iter()
        .any(|model| model.eq_ignore_ascii_case(trimmed))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    static PENDING_OAUTH_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct PendingOAuthTestGuard {
        _guard: MutexGuard<'static, ()>,
    }

    impl PendingOAuthTestGuard {
        fn acquire() -> Self {
            let guard = PENDING_OAUTH_TEST_LOCK
                .lock()
                .expect("pending OAuth test lock poisoned");
            let _ = PENDING_OAUTH.lock().take();
            Self { _guard: guard }
        }
    }

    impl Drop for PendingOAuthTestGuard {
        fn drop(&mut self) {
            let _ = PENDING_OAUTH.lock().take();
        }
    }

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: AppState::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    // ---- start_pending() pure helpers ----

    #[test]
    fn test_start_pending_claude_ai_contains_url() {
        let _guard = PendingOAuthTestGuard::acquire();
        let msg = start_pending(config::OAuthMethod::ClaudeAi);
        assert!(
            msg.contains("Claude.ai"),
            "should mention Claude.ai, got: {}",
            msg
        );
        assert!(
            msg.contains("https://"),
            "should contain an auth URL, got: {}",
            msg
        );
        assert!(
            msg.contains("/login-code"),
            "should instruct user to use /login-code, got: {}",
            msg
        );
    }

    #[test]
    fn test_start_pending_console_contains_url() {
        let _guard = PendingOAuthTestGuard::acquire();
        let msg = start_pending(config::OAuthMethod::Console);
        assert!(
            msg.contains("Console"),
            "should mention Console, got: {}",
            msg
        );
        assert!(msg.contains("https://"));
    }

    #[test]
    fn test_start_pending_openai_codex_missing_client_id() {
        let _guard = PendingOAuthTestGuard::acquire();
        let saved = std::env::var(config::OPENAI_CODEX_OAUTH_CLIENT_ID_ENV).ok();
        std::env::remove_var(config::OPENAI_CODEX_OAUTH_CLIENT_ID_ENV);
        let msg = start_pending(config::OAuthMethod::OpenAiCodex);
        assert!(msg.contains("Cannot start OAuth flow"));
        if let Some(value) = saved {
            std::env::set_var(config::OPENAI_CODEX_OAUTH_CLIENT_ID_ENV, value);
        }
    }

    #[test]
    fn test_start_pending_stores_state() {
        let _guard = PendingOAuthTestGuard::acquire();
        start_pending(config::OAuthMethod::ClaudeAi);
        let pending = PENDING_OAUTH.lock().take();
        assert!(
            pending.is_some(),
            "start_pending should store pending OAuth state"
        );
    }

    // ---- LoginCodeHandler error paths (no network required) ----

    #[tokio::test]
    async fn test_login_code_empty_args_shows_usage() {
        let handler = LoginCodeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage"), "expected usage hint, got: {}", text);
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_login_code_no_pending_flow() {
        let _guard = PendingOAuthTestGuard::acquire();
        let handler = LoginCodeHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("some-fake-code", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("No pending OAuth flow"),
                    "expected no-pending message, got: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }

    #[test]
    fn test_extract_authorization_code_from_url() {
        let code = extract_authorization_code("https://example.com/callback?code=abc123&state=x");
        assert_eq!(code, "abc123");
    }
}
