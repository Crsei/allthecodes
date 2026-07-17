use super::*;

// --- Settings/env auth interaction ---

#[test]
fn settings_runtime_env_is_visible_to_anthropic_provider_detection() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "deepseek-v4-pro".to_string()),
    ]);

    let report =
        allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(report.applied, 3);
    assert_eq!(client.config().default_model, "deepseek-v4-pro");
    assert_eq!(
        client.config().provider.endpoint_kind(),
        Some(AnthropicEndpointKind::CompatibleAnthropic)
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn startup_settings_env_overrides_inherited_anthropic_provider_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-old-process-key");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://old.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-20250514");
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "deepseek-v4-pro".to_string()),
    ]);

    let report = allthecodes_config::settings::apply_startup_runtime_env(&env)
        .expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(report.overridden, 3);
    assert_eq!(client.config().default_model, "deepseek-v4-pro");
    assert_eq!(
        client.build_url(),
        "https://compatible.example.com/v1/messages"
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn settings_runtime_env_supports_anthropic_legacy_model_alias_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let env = std::collections::HashMap::from([
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-ant-api03-settings-runtime-alias".to_string(),
        ),
        (
            "ANTHROPIC_BASE_URL".to_string(),
            "https://compatible.example.com".to_string(),
        ),
        ("ANTHROPIC_MODEL".to_string(), "MOTA".to_string()),
        (
            ANTHROPIC_DEFAULT_SONNET_MODEL_ENV.to_string(),
            "deepseek-v4-pro".to_string(),
        ),
    ]);

    allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings env should build a client");

    assert_eq!(client.config().default_model, "deepseek-v4-pro");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn settings_runtime_env_is_visible_to_codex_backend_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    clear_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    let env = std::collections::HashMap::from([
        (
            OPENAI_CODEX_TOKEN_ENV.to_string(),
            "codex-settings-token".to_string(),
        ),
        (
            OPENAI_CODEX_BASE_URL_ENV.to_string(),
            "https://example.com/codex".to_string(),
        ),
        (
            OPENAI_CODEX_MODEL_ENV.to_string(),
            "gpt-5.3-codex-spark".to_string(),
        ),
    ]);

    allthecodes_config::settings::apply_runtime_env(&env).expect("settings env applies");
    let client = ApiClient::from_backend(Some("codex")).expect("codex settings env auth");

    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-settings-token");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.3-codex-spark");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn active_codex_profile_env_builds_codex_client() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    clear_env(&[
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("codex".to_string()),
        auth_profiles: Some(HashMap::from([(
            "codex".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("codex".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_OPENAI_CODEX.to_string(),
                ),
                model: Some("gpt-5.4".to_string()),
                base_url: Some("https://example.com/codex/".to_string()),
                api_key: Some("codex-profile-token".to_string()),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_backend(Some("codex")).expect("codex profile client");

    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-profile-token");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.4");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn active_custom_profile_env_builds_anthropic_compatible_client() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("custom".to_string()),
        auth_profiles: Some(HashMap::from([(
            "custom".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("native".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string(),
                ),
                model: Some("deepseek-v4-pro".to_string()),
                base_url: Some("https://compatible.example.com/anthropic".to_string()),
                env: Some(HashMap::from([(
                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                    "custom-profile-token".to_string(),
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("custom profile client");

    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            base_url,
            endpoint_kind,
        } => {
            assert_eq!(
                auth,
                &AnthropicAuth::BearerToken("custom-profile-token".to_string())
            );
            assert_eq!(
                base_url.as_deref(),
                Some("https://compatible.example.com/anthropic")
            );
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::CompatibleAnthropic);
            assert_eq!(client.config().default_model, "deepseek-v4-pro");
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    restore_env(saved);
}

// -----------------------------------------------------------------------
// from_provider_info
// -----------------------------------------------------------------------

#[test]
fn test_from_provider_info_anthropic() {
    use crate::api::providers::get_provider;
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(ANTHROPIC_MODEL_ENV_KEYS);
    clear_env(ANTHROPIC_MODEL_ENV_KEYS);
    let info = get_provider("anthropic").unwrap();
    let client = ApiClient::from_provider_info(info, "sk-test").unwrap();
    assert!(matches!(
        client.config().provider,
        ApiProvider::Anthropic { .. }
    ));
    assert_eq!(client.config().default_model, "claude-sonnet-4-6");
    restore_env(saved);
}

#[test]
fn test_from_provider_info_deepseek() {
    use crate::api::providers::get_provider;
    let info = get_provider("deepseek").unwrap();
    let client = ApiClient::from_provider_info(info, "sk-ds-key").unwrap();
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, base_url, .. } => {
            assert_eq!(name, "deepseek");
            assert_eq!(base_url, "https://api.deepseek.com/v1");
        }
        _ => panic!("expected OpenAiCompat"),
    }
}

#[test]
fn test_from_provider_info_google() {
    use crate::api::providers::get_provider;
    let info = get_provider("google").unwrap();
    let client = ApiClient::from_provider_info(info, "AIza-test").unwrap();
    assert!(matches!(
        client.config().provider,
        ApiProvider::Google { .. }
    ));
    assert_eq!(client.config().default_model, "gemini-2.0-flash");
}

// -----------------------------------------------------------------------
// from_env / from_auth
// -----------------------------------------------------------------------

#[test]
fn test_from_env_with_anthropic_key() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&[
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
    ]);
    clear_env(&[
        "ANTHROPIC_BASE_URL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
    ]);

    // Temporarily set the env var for this test
    let key = "sk-ant-api03-test-from-env-key";
    std::env::set_var("ANTHROPIC_API_KEY", key);

    let client = ApiClient::from_env();
    assert!(
        client.is_some(),
        "from_env should return Some when ANTHROPIC_API_KEY is set"
    );

    let client = client.unwrap();
    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            endpoint_kind,
            ..
        } => {
            assert_eq!(auth, &AnthropicAuth::ApiKey(key.to_string()));
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::DirectAnthropic);
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    // Clean up
    std::env::remove_var("ANTHROPIC_API_KEY");
    restore_env(saved_flags);
}

#[test]
fn test_from_env_no_keys() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);

    // Save and clear all provider keys
    let saved: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    let client = ApiClient::from_env();
    assert!(
        client.is_none(),
        "from_env should return None when no provider key is set"
    );

    // Restore
    for (key, val) in saved {
        std::env::set_var(key, val);
    }
    restore_env(saved_flags);
}

#[test]
fn test_from_auth_with_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);

    let key = "sk-ant-api03-test-from-auth-key";
    std::env::set_var("ANTHROPIC_API_KEY", key);

    let client = ApiClient::from_auth();
    assert!(client.is_some(), "from_auth should find the env var");

    // Clean up
    std::env::remove_var("ANTHROPIC_API_KEY");
    restore_env(saved_flags);
}

// -----------------------------------------------------------------------
// try_new proxy resolution (settings.json → env)
// -----------------------------------------------------------------------

fn make_proxy_settings_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "allthecodes-proxy-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".allthecodes")).expect("create temp .allthecodes");
    dir
}

#[test]
fn test_try_new_uses_proxy_url_from_settings() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&[
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ]);
    clear_env(&[
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ]);

    let dir = make_proxy_settings_dir();
    std::fs::write(
        dir.join(".allthecodes").join("settings.json"),
        r#"{"proxyUrl": "http://127.0.0.1:17891"}"#,
    )
    .expect("write settings.json");

    let _cwd_guard = CwdGuard::set(&dir);
    let client = ApiClient::try_new(anthropic_config());
    assert!(
        client.is_ok(),
        "try_new should succeed with proxyUrl in settings.json: {:?}",
        client.err()
    );

    // Env vars stay cleared inside this scope; restore_env below resets them.
    restore_env(saved_flags);
}

#[test]
fn test_try_new_env_proxy_fallbacks_when_settings_absent() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_flags = save_env(&[
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ]);
    // Point cwd at a temp dir with no .allthecodes/settings.json so the
    // settings-driven proxy path is skipped and env fallback kicks in.
    let dir = std::env::temp_dir().join(format!(
        "allthecodes-proxy-no-settings-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let _cwd_guard = CwdGuard::set(&dir);

    clear_env(&[
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ]);
    std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:17891");

    let client = ApiClient::try_new(anthropic_config());
    assert!(
        client.is_ok(),
        "try_new should succeed via env proxy fallback: {:?}",
        client.err()
    );

    std::env::remove_var("HTTPS_PROXY");
    restore_env(saved_flags);
}

#[test]
fn test_from_codex_auth_with_env() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "codex-token-test");
    std::env::set_var(OPENAI_CODEX_BASE_URL_ENV, "https://example.com/codex/");
    std::env::set_var(OPENAI_CODEX_MODEL_ENV, "gpt-5.3-codex-spark");

    let client = ApiClient::from_codex_auth().expect("from_codex_auth should return Some");
    match &client.config().provider {
        ApiProvider::OpenAiCompat {
            name,
            api_key,
            base_url,
            default_model,
        } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-token-test");
            assert_eq!(base_url, "https://example.com/codex");
            assert_eq!(default_model, "gpt-5.3-codex-spark");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }
    assert_eq!(client.config().default_model, "gpt-5.3-codex-spark");

    std::env::remove_var(OPENAI_CODEX_TOKEN_ENV);
    std::env::remove_var(OPENAI_CODEX_BASE_URL_ENV);
    std::env::remove_var(OPENAI_CODEX_MODEL_ENV);
}

#[test]
fn test_from_env_prefers_bedrock_when_flag_set() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_extra = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_REGION",
    ]);
    // Save + clear all provider keys so Anthropic-API-key detection doesn't shadow.
    let saved_keys: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    std::env::set_var("ALLTHECODES_USE_BEDROCK", "1");
    std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", "bedrock-123");
    std::env::set_var("AWS_REGION", "us-west-2");

    let client = ApiClient::from_env().expect("Bedrock flag should produce a client");
    match &client.config().provider {
        ApiProvider::Bedrock { region, .. } => assert_eq!(region, "us-west-2"),
        other => panic!("expected Bedrock provider, got {:?}", other),
    }

    for (k, v) in saved_keys {
        std::env::set_var(k, v);
    }
    restore_env(saved_extra);
}

#[test]
fn test_from_env_prefers_vertex_when_flag_set() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved_extra = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "ALLTHECODES_VERTEX_ACCESS_TOKEN",
        "CLOUD_ML_REGION",
    ]);
    let saved_keys: Vec<_> = crate::api::providers::PROVIDERS
        .iter()
        .filter_map(|p| std::env::var(p.env_key).ok().map(|v| (p.env_key, v)))
        .collect();
    for p in crate::api::providers::PROVIDERS {
        std::env::remove_var(p.env_key);
    }

    std::env::set_var("ALLTHECODES_USE_VERTEX", "true");
    std::env::set_var("ANTHROPIC_VERTEX_PROJECT_ID", "proj-42");
    std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", "ya29.test");
    std::env::set_var("CLOUD_ML_REGION", "europe-west4");

    let client = ApiClient::from_env().expect("Vertex flag should produce a client");
    match &client.config().provider {
        ApiProvider::Vertex {
            project_id, region, ..
        } => {
            assert_eq!(project_id, "proj-42");
            assert_eq!(region, "europe-west4");
        }
        other => panic!("expected Vertex provider, got {:?}", other),
    }

    for (k, v) in saved_keys {
        std::env::set_var(k, v);
    }
    restore_env(saved_extra);
}

#[test]
fn test_from_bedrock_env_returns_none_without_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    // Ensure neither auth method is available.
    let saved_bearer = std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok();
    let saved_ak = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let saved_sk = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
    std::env::remove_var("AWS_ACCESS_KEY_ID");
    std::env::remove_var("AWS_SECRET_ACCESS_KEY");

    assert!(
        ApiClient::from_bedrock_env_result().is_err(),
        "from_bedrock_env_result should reject missing AWS creds"
    );

    if let Some(v) = saved_bearer {
        std::env::set_var("AWS_BEARER_TOKEN_BEDROCK", v);
    }
    if let Some(v) = saved_ak {
        std::env::set_var("AWS_ACCESS_KEY_ID", v);
    }
    if let Some(v) = saved_sk {
        std::env::set_var("AWS_SECRET_ACCESS_KEY", v);
    }
}

#[test]
fn test_from_env_result_errors_for_explicit_bedrock_without_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&[
        "ALLTHECODES_USE_VERTEX",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
    ]);
    std::env::set_var("ALLTHECODES_USE_BEDROCK", "1");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Bedrock config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("ALLTHECODES_USE_BEDROCK"));
    assert!(msg.contains("AWS_BEARER_TOKEN_BEDROCK"));

    restore_env(saved);
}

#[test]
fn test_from_env_result_errors_for_explicit_foundry() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_FOUNDRY",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&["ALLTHECODES_USE_BEDROCK", "ALLTHECODES_USE_VERTEX"]);
    std::env::set_var("ALLTHECODES_USE_FOUNDRY", "1");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Foundry config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("Foundry"));
    assert!(msg.contains("no Foundry request/auth adapter"));

    restore_env(saved);
}

#[test]
fn test_from_env_result_errors_for_explicit_vertex_without_project() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "GOOGLE_CLOUD_PROJECT",
        "GCLOUD_PROJECT",
        "ALLTHECODES_VERTEX_ACCESS_TOKEN",
        "ANTHROPIC_API_KEY",
    ]);
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ANTHROPIC_VERTEX_PROJECT_ID",
        "GOOGLE_CLOUD_PROJECT",
        "GCLOUD_PROJECT",
    ]);
    std::env::set_var("ALLTHECODES_USE_VERTEX", "1");
    std::env::set_var("ALLTHECODES_VERTEX_ACCESS_TOKEN", "vertex-token");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-fallback");

    let err = match ApiClient::from_env_result() {
        Err(error) => error,
        Ok(_) => panic!("Vertex config must fail early"),
    };
    let msg = err.to_string();
    assert!(msg.contains("ALLTHECODES_USE_VERTEX"));
    assert!(msg.contains("ANTHROPIC_VERTEX_PROJECT_ID"));

    restore_env(saved);
}

#[test]
fn test_from_backend_codex_prefers_codex_auth() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "codex-token-backend");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-should-not-win");

    let client = ApiClient::from_backend(Some("codex")).expect("from_backend should return Some");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_CODEX_PROVIDER_NAME);
            assert_eq!(api_key, "codex-token-backend");
        }
        other => panic!("expected OpenAiCompat provider, got {:?}", other),
    }

    std::env::remove_var(OPENAI_CODEX_TOKEN_ENV);
    std::env::remove_var("ANTHROPIC_API_KEY");
}

#[test]
fn test_from_auth_uses_openai_keychain_when_api_provider_is_openai() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    allthecodes_auth::api_key::remove_api_key().unwrap();
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-123456").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_OPENAI.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("openai keychain client");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_PROVIDER_NAME);
            assert_eq!(api_key, "sk-proj-keychain-openai-123456");
        }
        other => panic!("expected OpenAI-compatible provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn test_from_auth_anthropic_provider_does_not_read_openai_keychain() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    allthecodes_auth::api_key::remove_api_key().unwrap();
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-abcdef").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result().expect("auth resolution should not error");
    assert!(
        client.is_none(),
        "anthropic provider selection must not consume OpenAI keychain"
    );

    restore_env(saved);
}

#[test]
fn test_from_auth_settings_provider_takes_priority_over_other_env_key() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    use_persistent_test_keyring();
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        OPENAI_CODEX_TOKEN_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-env-priority-key");
    allthecodes_auth::api_key::remove_openai_api_key().unwrap();
    allthecodes_auth::api_key::store_openai_api_key("sk-proj-keychain-openai-priority").unwrap();
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        api_provider: Some(allthecodes_config::settings::API_PROVIDER_OPENAI.to_string()),
        backend: Some("native".to_string()),
        ..Default::default()
    })
    .unwrap();

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("settings-selected provider client");
    match &client.config().provider {
        ApiProvider::OpenAiCompat { name, api_key, .. } => {
            assert_eq!(name, OPENAI_PROVIDER_NAME);
            assert_eq!(api_key, "sk-proj-keychain-openai-priority");
        }
        other => panic!("expected OpenAI provider, got {:?}", other),
    }

    restore_env(saved);
}

#[test]
fn test_active_anthropic_profile_ignores_inherited_codex_token() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let temp = tempfile::tempdir().expect("tempdir");
    let saved = save_env(&[
        "ALLTHECODES_HOME",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        OPENAI_CODEX_TOKEN_ENV,
        OPENAI_CODEX_BASE_URL_ENV,
        OPENAI_CODEX_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    std::env::set_var("ALLTHECODES_HOME", temp.path());
    let _cwd = CwdGuard::set(temp.path());
    std::env::set_var(OPENAI_CODEX_TOKEN_ENV, "inherited-codex-token");
    allthecodes_config::settings::write_user_settings(&allthecodes_config::settings::RawSettings {
        active_auth_profile: Some("claude_code".to_string()),
        auth_profiles: Some(HashMap::from([(
            "claude_code".to_string(),
            allthecodes_config::settings::ProviderProfileSettings {
                backend: Some("native".to_string()),
                api_provider: Some(
                    allthecodes_config::settings::API_PROVIDER_ANTHROPIC.to_string(),
                ),
                model: Some("deepseek-v4-pro".to_string()),
                base_url: Some("https://compatible.example.com/anthropic".to_string()),
                env: Some(HashMap::from([(
                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                    "anthropic-profile-token".to_string(),
                )])),
                ..Default::default()
            },
        )])),
        ..Default::default()
    })
    .unwrap();
    let loaded = allthecodes_config::settings::load_effective(temp.path()).unwrap();
    allthecodes_config::settings::apply_startup_runtime_env(&loaded.effective.env)
        .expect("profile env applies");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("anthropic profile client");

    match &client.config().provider {
        ApiProvider::Anthropic {
            auth,
            base_url,
            endpoint_kind,
        } => {
            assert_eq!(
                auth,
                &AnthropicAuth::BearerToken("anthropic-profile-token".to_string())
            );
            assert_eq!(
                base_url.as_deref(),
                Some("https://compatible.example.com/anthropic")
            );
            assert_eq!(endpoint_kind, &AnthropicEndpointKind::CompatibleAnthropic);
            assert_eq!(client.config().default_model, "deepseek-v4-pro");
        }
        other => panic!("expected Anthropic provider, got {:?}", other),
    }

    restore_env(saved);
}
