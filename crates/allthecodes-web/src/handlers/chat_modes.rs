//! Chat mode bundle REST handlers and request-time resolution.

use std::collections::BTreeSet;
use std::path::Path;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use allthecodes_config::settings::{user_settings_path, write_settings_file, RawSettings};
use allthecodes_mcp::discovery::discover_mcp_servers_scoped;
use allthecodes_plugins::{get_enabled_plugins, PluginStatus};
use allthecodes_session::storage;

use crate::handlers::ApiError;
use crate::state::WebState;
use crate::workspace_metadata;

const CHAT_MODES_KEY: &str = "chatModes";
pub const NORMAL_CHAT_MODE_ID: &str = "normal";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeBundle {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub plugin_ids: Vec<String>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub mcp_server_names: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub built_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ChatModesSettings {
    #[serde(default)]
    bundles: Vec<ChatModeBundle>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResolved {
    pub bundle: ChatModeBundle,
    pub status: String,
    pub missing_plugins: Vec<String>,
    pub missing_skills: Vec<String>,
    pub missing_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModesResponse {
    pub modes: Vec<ChatModeResolved>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModePreference {
    pub workspace_key: String,
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResourcesResponse {
    pub plugins: Vec<ModePluginResource>,
    pub skills: Vec<ModeSkillResource>,
    pub mcp_servers: Vec<ModeMcpServerResource>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModePluginResource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSkillResource {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub source: String,
    pub description: String,
    pub model_invocable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeMcpServerResource {
    pub name: String,
    pub scope: String,
    pub transport: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeBundleUpsertRequest {
    pub bundle: ChatModeBundle,
}

#[derive(Debug, Clone)]
pub struct ModeActivation {
    pub mode_id: String,
    pub prompt_prefix: String,
}

pub async fn chat_modes_list_handler(State(state): State<WebState>) -> Response {
    match load_mode_bundles() {
        Ok(bundles) => {
            let resources = resource_index(&state);
            let modes = bundles
                .into_iter()
                .map(|bundle| resolve_bundle(bundle, &resources))
                .collect();
            Json(ChatModesResponse { modes }).into_response()
        }
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn chat_modes_resources_handler(State(state): State<WebState>) -> Response {
    let engine = state.engine();
    let cwd = engine.cwd();
    match list_resources(Path::new(&cwd)) {
        Ok(resources) => Json(resources).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn chat_modes_upsert_handler(
    AxumPath(id): AxumPath<String>,
    Json(req): Json<ChatModeBundleUpsertRequest>,
) -> Response {
    let mut bundle = req.bundle;
    let normalized_id = normalize_mode_id(&id);
    if normalized_id.is_empty() {
        return validation_error("mode id is required").into_response();
    }
    if normalize_mode_id(&bundle.id) != normalized_id {
        return validation_error("mode id does not match path").into_response();
    }
    bundle.id = normalized_id;
    bundle.display_name = bundle.display_name.trim().to_string();
    if bundle.display_name.is_empty() {
        bundle.display_name = bundle.id.clone();
    }
    bundle.plugin_ids = unique_trimmed(bundle.plugin_ids);
    bundle.skill_ids = unique_trimmed(bundle.skill_ids);
    bundle.mcp_server_names = unique_trimmed(bundle.mcp_server_names);
    bundle.built_in = is_builtin_mode(&bundle.id);

    match save_mode_bundle(bundle.clone()) {
        Ok(()) => Json(bundle).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn chat_modes_delete_handler(AxumPath(id): AxumPath<String>) -> Response {
    let id = normalize_mode_id(&id);
    if is_builtin_mode(&id) {
        return validation_error("built-in modes cannot be deleted").into_response();
    }
    match delete_mode_bundle(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub fn resolve_mode_activation(
    state: &WebState,
    mode: Option<&str>,
) -> Result<Option<ModeActivation>, (StatusCode, ApiError)> {
    let mode_id = normalize_mode_or_normal(mode);
    if mode_id == NORMAL_CHAT_MODE_ID {
        return Ok(None);
    }

    let bundles = load_mode_bundles().map_err(internal_api_error)?;
    let Some(bundle) = bundles.into_iter().find(|bundle| bundle.id == mode_id) else {
        return Err(validation_api_error(
            "mode_not_found",
            format!("Chat mode '{}' is not configured", mode_id),
        ));
    };
    if !bundle.enabled {
        return Err(validation_api_error(
            "mode_disabled",
            format!("Chat mode '{}' is disabled", mode_id),
        ));
    }

    let resources = resource_index(state);
    let resolved = resolve_bundle(bundle, &resources);
    if resolved.status != "ready" {
        return Err((
            StatusCode::BAD_REQUEST,
            ApiError {
                error: format!(
                    "Chat mode '{}' is incomplete: missing plugins [{}], skills [{}], MCP servers [{}]",
                    resolved.bundle.id,
                    resolved.missing_plugins.join(", "),
                    resolved.missing_skills.join(", "),
                    resolved.missing_mcp_servers.join(", ")
                ),
                code: "mode_bundle_incomplete".into(),

                details: serde_json::json!({}),},
        ));
    }

    Ok(Some(ModeActivation {
        mode_id: resolved.bundle.id.clone(),
        prompt_prefix: build_mode_prompt(&resolved.bundle),
    }))
}

pub fn chat_mode_preference_for_cwd_session(cwd: &str, session_id: &str) -> ChatModePreference {
    if let Ok(info) = storage::load_session_info(session_id) {
        return chat_mode_preference_for_session_info(&info);
    }

    let workspace_key = storage::workspace_key(Path::new(cwd));
    let default_chat_mode = workspace_default_chat_mode(&workspace_key);
    ChatModePreference {
        workspace_key,
        effective_chat_mode: default_chat_mode.clone(),
        default_chat_mode,
        chat_mode_override: None,
    }
}

pub fn chat_mode_preference_for_session_info(info: &storage::SessionInfo) -> ChatModePreference {
    let workspace_key = info.workspace_key.clone();
    let default_chat_mode = workspace_default_chat_mode(&workspace_key);
    let chat_mode_override = normalize_optional_mode(info.chat_mode_override.as_deref());
    let effective_chat_mode = chat_mode_override
        .clone()
        .unwrap_or_else(|| default_chat_mode.clone());
    ChatModePreference {
        workspace_key,
        default_chat_mode,
        chat_mode_override,
        effective_chat_mode,
    }
}

pub fn workspace_default_chat_mode(workspace_key: &str) -> String {
    workspace_metadata::load_metadata()
        .ok()
        .and_then(|metadata| metadata.get(workspace_key).cloned())
        .and_then(|metadata| normalize_optional_mode(metadata.default_chat_mode.as_deref()))
        .unwrap_or_else(|| NORMAL_CHAT_MODE_ID.to_string())
}

pub fn normalize_mode_or_normal(mode: Option<&str>) -> String {
    normalize_optional_mode(mode).unwrap_or_else(|| NORMAL_CHAT_MODE_ID.to_string())
}

pub fn normalize_optional_mode(mode: Option<&str>) -> Option<String> {
    mode.map(normalize_mode_id)
        .and_then(|mode| (!mode.is_empty()).then_some(mode))
}

fn build_mode_prompt(bundle: &ChatModeBundle) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Chat mode '{}' is active for this turn.",
        bundle.display_name
    ));
    if !bundle.prompt.trim().is_empty() {
        lines.push(bundle.prompt.trim().to_string());
    }
    if !bundle.plugin_ids.is_empty() {
        lines.push(format!(
            "Active mode plugins: {}.",
            bundle.plugin_ids.join(", ")
        ));
    }
    if !bundle.skill_ids.is_empty() {
        lines.push(format!(
            "Prefer these mode skills when relevant: {}.",
            bundle.skill_ids.join(", ")
        ));
    }
    if !bundle.mcp_server_names.is_empty() {
        lines.push(format!(
            "Prefer tools from these MCP servers when relevant: {}.",
            bundle.mcp_server_names.join(", ")
        ));
    }
    format!("<mode>\n{}\n</mode>\n\n", lines.join("\n"))
}

fn load_mode_bundles() -> Result<Vec<ChatModeBundle>, String> {
    let settings = read_user_raw_settings()?;
    let mut bundles = built_in_bundles();
    let stored = settings
        .extra
        .get(CHAT_MODES_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<ChatModesSettings>(value).ok())
        .unwrap_or_default();

    for mut bundle in stored.bundles {
        bundle.id = normalize_mode_id(&bundle.id);
        if bundle.id.is_empty() {
            continue;
        }
        bundle.built_in = is_builtin_mode(&bundle.id);
        if let Some(existing) = bundles.iter_mut().find(|item| item.id == bundle.id) {
            *existing = merge_builtin_flags(bundle);
        } else {
            bundles.push(bundle);
        }
    }
    bundles.sort_by(|a, b| mode_sort_key(a).cmp(&mode_sort_key(b)));
    Ok(bundles)
}

fn save_mode_bundle(bundle: ChatModeBundle) -> Result<(), String> {
    let mut settings = read_user_raw_settings()?;
    let mut bundles = load_stored_bundles(&settings);
    if let Some(existing) = bundles.iter_mut().find(|item| item.id == bundle.id) {
        *existing = bundle;
    } else {
        bundles.push(bundle);
    }
    write_chat_modes(&mut settings, bundles)
}

fn delete_mode_bundle(id: &str) -> Result<(), String> {
    let mut settings = read_user_raw_settings()?;
    let mut bundles = load_stored_bundles(&settings);
    bundles.retain(|bundle| bundle.id != id);
    write_chat_modes(&mut settings, bundles)
}

fn load_stored_bundles(settings: &RawSettings) -> Vec<ChatModeBundle> {
    settings
        .extra
        .get(CHAT_MODES_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<ChatModesSettings>(value).ok())
        .unwrap_or_default()
        .bundles
        .into_iter()
        .filter_map(|mut bundle| {
            bundle.id = normalize_mode_id(&bundle.id);
            (!bundle.id.is_empty()).then_some(bundle)
        })
        .collect()
}

fn write_chat_modes(
    settings: &mut RawSettings,
    bundles: Vec<ChatModeBundle>,
) -> Result<(), String> {
    let value = serde_json::to_value(ChatModesSettings { bundles })
        .map_err(|error| format!("failed to serialize chat modes: {error}"))?;
    settings.extra.insert(CHAT_MODES_KEY.to_string(), value);
    write_settings_file(&user_settings_path(), settings)
        .map(|_| ())
        .map_err(|error| format!("failed to write settings: {error}"))
}

fn read_user_raw_settings() -> Result<RawSettings, String> {
    let path = user_settings_path();
    if !path.exists() {
        return Ok(RawSettings::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    if content.trim().is_empty() {
        return Ok(RawSettings::default());
    }
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))
}

fn built_in_bundles() -> Vec<ChatModeBundle> {
    vec![
        ChatModeBundle {
            id: "normal".into(),
            display_name: "Normal".into(),
            prompt: String::new(),
            plugin_ids: Vec::new(),
            skill_ids: Vec::new(),
            mcp_server_names: Vec::new(),
            enabled: true,
            built_in: true,
        },
        ChatModeBundle {
            id: "eco-boost".into(),
            display_name: "Eco Boost".into(),
            prompt: concat!(
                "Use concise, high-signal responses. Prefer eco MCP tools for filesystem, ",
                "git, and search work when they are available. Use blunt skills when they ",
                "match the task."
            )
            .into(),
            plugin_ids: vec!["eco-boost@local".into()],
            skill_ids: vec!["blunt".into(), "blunt-compress".into()],
            mcp_server_names: vec!["mcp-cli-bridge".into()],
            enabled: true,
            built_in: true,
        },
    ]
}

fn merge_builtin_flags(mut bundle: ChatModeBundle) -> ChatModeBundle {
    bundle.built_in = is_builtin_mode(&bundle.id);
    bundle
}

fn is_builtin_mode(id: &str) -> bool {
    matches!(id, "normal" | "eco-boost")
}

fn mode_sort_key(bundle: &ChatModeBundle) -> (u8, String) {
    let order = match bundle.id.as_str() {
        "normal" => 0,
        "eco-boost" => 1,
        _ => 2,
    };
    (order, bundle.display_name.to_lowercase())
}

fn resolve_bundle(bundle: ChatModeBundle, resources: &ResourceIndex) -> ChatModeResolved {
    let missing_plugins = missing_from(&bundle.plugin_ids, &resources.plugins);
    let missing_skills = missing_from(&bundle.skill_ids, &resources.skills);
    let missing_mcp_servers = missing_from(&bundle.mcp_server_names, &resources.mcp_servers);
    let status = if !bundle.enabled {
        "disabled"
    } else if missing_plugins.is_empty()
        && missing_skills.is_empty()
        && missing_mcp_servers.is_empty()
    {
        "ready"
    } else {
        "incomplete"
    };
    ChatModeResolved {
        bundle,
        status: status.into(),
        missing_plugins,
        missing_skills,
        missing_mcp_servers,
    }
}

fn missing_from(items: &[String], available: &BTreeSet<String>) -> Vec<String> {
    items
        .iter()
        .filter(|item| !available.contains(item.as_str()))
        .cloned()
        .collect()
}

#[derive(Default)]
struct ResourceIndex {
    plugins: BTreeSet<String>,
    skills: BTreeSet<String>,
    mcp_servers: BTreeSet<String>,
}

fn resource_index(state: &WebState) -> ResourceIndex {
    let engine = state.engine();
    let cwd = engine.cwd();
    let mut index = ResourceIndex::default();

    for plugin in get_enabled_plugins() {
        index.plugins.insert(plugin.id);
        index.plugins.insert(plugin.name);
    }
    for skill in allthecodes_skills::get_all_skills() {
        index.skills.insert(skill.name);
    }
    if let Ok(servers) = discover_mcp_servers_scoped(Path::new(&cwd)) {
        for server in servers {
            if server.error.is_none() && server.config.disabled != Some(true) {
                index.mcp_servers.insert(server.config.name);
            }
        }
    }
    index
}

fn list_resources(cwd: &Path) -> Result<ChatModeResourcesResponse, String> {
    let plugins = allthecodes_plugins::get_all_plugins()
        .into_iter()
        .map(|plugin| ModePluginResource {
            id: plugin.id,
            name: plugin.name,
            description: plugin.description,
            enabled: plugin.status == PluginStatus::Installed,
            skills: plugin.skills,
            mcp_servers: plugin.mcp_servers,
        })
        .collect();

    let skills = allthecodes_skills::get_all_skills()
        .into_iter()
        .map(|skill| {
            let id = skill.name.clone();
            let name = skill.name.clone();
            let display_name = skill.display_name().to_string();
            let source = skill_source_label(&skill.source);
            let description = skill.frontmatter.description.clone();
            let model_invocable = skill.is_model_invocable();
            ModeSkillResource {
                id,
                name,
                display_name,
                source,
                description,
                model_invocable,
            }
        })
        .collect();

    let mcp_servers = discover_mcp_servers_scoped(cwd)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|server| ModeMcpServerResource {
            name: server.config.name,
            scope: format!("{:?}", server.scope),
            transport: server.config.transport,
            disabled: server.config.disabled.unwrap_or(false) || server.error.is_some(),
        })
        .collect();

    Ok(ChatModeResourcesResponse {
        plugins,
        skills,
        mcp_servers,
    })
}

fn skill_source_label(source: &allthecodes_skills::SkillSource) -> String {
    match source {
        allthecodes_skills::SkillSource::Bundled => "bundled".into(),
        allthecodes_skills::SkillSource::User => "user".into(),
        allthecodes_skills::SkillSource::Project => "project".into(),
        allthecodes_skills::SkillSource::Plugin(id) => format!("plugin:{id}"),
        allthecodes_skills::SkillSource::Mcp(id) => format!("mcp:{id}"),
    }
}

pub fn normalize_mode_id(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(48)
        .collect()
}

fn unique_trimmed(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn default_enabled() -> bool {
    true
}

fn validation_error(message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            error: message.into(),
            code: "invalid_chat_mode".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn internal_error(message: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: message.into(),
            code: "chat_modes_error".into(),

            details: serde_json::json!({}),
        }),
    )
}

fn validation_api_error(
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, ApiError) {
    (
        StatusCode::BAD_REQUEST,
        ApiError {
            error: message.into(),
            code: code.into(),

            details: serde_json::json!({}),
        },
    )
}

fn internal_api_error(message: String) -> (StatusCode, ApiError) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiError {
            error: message,
            code: "chat_modes_error".into(),

            details: serde_json::json!({}),
        },
    )
}
