//! Phase 1 settings-adjacent handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use allthecodes_config::paths;
use allthecodes_config::settings::{
    load_effective_with_options, validate_user_settings_candidate, write_user_settings,
    ConfigLayer, ConfigLayerEntry, LoadOptions, ProjectTrustPolicy, RawSettings,
    RequirementViolation, SourceMap,
};
use allthecodes_session::memdir::{self, MemoryScope};

use crate::handlers::admin::{normalize_settings_path, persist_setting};
use crate::state::WebState;
use allthecodes_protocol::ApiError as ProtocolApiError;

#[derive(Serialize)]
pub struct MemoryConfigResponse {
    pub config: HashMap<String, Value>,
    pub stats: MemoryStats,
}

#[derive(Serialize)]
pub struct MemoryStats {
    pub total: usize,
    pub global: usize,
    pub project: usize,
    pub team: usize,
    pub auto_generated: usize,
}

#[derive(Deserialize)]
pub struct MemoryConfigPatch {
    #[serde(flatten)]
    pub values: HashMap<String, Value>,
}

#[derive(Serialize)]
pub struct SpeechModelsResponse {
    pub models: Vec<WhisperModel>,
}

#[derive(Serialize)]
pub struct WhisperModel {
    pub id: String,
    pub name: String,
    pub size_mb: u64,
    pub description: String,
    pub downloaded: bool,
    pub download_progress: Option<u8>,
    pub recommended: bool,
}

#[derive(Deserialize)]
pub struct SpeechModelDownloadRequest {
    pub model_id: String,
}

#[derive(Deserialize)]
pub struct SearchCookiesImportRequest {
    pub cookies: Value,
}

#[derive(Serialize)]
pub struct SearchCookiesResponse {
    pub ok: bool,
    pub path: String,
    pub cookies: Option<Value>,
}

#[derive(Serialize)]
pub struct DataExportResponse {
    pub ok: bool,
    pub path: String,
    pub bytes: usize,
}

#[derive(Deserialize)]
pub struct DataImportRequest {
    #[serde(default)]
    pub settings: Option<Value>,
}

#[derive(Serialize)]
pub struct DataImportResponse {
    pub ok: bool,
    pub imported_settings: bool,
}

#[derive(Serialize)]
pub struct SettingsLayersResponse {
    pub effective_map: HashMap<String, Value>,
    pub source_map: SourceMap,
    pub layers: Vec<ConfigLayer>,
    pub entries: Vec<ConfigLayerEntry>,
    pub requirement_violations: Vec<RequirementViolation>,
    pub diagnostics: Vec<String>,
}

#[derive(Serialize)]
pub struct TokenSavingsResponse {
    pub total_saved_tokens: u64,
    pub total_saved_cost: f64,
    pub cache_hit_rate: f64,
    pub by_model: Vec<TokenSavingsByModel>,
}

#[derive(Serialize)]
pub struct TokenSavingsByModel {
    pub model: String,
    pub cache_reads: u64,
    pub tokens_saved: u64,
    pub cost_saved: f64,
}

pub async fn memory_config_get_handler(State(state): State<WebState>) -> impl IntoResponse {
    Json(MemoryConfigResponse {
        config: memory_config_map(&state),
        stats: memory_stats(&state),
    })
}

pub async fn memory_config_patch_handler(
    State(state): State<WebState>,
    Json(req): Json<MemoryConfigPatch>,
) -> impl IntoResponse {
    for (key, value) in req.values {
        let path = if key.contains('.') {
            key
        } else {
            format!("memory.{key}")
        };
        let Some(setting_key) = normalize_settings_path(&path) else {
            return bad_request(format!("Unknown memory setting: {path}"));
        };
        if let Err(err) = persist_setting(&state, setting_key, value) {
            return bad_request(err.to_string());
        }
    }

    Json(MemoryConfigResponse {
        config: memory_config_map(&state),
        stats: memory_stats(&state),
    })
    .into_response()
}

pub async fn speech_models_handler() -> impl IntoResponse {
    let model_root = paths::data_root().join("speech").join("models");
    let models = [
        (
            "tiny",
            "Whisper Tiny",
            75,
            "Smallest local Whisper model.",
            false,
        ),
        (
            "base",
            "Whisper Base",
            142,
            "Balanced baseline model.",
            false,
        ),
        (
            "small",
            "Whisper Small",
            466,
            "Higher quality local model.",
            false,
        ),
        (
            "large-v3-turbo",
            "Whisper Large v3 Turbo",
            1620,
            "Recommended high-quality transcription model.",
            true,
        ),
    ]
    .into_iter()
    .map(
        |(id, name, size_mb, description, recommended)| WhisperModel {
            id: id.to_string(),
            name: name.to_string(),
            size_mb,
            description: description.to_string(),
            downloaded: model_downloaded(&model_root, id),
            download_progress: None,
            recommended,
        },
    )
    .collect();

    Json(SpeechModelsResponse { models })
}

pub async fn speech_model_download_handler(
    Json(req): Json<SpeechModelDownloadRequest>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            ProtocolApiError::BadRequest {
                code: "speech_download_not_implemented",
                message: format!(
                    "Speech model download is not implemented by this backend yet: {}",
                    req.model_id
                ),
            }
            .into_body(),
        ),
    )
}

pub async fn speech_model_delete_handler(AxumPath(id): AxumPath<String>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(
            ProtocolApiError::BadRequest {
                code: "speech_delete_not_implemented",
                message: format!(
                    "Speech model deletion is not implemented by this backend yet: {id}"
                ),
            }
            .into_body(),
        ),
    )
}

pub async fn search_cookies_export_handler() -> impl IntoResponse {
    let path = search_cookies_path();
    let cookies = read_json_file(&path).ok();
    Json(SearchCookiesResponse {
        ok: true,
        path: path.display().to_string(),
        cookies,
    })
}

pub async fn search_cookies_import_handler(
    Json(req): Json<SearchCookiesImportRequest>,
) -> impl IntoResponse {
    let path = search_cookies_path();
    match write_json_file(&path, &req.cookies) {
        Ok(()) => Json(SearchCookiesResponse {
            ok: true,
            path: path.display().to_string(),
            cookies: None,
        })
        .into_response(),
        Err(err) => internal_error(err.to_string()),
    }
}

pub async fn search_cookies_clear_handler() -> impl IntoResponse {
    let path = search_cookies_path();
    if let Err(err) = std::fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return internal_error(err.to_string());
        }
    }
    Json(SearchCookiesResponse {
        ok: true,
        path: path.display().to_string(),
        cookies: None,
    })
    .into_response()
}

pub async fn data_export_handler(State(state): State<WebState>) -> impl IntoResponse {
    let export = json!({
        "version": state.app_version(),
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "settings": state.engine().app_state().settings.settings_map(),
        "memory": {
            "stats": memory_stats(&state),
        },
    });
    let dir = paths::exports_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return internal_error(err.to_string());
    }
    let path = dir.join(format!(
        "settings-export-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    let Ok(bytes) = serde_json::to_vec_pretty(&export) else {
        return internal_error("failed to serialize export".into());
    };
    if let Err(err) = std::fs::write(&path, &bytes) {
        return internal_error(err.to_string());
    }

    Json(DataExportResponse {
        ok: true,
        path: path.display().to_string(),
        bytes: bytes.len(),
    })
    .into_response()
}

pub async fn data_import_handler(
    State(state): State<WebState>,
    Json(req): Json<DataImportRequest>,
) -> impl IntoResponse {
    let mut imported_settings = false;
    if let Some(settings) = req.settings {
        if contains_sensitive_key(&settings) {
            return bad_request("settings import refuses sensitive key material".into());
        }
        let raw: RawSettings = match serde_json::from_value(settings) {
            Ok(raw) => raw,
            Err(err) => return bad_request(err.to_string()),
        };
        if let Err(err) =
            validate_user_settings_candidate(Path::new(state.engine().cwd()), raw.clone())
        {
            return bad_request(err.to_string());
        }
        if let Err(err) = write_user_settings(&raw) {
            return internal_error(err.to_string());
        }
        imported_settings = true;
    }

    Json(DataImportResponse {
        ok: true,
        imported_settings,
    })
    .into_response()
}

pub async fn settings_layers_handler(State(state): State<WebState>) -> impl IntoResponse {
    let cwd = PathBuf::from(state.engine().cwd());
    match load_effective_with_options(
        &cwd,
        LoadOptions {
            trust_policy: ProjectTrustPolicy::TrustConfiguredOnly,
            enforce_requirements: false,
            ..Default::default()
        },
    ) {
        Ok(loaded) => {
            let diagnostics = loaded
                .requirement_violations
                .iter()
                .map(|violation| violation.message.clone())
                .collect();
            Json(SettingsLayersResponse {
                effective_map: state.engine().app_state().settings.settings_map(),
                source_map: loaded.sources,
                layers: loaded.layers,
                entries: loaded.entries,
                requirement_violations: loaded.requirement_violations,
                diagnostics,
            })
            .into_response()
        }
        Err(err) => internal_error(err.to_string()),
    }
}

pub async fn token_savings_handler(State(state): State<WebState>) -> impl IntoResponse {
    let usage = state.engine().usage();
    let saved = usage.total_cache_read_tokens;
    let total = usage
        .total_input_tokens
        .saturating_add(usage.total_cache_read_tokens)
        .saturating_add(usage.total_cache_creation_tokens);
    let cache_hit_rate = if total == 0 {
        0.0
    } else {
        saved as f64 / total as f64
    };
    let model = state.engine().app_state().main_loop_model;

    Json(TokenSavingsResponse {
        total_saved_tokens: saved,
        total_saved_cost: 0.0,
        cache_hit_rate,
        by_model: vec![TokenSavingsByModel {
            model,
            cache_reads: saved,
            tokens_saved: saved,
            cost_saved: 0.0,
        }],
    })
}

fn memory_config_map(state: &WebState) -> HashMap<String, Value> {
    state
        .engine()
        .app_state()
        .settings
        .settings_map()
        .into_iter()
        .filter(|(key, _)| key.starts_with("memory_") || key == "auto_memory_enabled")
        .collect()
}

fn memory_stats(state: &WebState) -> MemoryStats {
    let cwd = PathBuf::from(state.engine().cwd());
    let global = count_memories(MemoryScope::Global, &cwd);
    let project = count_memories(MemoryScope::Project, &cwd);
    let team = count_memories(MemoryScope::Team, &cwd);
    let auto_generated = count_memories(MemoryScope::Auto, &cwd);
    MemoryStats {
        total: global + project + team + auto_generated,
        global,
        project,
        team,
        auto_generated,
    }
}

fn count_memories(scope: MemoryScope, cwd: &Path) -> usize {
    memdir::list_memories(scope, cwd)
        .map(|entries| entries.len())
        .unwrap_or(0)
}

fn model_downloaded(root: &Path, id: &str) -> bool {
    root.join(id).exists()
        || root.join(format!("{id}.bin")).exists()
        || root.join(format!("{id}.gguf")).exists()
}

fn search_cookies_path() -> PathBuf {
    paths::data_root().join("search-cookies.json")
}

fn read_json_file(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&text).map_err(|err| err.to_string())
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    std::fs::write(path, bytes).map_err(|err| err.to_string())
}

fn contains_sensitive_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let normalized = key
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            normalized.contains("apikey")
                || normalized.contains("token")
                || normalized.contains("secret")
                || normalized.contains("password")
                || contains_sensitive_key(value)
        }),
        Value::Array(items) => items.iter().any(contains_sensitive_key),
        _ => false,
    }
}

fn bad_request(message: String) -> Response {
    let body = ProtocolApiError::BadRequest {
        code: "bad_request",
        message,
    }
    .into_body();
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

fn internal_error(message: String) -> Response {
    let body = ProtocolApiError::Internal { message }.into_body();
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::test_support::*;
    use crate::handlers::{settings_handler, SettingsRequest};
    use allthecodes_engine::types::tool::PermissionMode;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn set_model_rejects_values_outside_available_models() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.available_models = vec!["gpt-4o".to_string()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("SOTA"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(false));
        assert!(body["message"]
            .as_str()
            .expect("message")
            .contains("not in availableModels"));
        assert_ne!(
            state.engine().app_state().main_loop_model,
            "claude-opus-4-20250514"
        );
    }

    #[tokio::test]
    #[serial]
    async fn set_model_accepts_alias_when_full_id_is_allowlisted() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        let expected_model = allthecodes_commands::model::resolve_model_alias("SOTA");
        state.engine().update_app_state(|s| {
            s.settings.sota_model = Some(expected_model.clone());
            s.settings.available_models = vec![expected_model.clone()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("SOTA"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert_eq!(state.engine().app_state().main_loop_model, expected_model);
        assert_eq!(
            state.engine().app_state().settings.model.as_deref(),
            Some(expected_model.as_str())
        );
    }

    #[tokio::test]
    #[serial]
    async fn set_permission_mode_auto_respects_disabled_policy() {
        let (_home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.tool_permission_context.is_auto_mode_available = Some(false);
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_permission_mode".to_string(),
                value: json!("auto"),
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(false));
        assert!(body["message"]
            .as_str()
            .expect("message")
            .contains("permissions.enableAutoMode=false"));
        assert_eq!(
            state.engine().app_state().tool_permission_context.mode,
            PermissionMode::Default
        );
    }

    #[tokio::test]
    #[serial]
    async fn compatibility_settings_actions_persist_user_settings() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.available_models = vec!["gpt-4o".to_string()];
        });

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_model".to_string(),
                value: json!("gpt-4o"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.model.as_deref(), Some("gpt-4o"));
        assert_eq!(state.engine().app_state().main_loop_model, "gpt-4o");

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_permission_mode".to_string(),
                value: json!("plan"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(raw.permission_mode.as_deref(), Some("plan"));
        assert_eq!(
            state.engine().app_state().tool_permission_context.mode,
            PermissionMode::Plan
        );

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_thinking".to_string(),
                value: json!(true),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(
            raw.thinking
                .as_ref()
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str),
            Some("enabled")
        );
        assert_eq!(state.engine().app_state().thinking_enabled, Some(true));

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_web_search_provider".to_string(),
                value: json!("brave"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_web_search_brave_api_key".to_string(),
                value: json!("brave-test-key"),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let raw = read_user_settings(&home);
        assert_eq!(raw.web_search_provider.as_deref(), Some("brave"));
        assert_eq!(
            raw.web_search_brave_api_key.as_deref(),
            Some("brave-test-key")
        );
        let settings_map = state.engine().app_state().settings.settings_map();
        assert_eq!(
            settings_map.get("web_search_provider"),
            Some(&json!("brave"))
        );
        assert_eq!(
            settings_map.get("web_search_brave_configured"),
            Some(&json!(true))
        );
        assert!(!settings_map.contains_key("web_search_brave_api_key"));
    }

    #[tokio::test]
    #[serial]
    async fn set_ext_persists_typed_and_unknown_paths() {
        let (home, _guard) = temp_home();
        let state = make_web_state();

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_ext".to_string(),
                value: json!({
                    "path": "network.proxy_enabled",
                    "value": true
                }),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            state.engine().app_state().settings.proxy_enabled,
            Some(true)
        );
        let raw = read_user_settings(&home);
        assert_eq!(raw.proxy_enabled, Some(true));

        let response = settings_handler(
            State(state.clone()),
            Json(SettingsRequest {
                action: "set_ext".to_string(),
                value: json!({
                    "path": "customPanel.feature_flag",
                    "value": "enabled"
                }),
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let raw = read_user_settings(&home);
        assert_eq!(
            raw.extra
                .get("customPanel")
                .and_then(|value| value.get("feature_flag")),
            Some(&json!("enabled"))
        );
        assert_eq!(
            state
                .engine()
                .app_state()
                .settings
                .settings_map()
                .get("customPanel"),
            Some(&json!({ "feature_flag": "enabled" }))
        );
    }

    #[tokio::test]
    #[serial]
    async fn phase1_helper_endpoints_return_stable_json() {
        let (home, _guard) = temp_home();
        let state = make_web_state();
        state.engine().update_app_state(|s| {
            s.settings.auto_memory_enabled = Some(true);
            s.settings.memory_max_retrieved = Some(8);
        });

        let response = memory_config_get_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["config"]["auto_memory_enabled"], json!(true));
        assert_eq!(body["config"]["memory_max_retrieved"], json!(8));
        assert!(body["stats"]["total"].as_u64().is_some());

        let response = speech_models_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(body["models"].as_array().expect("models").len() >= 4);

        let response = speech_model_download_handler(Json(SpeechModelDownloadRequest {
            model_id: "tiny".to_string(),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        let body = response_json(response).await;
        assert_eq!(body["code"], json!("speech_download_not_implemented"));

        let response = search_cookies_import_handler(Json(SearchCookiesImportRequest {
            cookies: json!([{ "name": "session", "value": "redacted" }]),
        }))
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(home.path().join("search-cookies.json").exists());

        let response = search_cookies_export_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["cookies"][0]["name"], json!("session"));

        let response = search_cookies_clear_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!home.path().join("search-cookies.json").exists());

        let response = data_export_handler(State(state.clone()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["ok"], json!(true));
        assert!(body["bytes"].as_u64().expect("bytes") > 0);

        let response = token_savings_handler(State(state)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["total_saved_tokens"], json!(0));
        assert_eq!(body["cache_hit_rate"], json!(0.0));
    }
}
