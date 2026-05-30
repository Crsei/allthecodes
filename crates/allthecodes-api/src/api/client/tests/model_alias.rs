use super::*;

// --- Model alias ---

#[test]
fn regression_anthropic_model_alias_currently_not_resolved_for_wire_model() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
        "ANTHROPIC_BASE_URL",
        ANTHROPIC_DEFAULT_SOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_FOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_OPUS_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        ANTHROPIC_DEFAULT_HAIKU_MODEL_ENV,
    ]);
    let fixture = fixture_json("model_alias_expected");
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-model-alias-test");
    std::env::set_var("ANTHROPIC_MODEL", fixture["alias"].as_str().unwrap());

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(
        client.config().default_model,
        fixture["wire_model"].as_str().unwrap()
    );

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_alias_uses_new_default_model_env_before_official_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
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
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-model-env-test");
    std::env::set_var("ANTHROPIC_MODEL", "MOTA");
    std::env::set_var(ANTHROPIC_DEFAULT_MOTA_MODEL_ENV, "compatible-sonnet-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "compatible-sonnet-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_alias_uses_legacy_default_model_env_as_fallback() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
        ANTHROPIC_DEFAULT_SONNET_MODEL_ENV,
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
    ]);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-legacy-model-env-test");
    std::env::set_var("ANTHROPIC_MODEL", "MOTA");
    std::env::set_var(ANTHROPIC_DEFAULT_SONNET_MODEL_ENV, "legacy-sonnet-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "legacy-sonnet-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}

#[test]
fn anthropic_compatible_alias_without_provider_default_is_config_error() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    let saved_model_env = save_env(ANTHROPIC_MODEL_ENV_KEYS);
    let saved_keys = save_and_clear_provider_keys();
    clear_env(&[
        "ALLTHECODES_USE_BEDROCK",
        "ALLTHECODES_USE_VERTEX",
        "ALLTHECODES_USE_FOUNDRY",
    ]);
    clear_env(ANTHROPIC_MODEL_ENV_KEYS);
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-compatible-model-error");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://compatible.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "FOTA");

    let err = match ApiClient::from_auth_result() {
        Err(error) => error,
        Ok(_) => panic!("compatible Anthropic alias without fallback must fail"),
    };
    let msg = err.to_string();
    assert!(msg.contains("Anthropic-compatible provider"));
    assert!(msg.contains(ANTHROPIC_DEFAULT_FOTA_MODEL_ENV));
    assert!(!msg.contains("claude-haiku-4-5-20251001"));

    restore_provider_keys(saved_keys);
    restore_env(saved_model_env);
    restore_env(saved);
}

#[test]
fn anthropic_compatible_explicit_model_id_wins_over_alias_defaults() {
    let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
    let saved = save_env(&[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        ANTHROPIC_DEFAULT_MOTA_MODEL_ENV,
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
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-api03-compatible-explicit");
    std::env::set_var("ANTHROPIC_BASE_URL", "https://compatible.example.com");
    std::env::set_var("ANTHROPIC_MODEL", "provider-explicit-model");
    std::env::set_var(ANTHROPIC_DEFAULT_MOTA_MODEL_ENV, "provider-mota-model");

    let client = ApiClient::from_auth_result()
        .expect("auth resolution should not error")
        .expect("API key should build a client");

    assert_eq!(client.config().default_model, "provider-explicit-model");

    restore_provider_keys(saved_keys);
    restore_env(saved);
}
