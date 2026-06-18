//! `/login` command - authenticate with an LLM provider.
//!
//! Usage:
//!   /login                  - interactive login (choose method)
//!   /login status           - show current auth status
//!   /login sk-...           - store Anthropic/OpenAI API key directly
//!   /login claude-code      - select the Claude Code / Anthropic-compatible profile
//!   /login codex            - select the OpenAI Codex profile
//!   /login custom           - select an existing custom auth profile
//!   /login bedrock|vertex   - enable a cloud provider for this process

use allthecodes_config::settings::{self, RawSettings};
use anyhow::Result;
use async_trait::async_trait;

use super::login_code;
use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_auth::{self as auth, oauth::OAuthMethod};

pub struct LoginHandler;

#[async_trait]
impl CommandHandler for LoginHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args == "status" {
            return Ok(CommandResult::Output(auth_status_text()));
        }

        if args.starts_with("sk-ant-") {
            return Ok(CommandResult::Output(store_anthropic_api_key(args, ctx)));
        }

        if args.starts_with("sk-") {
            return Ok(CommandResult::Output(store_openai_api_key(args, ctx)));
        }

        if args.is_empty() {
            return Ok(CommandResult::Output(login_menu()));
        }

        let mut parts = args.split_whitespace();
        let head = parts.next().unwrap_or_default();
        let rest = parts.collect::<Vec<_>>().join(" ");

        match head {
            "claude-code" | "claude_code" | "claude" | "anthropic_method" | "anthropic-method"
            | "anthropic" => {
                if rest.trim().is_empty() {
                    Ok(CommandResult::Output(select_provider_profile(
                        settings::API_PROVIDER_ANTHROPIC,
                        Some("native"),
                        ctx,
                    )))
                } else {
                    execute_claude_code_method(&rest, ctx)
                }
            }
            "claude-ai" | "claude_ai" => Ok(CommandResult::Output(login_code::start_pending(
                OAuthMethod::ClaudeAi,
            ))),
            "console" => Ok(CommandResult::Output(login_code::start_pending(
                OAuthMethod::Console,
            ))),
            "openai_api" | "openai-api" | "openai" => {
                if rest.trim().is_empty() {
                    Ok(CommandResult::Output(openai_api_prompt()))
                } else {
                    Ok(CommandResult::Output(store_openai_api_key(&rest, ctx)))
                }
            }
            "openai_codex" | "openai-codex" | "codex" => Ok(CommandResult::Output(
                select_provider_profile(settings::API_PROVIDER_OPENAI_CODEX, Some("codex"), ctx),
            )),
            "codex-oauth" | "openai-codex-oauth" => {
                Ok(CommandResult::Output(start_codex_oauth(ctx)))
            }
            "codex-cli" => Ok(CommandResult::Output(check_codex_cli(ctx))),
            "custom" => Ok(CommandResult::Output(select_custom_profile(ctx))),
            "bedrock" | "aws" => Ok(CommandResult::Output(enable_bedrock_session())),
            "vertex" | "vertex-ai" | "gcp" => Ok(CommandResult::Output(enable_vertex_session())),
            "cloud" | "platform" | "platforms" => Ok(CommandResult::Output(cloud_setup_text())),
            _ => Ok(CommandResult::Output(format!(
                "Unknown option: \"{}\"\n\n{}",
                args,
                login_menu()
            ))),
        }
    }
}

fn login_menu() -> String {
    "Select login method:\n\
     \n  claude-code       Select Claude Code / Anthropic-compatible profile\
     \n  claude-ai         Start Claude.ai OAuth\
     \n  console           Start Console OAuth\
     \n  codex             Select OpenAI Codex profile\
     \n  codex-oauth       Start OpenAI Codex OAuth\
     \n  codex-cli         Check/import Codex CLI credentials\
     \n  custom            Select existing custom auth profile\
     \n  openai-api        Store an OpenAI API key\
     \n  bedrock           Enable AWS Bedrock for this session\
     \n  vertex            Enable GCP Vertex AI for this session"
        .to_string()
}

fn claude_code_login_menu() -> String {
    "Claude Code / Anthropic-compatible login methods:\n\
     \n  api-key           Paste an API key with /login sk-ant-api03-...\
     \n  claude-ai         Claude.ai OAuth (Pro/Max subscription)\
     \n  console           Console OAuth (API billing)\
     \n\nType /login claude-code api-key, /login claude-ai, /login console, or paste a key with /login sk-ant-api03-..."
        .to_string()
}

fn openai_api_prompt() -> String {
    "Paste your OpenAI API key:\n  /login openai_api sk-...".to_string()
}

fn execute_claude_code_method(args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
    match args.trim() {
        "api" | "api-key" | "api_key" => Ok(CommandResult::Output(
            "Paste your Claude Code / Anthropic-compatible API key:\n  /login sk-ant-api03-..."
                .to_string(),
        )),
        "claude" | "claude-ai" | "claude_ai" => Ok(CommandResult::Output(
            login_code::start_pending(OAuthMethod::ClaudeAi),
        )),
        "console" => Ok(CommandResult::Output(login_code::start_pending(
            OAuthMethod::Console,
        ))),
        key if key.starts_with("sk-ant-") => {
            Ok(CommandResult::Output(store_anthropic_api_key(key, ctx)))
        }
        other => Ok(CommandResult::Output(format!(
            "Unknown Claude Code / Anthropic-compatible login option: \"{}\"\n\n{}",
            other,
            claude_code_login_menu()
        ))),
    }
}

fn auth_status_text() -> String {
    let status = if let Some(cloud_status) = cloud_auth_status_text() {
        cloud_status
    } else if let Some(openai_status) = openai_api_status_text() {
        openai_status
    } else if let Some(codex_status) = codex_auth_status_text() {
        codex_status
    } else {
        let current = match auth::try_resolve_auth() {
            Ok(current) => current,
            Err(error) => return format!("Authentication error: {error}"),
        };
        match &current {
            auth::AuthMethod::ApiKey(key) => {
                let source = if std::env::var("ANTHROPIC_API_KEY")
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
                {
                    "env ANTHROPIC_API_KEY"
                } else {
                    "system keychain"
                };
                format!(
                    "Authenticated: API Key {} (source: {})",
                    mask_key(key),
                    source
                )
            }
            auth::AuthMethod::ExternalToken(_) => {
                "Authenticated: External Token (ANTHROPIC_AUTH_TOKEN)".to_string()
            }
            auth::AuthMethod::OAuthToken { method, .. } => {
                format!("Authenticated: OAuth ({})", method)
            }
            auth::AuthMethod::None => "Not authenticated".to_string(),
        }
    };
    append_active_profile_status(status)
}

fn append_active_profile_status(mut status: String) -> String {
    let Some(profile_name) = active_auth_profile_display_name() else {
        return status;
    };
    status.push_str("\nActive profile: ");
    status.push_str(&profile_name);
    status
}

fn active_auth_profile_display_name() -> Option<String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    settings::load_effective(&cwd)
        .ok()
        .and_then(|loaded| loaded.effective.active_auth_profile)
        .map(|name| settings::display_auth_profile_name(name.trim()).to_string())
        .filter(|name| !name.is_empty())
}

fn openai_api_status_text() -> Option<String> {
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.trim().is_empty() {
            return Some(format!(
                "Authenticated: OpenAI API Key {} (source: env OPENAI_API_KEY)",
                mask_key(key.trim())
            ));
        }
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let provider = settings::load_effective(&cwd)
        .ok()
        .and_then(|loaded| loaded.effective.api_provider)
        .and_then(|value| settings::normalize_api_provider(&value).map(str::to_string));
    if provider.as_deref() != Some(settings::API_PROVIDER_OPENAI) {
        return None;
    }

    match auth::api_key::load_openai_api_key() {
        Ok(Some(key)) if auth::api_key::validate_openai_api_key(&key) => Some(format!(
            "Authenticated: OpenAI API Key {} (source: system keychain)",
            mask_key(key.trim())
        )),
        Ok(Some(_)) => Some("OpenAI API key in system keychain is invalid.".to_string()),
        Ok(None) => {
            Some("OpenAI API provider selected, but no OpenAI API key is stored.".to_string())
        }
        Err(error) => Some(format!("OpenAI API keychain is not usable: {error}")),
    }
}

fn cloud_auth_status_text() -> Option<String> {
    if allthecodes_api::api::client::is_env_truthy("ALLTHECODES_USE_FOUNDRY") {
        return Some(foundry_status_text());
    }
    if allthecodes_api::api::client::is_env_truthy("ALLTHECODES_USE_BEDROCK") {
        return Some(bedrock_status_text());
    }
    if allthecodes_api::api::client::is_env_truthy("ALLTHECODES_USE_VERTEX") {
        return Some(vertex_status_text());
    }
    None
}

fn enable_bedrock_session() -> String {
    std::env::set_var("ALLTHECODES_USE_BEDROCK", "1");
    std::env::remove_var("ALLTHECODES_USE_VERTEX");
    std::env::remove_var("ALLTHECODES_USE_FOUNDRY");

    let mut lines = vec![
        "AWS Bedrock provider enabled for this allthecodes session.".to_string(),
        "Next non-command prompt will use Bedrock because API clients are rebuilt per turn."
            .to_string(),
        String::new(),
        bedrock_status_text(),
    ];
    if allthecodes_api::api::bedrock::BedrockAuth::from_env().is_none() {
        lines.push(String::new());
        lines.push(bedrock_setup_text());
    } else {
        lines.push(String::new());
        lines.push(
            "For future sessions, set ALLTHECODES_USE_BEDROCK=1 before launching allthecodes."
                .to_string(),
        );
    }
    lines.join("\n")
}

fn enable_vertex_session() -> String {
    std::env::set_var("ALLTHECODES_USE_VERTEX", "1");
    std::env::remove_var("ALLTHECODES_USE_BEDROCK");
    std::env::remove_var("ALLTHECODES_USE_FOUNDRY");

    let mut lines = vec![
        "GCP Vertex AI provider enabled for this allthecodes session.".to_string(),
        "Next non-command prompt will use Vertex because API clients are rebuilt per turn."
            .to_string(),
        String::new(),
        vertex_status_text(),
    ];
    if allthecodes_api::api::vertex::resolve_project_id().is_none()
        || allthecodes_api::api::vertex::VertexAccessToken::from_env_or_gcloud().is_none()
    {
        lines.push(String::new());
        lines.push(vertex_setup_text());
    } else {
        lines.push(String::new());
        lines.push(
            "For future sessions, set ALLTHECODES_USE_VERTEX=1 before launching allthecodes."
                .to_string(),
        );
    }
    lines.join("\n")
}

fn bedrock_status_text() -> String {
    let region = allthecodes_api::api::bedrock::resolve_region();
    let base_url = std::env::var("ANTHROPIC_BEDROCK_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let model = std::env::var("ANTHROPIC_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());

    let auth = match allthecodes_api::api::bedrock::BedrockAuth::from_env() {
        Some(allthecodes_api::api::bedrock::BedrockAuth::BearerToken(token)) => {
            format!("Bearer token {}", mask_secret(&token))
        }
        Some(allthecodes_api::api::bedrock::BedrockAuth::AwsCredentials(creds)) => {
            let session = if creds.session_token.is_some() {
                " + AWS_SESSION_TOKEN"
            } else {
                ""
            };
            format!(
                "SigV4 credentials {}{}",
                mask_secret(&creds.access_key_id),
                session
            )
        }
        None => {
            "missing (set AWS_BEARER_TOKEN_BEDROCK or AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY)"
                .to_string()
        }
    };

    format!(
        "Provider: AWS Bedrock\n\
         Enabled: {}\n\
         Region: {}\n\
         Auth: {}\n\
         Model: {}\n\
         Base URL: {}",
        allthecodes_api::api::client::is_env_truthy("ALLTHECODES_USE_BEDROCK"),
        region,
        auth,
        model,
        base_url.unwrap_or_else(|| "default bedrock-runtime endpoint".to_string())
    )
}

fn vertex_status_text() -> String {
    let default_region = allthecodes_api::api::vertex::resolve_region();
    let model = std::env::var("ANTHROPIC_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "claude-sonnet-4-5-20250929".to_string());
    let region = allthecodes_api::api::vertex::resolve_region_for_model_with_default(
        Some(&model),
        &default_region,
    );
    let project_id = allthecodes_api::api::vertex::resolve_project_id()
        .unwrap_or_else(|| "missing (set ANTHROPIC_VERTEX_PROJECT_ID)".to_string());
    let token_source = vertex_token_source();

    format!(
        "Provider: GCP Vertex AI\n\
         Enabled: {}\n\
         Project: {}\n\
         Region: {}\n\
         Auth: {}\n\
         Model: {}",
        allthecodes_api::api::client::is_env_truthy("ALLTHECODES_USE_VERTEX"),
        project_id,
        region,
        token_source,
        model
    )
}

fn vertex_token_source() -> String {
    if std::env::var("ALLTHECODES_VERTEX_ACCESS_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return "ALLTHECODES_VERTEX_ACCESS_TOKEN".to_string();
    }
    if std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return "GOOGLE_OAUTH_ACCESS_TOKEN".to_string();
    }
    if allthecodes_api::api::vertex::VertexAccessToken::from_env_or_gcloud().is_some() {
        return "gcloud application-default access token".to_string();
    }
    "missing (set ALLTHECODES_VERTEX_ACCESS_TOKEN or run gcloud auth application-default login)"
        .to_string()
}

fn foundry_status_text() -> String {
    let validation = allthecodes_api::api::providers::validate_provider_name("azure-foundry");
    let diagnostic = validation
        .diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.as_str())
        .unwrap_or(allthecodes_api::api::providers::FOUNDRY_UNSUPPORTED_REASON);
    format!(
        "Provider: Microsoft Foundry\n\
         Enabled: true\n\
         Status: unsupported\n\
         Diagnostic: {}\n\
         Action: unset ALLTHECODES_USE_FOUNDRY or choose /login bedrock or /login vertex.",
        diagnostic
    )
}

fn codex_auth_status_text() -> Option<String> {
    if std::env::var("OPENAI_CODEX_AUTH_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return Some("Authenticated: OpenAI Codex OAuth (env OPENAI_CODEX_AUTH_TOKEN)".to_string());
    }

    // Check allthecodes's own credentials.json
    if let Ok(Some(stored)) = auth::token::load_token() {
        let method = stored.oauth_method.as_deref().unwrap_or_default();
        if method.eq_ignore_ascii_case("openai_codex") {
            return if auth::token::is_token_expired(&stored) {
                Some(
                    "OpenAI Codex OAuth token is expired. Run /login codex-oauth to refresh."
                        .to_string(),
                )
            } else {
                Some("Authenticated: OpenAI Codex OAuth (stored credentials)".to_string())
            };
        }
    }

    // Check Codex CLI fallback
    match auth::codex_cli::read_codex_cli_credential() {
        Ok(Some(cred)) => {
            return if auth::codex_cli::is_credential_expired(&cred) {
                Some(
                    "OpenAI Codex OAuth (from Codex CLI) is expired. \
                     Run /login codex-cli to refresh or /login codex-oauth for a fresh login."
                        .to_string(),
                )
            } else {
                Some(
                    "Authenticated: OpenAI Codex OAuth (from Codex CLI ~/.codex/auth.json)"
                        .to_string(),
                )
            };
        }
        Ok(None) => {}
        Err(error) => {
            return Some(format!("Codex CLI auth.json is not usable: {error}"));
        }
    }

    None
}

fn cloud_setup_text() -> String {
    format!(
        "{}\n\n{}\n\n{}",
        bedrock_setup_text(),
        vertex_setup_text(),
        foundry_setup_text()
    )
}

fn bedrock_setup_text() -> String {
    "AWS Bedrock setup:\n\
     1. Set ALLTHECODES_USE_BEDROCK=1.\n\
     2. Set AWS_REGION or AWS_DEFAULT_REGION (default: us-east-1).\n\
     3. Use one auth mode:\n\
        - AWS_BEARER_TOKEN_BEDROCK=<bedrock-api-key>\n\
        - AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY (+ AWS_SESSION_TOKEN if needed)\n\
     4. Optional: ANTHROPIC_MODEL and ANTHROPIC_BEDROCK_BASE_URL.\n\
     Current session shortcut: /login bedrock"
        .to_string()
}

fn vertex_setup_text() -> String {
    "GCP Vertex AI setup:\n\
     1. Set ALLTHECODES_USE_VERTEX=1.\n\
     2. Set ANTHROPIC_VERTEX_PROJECT_ID (or GOOGLE_CLOUD_PROJECT / GCLOUD_PROJECT).\n\
     3. Set CLOUD_ML_REGION (default: us-east5) or per-model VERTEX_REGION_* overrides.\n\
     4. Provide auth with ALLTHECODES_VERTEX_ACCESS_TOKEN, GOOGLE_OAUTH_ACCESS_TOKEN,\n\
        or `gcloud auth application-default login`.\n\
     5. Optional: ANTHROPIC_MODEL.\n\
     Current session shortcut: /login vertex"
        .to_string()
}

fn foundry_setup_text() -> String {
    let validation = allthecodes_api::api::providers::validate_provider_name("azure-foundry");
    let diagnostic = validation
        .diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.as_str())
        .unwrap_or(allthecodes_api::api::providers::FOUNDRY_UNSUPPORTED_REASON);
    format!(
        "Microsoft Foundry setup:\n\
         Status: unsupported in this allthecodes build.\n\
         Diagnostic: {}\n\
         Do not set ALLTHECODES_USE_FOUNDRY for this release.",
        diagnostic
    )
}

fn start_codex_oauth(ctx: &mut CommandContext) -> String {
    let mut msg = login_code::start_pending(OAuthMethod::OpenAiCodex);
    if !msg.starts_with("Cannot start OAuth flow") {
        let provider_msg =
            persist_provider_selection(settings::API_PROVIDER_OPENAI_CODEX, Some("codex"), ctx);
        if let Some(provider_msg) = provider_msg {
            msg.push_str("\n\n");
            msg.push_str(&provider_msg);
        }
    }
    msg
}

fn select_provider_profile(
    api_provider: &str,
    backend: Option<&str>,
    ctx: &mut CommandContext,
) -> String {
    persist_provider_selection(api_provider, backend, ctx).unwrap_or_else(|| {
        format!(
            "Selected apiProvider={}{}.",
            api_provider,
            backend
                .map(|value| format!(", backend={value}"))
                .unwrap_or_default()
        )
    })
}

fn select_custom_profile(ctx: &mut CommandContext) -> String {
    let path = settings::user_settings_path();
    let mut raw = if path.exists() {
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|txt| serde_json::from_str::<RawSettings>(&txt).map_err(anyhow::Error::from))
        {
            Ok(raw) => raw,
            Err(error) => {
                return format!("Custom profile was not selected: {error}");
            }
        }
    } else {
        RawSettings::default()
    };

    let Some(profile) = raw
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(settings::AUTH_PROFILE_CUSTOM))
        .cloned()
    else {
        return "No authProfiles.custom entry is configured. Add one to settings.json, then run /login custom."
            .to_string();
    };

    raw.active_auth_profile = Some(settings::AUTH_PROFILE_CUSTOM.to_string());
    ctx.app_state.settings.active_auth_profile = Some(settings::AUTH_PROFILE_CUSTOM.to_string());
    ctx.app_state
        .settings
        .auth_profiles
        .insert(settings::AUTH_PROFILE_CUSTOM.to_string(), profile.clone());
    if let Some(api_provider) = profile.api_provider.as_deref() {
        ctx.app_state.settings.api_provider = Some(api_provider.to_string());
        ctx.app_state
            .settings
            .sources
            .insert("apiProvider".to_string(), settings::SettingsSource::User);
    }
    if let Some(backend) = profile.backend.as_deref() {
        ctx.app_state.main_loop_backend = backend.to_string();
        ctx.app_state.settings.backend = Some(backend.to_string());
        ctx.app_state
            .settings
            .sources
            .insert("backend".to_string(), settings::SettingsSource::User);
    }
    apply_profile_runtime_fields(&profile, ctx);
    ctx.app_state.settings.sources.insert(
        "activeAuthProfile".to_string(),
        settings::SettingsSource::User,
    );
    ctx.app_state
        .settings
        .sources
        .insert("authProfiles".to_string(), settings::SettingsSource::User);

    match settings::write_user_settings(&raw) {
        Ok(path) => format!(
            "Selected authProfile=custom{} (persisted to {}).",
            profile
                .api_provider
                .as_deref()
                .map(|value| format!(", apiProvider={value}"))
                .unwrap_or_default(),
            path.display()
        ),
        Err(error) => format!(
            "Custom profile selected for this session, but user settings were not updated: {error}"
        ),
    }
}

fn check_codex_cli(ctx: &mut CommandContext) -> String {
    let cred = match auth::codex_cli::read_codex_cli_credential() {
        Ok(Some(c)) => c,
        Ok(None) => {
            // Distinguish: file doesn't exist vs. wrong auth_mode
            if auth::codex_cli::codex_cli_auth_path().is_none() {
                return "Codex CLI not found. Install Codex CLI and run \
                        'codex' to log in first."
                    .to_string();
            }
            return "Codex CLI auth.json found but not usable \
                    (auth_mode is not chatgpt or tokens are missing). \
                    Use /login openai-api to paste your API key, or /login codex-oauth for OAuth."
                .to_string();
        }
        Err(error) => {
            return format!("Codex CLI auth.json is not usable: {error}");
        }
    };

    if !auth::codex_cli::is_credential_expired(&cred) {
        let mut msg = "Codex CLI credentials detected and valid. \
                allthecodes will use them automatically."
            .to_string();
        if let Some(provider_msg) =
            persist_provider_selection(settings::API_PROVIDER_OPENAI_CODEX, Some("codex"), ctx)
        {
            msg.push_str("\n\n");
            msg.push_str(&provider_msg);
        }
        return msg;
    }

    // Expired — try to refresh now
    match auth::try_resolve_codex_auth_token() {
        Ok(Some(_)) => {
            let mut msg = "Codex CLI token was expired but has been refreshed successfully. \
                 allthecodes will use it automatically."
                .to_string();
            if let Some(provider_msg) =
                persist_provider_selection(settings::API_PROVIDER_OPENAI_CODEX, Some("codex"), ctx)
            {
                msg.push_str("\n\n");
                msg.push_str(&provider_msg);
            }
            msg
        }
        Ok(None) => "Codex CLI token is expired and refresh failed. \
             Run /login codex-oauth for a fresh OAuth login, or re-login in Codex CLI."
            .to_string(),
        Err(error) => format!("Codex CLI token refresh failed: {error}"),
    }
}

fn store_anthropic_api_key(key: &str, ctx: &mut CommandContext) -> String {
    if !auth::api_key::validate_api_key(key) {
        return "Invalid API key format. Keys start with \"sk-ant-\" and are >20 chars.".into();
    }
    match auth::api_key::store_api_key(key) {
        Ok(()) => {
            let mut msg = format!(
                "Claude Code / Anthropic-compatible API key {} stored to keychain.",
                mask_key(key)
            );
            if let Some(provider_msg) =
                persist_provider_selection(settings::API_PROVIDER_ANTHROPIC, Some("native"), ctx)
            {
                msg.push_str("\n\n");
                msg.push_str(&provider_msg);
            }
            msg
        }
        Err(e) => format!("Failed to store API key: {}", e),
    }
}

fn store_openai_api_key(key: &str, ctx: &mut CommandContext) -> String {
    if !auth::api_key::validate_openai_api_key(key) {
        return "Invalid OpenAI API key format. Paste the full key after /login openai_api.".into();
    }
    match auth::api_key::store_openai_api_key(key) {
        Ok(()) => {
            let trimmed = key.trim();
            let mut msg = format!("OpenAI API Key {} stored to keychain.", mask_key(trimmed));
            if let Some(provider_msg) =
                persist_provider_selection(settings::API_PROVIDER_OPENAI, Some("native"), ctx)
            {
                msg.push_str("\n\n");
                msg.push_str(&provider_msg);
            }
            msg
        }
        Err(e) => format!("Failed to store OpenAI API key: {}", e),
    }
}

fn persist_provider_selection(
    api_provider: &str,
    backend: Option<&str>,
    ctx: &mut CommandContext,
) -> Option<String> {
    let path = settings::user_settings_path();
    let mut raw = if path.exists() {
        match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|txt| serde_json::from_str::<RawSettings>(&txt).map_err(anyhow::Error::from))
        {
            Ok(raw) => raw,
            Err(error) => {
                return Some(format!(
                    "Provider selected for this session, but user settings were not updated: {}",
                    error
                ));
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
    if let Some(backend) = backend {
        profile.backend = Some(backend.to_string());
        ctx.app_state.main_loop_backend = backend.to_string();
        ctx.app_state.settings.backend = Some(backend.to_string());
        ctx.app_state
            .settings
            .sources
            .insert("backend".to_string(), settings::SettingsSource::User);
    }
    if api_provider == settings::API_PROVIDER_OPENAI_CODEX {
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
    }
    let selected_model = apply_profile_runtime_fields(&profile, ctx);
    settings::upsert_auth_profile(&mut raw, profile_name, profile, true);
    ctx.app_state.settings.api_provider = Some(api_provider.to_string());
    ctx.app_state.settings.active_auth_profile = Some(profile_name.to_string());
    ctx.app_state.settings.auth_profiles = raw.auth_profiles.clone().unwrap_or_default();
    ctx.app_state
        .settings
        .sources
        .insert("apiProvider".to_string(), settings::SettingsSource::User);
    ctx.app_state.settings.sources.insert(
        "activeAuthProfile".to_string(),
        settings::SettingsSource::User,
    );
    ctx.app_state
        .settings
        .sources
        .insert("authProfiles".to_string(), settings::SettingsSource::User);

    match settings::write_user_settings(&raw) {
        Ok(path) => Some(format!(
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
        )),
        Err(error) => Some(format!(
            "Provider selected for this session, but user settings were not updated: {}",
            error
        )),
    }
}

fn apply_profile_runtime_fields(
    profile: &settings::ProviderProfileSettings,
    ctx: &mut CommandContext,
) -> Option<String> {
    if let Some(model) = profile.model.as_deref() {
        ctx.app_state.main_loop_model = model.to_string();
        ctx.app_state.settings.model = Some(model.to_string());
        ctx.app_state
            .settings
            .sources
            .insert("model".to_string(), settings::SettingsSource::User);
    }
    if let Some(available_models) = profile.available_models.as_ref() {
        ctx.app_state.settings.available_models = available_models.clone();
        ctx.app_state.settings.sources.insert(
            "availableModels".to_string(),
            settings::SettingsSource::User,
        );
    }
    if let Some(model_capabilities) = profile.model_capabilities.as_ref() {
        ctx.app_state.settings.model_capabilities = model_capabilities.clone();
        ctx.app_state.settings.sources.insert(
            "modelCapabilities".to_string(),
            settings::SettingsSource::User,
        );
    }
    if let Some(effort) = profile.model_reasoning_effort.as_deref() {
        ctx.app_state.settings.model_reasoning_effort = Some(effort.to_string());
        ctx.app_state.settings.sources.insert(
            "modelReasoningEffort".to_string(),
            settings::SettingsSource::User,
        );
    }
    ctx.app_state.settings.model.clone()
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

fn mask_secret(value: &str) -> String {
    if value.len() > 12 {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn mask_key(key: &str) -> String {
    if key.len() > 12 {
        format!("{}...{}", &key[..7], &key[key.len() - 4..])
    } else {
        "****".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
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

    #[test]
    fn test_mask_key_long() {
        let key = "sk-ant-api03-abcdefghijklmnop";
        let masked = mask_key(key);
        assert!(masked.starts_with("sk-ant-"));
        assert!(masked.contains("..."));
        assert!(masked.ends_with(&key[key.len() - 4..]));
    }

    #[test]
    fn test_mask_key_short() {
        let masked = mask_key("short");
        assert_eq!(masked, "****");
    }

    #[test]
    fn test_login_menu_contains_options() {
        let menu = login_menu();
        assert!(menu.contains("claude-code"));
        assert!(menu.contains("Claude Code / Anthropic-compatible"));
        assert!(menu.contains("claude-ai"));
        assert!(menu.contains("console"));
        assert!(menu.contains("codex-oauth"));
        assert!(menu.contains("openai-api"));
        assert!(!menu.contains("/login 1..7"));
    }

    #[test]
    fn test_claude_code_menu_contains_legacy_options() {
        let menu = claude_code_login_menu();
        assert!(menu.contains("Claude Code / Anthropic-compatible"));
        assert!(menu.contains("api-key"));
        assert!(menu.contains("claude-ai"));
        assert!(menu.contains("console"));
        assert!(menu.contains("sk-ant-api03"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_legacy_anthropic_alias_selects_claude_code_profile() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let mut ctx = test_ctx();

        let result = LoginHandler
            .execute("anthropic", &mut ctx)
            .await
            .expect("login command succeeds");

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("apiProvider=anthropic"));
                assert_eq!(
                    ctx.app_state.settings.active_auth_profile.as_deref(),
                    Some(settings::AUTH_PROFILE_CLAUDE_CODE)
                );
            }
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_named_codex_login_selects_profile_without_oauth() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let mut ctx = test_ctx();

        let result = LoginHandler
            .execute("codex", &mut ctx)
            .await
            .expect("login command succeeds");

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("apiProvider=openai-codex"));
                assert!(!text.contains("OAuth URL"));
                assert_eq!(
                    ctx.app_state.settings.active_auth_profile.as_deref(),
                    Some(settings::AUTH_PROFILE_CODEX)
                );
            }
            _ => panic!("expected output"),
        }
    }

    #[tokio::test]
    async fn test_numeric_login_shortcut_is_not_registered() {
        let mut ctx = test_ctx();

        let result = LoginHandler
            .execute("2", &mut ctx)
            .await
            .expect("login command succeeds");

        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown option"));
                assert!(text.contains("claude-ai"));
                assert!(!text.contains("/login 2"));
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn test_enable_bedrock_session_sets_flag_and_reports_status() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let _bedrock = EnvGuard::set("ALLTHECODES_USE_BEDROCK", None);
        let _vertex = EnvGuard::set("ALLTHECODES_USE_VERTEX", Some("1"));
        let _foundry = EnvGuard::set("ALLTHECODES_USE_FOUNDRY", Some("1"));
        let _bearer = EnvGuard::set("AWS_BEARER_TOKEN_BEDROCK", Some("bedrock-token-1234"));
        let _region = EnvGuard::set("AWS_REGION", Some("us-west-2"));

        let text = enable_bedrock_session();

        assert!(allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_BEDROCK"
        ));
        assert!(!allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_VERTEX"
        ));
        assert!(!allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_FOUNDRY"
        ));
        assert!(text.contains("AWS Bedrock provider enabled"));
        assert!(text.contains("Region: us-west-2"));
        assert!(text.contains("Bearer token"));
    }

    #[test]
    fn test_enable_vertex_session_sets_flag_and_reports_status() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let _bedrock = EnvGuard::set("ALLTHECODES_USE_BEDROCK", Some("1"));
        let _vertex = EnvGuard::set("ALLTHECODES_USE_VERTEX", None);
        let _foundry = EnvGuard::set("ALLTHECODES_USE_FOUNDRY", Some("1"));
        let _project = EnvGuard::set("ANTHROPIC_VERTEX_PROJECT_ID", Some("proj-123"));
        let _token = EnvGuard::set("ALLTHECODES_VERTEX_ACCESS_TOKEN", Some("vertex-token"));
        let _region = EnvGuard::set("CLOUD_ML_REGION", Some("europe-west4"));

        let text = enable_vertex_session();

        assert!(allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_VERTEX"
        ));
        assert!(!allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_BEDROCK"
        ));
        assert!(!allthecodes_api::api::client::is_env_truthy(
            "ALLTHECODES_USE_FOUNDRY"
        ));
        assert!(text.contains("GCP Vertex AI provider enabled"));
        assert!(text.contains("Project: proj-123"));
        assert!(text.contains("Region: europe-west4"));
        assert!(text.contains("ALLTHECODES_VERTEX_ACCESS_TOKEN"));
    }

    #[test]
    fn test_foundry_status_surfaces_provider_validation_diagnostic() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let _foundry = EnvGuard::set("ALLTHECODES_USE_FOUNDRY", Some("1"));
        let _bedrock = EnvGuard::set("ALLTHECODES_USE_BEDROCK", None);
        let _vertex = EnvGuard::set("ALLTHECODES_USE_VERTEX", None);

        let text = auth_status_text();

        assert!(text.contains("Microsoft Foundry"));
        assert!(text.contains("unsupported"));
        assert!(text.contains("no Foundry request/auth adapter"));
    }

    #[test]
    #[serial_test::serial]
    fn test_anthropic_provider_selection_persists_claude_code_profile() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let mut ctx = test_ctx();

        let msg =
            persist_provider_selection(settings::API_PROVIDER_ANTHROPIC, Some("native"), &mut ctx)
                .expect("message");

        assert!(msg.contains("apiProvider=anthropic"));
        assert_eq!(
            ctx.app_state.settings.active_auth_profile.as_deref(),
            Some(settings::AUTH_PROFILE_CLAUDE_CODE)
        );
        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            raw.active_auth_profile.as_deref(),
            Some(settings::AUTH_PROFILE_CLAUDE_CODE)
        );
        let profiles = raw.auth_profiles.as_ref().expect("auth profiles");
        assert!(profiles.contains_key(settings::AUTH_PROFILE_CLAUDE_CODE));
        assert!(!profiles.contains_key(settings::AUTH_PROFILE_ANTHROPIC_LEGACY));
    }

    #[test]
    #[serial_test::serial]
    fn test_anthropic_provider_selection_copies_legacy_profile() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let legacy = settings::ProviderProfileSettings {
            backend: Some("native".to_string()),
            api_provider: Some(settings::API_PROVIDER_ANTHROPIC.to_string()),
            model: Some("deepseek-v4-pro".to_string()),
            base_url: Some("https://inferaichat.com".to_string()),
            ..Default::default()
        };
        settings::write_user_settings(&RawSettings {
            active_auth_profile: Some(settings::AUTH_PROFILE_ANTHROPIC_LEGACY.to_string()),
            auth_profiles: Some(std::collections::HashMap::from([(
                settings::AUTH_PROFILE_ANTHROPIC_LEGACY.to_string(),
                legacy,
            )])),
            ..RawSettings::default()
        })
        .unwrap();
        let mut ctx = test_ctx();

        persist_provider_selection(settings::API_PROVIDER_ANTHROPIC, Some("native"), &mut ctx)
            .expect("message");

        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            raw.active_auth_profile.as_deref(),
            Some(settings::AUTH_PROFILE_CLAUDE_CODE)
        );
        let claude_code = raw
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(settings::AUTH_PROFILE_CLAUDE_CODE))
            .expect("claude_code profile persisted");
        assert_eq!(claude_code.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(
            ctx.app_state.main_loop_model.as_str(),
            "deepseek-v4-pro",
            "profile selection should update the active runtime model"
        );
        assert_eq!(
            ctx.app_state.settings.model.as_deref(),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            claude_code.base_url.as_deref(),
            Some("https://inferaichat.com")
        );
    }

    #[test]
    fn test_check_codex_cli_no_file() {
        // Point CODEX_HOME at a nonexistent directory
        let dir = tempfile::TempDir::new().unwrap();
        let empty_sub = dir.path().join("empty");
        std::fs::create_dir_all(&empty_sub).unwrap();
        let prev = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", empty_sub.to_str().unwrap());

        let mut ctx = test_ctx();
        let msg = check_codex_cli(&mut ctx);
        assert!(
            msg.contains("not found"),
            "expected 'not found' message, got: {}",
            msg
        );

        match prev {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_codex_provider_selection_syncs_backend_and_model() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let _model = EnvGuard::set("OPENAI_CODEX_MODEL", Some("gpt-5.5"));
        let mut ctx = test_ctx();
        ctx.app_state.main_loop_model = "deepseek-v4-pro".to_string();

        let msg = persist_provider_selection(
            settings::API_PROVIDER_OPENAI_CODEX,
            Some("codex"),
            &mut ctx,
        )
        .expect("message");

        assert!(msg.contains("apiProvider=openai-codex"));
        assert!(msg.contains("backend=codex"));
        assert!(msg.contains("model=gpt-5.5"));
        assert_eq!(ctx.app_state.main_loop_backend, "codex");
        assert_eq!(ctx.app_state.main_loop_model, "gpt-5.5");
        assert!(ctx
            .app_state
            .settings
            .available_models
            .contains(&"gpt-5.5".to_string()));
        assert!(ctx
            .app_state
            .settings
            .model_capabilities
            .contains_key("gpt-5.5"));

        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.active_auth_profile.as_deref(), Some("codex"));
        let codex = raw
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get("codex"))
            .expect("codex profile persisted");
        assert_eq!(
            codex.api_provider.as_deref(),
            Some(settings::API_PROVIDER_OPENAI_CODEX)
        );
        assert_eq!(codex.backend.as_deref(), Some("codex"));
        assert_eq!(codex.model.as_deref(), Some("gpt-5.5"));
        assert!(codex
            .available_models
            .as_ref()
            .expect("availableModels persisted")
            .contains(&"gpt-5.5".to_string()));
        assert!(codex
            .model_capabilities
            .as_ref()
            .expect("modelCapabilities persisted")
            .contains_key("gpt-5.5"));
    }

    #[test]
    #[serial_test::serial]
    fn test_codex_provider_selection_replaces_deepseek_model_list() {
        let _lock = ENV_LOCK.lock().expect("env lock poisoned");
        let dir = tempfile::TempDir::new().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", dir.path().to_str());
        let _model = EnvGuard::set("OPENAI_CODEX_MODEL", None);
        let mut ctx = test_ctx();
        ctx.app_state.main_loop_model = "deepseek-v4-pro".to_string();
        ctx.app_state.settings.model = Some("deepseek-v4-pro".to_string());
        ctx.app_state.settings.available_models = vec![
            "SOTA".to_string(),
            "MOTA".to_string(),
            "FOTA".to_string(),
            "deepseek-v4-pro".to_string(),
        ];
        settings::write_user_settings(&RawSettings {
            model: Some("deepseek-v4-pro".to_string()),
            available_models: Some(ctx.app_state.settings.available_models.clone()),
            ..RawSettings::default()
        })
        .unwrap();

        let msg = persist_provider_selection(
            settings::API_PROVIDER_OPENAI_CODEX,
            Some("codex"),
            &mut ctx,
        )
        .expect("message");

        assert!(msg.contains("apiProvider=openai-codex"));
        assert!(msg.contains("model=gpt-5.4"));
        assert_eq!(ctx.app_state.main_loop_backend, "codex");
        assert_eq!(ctx.app_state.main_loop_model, "gpt-5.4");
        assert!(!ctx
            .app_state
            .settings
            .available_models
            .contains(&"deepseek-v4-pro".to_string()));
        assert!(ctx
            .app_state
            .settings
            .available_models
            .contains(&"gpt-5.4-mini".to_string()));
        assert!(ctx
            .app_state
            .settings
            .model_capabilities
            .contains_key("gpt-5.4"));

        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.active_auth_profile.as_deref(), Some("codex"));
        let codex = raw
            .auth_profiles
            .as_ref()
            .and_then(|profiles| profiles.get("codex"))
            .expect("codex profile persisted");
        assert_eq!(codex.model.as_deref(), Some("gpt-5.4"));
        assert!(!codex
            .available_models
            .as_ref()
            .expect("availableModels persisted")
            .contains(&"deepseek-v4-pro".to_string()));
        assert!(codex
            .model_capabilities
            .as_ref()
            .expect("modelCapabilities persisted")
            .contains_key("gpt-5.4"));
    }
}
