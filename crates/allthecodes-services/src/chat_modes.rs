//! Project-scoped chat mode storage, resource resolution, and prompt assembly.

use std::collections::BTreeSet;
use std::path::Path;

use allthecodes_config::settings::{
    project_settings_path, user_settings_path, write_settings_file, RawSettings,
};
use allthecodes_mcp::discovery::discover_mcp_servers_scoped;
use allthecodes_plugins::installation::{install_plugin, InstallError, InstallScope};
use allthecodes_plugins::{PluginEntry, PluginSource, PluginStatus};
use allthecodes_session::storage;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

const CHAT_MODES_KEY: &str = "chatModes";
pub const NORMAL_CHAT_MODE_ID: &str = "normal";

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ChatModeError {
    #[error("{message}")]
    BadRequest { code: &'static str, message: String },
    #[error("{0}")]
    Internal(String),
}

impl ChatModeError {
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::BadRequest {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest { code, .. } => code,
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::BadRequest { message, .. } => message.clone(),
            Self::Internal(message) => message.clone(),
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModesSettings {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub bundles: Vec<ChatModeBundle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResolved {
    pub bundle: ChatModeBundle,
    pub status: String,
    pub missing_plugins: Vec<String>,
    pub missing_skills: Vec<String>,
    pub missing_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModesResponse {
    pub modes: Vec<ChatModeResolved>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModePreference {
    pub workspace_key: String,
    pub default_chat_mode: String,
    pub chat_mode_override: Option<String>,
    pub effective_chat_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatModeResourcesResponse {
    pub plugins: Vec<ModePluginResource>,
    pub skills: Vec<ModeSkillResource>,
    pub mcp_servers: Vec<ModeMcpServerResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModePluginResource {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModeSkillResource {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub source: String,
    pub description: String,
    pub model_invocable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModeMcpServerResource {
    pub name: String,
    pub scope: String,
    pub transport: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeActivation {
    pub mode_id: String,
    pub system_prompt_append: String,
}

pub fn list_modes(cwd: &Path) -> Result<ChatModesResponse, ChatModeError> {
    let resources = resource_index(cwd);
    let modes = load_mode_bundles(cwd)?
        .into_iter()
        .map(|bundle| resolve_bundle(bundle, &resources))
        .collect();
    Ok(ChatModesResponse { modes })
}

pub fn load_mode_bundles(cwd: &Path) -> Result<Vec<ChatModeBundle>, ChatModeError> {
    let mut bundles = built_in_bundles();
    for bundle in load_user_legacy_chat_modes()?.bundles {
        merge_bundle(&mut bundles, bundle);
    }
    for bundle in load_project_chat_modes(cwd)?.bundles {
        merge_bundle(&mut bundles, bundle);
    }
    bundles.sort_by_key(mode_sort_key);
    Ok(bundles)
}

pub fn project_default_chat_mode(cwd: &Path) -> String {
    load_project_chat_modes(cwd)
        .ok()
        .and_then(|settings| normalize_optional_mode(settings.default.as_deref()))
        .unwrap_or_else(|| NORMAL_CHAT_MODE_ID.to_string())
}

pub fn chat_mode_preference_for_cwd_session(cwd: &str, session_id: &str) -> ChatModePreference {
    if let Ok(info) = storage::load_session_info(session_id) {
        return chat_mode_preference_for_session_info(&info);
    }

    let cwd_path = Path::new(cwd);
    let workspace_key = storage::workspace_key(cwd_path);
    let default_chat_mode = project_default_chat_mode(cwd_path);
    ChatModePreference {
        workspace_key,
        effective_chat_mode: default_chat_mode.clone(),
        default_chat_mode,
        chat_mode_override: None,
    }
}

pub fn chat_mode_preference_for_session_info(info: &storage::SessionInfo) -> ChatModePreference {
    let default_chat_mode = project_default_chat_mode(Path::new(&info.cwd));
    let chat_mode_override = normalize_optional_mode(info.chat_mode_override.as_deref());
    let effective_chat_mode = chat_mode_override
        .clone()
        .unwrap_or_else(|| default_chat_mode.clone());
    ChatModePreference {
        workspace_key: info.workspace_key.clone(),
        default_chat_mode,
        chat_mode_override,
        effective_chat_mode,
    }
}

pub async fn upsert_mode(
    cwd: &Path,
    bundle: ChatModeBundle,
    app_version: Option<&str>,
) -> Result<ChatModeBundle, ChatModeError> {
    let bundle = normalize_bundle(bundle, None)?;
    if bundle.enabled {
        setup_enabled_mode(cwd, &bundle, app_version).await?;
    }
    save_mode_bundle(cwd, bundle.clone())?;
    Ok(bundle)
}

pub async fn enable_mode(
    cwd: &Path,
    id: &str,
    app_version: Option<&str>,
) -> Result<ChatModeBundle, ChatModeError> {
    let id = normalize_mode_id(id);
    if id.is_empty() {
        return Err(ChatModeError::bad_request(
            "invalid_chat_mode",
            "mode id is required",
        ));
    }
    let mut bundle = find_mode_bundle(cwd, &id)?;
    bundle.enabled = true;
    setup_enabled_mode(cwd, &bundle, app_version).await?;
    save_mode_bundle(cwd, bundle.clone())?;
    Ok(bundle)
}

pub fn disable_mode(cwd: &Path, id: &str) -> Result<ChatModeBundle, ChatModeError> {
    let id = normalize_mode_id(id);
    if id.is_empty() {
        return Err(ChatModeError::bad_request(
            "invalid_chat_mode",
            "mode id is required",
        ));
    }
    let mut bundle = find_mode_bundle(cwd, &id)?;
    bundle.enabled = false;
    save_mode_bundle(cwd, bundle.clone())?;
    disable_unshared_mode_plugins(cwd, &bundle)?;
    Ok(bundle)
}

pub fn delete_mode(cwd: &Path, id: &str) -> Result<(), ChatModeError> {
    let id = normalize_mode_id(id);
    if is_builtin_mode(&id) {
        return Err(ChatModeError::bad_request(
            "invalid_chat_mode",
            "built-in modes cannot be deleted",
        ));
    }
    let mut bundle = find_mode_bundle(cwd, &id)?;
    bundle.enabled = false;
    save_mode_bundle(cwd, bundle.clone())?;
    disable_unshared_mode_plugins(cwd, &bundle)
}

pub fn set_default_mode(cwd: &Path, id: &str) -> Result<String, ChatModeError> {
    let id = normalize_mode_or_normal(Some(id));
    if id != NORMAL_CHAT_MODE_ID {
        let bundle = find_mode_bundle(cwd, &id)?;
        let resolved = resolve_bundle(bundle, &resource_index(cwd));
        ensure_ready(&resolved)?;
    }
    write_project_chat_modes(cwd, |settings| {
        settings.default = (id != NORMAL_CHAT_MODE_ID).then_some(id.clone());
    })?;
    Ok(id)
}

pub fn clear_default_mode(cwd: &Path) -> Result<(), ChatModeError> {
    write_project_chat_modes(cwd, |settings| {
        settings.default = None;
    })
}

pub fn resolve_mode_activation(
    cwd: &Path,
    mode: Option<&str>,
) -> Result<Option<ModeActivation>, ChatModeError> {
    let mode_id = normalize_mode_or_normal(mode);
    if mode_id == NORMAL_CHAT_MODE_ID {
        return Ok(None);
    }

    let bundle = find_mode_bundle(cwd, &mode_id)?;
    let resolved = resolve_bundle(bundle, &resource_index(cwd));
    ensure_ready(&resolved)?;

    Ok(Some(ModeActivation {
        mode_id: resolved.bundle.id.clone(),
        system_prompt_append: build_mode_prompt(&resolved.bundle),
    }))
}

pub fn build_mode_prompt(bundle: &ChatModeBundle) -> String {
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
    format!("<mode>\n{}\n</mode>", lines.join("\n"))
}

pub fn list_resources(cwd: &Path) -> Result<ChatModeResourcesResponse, ChatModeError> {
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
        .map_err(|error| ChatModeError::Internal(error.to_string()))?
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

pub fn normalize_mode_or_normal(mode: Option<&str>) -> String {
    normalize_optional_mode(mode).unwrap_or_else(|| NORMAL_CHAT_MODE_ID.to_string())
}

pub fn normalize_optional_mode(mode: Option<&str>) -> Option<String> {
    mode.map(normalize_mode_id)
        .and_then(|mode| (!mode.is_empty()).then_some(mode))
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

fn normalize_bundle(
    mut bundle: ChatModeBundle,
    expected_id: Option<&str>,
) -> Result<ChatModeBundle, ChatModeError> {
    let normalized_id = expected_id
        .map(normalize_mode_id)
        .unwrap_or_else(|| normalize_mode_id(&bundle.id));
    if normalized_id.is_empty() {
        return Err(ChatModeError::bad_request(
            "invalid_chat_mode",
            "mode id is required",
        ));
    }
    if normalize_mode_id(&bundle.id) != normalized_id {
        return Err(ChatModeError::bad_request(
            "invalid_chat_mode",
            "mode id does not match path",
        ));
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
    Ok(bundle)
}

fn find_mode_bundle(cwd: &Path, id: &str) -> Result<ChatModeBundle, ChatModeError> {
    load_mode_bundles(cwd)?
        .into_iter()
        .find(|bundle| bundle.id == id)
        .ok_or_else(|| {
            ChatModeError::bad_request(
                "mode_not_found",
                format!("Chat mode '{}' is not configured", id),
            )
        })
}

async fn setup_enabled_mode(
    cwd: &Path,
    bundle: &ChatModeBundle,
    app_version: Option<&str>,
) -> Result<(), ChatModeError> {
    for plugin_id in &bundle.plugin_ids {
        install_or_enable_plugin(plugin_id, app_version).await?;
    }
    refresh_plugin_contributed_skills(cwd, app_version);
    let resolved = resolve_bundle(bundle.clone(), &resource_index(cwd));
    ensure_ready(&resolved)
}

fn ensure_ready(resolved: &ChatModeResolved) -> Result<(), ChatModeError> {
    if !resolved.bundle.enabled {
        return Err(ChatModeError::bad_request(
            "mode_disabled",
            format!("Chat mode '{}' is disabled", resolved.bundle.id),
        ));
    }
    if resolved.status == "ready" {
        return Ok(());
    }
    Err(ChatModeError::bad_request(
        "mode_bundle_incomplete",
        format!(
            "Chat mode '{}' is incomplete: missing plugins [{}], skills [{}], MCP servers [{}]",
            resolved.bundle.id,
            resolved.missing_plugins.join(", "),
            resolved.missing_skills.join(", "),
            resolved.missing_mcp_servers.join(", ")
        ),
    ))
}

async fn install_or_enable_plugin(
    plugin_ref: &str,
    app_version: Option<&str>,
) -> Result<(), ChatModeError> {
    if set_installed_plugin_status_by_ref(plugin_ref, PluginStatus::Installed)?.is_some() {
        return Ok(());
    }

    let available_plugins = std::collections::HashMap::new();
    let manifests = std::collections::HashMap::new();
    match install_plugin(
        plugin_ref,
        Some(InstallScope::Project),
        app_version,
        None,
        &available_plugins,
        &manifests,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(InstallError::AlreadyInstalled(_)) => {
            set_installed_plugin_status_by_ref(plugin_ref, PluginStatus::Installed)?;
            Ok(())
        }
        Err(error) => Err(ChatModeError::bad_request(
            "mode_plugin_setup_failed",
            format!("Failed to install plugin '{}': {}", plugin_ref, error),
        )),
    }
}

fn disable_unshared_mode_plugins(
    cwd: &Path,
    disabled: &ChatModeBundle,
) -> Result<(), ChatModeError> {
    if disabled.plugin_ids.is_empty() {
        return Ok(());
    }
    let enabled_modes = load_mode_bundles(cwd)?
        .into_iter()
        .filter(|bundle| bundle.enabled && bundle.id != disabled.id)
        .collect::<Vec<_>>();

    for plugin_ref in &disabled.plugin_ids {
        let still_used = enabled_modes.iter().any(|bundle| {
            bundle
                .plugin_ids
                .iter()
                .any(|other| plugin_refs_equal(other, plugin_ref))
        });
        if !still_used {
            let _ = set_installed_plugin_status_by_ref(plugin_ref, PluginStatus::Disabled)?;
        }
    }
    Ok(())
}

fn set_installed_plugin_status_by_ref(
    plugin_ref: &str,
    status: PluginStatus,
) -> Result<Option<PluginEntry>, ChatModeError> {
    let mut installed = allthecodes_plugins::loader::load_installed_plugins();
    let Some(plugin) = installed
        .iter_mut()
        .find(|plugin| installed_plugin_matches(plugin, plugin_ref))
    else {
        return Ok(None);
    };
    plugin.status = status.clone();
    let updated = plugin.clone();
    allthecodes_plugins::loader::save_installed_plugins(&installed)
        .map_err(|error| ChatModeError::Internal(error.to_string()))?;
    if allthecodes_plugins::set_plugin_status(&updated.id, status).is_none() {
        allthecodes_plugins::register_plugin(updated.clone());
    }
    Ok(Some(updated))
}

fn installed_plugin_matches(plugin: &PluginEntry, plugin_ref: &str) -> bool {
    plugin_refs_equal(&plugin.id, plugin_ref)
        || plugin
            .id
            .split_once('@')
            .is_some_and(|(name, _)| plugin_refs_equal(name, plugin_ref))
        || plugin_refs_equal(&plugin.name, plugin_ref)
        || matches!(&plugin.source, PluginSource::Marketplace { id, .. } if plugin_refs_equal(id, plugin_ref))
}

fn plugin_refs_equal(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

pub fn refresh_plugin_contributed_skills(cwd: &Path, app_version: Option<&str>) {
    let report = allthecodes_skills::reload_skills_with_extra(
        &allthecodes_config::paths::skills_dir_global(),
        Some(cwd),
        load_plugin_contributed_skills(),
        allthecodes_skills::SkillLoadOptions::for_app_version(app_version.unwrap_or("")),
    );

    if report.error_count() > 0 || report.warning_count() > 0 {
        warn!(
            loaded = report.loaded,
            skipped = report.skipped,
            revision = report.revision,
            warnings = report.warning_count(),
            errors = report.error_count(),
            "Plugin: refreshed contributed skills with diagnostics"
        );
    }
}

fn load_plugin_contributed_skills() -> Vec<allthecodes_skills::SkillDefinition> {
    let mut out = Vec::new();
    for contributed in allthecodes_plugins::discover_plugin_skill_definitions() {
        let source = allthecodes_skills::SkillSource::Plugin(contributed.plugin_id.clone());
        let mut skill = match allthecodes_skills::loader::load_skill_from_file_path(
            &contributed.path,
            source,
        ) {
            Some(skill) => skill,
            None => {
                warn!(
                    plugin = %contributed.plugin_id,
                    path = %contributed.path.display(),
                    "Plugin: failed to load contributed skill file"
                );
                continue;
            }
        };

        skill.name = contributed.name;
        if let Some(desc) = contributed.description {
            if !desc.trim().is_empty() {
                skill.frontmatter.description = desc;
            }
        }
        out.push(skill);
    }
    out
}

fn save_mode_bundle(cwd: &Path, bundle: ChatModeBundle) -> Result<(), ChatModeError> {
    write_project_chat_modes(cwd, |settings| {
        upsert_bundle(&mut settings.bundles, bundle);
    })
}

fn write_project_chat_modes(
    cwd: &Path,
    mutate: impl FnOnce(&mut ChatModesSettings),
) -> Result<(), ChatModeError> {
    let path = project_settings_path(cwd);
    let mut raw = read_raw_settings(&path)?;
    let mut settings = raw_chat_modes(&raw);
    mutate(&mut settings);
    normalize_stored_settings(&mut settings);
    let value = serde_json::to_value(settings).map_err(|error| {
        ChatModeError::Internal(format!("failed to serialize chat modes: {error}"))
    })?;
    raw.extra.insert(CHAT_MODES_KEY.to_string(), value);
    write_settings_file(&path, &raw)
        .map_err(|error| ChatModeError::Internal(format!("failed to write settings: {error}")))
}

fn load_project_chat_modes(cwd: &Path) -> Result<ChatModesSettings, ChatModeError> {
    let raw = read_raw_settings(&project_settings_path(cwd))?;
    let mut settings = raw_chat_modes(&raw);
    normalize_stored_settings(&mut settings);
    Ok(settings)
}

fn load_user_legacy_chat_modes() -> Result<ChatModesSettings, ChatModeError> {
    let raw = read_raw_settings(&user_settings_path())?;
    let mut settings = raw_chat_modes(&raw);
    normalize_stored_settings(&mut settings);
    Ok(settings)
}

fn raw_chat_modes(raw: &RawSettings) -> ChatModesSettings {
    raw.extra
        .get(CHAT_MODES_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<ChatModesSettings>(value).ok())
        .unwrap_or_default()
}

fn read_raw_settings(path: &Path) -> Result<RawSettings, ChatModeError> {
    if !path.exists() {
        return Ok(RawSettings::default());
    }
    let content = std::fs::read_to_string(path).map_err(|error| {
        ChatModeError::Internal(format!("failed to read {}: {}", path.display(), error))
    })?;
    if content.trim().is_empty() {
        return Ok(RawSettings::default());
    }
    serde_json::from_str(&content).map_err(|error| {
        ChatModeError::Internal(format!("failed to parse {}: {}", path.display(), error))
    })
}

fn normalize_stored_settings(settings: &mut ChatModesSettings) {
    settings.default = normalize_optional_mode(settings.default.as_deref());
    let mut bundles = Vec::new();
    for bundle in std::mem::take(&mut settings.bundles) {
        if let Ok(bundle) = normalize_bundle(bundle, None) {
            upsert_bundle(&mut bundles, bundle);
        }
    }
    settings.bundles = bundles;
}

fn merge_bundle(bundles: &mut Vec<ChatModeBundle>, bundle: ChatModeBundle) {
    if let Ok(bundle) = normalize_bundle(bundle, None) {
        upsert_bundle(bundles, bundle);
    }
}

fn upsert_bundle(bundles: &mut Vec<ChatModeBundle>, mut bundle: ChatModeBundle) {
    bundle.built_in = is_builtin_mode(&bundle.id);
    if let Some(existing) = bundles.iter_mut().find(|item| item.id == bundle.id) {
        *existing = bundle;
    } else {
        bundles.push(bundle);
    }
}

pub fn built_in_bundles() -> Vec<ChatModeBundle> {
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
            plugin_ids: vec!["eco-boost".into()],
            skill_ids: vec!["blunt".into(), "blunt-compress".into()],
            mcp_server_names: vec!["mcp-cli-bridge".into()],
            enabled: true,
            built_in: true,
        },
        ChatModeBundle {
            id: "orchestrator".into(),
            display_name: "Orchestrator".into(),
            prompt: concat!(
                "You are the Orchestrator. Coordinate work through the ",
                "allthecodes-bridge shared projects, tasks, agents, and knowledge state. ",
                "Use the local allthecodes-bridge-cli plugin and the allthecodes-bridge ",
                "MCP server when bridge coordination is relevant. You may delegate work ",
                "to Claude Code- and Codex-capable agents through the bridge/plugin, ",
                "and when the user asks to open, start, launch, or create workbench ",
                "Claude Code/Codex agents or terminals, call launch_workbench_agents ",
                "instead of only creating shared agent records. Then inspect or summarize ",
                "terminal and tool results before declaring the work complete."
            )
            .into(),
            plugin_ids: vec!["allthecodes-bridge-cli@local".into()],
            skill_ids: Vec::new(),
            mcp_server_names: vec!["allthecodes-bridge".into()],
            enabled: true,
            built_in: true,
        },
    ]
}

fn is_builtin_mode(id: &str) -> bool {
    matches!(id, "normal" | "eco-boost" | "orchestrator")
}

fn mode_sort_key(bundle: &ChatModeBundle) -> (u8, String) {
    let order = match bundle.id.as_str() {
        "normal" => 0,
        "eco-boost" => 1,
        "orchestrator" => 2,
        _ => 3,
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

fn resource_index(cwd: &Path) -> ResourceIndex {
    let mut index = ResourceIndex::default();

    for plugin in allthecodes_plugins::get_enabled_plugins() {
        let plugin_id = plugin.id;
        insert_plugin_compat_aliases(&mut index.plugins, &plugin_id);
        index.plugins.insert(plugin_id);
        index.plugins.insert(plugin.name);
    }
    for skill in allthecodes_skills::get_all_skills() {
        index.skills.insert(skill.name);
    }
    if let Ok(servers) = discover_mcp_servers_scoped(cwd) {
        for server in servers {
            if server.error.is_none() && server.config.disabled != Some(true) {
                index.mcp_servers.insert(server.config.name);
            }
        }
    }
    index
}

fn insert_plugin_compat_aliases(plugins: &mut BTreeSet<String>, id: &str) {
    match id {
        "eco-boost-linux-release" => {
            plugins.insert("eco-boost".to_string());
        }
        "allthecodes-bridge-cli-linux-release" => {
            plugins.insert("allthecodes-bridge-cli@local".to_string());
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use serial_test::serial;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
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

    fn make_cwd() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".allthecodes")).unwrap();
        dir
    }

    #[test]
    fn build_mode_prompt_includes_mode_resources() {
        let prompt = build_mode_prompt(&ChatModeBundle {
            id: "review".into(),
            display_name: "Review".into(),
            prompt: "Audit changes.".into(),
            plugin_ids: vec!["p".into()],
            skill_ids: vec!["s".into()],
            mcp_server_names: vec!["m".into()],
            enabled: true,
            built_in: false,
        });

        assert!(prompt.starts_with("<mode>"));
        assert!(prompt.contains("Chat mode 'Review' is active"));
        assert!(prompt.contains("Audit changes."));
        assert!(prompt.contains("Active mode plugins: p."));
        assert!(prompt.contains("Prefer these mode skills when relevant: s."));
        assert!(prompt.contains("Prefer tools from these MCP servers when relevant: m."));
        assert!(prompt.ends_with("</mode>"));
    }

    #[test]
    #[serial]
    fn project_scope_overrides_user_legacy_bundles() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let cwd = make_cwd();
        let user_settings = RawSettings {
            extra: std::collections::HashMap::from([(
                CHAT_MODES_KEY.to_string(),
                serde_json::json!({
                    "bundles": [{
                        "id": "focus",
                        "displayName": "User Focus",
                        "prompt": "user",
                        "enabled": true
                    }]
                }),
            )]),
            ..Default::default()
        };
        write_settings_file(&user_settings_path(), &user_settings).unwrap();
        let project_settings = RawSettings {
            extra: std::collections::HashMap::from([(
                CHAT_MODES_KEY.to_string(),
                serde_json::json!({
                    "default": "focus",
                    "bundles": [{
                        "id": "focus",
                        "displayName": "Project Focus",
                        "prompt": "project",
                        "enabled": false
                    }]
                }),
            )]),
            ..Default::default()
        };
        write_settings_file(&project_settings_path(cwd.path()), &project_settings).unwrap();

        let bundles = load_mode_bundles(cwd.path()).unwrap();
        let focus = bundles.iter().find(|bundle| bundle.id == "focus").unwrap();

        assert_eq!(focus.display_name, "Project Focus");
        assert!(!focus.enabled);
        assert_eq!(project_default_chat_mode(cwd.path()), "focus");
    }

    #[test]
    fn normalize_mode_id_is_stable() {
        assert_eq!(normalize_mode_id(" Eco Boost!! "), "eco-boost");
        assert_eq!(normalize_mode_id("Mixed_Mode 42"), "mixed_mode-42");
    }

    #[test]
    fn builtin_modes_have_code_owned_builtin_flag() {
        let mut bundle = built_in_bundles()
            .into_iter()
            .find(|bundle| bundle.id == "eco-boost")
            .unwrap();
        bundle.built_in = false;
        let mut bundles = Vec::new();
        upsert_bundle(&mut bundles, bundle);
        assert!(bundles[0].built_in);
    }

    #[test]
    fn raw_chat_modes_ignores_invalid_shape() {
        let raw = RawSettings {
            extra: std::collections::HashMap::from([(
                CHAT_MODES_KEY.to_string(),
                Value::Bool(true),
            )]),
            ..Default::default()
        };

        assert!(raw_chat_modes(&raw).bundles.is_empty());
    }
}
