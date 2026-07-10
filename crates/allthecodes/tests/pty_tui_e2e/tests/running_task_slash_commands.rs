#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::{skip_trust_gate, PtySession, QUICK_TIMEOUT, RENDER_WAIT};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn write_slow_user_prompt_hook(workspace: &Path) {
    let settings_dir = workspace.join(".allthecodes");
    std::fs::create_dir_all(&settings_dir).expect("create project settings dir");
    let settings = json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "while [ ! -f .allthecodes/release-slow-hook ]; do sleep 0.1; done",
                            "timeout": 20
                        }
                    ]
                }
            ]
        }
    });
    std::fs::write(
        settings_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).expect("serialize settings"),
    )
    .expect("write project settings");
}

fn spawn_session_with_slow_prompt_hook(temp: &tempfile::TempDir) -> (PtySession, PathBuf, PathBuf) {
    let workspace = temp.path().join("workspace");
    let cc_home = temp.path().join("home");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    std::fs::create_dir_all(&cc_home).expect("create cc home");
    write_slow_user_prompt_hook(&workspace);

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let cc_home_env = cc_home.to_string_lossy().into_owned();
    let args = ["-C", workspace_arg.as_str(), "--permission-mode", "bypass"];
    let env = [("ALLTHECODES_HOME", cc_home_env.as_str())];
    let session = PtySession::spawn_with_env(&args, 120, 40, true, &env);
    (session, workspace, cc_home)
}

fn start_slow_turn(session: &PtySession) {
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(session);
    std::thread::sleep(Duration::from_millis(500));
    if session.current_screen().contains("Bypass Permissions mode") {
        session.send_raw(b"y");
        std::thread::sleep(Duration::from_millis(500));
    }
    session.send_line("start a slow hook turn");
    std::thread::sleep(Duration::from_millis(600));
}

fn release_slow_hook(workspace: &Path) {
    std::fs::write(
        workspace.join(".allthecodes").join("release-slow-hook"),
        b"done",
    )
    .expect("release slow hook");
}

fn exit_session(
    session: PtySession,
    workspace: &Path,
    label: &str,
) -> crate::harness::CapturedOutput {
    release_slow_hook(workspace);
    exit_session_without_releasing_hook(session, label)
}

fn exit_session_without_releasing_hook(
    session: PtySession,
    label: &str,
) -> crate::harness::CapturedOutput {
    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));
    session.send_ctrl_c();
    session.finish(QUICK_TIMEOUT, label)
}

#[test]
fn running_task_enter_disables_unavailable_slash_command_and_keeps_prompt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (session, workspace, _cc_home) = spawn_session_with_slow_prompt_hook(&temp);
    start_slow_turn(&session);

    session.send_line("/review these changes");
    let found_error = session.wait_for_text(
        "'/review' is disabled while a task is in progress.",
        Duration::from_secs(3),
    );
    let prompt_retained =
        session.wait_for_screen_text("/review these changes", Duration::from_secs(2));

    let output = exit_session(session, &workspace, "running_task_slash_disabled");

    assert!(
        found_error,
        "disabled command message should render, got:\n{}",
        output.text()
    );
    assert!(
        prompt_retained,
        "disabled command text should stay in the prompt, got:\n{}",
        output.text()
    );
    assert!(!output.contains("panicked"));
}

#[test]
fn running_task_enter_executes_available_slash_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (session, workspace, _cc_home) = spawn_session_with_slow_prompt_hook(&temp);
    start_slow_turn(&session);

    session.send_line("/status");
    let found_status = session.wait_for_text("Session Status", Duration::from_secs(3));

    let output = exit_session(session, &workspace, "running_task_slash_status");

    assert!(
        found_status,
        "available command should execute while task is running, got:\n{}",
        output.text()
    );
    assert!(!output.contains("disabled while a task is in progress"));
    assert!(!output.contains("panicked"));
}

#[test]
fn running_task_tab_queues_unavailable_slash_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (session, _workspace, _cc_home) = spawn_session_with_slow_prompt_hook(&temp);
    start_slow_turn(&session);

    session.send_raw(b"/review these changes");
    std::thread::sleep(Duration::from_millis(100));
    session.send_tab();

    let queued = session.wait_for_text("Queued next prompt (1 pending).", Duration::from_secs(3));
    let disabled = session
        .current_text()
        .contains("disabled while a task is in progress");

    let output = exit_session_without_releasing_hook(session, "running_task_slash_queue");

    assert!(
        queued,
        "Tab should queue slash command while task is running, got:\n{}",
        output.text()
    );
    assert!(
        !disabled,
        "Tab queue should not run immediate availability rejection, got:\n{}",
        output.text()
    );
    assert!(!output.contains("panicked"));
}
