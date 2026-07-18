#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::{skip_trust_gate, PtySession, RENDER_WAIT};
use std::time::Duration;

fn isolated_session(
    cols: u16,
    rows: u16,
    requested_model: &str,
) -> (tempfile::TempDir, PtySession) {
    let temp = tempfile::tempdir().expect("isolated PTY root");
    let workspace = temp.path().join("workspace");
    let allthecodes_home = temp.path().join("allthecodes-home");
    let codex_home = temp.path().join("codex-home");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(&allthecodes_home).expect("allthecodes home");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::write(
        allthecodes_home.join("settings.json"),
        r#"{"defaultModel":"gpt-5.4","availableModels":["gpt-5.4"]}"#,
    )
    .expect("isolated settings");

    let workspace = workspace.to_string_lossy().into_owned();
    let allthecodes_home = allthecodes_home.to_string_lossy().into_owned();
    let codex_home = codex_home.to_string_lossy().into_owned();
    let session = PtySession::spawn_with_env(
        &[
            "-C",
            &workspace,
            "--permission-mode",
            "bypass",
            "--model",
            requested_model,
        ],
        cols,
        rows,
        true,
        &[
            ("ALLTHECODES_HOME", &allthecodes_home),
            ("CODEX_HOME", &codex_home),
            ("OPENAI_CODEX_AUTH_TOKEN", ""),
            ("RUNTIME_ENVIRONMENT", "test"),
            ("NO_PROXY", "*"),
        ],
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    if session.current_screen().contains("Bypass Permissions mode") {
        session.send_raw(b"\r");
        std::thread::sleep(Duration::from_millis(500));
    }
    (temp, session)
}

#[test]
fn large_paste_renders_inline_reference_in_real_pty() {
    let (_temp, session) = isolated_session(100, 24, "gpt-5.4");
    let pasted = "x".repeat(512);
    let mut bracketed_paste = b"\x1b[200~".to_vec();
    bracketed_paste.extend_from_slice(pasted.as_bytes());
    bracketed_paste.extend_from_slice(b"\x1b[201~");

    session.send_raw(&bracketed_paste);

    assert!(
        session.wait_for_screen_text("[Pasted Content 512 chars]", Duration::from_secs(3)),
        "large paste reference missing from PTY screen:\n{}",
        session.current_screen()
    );
    let output = session.finish_after_quit("paste_status_followup_large_paste");
    assert!(!output.contains("panicked"));
}

#[test]
fn startup_warning_single_model_and_bottom_anchor_are_visible_in_real_pty() {
    let (_temp, session) = isolated_session(100, 24, "missing-startup-model");
    let screen = session.current_screen();

    assert!(
        screen.contains("Requested model was unavailable"),
        "startup warning missing from session screen:\n{screen}"
    );
    assert_eq!(
        screen.matches("model: gpt-5.4").count(),
        1,
        "model should have one prompt-adjacent owner:\n{screen}"
    );

    let prompt_row = (0..24)
        .find(|row| session.screen_row(*row).trim_start().starts_with('>'))
        .expect("prompt row");
    assert_eq!(
        prompt_row, 20,
        "prompt should expand upward from the bottom"
    );
    assert!(
        session.screen_row(22).contains("model: gpt-5.4"),
        "context row should stay directly above the footer:\n{screen}"
    );
    assert!(
        !session.screen_row(23).trim().is_empty(),
        "footer must occupy the final terminal row:\n{screen}"
    );

    let output = session.finish_after_quit("paste_status_followup_warning_layout");
    assert!(!output.contains("panicked"));
}
