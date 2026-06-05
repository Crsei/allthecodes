//! Settings, command execution, and debug handlers — admin operations.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;

use allthecodes_config::settings::{
    load_global_config, write_user_settings, RawSettings, SettingsSource,
};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::PermissionMode;

use crate::handlers::ApiError;
use crate::state::{SessionOwner, WebState};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SettingsRequest {
    pub action: String,
    pub value: serde_json::Value,
}

#[derive(Serialize)]
pub struct SettingsResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Deserialize)]
pub struct CommandRequest {
    pub command: String,
    #[serde(default)]
    pub args: String,
}

#[derive(Serialize)]
pub struct CommandResponse {
    #[serde(rename = "type")]
    pub response_type: String, // "output" | "clear" | "error"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
pub struct DebugActionRequest {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize)]
pub struct DebugActionResponse {
    pub action_id: String,
    pub session_id: String,
    pub trace_ref: String,
    pub mutates_runtime: bool,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct DebugStateResponse {
    pub enabled: bool,
    pub session_id: String,
    pub ownership: crate::state::SessionOwnership,
    pub is_streaming: bool,
    pub pty: crate::ws::tui::PtyDiagnosticsSnapshot,
}

#[derive(Serialize)]
pub struct SessionTraceResponse {
    pub session_id: String,
    pub ownership: crate::state::SessionOwnership,
    pub is_streaming: bool,
    pub current_turn: Option<TurnTraceItem>,
    pub pending_permissions: Vec<String>,
    pub pending_questions: Vec<String>,
}

#[derive(Serialize)]
pub struct TurnTraceItem {
    pub status: String,
    pub started_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Settings handler
// ---------------------------------------------------------------------------

/// POST /api/settings -- Mutate application settings.
pub async fn settings_handler(
    State(state): State<WebState>,
    Json(req): Json<SettingsRequest>,
) -> impl IntoResponse {
    info!(action = %req.action, "POST /api/settings");

    match req.action.as_str() {
        "set_model" => {
            let model = req.value.as_str().unwrap_or("").to_string();
            let settings = state.engine().app_state().settings.clone();
            let available = settings.available_models.clone();
            let resolved =
                allthecodes_commands::model::resolve_model_alias_with_settings(&model, &settings);
            if let Err(message) = allthecodes_commands::model::check_available_with_settings(
                &resolved, &available, &settings,
            ) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: format!("Rejected: {}", message),
                    }),
                );
            }
            match persist_setting(&state, "model", serde_json::json!(resolved.clone())) {
                Ok(message) => (
                    StatusCode::OK,
                    Json(SettingsResponse {
                        ok: true,
                        message: format!("Model set to {resolved}; {message}"),
                    }),
                ),
                Err(err) => (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: err.to_string(),
                    }),
                ),
            }
        }
        "set_permission_mode" => {
            let mode_str = req.value.as_str().unwrap_or("default");
            let mode = match mode_str {
                "auto" => PermissionMode::Auto,
                "bypass" => PermissionMode::Bypass,
                "plan" => PermissionMode::Plan,
                _ => PermissionMode::Default,
            };
            let mut permission_context = state.engine().app_state().tool_permission_context;
            let transition =
                allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
                    &mut permission_context,
                    mode,
                );
            if transition.auto_mode_blocked_by_policy {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message:
                            "Auto mode is disabled by configuration (permissions.enableAutoMode=false)."
                                .to_string(),
                    }),
                );
            }
            let effective_mode = permission_context.mode;
            match persist_setting(
                &state,
                "permission_mode",
                serde_json::json!(effective_mode.as_str()),
            ) {
                Ok(message) => (
                    StatusCode::OK,
                    Json(SettingsResponse {
                        ok: true,
                        message: format!(
                            "Permission mode set to {}; {message}",
                            effective_mode.as_str()
                        ),
                    }),
                ),
                Err(err) => (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: err.to_string(),
                    }),
                ),
            }
        }
        "set_thinking" => {
            let enabled = thinking_enabled_from_value(&req.value);
            match persist_setting(&state, "thinking", thinking_setting_value(enabled)) {
                Ok(message) => (
                    StatusCode::OK,
                    Json(SettingsResponse {
                        ok: true,
                        message: format!("Thinking set to {enabled:?}; {message}"),
                    }),
                ),
                Err(err) => (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: err.to_string(),
                    }),
                ),
            }
        }
        "set_fast_mode" => match persist_setting(&state, "fast_mode", req.value.clone()) {
            Ok(message) => (StatusCode::OK, Json(SettingsResponse { ok: true, message })),
            Err(err) => (
                StatusCode::BAD_REQUEST,
                Json(SettingsResponse {
                    ok: false,
                    message: err.to_string(),
                }),
            ),
        },
        "set_effort" => match persist_setting(&state, "effort_level", req.value.clone()) {
            Ok(message) => (StatusCode::OK, Json(SettingsResponse { ok: true, message })),
            Err(err) => (
                StatusCode::BAD_REQUEST,
                Json(SettingsResponse {
                    ok: false,
                    message: err.to_string(),
                }),
            ),
        },
        "set_auto_compact" if req.value.is_object() => {
            match handle_auto_compact(&state, &req.value) {
                Ok(message) => (StatusCode::OK, Json(SettingsResponse { ok: true, message })),
                Err(err) => (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: err.to_string(),
                    }),
                ),
            }
        }
        "set_ext" => match handle_set_ext(&state, &req.value) {
            Ok(message) => (StatusCode::OK, Json(SettingsResponse { ok: true, message })),
            Err(err) => (
                StatusCode::BAD_REQUEST,
                Json(SettingsResponse {
                    ok: false,
                    message: err.to_string(),
                }),
            ),
        },
        action => {
            if let Some(key) = action_to_setting_key(action) {
                match persist_setting(&state, key, req.value.clone()) {
                    Ok(message) => (StatusCode::OK, Json(SettingsResponse { ok: true, message })),
                    Err(err) => (
                        StatusCode::BAD_REQUEST,
                        Json(SettingsResponse {
                            ok: false,
                            message: err.to_string(),
                        }),
                    ),
                }
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(SettingsResponse {
                        ok: false,
                        message: format!("Unknown action: {}", req.action),
                    }),
                )
            }
        }
    }
}

fn handle_set_ext(state: &WebState, value: &Value) -> Result<String> {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .context("set_ext requires value.path")?;
    let setting_value = value
        .get("value")
        .cloned()
        .context("set_ext requires value.value")?;

    if let Some(key) = normalize_settings_path(path) {
        persist_setting(state, key, setting_value)
    } else {
        persist_extra_setting(state, path, setting_value)
    }
}

fn handle_auto_compact(state: &WebState, value: &Value) -> Result<String> {
    let enabled = value
        .get("enabled")
        .or_else(|| value.get("auto_compact"))
        .cloned()
        .context("set_auto_compact object requires enabled")?;
    persist_setting(state, "auto_compact", enabled)?;

    if let Some(threshold) = value
        .get("threshold")
        .or_else(|| value.get("compact_threshold"))
    {
        persist_setting(state, "compact_threshold", threshold.clone())?;
    }
    if let Some(keep_recent) = value
        .get("keep_recent")
        .or_else(|| value.get("keep_recent_messages"))
    {
        persist_setting(state, "keep_recent_messages", keep_recent.clone())?;
    }

    Ok("auto_compact persisted".to_string())
}

pub(crate) fn persist_setting(state: &WebState, key: &str, value: Value) -> Result<String> {
    validate_setting_value(key, &value)?;

    let mut raw = load_global_config().context("failed to load user settings")?;
    apply_value_to_raw(&mut raw, key, value.clone())?;
    let written = write_user_settings(&raw).context("failed to write user settings")?;

    state.engine().update_app_state(|app_state| {
        apply_value_to_app_state(app_state, key, value.clone());
        app_state
            .settings
            .sources
            .insert(key.to_string(), SettingsSource::User);
    });

    Ok(format!("{} persisted to {}", key, written.display()))
}

fn persist_extra_setting(state: &WebState, path: &str, value: Value) -> Result<String> {
    reject_sensitive_path(path)?;
    let mut raw = load_global_config().context("failed to load user settings")?;
    raw.set_extra_path(path, value)
        .with_context(|| format!("invalid settings path: {path}"))?;
    let extra = raw.extra.clone();
    let written = write_user_settings(&raw).context("failed to write user settings")?;

    state.engine().update_app_state(|app_state| {
        app_state.settings.extra = extra;
        app_state
            .settings
            .sources
            .insert(path.to_string(), SettingsSource::User);
    });

    Ok(format!("{} persisted to {}", path, written.display()))
}

fn action_to_setting_key(action: &str) -> Option<&'static str> {
    Some(match action {
        "set_coding_agent" => "backend",
        "set_language" => "language",
        "set_app_icon" => "app_icon",
        "set_auto_start" => "auto_start",
        "set_start_minimized" => "start_minimized",
        "set_minimize_to_tray" => "minimize_to_tray",
        "set_close_to_tray" => "close_to_tray",
        "set_quick_chat_hide_on_blur" => "quick_chat_hide_on_blur",
        "set_quick_chat_inject_screen" => "quick_chat_inject_screen",
        "set_quick_chat_ambient" => "quick_chat_ambient",
        "set_auto_approve" | "set_auto_approve_tools" => "auto_approve_tools",
        "set_analytics" | "set_analytics_enabled" => "analytics_enabled",
        "set_default_model" => "default_model",
        "set_system_prompt" => "system_prompt",
        "set_context_window" => "context_window",
        "set_max_messages" => "max_messages",
        "set_auto_title" => "auto_title",
        "set_temperature" => "temperature",
        "set_max_tokens" => "max_tokens",
        "set_streaming" => "streaming",
        "set_show_token_usage" => "show_token_usage",
        "set_markdown" | "set_markdown_rendering" => "markdown_rendering",
        "set_single_dollar_math" => "single_dollar_math",
        "set_infographic" | "set_infographic_visualization" => "infographic",
        "set_auto_collapse" | "set_auto_collapse_reasoning" => "auto_collapse_reasoning",
        "set_quick_reply" | "set_quick_reply_suggestions" => "quick_reply_suggestions",
        "set_default_tool_selection" => "default_tool_selection",
        "set_default_skill_selection" => "default_skill_selection",
        "set_sound_effects" => "sound_effects",
        "set_auto_compact" => "auto_compact",
        "set_compact_threshold" => "compact_threshold",
        "set_keep_recent" | "set_keep_recent_messages" => "keep_recent_messages",
        "set_hashline_mode" => "hashline_mode",
        "set_effort_level" => "effort_level",
        "set_theme" => "theme",
        "set_font_family" => "font_family",
        "set_font_size" => "font_size",
        "set_density" => "density",
        "set_sidebar_width" => "sidebar_width",
        "set_sidebar_mode" => "sidebar_mode",
        "set_line_numbers" => "line_numbers",
        "set_word_wrap" => "word_wrap",
        "set_minimap" => "minimap",
        "set_use_system_caret" => "use_system_caret",
        "set_tool_card_expand" => "tool_card_expand",
        "set_terminal_font" => "terminal_font",
        "set_terminal_font_size" => "terminal_font_size",
        "set_persist_terminals" => "persist_terminals",
        "set_blink_cursor" => "blink_cursor",
        "set_proxy_enabled" => "proxy_enabled",
        "set_proxy_url" => "proxy_url",
        "set_prefer_ipv4" => "prefer_ipv4",
        "set_request_timeout" => "request_timeout",
        "set_retry_attempts" => "retry_attempts",
        "set_user_agent" | "set_custom_user_agent" => "custom_user_agent",
        "set_speech_enabled" => "speech_enabled",
        "set_speech_model" | "set_speech_active_model" => "speech_active_model",
        "set_speech_language" => "speech_language",
        "set_tts_provider" => "tts_provider",
        "set_tts_api_key" => "tts_api_key",
        "set_tts_voice" => "tts_voice",
        "set_tts_voice_custom_id" => "tts_voice_custom_id",
        "set_tts_model" => "tts_model",
        "set_search_engine" => "search_engine",
        "set_memory_enabled" => "auto_memory_enabled",
        "set_auto_retrieve" | "set_memory_auto_retrieve" => "memory_auto_retrieve",
        "set_query_rewriting" | "set_memory_query_rewriting" => "memory_query_rewriting",
        "set_max_retrieved" | "set_memory_max_retrieved" => "memory_max_retrieved",
        "set_similarity_threshold" | "set_memory_similarity_threshold" => {
            "memory_similarity_threshold"
        }
        "set_auto_summarize" | "set_memory_auto_summarize" => "memory_auto_summarize",
        "set_nightly_consolidation" | "set_memory_nightly" => "memory_nightly",
        "set_sleep_time" | "set_memory_sleep_time" => "memory_sleep_time",
        "set_temp_ttl" | "set_memory_temp_ttl" => "memory_temp_ttl",
        "set_archive_retention" | "set_memory_archive_retention" => "memory_archive_retention",
        "set_memory_tool_model" => "memory_tool_model",
        "set_embedding_model" | "set_memory_embedding_model" => "memory_embedding_model",
        "set_cloud_sync_enabled" => "cloud_sync_enabled",
        "set_cloud_sync_path" => "cloud_sync_path",
        "set_token_savings_tracking" => "token_savings_tracking",
        _ => return None,
    })
}

pub(crate) fn normalize_settings_path(path: &str) -> Option<&'static str> {
    Some(match path {
        "general.coding_agent" | "general.backend" | "coding_agent" | "backend" => "backend",
        "general.language" | "language" => "language",
        "general.app_icon" | "app_icon" => "app_icon",
        "general.auto_start" | "auto_start" => "auto_start",
        "general.start_minimized" | "start_minimized" => "start_minimized",
        "general.minimize_to_tray" | "minimize_to_tray" => "minimize_to_tray",
        "general.close_to_tray" | "close_to_tray" => "close_to_tray",
        "general.quick_chat_hide_on_blur"
        | "quick_chat.hide_on_blur"
        | "quick_chat_hide_on_blur" => "quick_chat_hide_on_blur",
        "general.quick_chat_inject_screen" | "quick_chat.inject_screen" => {
            "quick_chat_inject_screen"
        }
        "quick_chat_inject_screen" => "quick_chat_inject_screen",
        "general.quick_chat_ambient" | "quick_chat.ambient" | "quick_chat_ambient" => {
            "quick_chat_ambient"
        }
        "general.auto_approve"
        | "general.auto_approve_tools"
        | "auto_approve"
        | "auto_approve_tools" => "auto_approve_tools",
        "general.analytics" | "general.analytics_enabled" | "analytics" | "analytics_enabled" => {
            "analytics_enabled"
        }
        "chat.default_model" | "default_model" => "default_model",
        "chat.system_prompt" | "system_prompt" => "system_prompt",
        "chat.context_window" | "context_window" => "context_window",
        "chat.max_messages" | "max_messages" => "max_messages",
        "chat.auto_title" | "auto_title" => "auto_title",
        "projects.temperature" | "temperature" => "temperature",
        "projects.max_tokens" | "max_tokens" => "max_tokens",
        "projects.streaming" | "streaming" => "streaming",
        "projects.show_token_usage" | "show_token_usage" => "show_token_usage",
        "projects.markdown" | "projects.markdown_rendering" | "markdown_rendering" => {
            "markdown_rendering"
        }
        "projects.single_dollar_math" | "single_dollar_math" => "single_dollar_math",
        "projects.infographic" | "infographic" => "infographic",
        "projects.auto_collapse" | "projects.auto_collapse_reasoning" => "auto_collapse_reasoning",
        "projects.quick_reply" | "projects.quick_reply_suggestions" => "quick_reply_suggestions",
        "projects.default_tool_selection" | "default_tool_selection" => "default_tool_selection",
        "projects.default_skill_selection" | "default_skill_selection" => "default_skill_selection",
        "projects.sound_effects" | "sound_effects" => "sound_effects",
        "projects.auto_compact" | "auto_compact" => "auto_compact",
        "projects.compact_threshold" | "compact_threshold" => "compact_threshold",
        "projects.keep_recent" | "projects.keep_recent_messages" | "keep_recent_messages" => {
            "keep_recent_messages"
        }
        "projects.hashline_mode" | "hashline_mode" => "hashline_mode",
        "projects.effort_level" | "effort_level" => "effort_level",
        "fast_mode" => "fast_mode",
        "ui.theme" | "theme" => "theme",
        "ui.font_family" | "font_family" => "font_family",
        "ui.font_size" | "font_size" => "font_size",
        "ui.density" | "density" => "density",
        "ui.sidebar_width" | "sidebar_width" => "sidebar_width",
        "ui.sidebar_mode" | "sidebar_mode" => "sidebar_mode",
        "ui.line_numbers" | "line_numbers" => "line_numbers",
        "ui.word_wrap" | "word_wrap" => "word_wrap",
        "ui.minimap" | "minimap" => "minimap",
        "ui.use_system_caret" | "use_system_caret" => "use_system_caret",
        "ui.tool_card_expand" | "tool_card_expand" => "tool_card_expand",
        "ui.terminal_font" | "terminal_font" => "terminal_font",
        "ui.terminal_font_size" | "terminal_font_size" => "terminal_font_size",
        "ui.persist_terminals" | "persist_terminals" => "persist_terminals",
        "ui.blink_cursor" | "blink_cursor" => "blink_cursor",
        "network.proxy_enabled" | "proxy_enabled" => "proxy_enabled",
        "network.proxy_url" | "proxy_url" => "proxy_url",
        "network.prefer_ipv4" | "prefer_ipv4" => "prefer_ipv4",
        "network.request_timeout" | "request_timeout" => "request_timeout",
        "network.retry_attempts" | "retry_attempts" => "retry_attempts",
        "network.user_agent" | "network.custom_user_agent" | "custom_user_agent" => {
            "custom_user_agent"
        }
        "speech.enabled" | "speech.speech_enabled" | "speech_enabled" => "speech_enabled",
        "speech.model" | "speech.active_model" | "speech_active_model" => "speech_active_model",
        "speech.language" | "speech.speech_language" | "speech_language" => "speech_language",
        "tts.provider" | "tts_provider" => "tts_provider",
        "tts.api_key" | "tts_api_key" => "tts_api_key",
        "tts.voice" | "tts_voice" => "tts_voice",
        "tts.voice_custom_id" | "tts_voice_custom_id" => "tts_voice_custom_id",
        "tts.model" | "tts_model" => "tts_model",
        "web_search.search_engine" | "search.search_engine" | "search_engine" => "search_engine",
        "memory.enabled" | "memory.auto_memory_enabled" | "auto_memory_enabled" => {
            "auto_memory_enabled"
        }
        "memory.auto_retrieve" | "memory_auto_retrieve" => "memory_auto_retrieve",
        "memory.query_rewriting" | "memory_query_rewriting" => "memory_query_rewriting",
        "memory.max_retrieved" | "memory_max_retrieved" => "memory_max_retrieved",
        "memory.similarity_threshold" | "memory_similarity_threshold" => {
            "memory_similarity_threshold"
        }
        "memory.auto_summarize" | "memory_auto_summarize" => "memory_auto_summarize",
        "memory.nightly" | "memory_nightly" => "memory_nightly",
        "memory.sleep_time" | "memory_sleep_time" => "memory_sleep_time",
        "memory.temp_ttl" | "memory_temp_ttl" => "memory_temp_ttl",
        "memory.archive_retention" | "memory_archive_retention" => "memory_archive_retention",
        "memory.tool_model" | "memory_tool_model" => "memory_tool_model",
        "memory.embedding_model" | "memory_embedding_model" => "memory_embedding_model",
        "data.cloud_sync_enabled" | "cloud_sync_enabled" => "cloud_sync_enabled",
        "data.cloud_sync_path" | "cloud_sync_path" => "cloud_sync_path",
        "token_savings.tracking" | "token_savings_tracking" => "token_savings_tracking",
        _ => return None,
    })
}

fn validate_setting_value(key: &str, value: &Value) -> Result<()> {
    match setting_kind(key) {
        SettingKind::Bool => {
            bool_value(key, value)?;
        }
        SettingKind::String => {
            string_value(key, value)?;
        }
        SettingKind::SensitiveString => {
            string_value(key, value)?;
        }
        SettingKind::Json => {}
        SettingKind::U64 => {
            u64_value(key, value)?;
        }
        SettingKind::U8 => {
            u8_value(key, value)?;
        }
        SettingKind::Temperature => {
            let value = f64_value(key, value)?;
            if !(0.0..=2.0).contains(&value) {
                bail!("{key} must be between 0 and 2");
            }
        }
    }
    if matches!(key, "tts_api_key") {
        reject_empty_string(key, value)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SettingKind {
    Bool,
    String,
    SensitiveString,
    Json,
    U64,
    U8,
    Temperature,
}

fn setting_kind(key: &str) -> SettingKind {
    match key {
        "thinking" => SettingKind::Json,
        "auto_start"
        | "start_minimized"
        | "minimize_to_tray"
        | "close_to_tray"
        | "quick_chat_hide_on_blur"
        | "quick_chat_inject_screen"
        | "quick_chat_ambient"
        | "auto_approve_tools"
        | "analytics_enabled"
        | "auto_title"
        | "streaming"
        | "show_token_usage"
        | "markdown_rendering"
        | "single_dollar_math"
        | "infographic"
        | "auto_collapse_reasoning"
        | "quick_reply_suggestions"
        | "sound_effects"
        | "auto_compact"
        | "hashline_mode"
        | "fast_mode"
        | "proxy_enabled"
        | "prefer_ipv4"
        | "speech_enabled"
        | "auto_memory_enabled"
        | "memory_auto_retrieve"
        | "memory_query_rewriting"
        | "memory_auto_summarize"
        | "memory_nightly"
        | "cloud_sync_enabled"
        | "token_savings_tracking"
        | "line_numbers"
        | "word_wrap"
        | "minimap"
        | "use_system_caret"
        | "persist_terminals"
        | "blink_cursor" => SettingKind::Bool,
        "context_window"
        | "max_messages"
        | "max_tokens"
        | "request_timeout"
        | "font_size"
        | "sidebar_width"
        | "terminal_font_size"
        | "memory_temp_ttl"
        | "memory_archive_retention" => SettingKind::U64,
        "retry_attempts"
        | "compact_threshold"
        | "keep_recent_messages"
        | "memory_max_retrieved"
        | "memory_similarity_threshold" => SettingKind::U8,
        "temperature" => SettingKind::Temperature,
        "tts_api_key" => SettingKind::SensitiveString,
        _ => SettingKind::String,
    }
}

fn apply_value_to_raw(raw: &mut RawSettings, key: &str, value: Value) -> Result<()> {
    match key {
        "model" => raw.model = Some(string_value(key, &value)?),
        "backend" => raw.backend = Some(normalize_backend_value(&string_value(key, &value)?)),
        "permission_mode" => raw.permission_mode = Some(string_value(key, &value)?),
        "thinking" => raw.thinking = value_to_optional(value),
        "language" => raw.language = Some(string_value(key, &value)?),
        "app_icon" => raw.app_icon = Some(string_value(key, &value)?),
        "auto_start" => raw.auto_start = Some(bool_value(key, &value)?),
        "start_minimized" => raw.start_minimized = Some(bool_value(key, &value)?),
        "minimize_to_tray" => raw.minimize_to_tray = Some(bool_value(key, &value)?),
        "close_to_tray" => raw.close_to_tray = Some(bool_value(key, &value)?),
        "quick_chat_hide_on_blur" => raw.quick_chat_hide_on_blur = Some(bool_value(key, &value)?),
        "quick_chat_inject_screen" => raw.quick_chat_inject_screen = Some(bool_value(key, &value)?),
        "quick_chat_ambient" => raw.quick_chat_ambient = Some(bool_value(key, &value)?),
        "auto_approve_tools" => raw.auto_approve_tools = Some(bool_value(key, &value)?),
        "analytics_enabled" => raw.analytics_enabled = Some(bool_value(key, &value)?),
        "default_model" => raw.default_model = Some(string_value(key, &value)?),
        "effort_level" => raw.effort_level = Some(string_value(key, &value)?),
        "system_prompt" => raw.system_prompt = Some(string_value(key, &value)?),
        "context_window" => raw.context_window = Some(u64_value(key, &value)?),
        "max_messages" => raw.max_messages = Some(u64_value(key, &value)?),
        "auto_title" => raw.auto_title = Some(bool_value(key, &value)?),
        "temperature" => raw.temperature = Some(f64_value(key, &value)?),
        "max_tokens" => raw.max_tokens = Some(u64_value(key, &value)?),
        "streaming" => raw.streaming = Some(bool_value(key, &value)?),
        "show_token_usage" => raw.show_token_usage = Some(bool_value(key, &value)?),
        "markdown_rendering" => raw.markdown_rendering = Some(bool_value(key, &value)?),
        "single_dollar_math" => raw.single_dollar_math = Some(bool_value(key, &value)?),
        "infographic" => raw.infographic = Some(bool_value(key, &value)?),
        "auto_collapse_reasoning" => raw.auto_collapse_reasoning = Some(bool_value(key, &value)?),
        "quick_reply_suggestions" => raw.quick_reply_suggestions = Some(bool_value(key, &value)?),
        "default_tool_selection" => raw.default_tool_selection = Some(string_value(key, &value)?),
        "default_skill_selection" => raw.default_skill_selection = Some(string_value(key, &value)?),
        "sound_effects" => raw.sound_effects = Some(bool_value(key, &value)?),
        "auto_compact" => raw.auto_compact = Some(bool_value(key, &value)?),
        "compact_threshold" => raw.compact_threshold = Some(u8_value(key, &value)?),
        "keep_recent_messages" => raw.keep_recent_messages = Some(u8_value(key, &value)?),
        "hashline_mode" => raw.hashline_mode = Some(bool_value(key, &value)?),
        "theme" => raw.theme = Some(string_value(key, &value)?),
        "fast_mode" => raw.fast_mode = Some(bool_value(key, &value)?),
        "proxy_enabled" => raw.proxy_enabled = Some(bool_value(key, &value)?),
        "proxy_url" => raw.proxy_url = Some(string_value(key, &value)?),
        "prefer_ipv4" => raw.prefer_ipv4 = Some(bool_value(key, &value)?),
        "request_timeout" => raw.request_timeout = Some(u64_value(key, &value)?),
        "retry_attempts" => raw.retry_attempts = Some(u8_value(key, &value)?),
        "custom_user_agent" => raw.custom_user_agent = Some(string_value(key, &value)?),
        "speech_enabled" => raw.speech_enabled = Some(bool_value(key, &value)?),
        "speech_active_model" => raw.speech_active_model = Some(string_value(key, &value)?),
        "speech_language" => raw.speech_language = Some(string_value(key, &value)?),
        "tts_provider" => raw.tts_provider = Some(string_value(key, &value)?),
        "tts_api_key" => raw.tts_api_key = Some(string_value(key, &value)?),
        "tts_voice" => raw.tts_voice = Some(string_value(key, &value)?),
        "tts_voice_custom_id" => raw.tts_voice_custom_id = Some(string_value(key, &value)?),
        "tts_model" => raw.tts_model = Some(string_value(key, &value)?),
        "search_engine" => raw.search_engine = Some(string_value(key, &value)?),
        "auto_memory_enabled" => raw.auto_memory_enabled = Some(bool_value(key, &value)?),
        "memory_auto_retrieve" => raw.memory_auto_retrieve = Some(bool_value(key, &value)?),
        "memory_query_rewriting" => raw.memory_query_rewriting = Some(bool_value(key, &value)?),
        "memory_max_retrieved" => raw.memory_max_retrieved = Some(u8_value(key, &value)?),
        "memory_similarity_threshold" => {
            raw.memory_similarity_threshold = Some(u8_value(key, &value)?)
        }
        "memory_auto_summarize" => raw.memory_auto_summarize = Some(bool_value(key, &value)?),
        "memory_nightly" => raw.memory_nightly = Some(bool_value(key, &value)?),
        "memory_sleep_time" => raw.memory_sleep_time = Some(string_value(key, &value)?),
        "memory_temp_ttl" => raw.memory_temp_ttl = Some(u32_value(key, &value)?),
        "memory_archive_retention" => raw.memory_archive_retention = Some(u32_value(key, &value)?),
        "memory_tool_model" => raw.memory_tool_model = Some(string_value(key, &value)?),
        "memory_embedding_model" => raw.memory_embedding_model = Some(string_value(key, &value)?),
        "cloud_sync_enabled" => raw.cloud_sync_enabled = Some(bool_value(key, &value)?),
        "cloud_sync_path" => raw.cloud_sync_path = Some(string_value(key, &value)?),
        "token_savings_tracking" => raw.token_savings_tracking = Some(bool_value(key, &value)?),
        _ => raw.set_extra_path(key, value)?,
    }
    Ok(())
}

fn apply_value_to_app_state(app_state: &mut AppState, key: &str, value: Value) {
    let settings = &mut app_state.settings;
    match key {
        "model" => {
            if let Some(value) = value.as_str() {
                app_state.main_loop_model = value.to_string();
                settings.model = Some(value.to_string());
            }
        }
        "backend" => {
            if let Ok(value) = string_value(key, &value) {
                let normalized = normalize_backend_value(&value);
                app_state.main_loop_backend = normalized.clone();
                settings.backend = Some(normalized);
            }
        }
        "permission_mode" => {
            let requested = value
                .as_str()
                .map(parse_permission_mode)
                .unwrap_or(PermissionMode::Default);
            allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
                &mut app_state.tool_permission_context,
                requested,
            );
            settings.permission_mode = Some(app_state.tool_permission_context.mode.as_str().into());
        }
        "thinking" => {
            app_state.thinking_enabled = thinking_enabled_from_value(&value);
            settings.thinking = value_to_optional(value);
        }
        "language" => settings.language = value.as_str().map(str::to_string),
        "app_icon" => settings.app_icon = value.as_str().map(str::to_string),
        "auto_start" => settings.auto_start = value.as_bool(),
        "start_minimized" => settings.start_minimized = value.as_bool(),
        "minimize_to_tray" => settings.minimize_to_tray = value.as_bool(),
        "close_to_tray" => settings.close_to_tray = value.as_bool(),
        "quick_chat_hide_on_blur" => settings.quick_chat_hide_on_blur = value.as_bool(),
        "quick_chat_inject_screen" => settings.quick_chat_inject_screen = value.as_bool(),
        "quick_chat_ambient" => settings.quick_chat_ambient = value.as_bool(),
        "auto_approve_tools" => settings.auto_approve_tools = value.as_bool(),
        "analytics_enabled" => settings.analytics_enabled = value.as_bool(),
        "default_model" => settings.default_model = value.as_str().map(str::to_string),
        "effort_level" => {
            settings.effort_level = value.as_str().map(str::to_string);
            app_state.effort_value = settings.effort_level.clone();
        }
        "system_prompt" => settings.system_prompt = value.as_str().map(str::to_string),
        "context_window" => settings.context_window = value.as_u64(),
        "max_messages" => settings.max_messages = value.as_u64(),
        "auto_title" => settings.auto_title = value.as_bool(),
        "temperature" => settings.temperature = value.as_f64(),
        "max_tokens" => settings.max_tokens = value.as_u64(),
        "streaming" => settings.streaming = value.as_bool(),
        "show_token_usage" => settings.show_token_usage = value.as_bool(),
        "markdown_rendering" => settings.markdown_rendering = value.as_bool(),
        "single_dollar_math" => settings.single_dollar_math = value.as_bool(),
        "infographic" => settings.infographic = value.as_bool(),
        "auto_collapse_reasoning" => settings.auto_collapse_reasoning = value.as_bool(),
        "quick_reply_suggestions" => settings.quick_reply_suggestions = value.as_bool(),
        "default_tool_selection" => {
            settings.default_tool_selection = value.as_str().map(str::to_string)
        }
        "default_skill_selection" => {
            settings.default_skill_selection = value.as_str().map(str::to_string)
        }
        "sound_effects" => settings.sound_effects = value.as_bool(),
        "auto_compact" => settings.auto_compact = value.as_bool(),
        "compact_threshold" => settings.compact_threshold = value.as_u64().map(|v| v as u8),
        "keep_recent_messages" => settings.keep_recent_messages = value.as_u64().map(|v| v as u8),
        "hashline_mode" => settings.hashline_mode = value.as_bool(),
        "theme" => settings.theme = value.as_str().map(str::to_string),
        "fast_mode" => {
            settings.fast_mode = value.as_bool();
            app_state.fast_mode = value.as_bool().unwrap_or(false);
        }
        "proxy_enabled" => settings.proxy_enabled = value.as_bool(),
        "proxy_url" => settings.proxy_url = value.as_str().map(str::to_string),
        "prefer_ipv4" => settings.prefer_ipv4 = value.as_bool(),
        "request_timeout" => settings.request_timeout = value.as_u64(),
        "retry_attempts" => settings.retry_attempts = value.as_u64().map(|v| v as u8),
        "custom_user_agent" => settings.custom_user_agent = value.as_str().map(str::to_string),
        "speech_enabled" => settings.speech_enabled = value.as_bool(),
        "speech_active_model" => settings.speech_active_model = value.as_str().map(str::to_string),
        "speech_language" => settings.speech_language = value.as_str().map(str::to_string),
        "tts_provider" => settings.tts_provider = value.as_str().map(str::to_string),
        "tts_api_key" => settings.tts_api_key = value.as_str().map(str::to_string),
        "tts_voice" => settings.tts_voice = value.as_str().map(str::to_string),
        "tts_voice_custom_id" => settings.tts_voice_custom_id = value.as_str().map(str::to_string),
        "tts_model" => settings.tts_model = value.as_str().map(str::to_string),
        "search_engine" => settings.search_engine = value.as_str().map(str::to_string),
        "auto_memory_enabled" => settings.auto_memory_enabled = value.as_bool(),
        "memory_auto_retrieve" => settings.memory_auto_retrieve = value.as_bool(),
        "memory_query_rewriting" => settings.memory_query_rewriting = value.as_bool(),
        "memory_max_retrieved" => {
            settings.memory_max_retrieved = value.as_u64().map(|value| value as u8)
        }
        "memory_similarity_threshold" => {
            settings.memory_similarity_threshold = value.as_u64().map(|value| value as u8)
        }
        "memory_auto_summarize" => settings.memory_auto_summarize = value.as_bool(),
        "memory_nightly" => settings.memory_nightly = value.as_bool(),
        "memory_sleep_time" => settings.memory_sleep_time = value.as_str().map(str::to_string),
        "memory_temp_ttl" => settings.memory_temp_ttl = value.as_u64().map(|value| value as u32),
        "memory_archive_retention" => {
            settings.memory_archive_retention = value.as_u64().map(|value| value as u32)
        }
        "memory_tool_model" => settings.memory_tool_model = value.as_str().map(str::to_string),
        "memory_embedding_model" => {
            settings.memory_embedding_model = value.as_str().map(str::to_string)
        }
        "cloud_sync_enabled" => settings.cloud_sync_enabled = value.as_bool(),
        "cloud_sync_path" => settings.cloud_sync_path = value.as_str().map(str::to_string),
        "token_savings_tracking" => settings.token_savings_tracking = value.as_bool(),
        _ => {
            settings.extra.insert(key.to_string(), value);
        }
    }
}

fn string_value(key: &str, value: &Value) -> Result<String> {
    value
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("{key} must be a string"))
}

fn reject_empty_string(key: &str, value: &Value) -> Result<()> {
    if string_value(key, value)?.trim().is_empty() {
        bail!("{key} cannot be empty");
    }
    Ok(())
}

fn bool_value(key: &str, value: &Value) -> Result<bool> {
    value
        .as_bool()
        .with_context(|| format!("{key} must be a boolean"))
}

fn u64_value(key: &str, value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|value| *value > 0)
        .with_context(|| format!("{key} must be a positive integer"))
}

fn u32_value(key: &str, value: &Value) -> Result<u32> {
    let value = u64_value(key, value)?;
    u32::try_from(value).with_context(|| format!("{key} is too large"))
}

fn u8_value(key: &str, value: &Value) -> Result<u8> {
    let value = u64_value(key, value)?;
    u8::try_from(value).with_context(|| format!("{key} must be between 1 and 255"))
}

fn f64_value(key: &str, value: &Value) -> Result<f64> {
    value
        .as_f64()
        .with_context(|| format!("{key} must be a number"))
}

fn value_to_optional(value: Value) -> Option<Value> {
    if value.is_null() {
        None
    } else {
        Some(value)
    }
}

fn thinking_setting_value(enabled: Option<bool>) -> Value {
    enabled
        .map(|value| {
            serde_json::json!({
                "type": if value { "enabled" } else { "disabled" }
            })
        })
        .unwrap_or(Value::Null)
}

fn thinking_enabled_from_value(value: &Value) -> Option<bool> {
    if let Some(value) = value.as_bool() {
        return Some(value);
    }
    let label = value
        .as_str()
        .or_else(|| value.get("type").and_then(Value::as_str))?;
    match label {
        "enabled" | "adaptive" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn parse_permission_mode(value: &str) -> PermissionMode {
    match value {
        "auto" => PermissionMode::Auto,
        "bypass" => PermissionMode::Bypass,
        "plan" => PermissionMode::Plan,
        _ => PermissionMode::Default,
    }
}

fn normalize_backend_value(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai-codex" => "codex".to_string(),
        "native" | "allthecodes" | "claude" | "claude-code" | "claude_code" | "auto" => {
            "native".to_string()
        }
        other => other.to_string(),
    }
}

fn reject_sensitive_path(path: &str) -> Result<()> {
    let normalized = path
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.contains("apikey")
        || normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
    {
        bail!("sensitive settings must use a typed setting action");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command handler
// ---------------------------------------------------------------------------

/// POST /api/command -- Execute a slash command.
pub async fn command_handler(
    State(state): State<WebState>,
    Json(req): Json<CommandRequest>,
) -> impl IntoResponse {
    info!(command = %req.command, args = %req.args, "POST /api/command");

    let commands = crate::handlers::get_all_commands();
    let cmd = commands
        .iter()
        .find(|c| c.name == req.command || c.aliases.contains(&req.command));

    let cmd = match cmd {
        Some(c) => c,
        None => {
            return Json(CommandResponse {
                response_type: "error".into(),
                content: format!("Unknown command: /{}", req.command),
                session_id: None,
            });
        }
    };

    // Build a CommandContext
    let messages = state.engine().messages();
    let app_state = state.engine().app_state();
    let cwd = std::path::PathBuf::from(state.engine().cwd());

    let mut ctx = allthecodes_commands::CommandContext {
        messages,
        cwd,
        app_state: app_state.clone(),
        session_id: state.engine().current_session_id(),
    };

    match cmd.handler.execute(&req.args, &mut ctx).await {
        Ok(result) => {
            // Apply any state mutations from the command
            state.engine().update_app_state(|s| {
                s.main_loop_model = ctx.app_state.main_loop_model.clone();
                s.settings = ctx.app_state.settings.clone();
                s.tool_permission_context = ctx.app_state.tool_permission_context.clone();
                s.thinking_enabled = ctx.app_state.thinking_enabled;
                s.fast_mode = ctx.app_state.fast_mode;
                s.effort_value = ctx.app_state.effort_value.clone();
            });

            match result {
                allthecodes_commands::CommandResult::Output(text) => Json(CommandResponse {
                    response_type: "output".into(),
                    content: text,
                    session_id: None,
                }),
                allthecodes_commands::CommandResult::SwitchSession {
                    session_id,
                    messages,
                    notice,
                } => {
                    state.engine().set_current_session_id(session_id.clone());
                    state.engine().replace_messages(messages);
                    Json(CommandResponse {
                        response_type: "switch_session".into(),
                        content: format!("{notice}\nSession: {session_id}"),
                        session_id: Some(session_id.to_string()),
                    })
                }
                allthecodes_commands::CommandResult::Clear => {
                    let session_id = state.engine().start_new_session();
                    Json(CommandResponse {
                        response_type: "clear".into(),
                        content: format!("Started a new session: {}", session_id),
                        session_id: Some(session_id.to_string()),
                    })
                }
                allthecodes_commands::CommandResult::Exit(msg) => Json(CommandResponse {
                    response_type: "output".into(),
                    content: msg,
                    session_id: None,
                }),
                allthecodes_commands::CommandResult::Query(_msgs) => Json(CommandResponse {
                    response_type: "output".into(),
                    content: "Command queued (query commands not yet supported in web UI)".into(),
                    session_id: None,
                }),
                allthecodes_commands::CommandResult::None => Json(CommandResponse {
                    response_type: "output".into(),
                    content: "OK".into(),
                    session_id: None,
                }),
            }
        }
        Err(e) => Json(CommandResponse {
            response_type: "error".into(),
            content: format!("Command error: {}", e),
            session_id: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Debug handlers
// ---------------------------------------------------------------------------

/// GET /api/debug/state -- Dev-only diagnostics snapshot.
pub async fn debug_state_handler(State(state): State<WebState>) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let engine = state.engine();
    Json(DebugStateResponse {
        enabled: true,
        session_id: engine.current_session_id().to_string(),
        ownership: state.ownership_snapshot(),
        is_streaming: state.is_streaming.load(std::sync::atomic::Ordering::SeqCst),
        pty: state.pty_diagnostics.snapshot(),
    })
    .into_response()
}

/// GET /api/debug/sessions/{id}/trace -- Dev-only session trace snapshot.
pub async fn debug_session_trace_handler(
    AxumPath(id): AxumPath<String>,
    State(state): State<WebState>,
) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let ownership = state.ownership_snapshot();
    let is_streaming = state.is_streaming.load(std::sync::atomic::Ordering::SeqCst);

    Json(SessionTraceResponse {
        session_id: id,
        ownership,
        is_streaming,
        current_turn: if is_streaming {
            Some(TurnTraceItem {
                status: "streaming".to_string(),
                started_at: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0),
                ),
            })
        } else {
            None
        },
        pending_permissions: Vec::new(),
        pending_questions: Vec::new(),
    })
    .into_response()
}

/// POST /api/debug/actions/{*action} -- Dev-only scripted action endpoint.
pub async fn debug_action_handler(
    AxumPath(path_action): AxumPath<String>,
    State(state): State<WebState>,
    Json(req): Json<DebugActionRequest>,
) -> Response {
    if !debug_enabled() {
        return debug_disabled_response();
    }

    let _params = req.params;
    let action = req.action.unwrap_or(path_action);
    let mutates_runtime = matches!(action.as_str(), "chat/abort");
    let mut ok = true;
    let mut error = None;

    match action.as_str() {
        "ui/state" | "fixtures/load" | "tui/open" | "tui/input" | "tui/resize" | "tui/close" => {}
        "chat/abort" => {
            state.engine().abort();
            state
                .is_streaming
                .store(false, std::sync::atomic::Ordering::SeqCst);
            state.release_owner(SessionOwner::ChatStream);
        }
        "chat/submit" => {
            ok = false;
            error = Some(
                "chat/submit must use /api/chat so the caller receives the SSE stream".to_string(),
            );
        }
        "permissions/respond" | "questions/respond" => {
            ok = true;
        }
        _ => {
            ok = false;
            error = Some(format!("Unknown debug action: {}", action));
        }
    }

    let session_id = state.engine().current_session_id().to_string();
    let action_id = format!(
        "debug-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    );
    Json(DebugActionResponse {
        trace_ref: format!("debug/actions/{action_id}"),
        action_id,
        session_id,
        mutates_runtime,
        ok,
        error,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn debug_enabled() -> bool {
    std::env::var("ALLTHECODES_WEB_DEBUG")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn debug_disabled_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "Debug API is disabled".into(),
            code: "debug_disabled".into(),
        }),
    )
        .into_response()
}
