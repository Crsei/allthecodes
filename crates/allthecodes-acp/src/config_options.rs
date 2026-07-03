//! ACP session configuration options.
//!
//! Builds config options from each session's AppState: model, mode, thought level.

use std::sync::Arc;

use agent_client_protocol_schema::v2::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigValueId,
};

use crate::session::AcpSession;

/// A session config option with its current value.
pub struct ConfigOptionEntry {
    pub config_option: SessionConfigOption,
    pub current_value: SessionConfigValueId,
}

/// Build the full set of config options for a session.
pub fn build_config_options(session: &Arc<AcpSession>) -> Vec<ConfigOptionEntry> {
    let app_state = session.engine.app_state();
    let mut entries = Vec::new();

    // Model option
    {
        let current_model: String = app_state.main_loop_model.clone();

        entries.push(ConfigOptionEntry {
            current_value: SessionConfigValueId::new(current_model.clone()),
            config_option: SessionConfigOption::select(
                "model",
                "Model",
                SessionConfigValueId::new(current_model),
                Vec::<SessionConfigSelectOption>::new(),
            )
            .category(Some(SessionConfigOptionCategory::Model)),
        });
    }

    // Thought level option
    {
        let current_effort: String = app_state
            .effort_value
            .clone()
            .unwrap_or_else(|| "medium".to_string());

        let options: Vec<SessionConfigSelectOption> = vec![
            SessionConfigSelectOption::new("low", "Low"),
            SessionConfigSelectOption::new("medium", "Medium"),
            SessionConfigSelectOption::new("high", "High"),
        ];

        entries.push(ConfigOptionEntry {
            current_value: SessionConfigValueId::new(current_effort.clone()),
            config_option: SessionConfigOption::select(
                "thought_level",
                "Thought Level",
                SessionConfigValueId::new(current_effort),
                options,
            )
            .category(Some(SessionConfigOptionCategory::ThoughtLevel)),
        });
    }

    // Mode option
    {
        let current_mode: String = app_state.tool_permission_context.mode.as_str().to_string();

        let options: Vec<SessionConfigSelectOption> = vec![
            SessionConfigSelectOption::new("default", "Default"),
            SessionConfigSelectOption::new("auto", "Auto"),
            SessionConfigSelectOption::new("bypass", "Bypass"),
        ];

        entries.push(ConfigOptionEntry {
            current_value: SessionConfigValueId::new(current_mode.clone()),
            config_option: SessionConfigOption::select(
                "mode",
                "Mode",
                SessionConfigValueId::new(current_mode),
                options,
            )
            .category(Some(SessionConfigOptionCategory::Mode)),
        });
    }

    entries
}
