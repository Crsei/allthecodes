use super::codex::{codex_apply_local_handler, codex_local_status};
use super::probe::provider_endpoint_for_probe;
use crate::handlers::test_support::{
    make_web_state, read_user_settings, response_json, temp_home, EnvGuard,
};
use allthecodes_config::settings::{
    ProviderProfileSettings, API_PROVIDER_OPENAI_CODEX, AUTH_PROFILE_CODEX,
};
use allthecodes_protocol::v1::providers::{ProviderProbeErrorKind, ProviderProbeStatus};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use serde_json::json;
use serial_test::serial;
use std::path::Path;

fn jwt_with_exp(exp: i64) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{}");
    let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#).as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(b"sig");
    format!("{header}.{payload}.{signature}")
}

fn write_codex_auth(codex_home: &Path, access_token: &str, refresh_token: Option<&str>) {
    let refresh = refresh_token
        .map(|token| format!(r#","refresh_token":"{token}""#))
        .unwrap_or_default();
    std::fs::create_dir_all(codex_home).expect("codex home");
    std::fs::write(
        codex_home.join("auth.json"),
        format!(
            r#"{{
                    "auth_mode": "chatgpt",
                    "tokens": {{
                        "access_token": "{access_token}"{refresh}
                    }}
                }}"#
        ),
    )
    .expect("auth json");
}

fn create_path_codex(bin_dir: &Path) {
    std::fs::create_dir_all(bin_dir).expect("bin dir");
    std::fs::write(bin_dir.join("codex"), "#!/bin/sh\n").expect("codex shim");
}

#[test]
#[serial]
fn provider_probe_endpoint_reports_missing_credential() {
    let _openai_key = EnvGuard::set_path("OPENAI_API_KEY", Path::new(""));
    let profile = ProviderProfileSettings {
        api_provider: Some("openai".to_string()),
        base_url: Some("https://example.test/v1".to_string()),
        ..ProviderProfileSettings::default()
    };

    let response = provider_endpoint_for_probe("work", Some(&profile)).unwrap_err();

    assert_eq!(response.status, ProviderProbeStatus::AuthFailed);
    assert_eq!(
        response.error_kind,
        Some(ProviderProbeErrorKind::AuthenticationFailed)
    );
    assert_eq!(response.base_url_host.as_deref(), Some("example.test"));
}

#[test]
#[serial]
fn provider_probe_endpoint_builds_openai_compatible_runtime() {
    let profile = ProviderProfileSettings {
        api_provider: Some("openai".to_string()),
        base_url: Some("https://example.test/v1".to_string()),
        api_key: Some("sk-test".to_string()),
        ..ProviderProfileSettings::default()
    };

    let endpoint = provider_endpoint_for_probe("work", Some(&profile)).unwrap();

    assert_eq!(endpoint.provider_name, "openai");
    assert_eq!(
        endpoint.url_for_path("/chat/completions").unwrap().as_str(),
        "https://example.test/v1/chat/completions"
    );
    assert!(endpoint.headers.contains_key("authorization"));
}

#[tokio::test]
#[serial]
async fn codex_local_status_missing_without_cli_or_auth() {
    let (_home, _home_guard) = temp_home();
    let codex_home = tempfile::tempdir().expect("codex home");
    let path_dir = tempfile::tempdir().expect("path dir");
    let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
    let _path_guard = EnvGuard::set_path("PATH", path_dir.path());

    let status = codex_local_status();

    assert!(!status.cli_installed);
    assert!(!status.auth_present);
    assert_eq!(status.auth_status, "missing");

    let response = codex_apply_local_handler(State(make_web_state()))
        .await
        .into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response_json(response).await;
    assert_eq!(body["code"], json!("codex_local_oauth_unavailable"));
}

#[tokio::test]
#[serial]
async fn codex_apply_local_writes_codex_profile_without_tokens() {
    let (home, _home_guard) = temp_home();
    let codex_home = tempfile::tempdir().expect("codex home");
    let path_dir = tempfile::tempdir().expect("path dir");
    create_path_codex(path_dir.path());
    let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
    let _path_guard = EnvGuard::set_path("PATH", path_dir.path());
    write_codex_auth(
        codex_home.path(),
        &jwt_with_exp(Utc::now().timestamp() + 3600),
        Some("refresh-token"),
    );
    let state = make_web_state();

    let response = codex_apply_local_handler(State(state.clone()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["auth_status"], json!("valid"));
    assert_eq!(body["active"], json!(true));

    let settings = read_user_settings(&home);
    assert_eq!(
        settings.api_provider.as_deref(),
        Some(API_PROVIDER_OPENAI_CODEX)
    );
    assert_eq!(
        settings.active_auth_profile.as_deref(),
        Some(AUTH_PROFILE_CODEX)
    );
    assert_eq!(settings.backend.as_deref(), Some("codex"));
    let profile = settings
        .auth_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(AUTH_PROFILE_CODEX))
        .expect("codex profile");
    assert_eq!(
        profile.api_provider.as_deref(),
        Some(API_PROVIDER_OPENAI_CODEX)
    );
    assert_eq!(profile.backend.as_deref(), Some("codex"));
    assert!(profile.api_key.is_none());
    assert!(profile.env.is_none());
    assert_eq!(
        profile.auth_source.as_ref(),
        Some(&json!({ "type": "codex_cli" }))
    );

    let app_state = state.engine().app_state();
    assert_eq!(app_state.main_loop_backend, "codex");
    assert_eq!(
        app_state.settings.active_auth_profile.as_deref(),
        Some("codex")
    );
    assert_eq!(
        app_state.settings.api_provider.as_deref(),
        Some(API_PROVIDER_OPENAI_CODEX)
    );
}

#[tokio::test]
#[serial]
async fn codex_apply_local_rejects_expired_unrefreshable_auth_without_changing_settings() {
    let (home, _home_guard) = temp_home();
    let codex_home = tempfile::tempdir().expect("codex home");
    let path_dir = tempfile::tempdir().expect("path dir");
    create_path_codex(path_dir.path());
    let _codex_home_guard = EnvGuard::set_path("CODEX_HOME", codex_home.path());
    let _path_guard = EnvGuard::set_path("PATH", path_dir.path());
    write_codex_auth(
        codex_home.path(),
        &jwt_with_exp(Utc::now().timestamp() - 3600),
        None,
    );

    let response = codex_apply_local_handler(State(make_web_state()))
        .await
        .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        !home.path().join("settings.json").exists(),
        "apply failure must not create settings"
    );
}
