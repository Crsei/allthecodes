//! Binary adapters for the reusable plan workflow implementation.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use allthecodes_engine::lifecycle::QueryEngine;
use allthecodes_engine::types::app_state::AppState;
use allthecodes_types::plan_workflow::PlanWorkflowRecord;
use anyhow::Result;

const DEFAULT_OWNER: &str = "main";

pub fn enter_engine_plan_mode(
    engine: &QueryEngine,
    source: &str,
    description: Option<&str>,
    classifier_reason: Option<&str>,
) -> Result<PlanWorkflowRecord> {
    let cwd = PathBuf::from(engine.cwd());
    let existing = allthecodes_commands::plan_workflow::load(&cwd)?;
    let slot: Arc<Mutex<Option<PlanWorkflowRecord>>> = Arc::new(Mutex::new(None));
    let slot_for_update = Arc::clone(&slot);

    engine.update_app_state(|state| {
        let record = allthecodes_commands::plan_workflow::enter_plan_mode_state(
            state,
            &cwd,
            existing,
            DEFAULT_OWNER,
            source,
            description,
            classifier_reason,
        );
        *slot_for_update.lock().expect("plan workflow slot poisoned") = Some(record);
    });

    let record = slot
        .lock()
        .expect("plan workflow slot poisoned")
        .clone()
        .expect("plan workflow update should set record");
    allthecodes_commands::plan_workflow::persist(&cwd, &record)?;
    Ok(record)
}

pub fn reject_engine_plan(
    engine: &QueryEngine,
    source: &str,
    feedback: Option<String>,
) -> Result<PlanWorkflowRecord> {
    let cwd = PathBuf::from(engine.cwd());
    let existing = allthecodes_commands::plan_workflow::load(&cwd)?;
    let slot: Arc<Mutex<Option<PlanWorkflowRecord>>> = Arc::new(Mutex::new(None));
    let slot_for_update = Arc::clone(&slot);

    engine.update_app_state(|state| {
        let record = allthecodes_commands::plan_workflow::reject_approval_state(
            state,
            &cwd,
            existing,
            DEFAULT_OWNER,
            source,
            feedback,
        );
        *slot_for_update.lock().expect("plan workflow slot poisoned") = Some(record);
    });

    let record = slot
        .lock()
        .expect("plan workflow slot poisoned")
        .clone()
        .expect("plan workflow update should set record");
    allthecodes_commands::plan_workflow::persist(&cwd, &record)?;
    Ok(record)
}

pub fn sync_command_app_state(engine: &QueryEngine, command_state: &AppState) {
    let command_state = command_state.to_tool_app_state();
    engine.update_app_state(|state| {
        state.apply_tool_app_state(command_state);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::config::QueryEngineConfig;

    fn make_engine() -> QueryEngine {
        QueryEngine::new(QueryEngineConfig {
            cwd: env!("CARGO_MANIFEST_DIR").to_string(),
            tools: vec![],
            custom_system_prompt: None,
            append_system_prompt: None,
            user_specified_model: None,
            fallback_model: None,
            max_turns: None,
            max_budget_usd: None,
            task_budget: None,
            verbose: false,
            initial_messages: None,
            commands: vec![],
            thinking_config: None,
            json_schema: None,
            replay_user_messages: false,
            persist_session: false,
            resolved_model: Some(allthecodes_types::models::default_model_id()),
            auto_save_session: false,
            agent_context: None,
        })
    }

    #[test]
    fn sync_command_app_state_preserves_runtime_settings_changes() {
        let engine = make_engine();
        engine.update_app_state(|state| {
            state.settings.hermes_enabled = Some(false);
        });
        let mut command_state = engine.app_state();
        command_state.settings.hermes_enabled = Some(true);
        command_state.settings.sources.insert(
            "hermesEnabled".to_string(),
            allthecodes_config::settings::SettingsSource::Project,
        );

        sync_command_app_state(&engine, &command_state);

        let synced = engine.app_state();
        assert_eq!(synced.settings.hermes_enabled, Some(true));
        assert_eq!(
            synced.settings.sources.get("hermesEnabled"),
            Some(&allthecodes_config::settings::SettingsSource::Project)
        );
    }
}
