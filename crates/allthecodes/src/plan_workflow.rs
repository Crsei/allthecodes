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
    let permission_context = command_state.tool_permission_context.clone();
    let team_context = command_state.team_context.clone();
    let plan_workflow = command_state.plan_workflow.clone();
    engine.update_app_state(|state| {
        state.tool_permission_context = permission_context;
        state.team_context = team_context;
        state.plan_workflow = plan_workflow;
    });
}
