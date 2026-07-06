use super::*;
use allthecodes_bootstrap::SessionId;
use allthecodes_engine::types::app_state::AppState;
use std::path::PathBuf;
use std::sync::Arc;

fn make_ctx() -> allthecodes_commands::CommandContext {
    allthecodes_commands::CommandContext {
        messages: Vec::new(),
        cwd: PathBuf::from("."),
        app_state: AppState::default(),
        session_id: SessionId::from_string("test-session"),
    }
}

#[tokio::test]
async fn status_reports_no_team_when_context_missing() {
    let mut ctx = make_ctx();
    let s = execute_team_command("", &mut ctx).await;
    let lower = s.to_lowercase();
    assert!(
        lower.contains("no active team") || lower.contains("inactive"),
        "unexpected status output: {s}"
    );
    assert!(lower.contains("/team create"));
}

#[tokio::test]
async fn unknown_subcommand_is_reported() {
    let mut ctx = make_ctx();
    let s = execute_team_command("frobnicate", &mut ctx).await;
    assert!(s.contains("Unknown /team subcommand"));
}

#[tokio::test]
async fn help_subcommand_lists_commands() {
    let mut ctx = make_ctx();
    let s = execute_team_command("help", &mut ctx).await;
    assert!(s.contains("/team create"));
    assert!(s.contains("/team spawn"));
    assert!(s.contains("/team send"));
}

#[tokio::test]
async fn create_requires_name() {
    let mut ctx = make_ctx();
    let s = execute_team_command("create", &mut ctx).await;
    assert!(s.contains("Usage"));
}

#[tokio::test]
async fn spawn_without_team_reports_error() {
    let mut ctx = make_ctx();
    let s = execute_team_command("spawn researcher find-bugs", &mut ctx).await;
    assert!(s.contains("No active team"));
}

#[tokio::test]
async fn spawn_uses_command_scoped_hook_runner() {
    let inherited: Arc<dyn allthecodes_types::hooks::HookRunner> =
        Arc::new(allthecodes_types::hooks::NoopHookRunner::new());

    let resolved = allthecodes_commands::runtime::scope_hook_runner(inherited.clone(), async {
        hook_runner_for_team_spawn()
    })
    .await;

    assert!(Arc::ptr_eq(&resolved, &inherited));
}

#[tokio::test]
async fn leave_without_team_reports_noop() {
    let mut ctx = make_ctx();
    let s = execute_team_command("leave", &mut ctx).await;
    assert!(s.to_lowercase().contains("no active team"));
}

#[tokio::test]
async fn send_without_team_reports_error() {
    let mut ctx = make_ctx();
    let s = execute_team_command("send alice hello", &mut ctx).await;
    assert!(s.contains("No active team"));
}
