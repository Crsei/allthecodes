//! `/plan` command: durable plan workflow controls.

use std::fs;
use std::path::{Path, PathBuf};

use allthecodes_engine::types::tool::PermissionMode;
use allthecodes_types::plan_workflow::PlanWorkflowRecord;
use anyhow::Result;
use async_trait::async_trait;

use super::browser::{ensure_and_open, format_open_outcome};
use crate::{plan_workflow, CommandContext, CommandHandler, CommandResult};

const PLAN_TEMPLATE: &str = "# Plan\n\n<!-- Draft your implementation plan here. -->\n";

pub struct PlanHandler;

#[async_trait]
impl CommandHandler for PlanHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let trimmed = args.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let sub = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();

        match sub {
            "" | "show" | "view" => enter_and_show(ctx, None),
            "enter" | "start" => enter_and_show(ctx, Some(rest)),
            "open" | "edit" => open_plan(&ctx.cwd),
            "path" => Ok(CommandResult::Output(format!(
                "Plan file: {}\nWorkflow file: {}",
                allthecodes_config::paths::current_plan_file_path(&ctx.cwd).display(),
                allthecodes_config::paths::current_plan_workflow_file_path(&ctx.cwd).display()
            ))),
            "status" => status(ctx),
            "trace" => trace(ctx),
            "approve" => approve(ctx),
            "reject" => reject(ctx, rest),
            "link" => link(ctx, rest),
            "classify" => classify(ctx, rest),
            other => Ok(CommandResult::Output(format!(
                "Unknown subcommand: `{}`\n\n\
                 Usage:\n\
                 \x20 /plan                    Enter plan mode and show the plan file\n\
                 \x20 /plan enter [desc]       Enter plan mode with trace context\n\
                 \x20 /plan open|edit          Open plan file in $EDITOR\n\
                 \x20 /plan path               Print plan and workflow paths\n\
                 \x20 /plan status             Show workflow status\n\
                 \x20 /plan trace              Show workflow trace\n\
                 \x20 /plan approve            Approve plan and exit plan mode\n\
                 \x20 /plan reject [feedback]  Reject plan\n\
                 \x20 /plan link <id> [note]   Link implementation evidence\n\
                 \x20 /plan classify <prompt>  Preview classifier entry",
                other
            ))),
        }
    }
}

fn enter_and_show(ctx: &mut CommandContext, description: Option<&str>) -> Result<CommandResult> {
    let was_in_plan = matches!(
        ctx.app_state.tool_permission_context.mode,
        PermissionMode::Plan
    );
    let existing = plan_workflow::load(&ctx.cwd)?;
    let record = plan_workflow::enter_plan_mode_state(
        &mut ctx.app_state,
        &ctx.cwd,
        existing,
        plan_workflow::default_owner(),
        "slash_command",
        description,
        None,
    );
    plan_workflow::persist(&ctx.cwd, &record)?;

    let path = allthecodes_config::paths::current_plan_file_path(&ctx.cwd);
    let body = read_plan_body(&path);

    let header = if was_in_plan {
        format!(
            "**Plan mode** (already active) - {}\n{}",
            path.display(),
            plan_workflow::summarize(&record)
        )
    } else {
        format!(
            "**Plan mode** (entered) - {}\n{}",
            path.display(),
            plan_workflow::summarize(&record)
        )
    };

    Ok(CommandResult::Output(match body {
        Some(content) if !content.trim().is_empty() => format!("{header}\n\n{content}"),
        _ => format!("{header}\n\n(empty plan - use `/plan open` to draft one)"),
    }))
}

fn open_plan(cwd: &Path) -> Result<CommandResult> {
    let path: PathBuf = allthecodes_config::paths::current_plan_file_path(cwd);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let outcome = ensure_and_open(&path, PLAN_TEMPLATE);
    Ok(CommandResult::Output(format_open_outcome(&outcome, &path)))
}

fn status(ctx: &mut CommandContext) -> Result<CommandResult> {
    match current_record(ctx)? {
        Some(record) => Ok(CommandResult::Output(format_record(&record))),
        None => Ok(CommandResult::Output(
            "No plan workflow exists yet. Use `/plan` to start one.".to_string(),
        )),
    }
}

fn trace(ctx: &mut CommandContext) -> Result<CommandResult> {
    match current_record(ctx)? {
        Some(record) if !record.trace.is_empty() => {
            let lines = record
                .trace
                .iter()
                .map(|event| {
                    format!(
                        "- {} [{:?}] {} ({})",
                        event.at, event.kind, event.summary, event.source
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(CommandResult::Output(format!(
                "Plan workflow trace: {}\n{}",
                record.id, lines
            )))
        }
        Some(record) => Ok(CommandResult::Output(format!(
            "Plan workflow {} has no trace events.",
            record.id
        ))),
        None => Ok(CommandResult::Output(
            "No plan workflow exists yet. Use `/plan` to start one.".to_string(),
        )),
    }
}

fn approve(ctx: &mut CommandContext) -> Result<CommandResult> {
    let plan_path = allthecodes_config::paths::current_plan_file_path(&ctx.cwd);
    let plan_text = read_plan_body(&plan_path);
    let existing = plan_workflow::load(&ctx.cwd)?;
    let record = plan_workflow::approve_and_exit_state(
        &mut ctx.app_state,
        &ctx.cwd,
        existing,
        plan_workflow::default_owner(),
        "slash_command",
        plan_text,
    );
    plan_workflow::persist(&ctx.cwd, &record)?;
    Ok(CommandResult::Output(format!(
        "Plan approved. Normal operations restored.\n{}",
        plan_workflow::summarize(&record)
    )))
}

fn reject(ctx: &mut CommandContext, feedback: &str) -> Result<CommandResult> {
    let existing = plan_workflow::load(&ctx.cwd)?;
    let record = plan_workflow::reject_approval_state(
        &mut ctx.app_state,
        &ctx.cwd,
        existing,
        plan_workflow::default_owner(),
        "slash_command",
        (!feedback.trim().is_empty()).then(|| feedback.trim().to_string()),
    );
    plan_workflow::persist(&ctx.cwd, &record)?;
    Ok(CommandResult::Output(format!(
        "Plan rejected. Revise the plan and request approval again.\n{}",
        plan_workflow::summarize(&record)
    )))
}

fn link(ctx: &mut CommandContext, rest: &str) -> Result<CommandResult> {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let task_id = parts.next().unwrap_or("").trim();
    if task_id.is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /plan link <task-id> [summary]".to_string(),
        ));
    }
    let summary = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let existing = plan_workflow::load(&ctx.cwd)?;
    let record = plan_workflow::link_task_state(
        &mut ctx.app_state,
        &ctx.cwd,
        existing,
        plan_workflow::default_owner(),
        "slash_command",
        task_id.to_string(),
        summary,
    );
    plan_workflow::persist(&ctx.cwd, &record)?;
    Ok(CommandResult::Output(format!(
        "Linked implementation evidence `{}`.\n{}",
        task_id,
        plan_workflow::summarize(&record)
    )))
}

fn classify(ctx: &mut CommandContext, prompt: &str) -> Result<CommandResult> {
    if prompt.trim().is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /plan classify <prompt>".to_string(),
        ));
    }
    let decision = plan_workflow::classify_plan_entry(prompt, &ctx.app_state);
    Ok(CommandResult::Output(format!(
        "Plan classifier: should_enter={}\nReason: {}\nMatched rule: {}",
        decision.should_enter,
        decision.reason,
        decision.matched_rule.as_deref().unwrap_or("(none)")
    )))
}

fn current_record(ctx: &CommandContext) -> Result<Option<PlanWorkflowRecord>> {
    Ok(ctx
        .app_state
        .plan_workflow
        .clone()
        .or(plan_workflow::load(&ctx.cwd)?))
}

fn format_record(record: &PlanWorkflowRecord) -> String {
    format!(
        "Plan workflow: {}\nStatus: {:?}\nApproval: {:?}\nPlan file: {}\nOwner: {}\nLinked tasks: {}\nUpdated: {}",
        record.id,
        record.status,
        record.approval_state,
        record.file_path,
        record.owner.as_deref().unwrap_or("(none)"),
        if record.linked_task_ids.is_empty() {
            "(none)".to_string()
        } else {
            record.linked_task_ids.join(", ")
        },
        record.updated_at,
    )
}

fn read_plan_body(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use allthecodes_types::commands::CommandDispatcher;
    use serial_test::serial;
    use std::env;
    use tempfile::tempdir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = env::var(key).ok();
            env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    fn make_ctx(cwd: PathBuf, start_mode: PermissionMode) -> CommandContext {
        let mut app_state = AppState::default();
        app_state.tool_permission_context.mode = start_mode;
        app_state.tool_permission_context.pre_plan_mode = None;
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state,
            session_id: SessionId::new(),
        }
    }

    #[test]
    fn dispatcher_parses_bare_plan_without_swallowing_it() {
        let dispatcher = crate::DefaultCommandDispatcher::for_full_registry();
        let parsed = dispatcher
            .parse_command_input("/plan")
            .expect("bare /plan should parse as a command");

        assert_eq!(
            dispatcher.command_name(parsed.index).as_deref(),
            Some("plan")
        );
        assert_eq!(parsed.args, "");
    }

    #[tokio::test]
    #[serial]
    async fn bare_plan_enters_plan_mode_and_shows_placeholder() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let _visual = EnvGuard::unset("VISUAL");
        let _editor = EnvGuard::unset("EDITOR");

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler.execute("", &mut ctx).await.unwrap();

        assert!(matches!(
            ctx.app_state.tool_permission_context.mode,
            PermissionMode::Plan
        ));
        assert_eq!(
            ctx.app_state.tool_permission_context.pre_plan_mode,
            Some(PermissionMode::Default)
        );
        assert!(ctx.app_state.plan_workflow.is_some());
        assert!(allthecodes_config::paths::current_plan_workflow_file_path(tmp.path()).is_file());
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Plan mode"));
                assert!(text.contains("empty plan"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn plan_is_idempotent_and_preserves_prior_mode() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::AcceptEdits);
        PlanHandler.execute("", &mut ctx).await.unwrap();
        PlanHandler.execute("", &mut ctx).await.unwrap();

        assert_eq!(
            ctx.app_state.tool_permission_context.pre_plan_mode,
            Some(PermissionMode::AcceptEdits),
            "second /plan invocation must not clobber pre_plan_mode"
        );
    }

    #[tokio::test]
    #[serial]
    async fn approve_restores_prior_mode_and_records_approval() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::AcceptEdits);
        PlanHandler.execute("", &mut ctx).await.unwrap();
        let result = PlanHandler.execute("approve", &mut ctx).await.unwrap();

        assert_eq!(
            ctx.app_state.tool_permission_context.mode,
            PermissionMode::AcceptEdits
        );
        let workflow = ctx.app_state.plan_workflow.as_ref().unwrap();
        assert_eq!(
            workflow.approval_state,
            allthecodes_types::plan_workflow::PlanApprovalState::Approved
        );
        match result {
            CommandResult::Output(text) => assert!(text.contains("Plan approved")),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn show_existing_plan_renders_body() {
        let tmp = tempdir().unwrap();
        let plan_dir = tmp.path().join(".allthecodes");
        fs::create_dir_all(&plan_dir).unwrap();
        let plan_path = plan_dir.join("plan.md");
        fs::write(&plan_path, "# My Plan\n\n1. Step one\n").unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler.execute("show", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("My Plan"));
                assert!(text.contains("Step one"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn unknown_subcommand_prints_usage() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler.execute("bogus", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown subcommand"));
                assert!(text.contains("bogus"));
                assert!(text.contains("/plan open"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn path_subcommand_prints_resolved_path() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler.execute("path", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Plan file:"));
                assert!(text.contains("Workflow file:"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn open_without_editor_creates_template_and_reports() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());
        let _visual = EnvGuard::unset("VISUAL");
        let _editor = EnvGuard::unset("EDITOR");

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler.execute("open", &mut ctx).await.unwrap();

        let body = fs::read_to_string(allthecodes_config::paths::current_plan_file_path(
            tmp.path(),
        ))
        .unwrap();
        assert!(body.starts_with("# Plan"));

        match result {
            CommandResult::Output(text) => assert!(!text.is_empty()),
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    #[serial]
    async fn classify_previews_explicit_entry() {
        let tmp = tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", tmp.path().to_str().unwrap());

        let mut ctx = make_ctx(tmp.path().to_path_buf(), PermissionMode::Default);
        let result = PlanHandler
            .execute("classify please plan first before editing", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("should_enter=true")),
            _ => panic!("expected Output"),
        }
    }
}
