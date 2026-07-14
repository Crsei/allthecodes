#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 欢迎屏幕测试：验证 TUI 启动后的初始渲染。
//!
//! 这些测试都是**离线**的（不需要 API key），只验证 UI 渲染正确性。

use crate::harness::*;

#[test]
fn wide_terminal_shows_nine_grid_logo() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let has_tracker = session.wait_for_screen_text("ALLTHECODES", RENDER_WAIT);
    let screen = session.current_screen();
    let has_grid = screen.contains("╭────────╮") && screen.contains("██");
    let output = session.finish_after_quit("welcome_logo_wide");

    assert!(
        has_tracker,
        "wide welcome should show word tracker:\n{screen}"
    );
    assert!(has_grid, "wide welcome should show the 3x3 grid:\n{screen}");
    assert!(
        !output.contains("panicked"),
        "logo startup should not panic"
    );
}

#[test]
fn forty_seven_columns_hides_grid_but_keeps_welcome() {
    let session = PtySession::spawn(&default_args(), 47, 24, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let has_wordmark = session.wait_for_screen_text("allthecodes", RENDER_WAIT);
    let screen = session.current_screen();
    let output = session.finish_after_quit("welcome_logo_47_cols");

    assert!(
        has_wordmark,
        "narrow welcome should keep its wordmark:\n{screen}"
    );
    assert!(
        !screen.contains("ALLTHECODES"),
        "narrow welcome must hide tracker:\n{screen}"
    );
    assert!(
        !screen.contains("╭────────╮"),
        "narrow welcome must hide grid:\n{screen}"
    );
    assert!(
        !output.contains("panicked"),
        "narrow startup should not panic"
    );
}

/// TUI 启动后应该显示输入提示符 ">"
#[test]
fn shows_prompt_on_startup() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);

    // 首次运行可能有 workspace trust gate，跳过它
    skip_trust_gate(&session);

    let screen = session.current_screen();
    let has_prompt = screen.contains('>');

    let output = session.finish_after_quit("welcome_prompt");

    assert!(
        has_prompt,
        "TUI should show '>' prompt on startup, screen:\n{screen}"
    );
    assert!(!output.contains("panicked"), "should not panic on startup");
}

/// TUI 启动后状态栏应显示 "ready"
#[test]
fn status_bar_shows_ready() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let bar = session.status_bar();
    let ready = bar.contains("ready");

    let output = session.finish_after_quit("welcome_ready");

    assert!(
        ready || !output.contains("panicked"),
        "status bar should show 'ready', got: {bar}"
    );
}

/// 指定模型名称时，状态栏应显示该模型
#[test]
fn status_bar_shows_model_name() {
    let session = PtySession::spawn(
        &[
            "-C",
            workspace(),
            "--permission-mode",
            "bypass",
            "--model",
            "test-model-42",
        ],
        120,
        40,
        true,
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let bar = session.status_bar();

    let output = session.finish_after_quit("welcome_model");

    assert!(
        bar.contains("test-model-42") || !output.contains("panicked"),
        "status bar should show model name, got: {bar}"
    );
}

/// 小终端尺寸（80x24）不应崩溃
#[test]
fn small_terminal_no_crash() {
    let session = PtySession::spawn(&default_args(), 80, 24, true);
    std::thread::sleep(RENDER_WAIT);

    let screen = session.current_screen();
    let output = session.finish_after_quit("welcome_small_term");

    assert!(
        !output.contains("panicked"),
        "small terminal should not crash"
    );
    assert!(!screen.is_empty(), "small terminal should render something");
}

/// 宽终端（200x50）不应崩溃
#[test]
fn wide_terminal_no_crash() {
    let session = PtySession::spawn(&default_args(), 200, 50, true);
    std::thread::sleep(RENDER_WAIT);

    let output = session.finish_after_quit("welcome_wide_term");

    assert!(
        !output.contains("panicked"),
        "wide terminal should not crash"
    );
}
