//! ACP session configuration options.
//!
//! Builds and applies session-scoped config options from each session's
//! AppState: model, mode, and thought level.

use std::sync::Arc;

use agent_client_protocol_schema::v2::{
    self, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigValueId,
};
use allthecodes_engine::types::config::SubmitMessageOverrides;
use allthecodes_types::permissions::PermissionMode;

use crate::session::AcpSession;

const CONFIG_MODEL: &str = "model";
const CONFIG_THOUGHT_LEVEL: &str = "thought_level";
const CONFIG_MODE: &str = "mode";

const DEFAULT_THOUGHT_LEVEL: &str = "medium";
const FALLBACK_THOUGHT_LEVELS: &[&str] = &["low", "medium", "high"];

const MODE_OPTIONS: &[(&str, &str)] = &[
    ("default", "Default"),
    ("auto", "Auto"),
    ("bypass", "Bypass"),
    ("plan", "Plan"),
    ("acceptEdits", "Accept Edits"),
    ("dontAsk", "Dont Ask"),
];

/// A session config option with its current value.
pub struct ConfigOptionEntry {
    pub config_option: SessionConfigOption,
    pub current_value: SessionConfigValueId,
}

/// Snapshot of the ACP-facing session configuration state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfigState {
    pub model: String,
    pub available_models: Vec<String>,
    pub thought_level: String,
    pub thought_levels: Vec<String>,
    pub mode: PermissionMode,
}

impl SessionConfigState {
    /// Build a config snapshot from a live session.
    pub fn from_session(session: &Arc<AcpSession>) -> Self {
        let app_state = session.engine.app_state();
        let model = app_state.main_loop_model.clone();
        let available_models =
            available_models_for_state(&model, app_state.settings.available_models.clone());
        let thought_levels = thought_levels_for_model(&app_state);
        let thought_level = app_state
            .effort_value
            .clone()
            .filter(|value| thought_levels.iter().any(|level| level == value))
            .unwrap_or_else(|| DEFAULT_THOUGHT_LEVEL.to_string());

        Self {
            model,
            available_models,
            thought_level,
            thought_levels,
            mode: app_state.tool_permission_context.mode.clone(),
        }
    }

    /// Convert the snapshot into ACP config options.
    pub fn entries(&self) -> Vec<ConfigOptionEntry> {
        vec![
            self.model_entry(),
            self.thought_level_entry(),
            self.mode_entry(),
        ]
    }

    fn model_entry(&self) -> ConfigOptionEntry {
        let options = self
            .available_models
            .iter()
            .map(|model| SessionConfigSelectOption::new(model.clone(), model.clone()))
            .collect::<Vec<_>>();

        ConfigOptionEntry {
            current_value: SessionConfigValueId::new(self.model.clone()),
            config_option: SessionConfigOption::select(
                CONFIG_MODEL,
                "Model",
                SessionConfigValueId::new(self.model.clone()),
                options,
            )
            .category(Some(SessionConfigOptionCategory::Model)),
        }
    }

    fn thought_level_entry(&self) -> ConfigOptionEntry {
        let options = self
            .thought_levels
            .iter()
            .map(|level| SessionConfigSelectOption::new(level.clone(), label_for_effort(level)))
            .collect::<Vec<_>>();

        ConfigOptionEntry {
            current_value: SessionConfigValueId::new(self.thought_level.clone()),
            config_option: SessionConfigOption::select(
                CONFIG_THOUGHT_LEVEL,
                "Thought Level",
                SessionConfigValueId::new(self.thought_level.clone()),
                options,
            )
            .category(Some(SessionConfigOptionCategory::ThoughtLevel)),
        }
    }

    fn mode_entry(&self) -> ConfigOptionEntry {
        let current_mode = self.mode.as_str().to_string();
        let options = MODE_OPTIONS
            .iter()
            .map(|(value, label)| SessionConfigSelectOption::new(*value, *label))
            .collect::<Vec<_>>();

        ConfigOptionEntry {
            current_value: SessionConfigValueId::new(current_mode.clone()),
            config_option: SessionConfigOption::select(
                CONFIG_MODE,
                "Mode",
                SessionConfigValueId::new(current_mode),
                options,
            )
            .category(Some(SessionConfigOptionCategory::Mode)),
        }
    }

    fn contains_value(&self, config_id: &str, value: &str) -> bool {
        match config_id {
            CONFIG_MODEL => self.available_models.iter().any(|model| model == value),
            CONFIG_THOUGHT_LEVEL => self.thought_levels.iter().any(|level| level == value),
            CONFIG_MODE => MODE_OPTIONS.iter().any(|(mode, _)| *mode == value),
            _ => false,
        }
    }
}

/// Build the full set of config options for a session.
pub fn build_config_options(session: &Arc<AcpSession>) -> Vec<ConfigOptionEntry> {
    SessionConfigState::from_session(session).entries()
}

/// Build only the protocol config option payloads for a session.
pub fn build_session_config_options(session: &Arc<AcpSession>) -> Vec<SessionConfigOption> {
    build_config_options(session)
        .into_iter()
        .map(|entry| entry.config_option)
        .collect()
}

/// Apply and validate a `session/set_config_option` request.
pub fn apply_config_option(
    session: &Arc<AcpSession>,
    config_id: &str,
    value: &str,
) -> Result<Vec<SessionConfigOption>, v2::Error> {
    let state = SessionConfigState::from_session(session);
    if !matches!(config_id, CONFIG_MODEL | CONFIG_THOUGHT_LEVEL | CONFIG_MODE) {
        return Err(v2::Error::invalid_params().data(format!("unknown config option: {config_id}")));
    }
    if !state.contains_value(config_id, value) {
        return Err(v2::Error::invalid_params().data(format!("invalid {config_id} value: {value}")));
    }

    let parsed_mode = if config_id == CONFIG_MODE {
        Some(
            PermissionMode::parse_configured(Some(value)).map_err(|error| {
                v2::Error::invalid_params().data(format!("invalid mode value: {error}"))
            })?,
        )
    } else {
        None
    };
    let value = value.to_string();
    session.engine.update_app_state(|state| match config_id {
        CONFIG_MODEL => {
            state.main_loop_model = value.clone();
        }
        CONFIG_THOUGHT_LEVEL => {
            state.effort_value = Some(value.clone());
        }
        CONFIG_MODE => {
            if let Some(mode) = parsed_mode {
                state.tool_permission_context.mode = mode;
            }
        }
        _ => unreachable!("config_id was validated above"),
    });

    Ok(build_session_config_options(session))
}

/// Build submit overrides from the current session config state.
pub fn submit_overrides_for_session(session: &Arc<AcpSession>) -> SubmitMessageOverrides {
    let app_state = session.engine.app_state();
    SubmitMessageOverrides {
        model: non_empty(app_state.main_loop_model),
        effort: app_state.effort_value.and_then(non_empty),
        ..SubmitMessageOverrides::default()
    }
}

fn available_models_for_state(current_model: &str, configured: Vec<String>) -> Vec<String> {
    let mut models = configured
        .into_iter()
        .filter(|model| !model.trim().is_empty())
        .collect::<Vec<_>>();
    if !current_model.trim().is_empty() && !models.iter().any(|model| model == current_model) {
        models.insert(0, current_model.to_string());
    }
    models.dedup();
    models
}

fn thought_levels_for_model(
    app_state: &allthecodes_engine::types::app_state::AppState,
) -> Vec<String> {
    let from_capabilities = app_state
        .settings
        .model_capabilities
        .get(&app_state.main_loop_model)
        .map(|capability| capability.supported_reasoning_levels.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|level| !level.trim().is_empty())
        .collect::<Vec<_>>();

    if from_capabilities.is_empty() {
        FALLBACK_THOUGHT_LEVELS
            .iter()
            .map(|level| (*level).to_string())
            .collect()
    } else {
        from_capabilities
    }
}

fn label_for_effort(value: &str) -> String {
    match value {
        "none" => "None".to_string(),
        "minimal" => "Minimal".to_string(),
        "low" => "Low".to_string(),
        "medium" => "Medium".to_string(),
        "high" => "High".to_string(),
        "xhigh" => "Extra High".to_string(),
        other => other.to_string(),
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}
