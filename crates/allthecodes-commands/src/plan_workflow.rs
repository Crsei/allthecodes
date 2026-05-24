//! Reusable plan-mode workflow transitions and persistence.

use std::fs;
use std::path::Path;

use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety;
use allthecodes_types::plan_workflow::{
    PlanEntryClassifierDecision, PlanWorkflowRecord, PlanWorkflowStatus,
};
use anyhow::{Context, Result};

const DEFAULT_OWNER: &str = "main";

pub fn default_owner() -> &'static str {
    DEFAULT_OWNER
}

pub fn load(cwd: &Path) -> Result<Option<PlanWorkflowRecord>> {
    let path = allthecodes_config::paths::current_plan_workflow_file_path(cwd);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read plan workflow: {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse plan workflow: {}", path.display()))?;
    Ok(Some(record))
}

pub fn persist(cwd: &Path, record: &PlanWorkflowRecord) -> Result<()> {
    let path = allthecodes_config::paths::current_plan_workflow_file_path(cwd);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create plan workflow dir: {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(record)?;
    fs::write(&path, raw)
        .with_context(|| format!("failed to write plan workflow: {}", path.display()))
}

pub fn enter_plan_mode_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    description: Option<&str>,
    classifier_reason: Option<&str>,
) -> PlanWorkflowRecord {
    if app_state.tool_permission_context.mode != PermissionMode::Plan {
        app_state.tool_permission_context.pre_plan_mode =
            Some(app_state.tool_permission_context.mode.clone());
        set_permission_mode_with_auto_mode_safety(
            &mut app_state.tool_permission_context,
            PermissionMode::Plan,
        );
    }

    let plan_file = allthecodes_config::paths::current_plan_file_path(cwd);
    let mut record = app_state
        .plan_workflow
        .clone()
        .or(existing)
        .unwrap_or_else(|| {
            PlanWorkflowRecord::new(display_path(&plan_file), Some(owner.to_string()), source)
        });

    record.file_path = display_path(&plan_file);
    if record.owner.is_none() {
        record.owner = Some(owner.to_string());
    }
    record.enter_plan_mode(source, description, classifier_reason);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn request_approval_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    plan_text: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    record.request_approval(source, plan_text);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn approve_and_exit_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    plan_text: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    if plan_text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty())
    {
        record.plan_text = plan_text;
    }
    record.approve(source);

    let restore_mode = app_state
        .tool_permission_context
        .pre_plan_mode
        .take()
        .unwrap_or(PermissionMode::Default);
    set_permission_mode_with_auto_mode_safety(&mut app_state.tool_permission_context, restore_mode);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn reject_approval_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    feedback: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    record.reject(source, feedback);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn link_task_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    task_id: String,
    summary: Option<String>,
) -> PlanWorkflowRecord {
    let mut record = ensure_record(app_state, cwd, existing, owner, source);
    record.link_task(source, task_id, summary);
    app_state.plan_workflow = Some(record.clone());
    record
}

pub fn maybe_link_implementation_task_state(
    app_state: &mut AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
    task_id: String,
    summary: Option<String>,
) -> Option<PlanWorkflowRecord> {
    let record = app_state.plan_workflow.clone().or(existing)?;
    if !matches!(
        record.status,
        PlanWorkflowStatus::Approved | PlanWorkflowStatus::Implementing
    ) {
        return None;
    }

    Some(link_task_state(
        app_state,
        cwd,
        Some(record),
        owner,
        source,
        task_id,
        summary,
    ))
}

pub fn classify_plan_entry(text: &str, app_state: &AppState) -> PlanEntryClassifierDecision {
    if app_state.tool_permission_context.mode == PermissionMode::Plan {
        return PlanEntryClassifierDecision {
            should_enter: false,
            reason: "plan mode is already active".to_string(),
            matched_rule: None,
        };
    }

    let trimmed = text.trim();
    if trimmed.starts_with('/') {
        return PlanEntryClassifierDecision {
            should_enter: false,
            reason: "slash commands own their own mode transitions".to_string(),
            matched_rule: None,
        };
    }

    let lower = trimmed.to_ascii_lowercase();
    for blocked in [
        "do not enter plan mode",
        "don't enter plan mode",
        "skip plan mode",
        "no plan mode",
    ] {
        if lower.contains(blocked) {
            return PlanEntryClassifierDecision {
                should_enter: false,
                reason: "explicit user override disables plan-mode classifier".to_string(),
                matched_rule: Some(blocked.to_string()),
            };
        }
    }

    for trigger in [
        "enter plan mode",
        "use plan mode",
        "plan mode first",
        "plan first",
        "make a plan first",
        "draft a plan first",
        "do not implement yet",
        "don't implement yet",
        "do not edit yet",
        "don't edit yet",
    ] {
        if lower.contains(trigger) {
            return PlanEntryClassifierDecision {
                should_enter: true,
                reason: "explicit user wording requested planning before edits".to_string(),
                matched_rule: Some(trigger.to_string()),
            };
        }
    }

    PlanEntryClassifierDecision {
        should_enter: false,
        reason: "no explicit plan-mode entry trigger matched".to_string(),
        matched_rule: None,
    }
}

pub fn summarize(record: &PlanWorkflowRecord) -> String {
    allthecodes_types::plan_workflow::summarize(record)
}

fn ensure_record(
    app_state: &AppState,
    cwd: &Path,
    existing: Option<PlanWorkflowRecord>,
    owner: &str,
    source: &str,
) -> PlanWorkflowRecord {
    let plan_file = allthecodes_config::paths::current_plan_file_path(cwd);
    let mut record = app_state
        .plan_workflow
        .clone()
        .or(existing)
        .unwrap_or_else(|| {
            PlanWorkflowRecord::new(display_path(&plan_file), Some(owner.to_string()), source)
        });
    record.file_path = display_path(&plan_file);
    if record.owner.is_none() {
        record.owner = Some(owner.to_string());
    }
    record
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_engine::types::app_state::AppState;
    use serial_test::serial;
    use tempfile::tempdir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
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

    #[test]
    #[serial]
    fn enter_plan_mode_sets_permission_and_persists_record_shape() {
        let tmp = tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let mut state = AppState::default();

        let record = enter_plan_mode_state(
            &mut state,
            tmp.path(),
            None,
            "main",
            "test",
            Some("design"),
            None,
        );
        persist(tmp.path(), &record).unwrap();
        let loaded = load(tmp.path()).unwrap().unwrap();

        assert_eq!(state.tool_permission_context.mode, PermissionMode::Plan);
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.status, PlanWorkflowStatus::Draft);
    }

    #[test]
    fn classifier_requires_explicit_plan_intent() {
        let state = AppState::default();
        let plain = classify_plan_entry("implement this feature", &state);
        assert!(!plain.should_enter);

        let explicit = classify_plan_entry("Plan first, then implement this feature", &state);
        assert!(explicit.should_enter);

        let blocked = classify_plan_entry("Do not enter plan mode; implement directly", &state);
        assert!(!blocked.should_enter);
    }

    #[test]
    fn implementation_task_link_requires_approved_plan() {
        let tmp = tempdir().unwrap();
        let mut state = AppState::default();
        let draft = PlanWorkflowRecord::new(
            tmp.path()
                .join(".allthecodes")
                .join("current-plan.md")
                .display()
                .to_string(),
            Some("main".to_string()),
            "test",
        );
        state.plan_workflow = Some(draft);

        let skipped = maybe_link_implementation_task_state(
            &mut state,
            tmp.path(),
            None,
            "main",
            "task_create",
            "task_draft".to_string(),
            Some("draft task".to_string()),
        );
        assert!(skipped.is_none());

        let mut approved = PlanWorkflowRecord::new(
            tmp.path()
                .join(".allthecodes")
                .join("current-plan.md")
                .display()
                .to_string(),
            Some("main".to_string()),
            "test",
        );
        approved.approve("test");
        state.plan_workflow = Some(approved);

        let linked = maybe_link_implementation_task_state(
            &mut state,
            tmp.path(),
            None,
            "main",
            "task_create",
            "task_impl".to_string(),
            Some("implementation task".to_string()),
        )
        .expect("approved plan should link implementation task");

        assert_eq!(linked.status, PlanWorkflowStatus::Implementing);
        assert_eq!(linked.linked_task_ids, vec!["task_impl"]);
        assert_eq!(
            state.plan_workflow.as_ref().unwrap().linked_task_ids,
            vec!["task_impl"]
        );
    }
}
