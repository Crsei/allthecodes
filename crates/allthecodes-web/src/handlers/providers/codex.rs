use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use allthecodes_config::settings::{
    codex_model_capabilities, codex_model_ids, load_global_config, user_settings_path,
    write_user_settings, ProviderProfileSettings, RawSettings, API_PROVIDER_OPENAI_CODEX,
    AUTH_PROFILE_CODEX,
};
use allthecodes_protocol::ApiError as ProtocolApiError;

use crate::state::WebState;

use super::types::{CodexApplyLocalResponse, CodexLocalStatusResponse};

/// GET /api/providers/openai-codex/local-status — Inspect local Codex CLI auth.
pub async fn codex_local_status_handler() -> impl IntoResponse {
    Json(codex_local_status())
}

/// POST /api/providers/openai-codex/apply-local — Use local Codex CLI OAuth.
pub async fn codex_apply_local_handler(State(state): State<WebState>) -> Response {
    let before = codex_local_status();
    if !before.cli_installed {
        return codex_apply_bad_request(
            before,
            "Codex CLI was not found on PATH. Install Codex CLI, run `codex login`, then refresh.",
        );
    }
    match before.auth_status.as_str() {
        "valid" | "expired_refreshable" => {}
        "missing" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI auth.json was not found. Run `codex login`, then refresh.",
            );
        }
        "expired" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI credentials are expired and have no refresh token. Run `codex login` again.",
            );
        }
        "invalid" => {
            return codex_apply_bad_request(
                before,
                "Codex CLI auth.json could not be read. Run `codex login` again.",
            );
        }
        _ => {
            return codex_apply_bad_request(before, "Codex CLI OAuth is not available.");
        }
    }

    if let Err(error) = allthecodes_auth::try_resolve_codex_cli_auth_token()
        .and_then(|token| token.ok_or_else(|| anyhow::anyhow!("Codex CLI OAuth token not found")))
        .map(drop)
    {
        return codex_apply_bad_request(before, &format!("Codex CLI OAuth is not usable: {error}"));
    }

    let default_model = default_codex_model();
    let mut settings = load_global_config().unwrap_or_default();
    apply_codex_profile_to_settings(&mut settings, &default_model);

    match write_user_settings(&settings) {
        Ok(settings_path) => {
            apply_codex_profile_to_app_state(&state, &settings);
            Json(CodexApplyLocalResponse {
                cli_installed: before.cli_installed,
                auth_present: true,
                auth_status: "valid".to_string(),
                expires_at: before.expires_at,
                settings_path: settings_path.display().to_string(),
                active: true,
                model: default_model,
                message: "Codex profile applied from local Codex CLI OAuth.".to_string(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                ProtocolApiError::Internal {
                    message: e.to_string(),
                }
                .into_body(),
            ),
        )
            .into_response(),
    }
}

fn codex_apply_bad_request(status: CodexLocalStatusResponse, message: &str) -> Response {
    let message = format!("{} {}", message, status.message);
    (
        StatusCode::BAD_REQUEST,
        Json(
            ProtocolApiError::BadRequest {
                code: "codex_local_oauth_unavailable",
                message,
            }
            .into_body(),
        ),
    )
        .into_response()
}

pub(super) fn codex_local_status() -> CodexLocalStatusResponse {
    let cli_installed = is_command_on_path("codex");
    let (auth_status, expires_at) = codex_local_auth_status();
    let auth_present = auth_status != "missing";
    let settings = load_global_config().unwrap_or_default();
    let (active, model) = codex_settings_state(&settings);
    let message = codex_status_message(cli_installed, auth_present, &auth_status, active);

    CodexLocalStatusResponse {
        cli_installed,
        auth_present,
        auth_status,
        expires_at,
        settings_path: user_settings_path().display().to_string(),
        active,
        model,
        message,
    }
}

pub(super) fn codex_local_auth_status() -> (String, Option<i64>) {
    match allthecodes_auth::codex_cli::read_codex_cli_credential() {
        Ok(Some(credential)) => {
            let expired = allthecodes_auth::codex_cli::is_credential_expired(&credential);
            let status = if expired {
                if credential
                    .refresh_token
                    .as_ref()
                    .is_some_and(|token| !token.trim().is_empty())
                {
                    "expired_refreshable"
                } else {
                    "expired"
                }
            } else {
                "valid"
            };
            (status.to_string(), credential.expires_at)
        }
        Ok(None) => {
            if allthecodes_auth::codex_cli::codex_cli_auth_path().is_some() {
                ("invalid".to_string(), None)
            } else {
                ("missing".to_string(), None)
            }
        }
        Err(_) => ("invalid".to_string(), None),
    }
}

fn codex_settings_state(settings: &RawSettings) -> (bool, Option<String>) {
    let codex_profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(AUTH_PROFILE_CODEX));
    let profile_is_codex = codex_profile.is_some_and(|profile| {
        profile.backend.as_deref() == Some("codex")
            || profile.api_provider.as_deref() == Some(API_PROVIDER_OPENAI_CODEX)
    });
    let top_level_is_codex = settings.api_provider.as_deref() == Some(API_PROVIDER_OPENAI_CODEX)
        || settings.backend.as_deref() == Some("codex");
    let active = settings.active_auth_profile.as_deref() == Some(AUTH_PROFILE_CODEX)
        && (profile_is_codex || top_level_is_codex);
    let model = codex_profile
        .and_then(|profile| profile.model.clone())
        .or_else(|| settings.model.clone());
    (active, model)
}

fn codex_status_message(
    cli_installed: bool,
    auth_present: bool,
    auth_status: &str,
    active: bool,
) -> String {
    if !cli_installed {
        return "Codex CLI was not found on PATH.".to_string();
    }
    if !auth_present {
        return "Codex CLI auth.json was not found. Run `codex login`.".to_string();
    }
    match auth_status {
        "valid" if active => "Local Codex OAuth is active in allthecodes settings.".to_string(),
        "valid" => "Local Codex OAuth is available.".to_string(),
        "expired_refreshable" => {
            "Local Codex OAuth is expired but has a refresh token.".to_string()
        }
        "expired" => "Local Codex OAuth is expired and has no refresh token.".to_string(),
        "invalid" => "Codex CLI auth.json could not be read.".to_string(),
        _ => "Codex CLI OAuth is not available.".to_string(),
    }
}

fn apply_codex_profile_to_settings(settings: &mut RawSettings, default_model: &str) {
    let models = codex_model_ids();
    let profile = ProviderProfileSettings {
        backend: Some("codex".to_string()),
        api_provider: Some(API_PROVIDER_OPENAI_CODEX.to_string()),
        model: Some(default_model.to_string()),
        available_models: Some(models.clone()),
        model_capabilities: Some(codex_model_capabilities()),
        model_reasoning_effort: Some("medium".to_string()),
        base_url: None,
        api_key: None,
        env: None,
        auth_source: Some(json!({ "type": "codex_cli" })),
        extra: HashMap::from([("enabled".to_string(), json!(true))]),
    };

    let profiles = settings.auth_profiles.get_or_insert_with(HashMap::new);
    profiles.insert(AUTH_PROFILE_CODEX.to_string(), profile);
    settings.active_auth_profile = Some(AUTH_PROFILE_CODEX.to_string());
    settings.api_provider = Some(API_PROVIDER_OPENAI_CODEX.to_string());
    settings.backend = Some("codex".to_string());
    settings.model = Some(default_model.to_string());
    settings.available_models = Some(models);
    settings.model_reasoning_effort = Some("medium".to_string());
}

fn apply_codex_profile_to_app_state(state: &WebState, settings: &RawSettings) {
    let profiles = settings.auth_profiles.clone().unwrap_or_default();
    let model = settings.model.clone().unwrap_or_else(default_codex_model);
    let available_models = settings
        .available_models
        .clone()
        .unwrap_or_else(codex_model_ids);
    let model_capabilities = profiles
        .get(AUTH_PROFILE_CODEX)
        .and_then(|profile| profile.model_capabilities.clone())
        .unwrap_or_else(codex_model_capabilities);

    state.engine().update_app_state(|app_state| {
        app_state.main_loop_model = model.clone();
        app_state.main_loop_backend = "codex".to_string();
        app_state.settings.model = Some(model.clone());
        app_state.settings.backend = Some("codex".to_string());
        app_state.settings.api_provider = Some(API_PROVIDER_OPENAI_CODEX.to_string());
        app_state.settings.active_auth_profile = Some(AUTH_PROFILE_CODEX.to_string());
        app_state.settings.auth_profiles = profiles;
        app_state.settings.available_models = available_models;
        app_state.settings.model_capabilities = model_capabilities;
        app_state.settings.model_reasoning_effort = settings.model_reasoning_effort.clone();
    });
}

fn default_codex_model() -> String {
    codex_model_ids()
        .into_iter()
        .next()
        .unwrap_or_else(|| "gpt-5.6-sol".to_string())
}

fn is_command_on_path(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return is_executable_file(Path::new(command));
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths)
        .any(|dir| command_candidates(&dir, command).any(|path| is_executable_file(&path)))
}

fn command_candidates<'a>(dir: &'a Path, command: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    #[cfg(windows)]
    {
        let extensions = std::env::var_os("PATHEXT")
            .map(|value| {
                std::env::split_paths(&value)
                    .filter_map(|path| path.into_os_string().into_string().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]);
        let mut candidates = vec![dir.join(command)];
        candidates.extend(
            extensions
                .into_iter()
                .map(move |ext| dir.join(format!("{command}{ext}"))),
        );
        candidates.into_iter()
    }
    #[cfg(not(windows))]
    {
        std::iter::once(dir.join(command))
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
