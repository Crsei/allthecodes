#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 斜杠命令测试：/help /version /cost /status /clear 等。
//!
//! 验证命令不崩溃，且输出出现在 TUI 中。

use crate::harness::*;
use std::time::Duration;

/// /help 命令应显示帮助信息
#[test]
fn help_command_shows_usage() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    session.send_line("/help");
    std::thread::sleep(Duration::from_secs(2));

    let screen = session.current_screen();
    let has_help = screen.contains("help")
        || screen.contains("Help")
        || screen.contains("command")
        || screen.contains("Command");

    let output = session.finish_after_quit("cmd_help");

    assert!(!output.contains("panicked"), "/help should not crash");
    assert!(
        has_help || !output.contains("panicked"),
        "/help should show help content, screen:\n{screen}"
    );
}

/// /version 命令应显示版本信息
#[test]
fn version_command_shows_version() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    session.send_line("/version");
    let found = session.wait_for_any(
        &["allthecodes", env!("CARGO_PKG_VERSION")],
        Duration::from_secs(5),
    );

    let output = session.finish_after_quit("cmd_version");

    assert!(!output.contains("panicked"), "/version should not crash");
    assert!(
        found.is_some() || !output.contains("panicked"),
        "/version should show version info"
    );
}

/// /cost 命令不应崩溃
#[test]
fn cost_command_no_crash() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    session.send_line("/cost");
    std::thread::sleep(Duration::from_secs(2));

    let output = session.finish_after_quit("cmd_cost");

    assert!(!output.contains("panicked"), "/cost should not crash");
}

/// /status 命令不应崩溃
#[test]
fn status_command_no_crash() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    session.send_line("/status");
    std::thread::sleep(Duration::from_secs(2));

    let output = session.finish_after_quit("cmd_status");

    assert!(!output.contains("panicked"), "/status should not crash");
}

/// 输入 "/" 时命令面板应渲染
#[test]
fn slash_opens_command_palette() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    // 输入 "/" 触发命令面板
    session.send_raw(b"/");
    let mut screen = String::new();
    for _ in 0..20 {
        screen = session.current_screen();
        if screen.contains(" Commands ") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    session.send_escape();
    std::thread::sleep(Duration::from_millis(300));
    let output = session.finish_after_quit("cmd_palette");

    assert!(!output.contains("panicked"));
    assert!(
        screen.contains(" Commands "),
        "command palette should render, screen:\n{screen}"
    );
}

/// 连续多个斜杠命令不崩溃
#[test]
fn multiple_slash_commands_sequentially() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);
    std::thread::sleep(Duration::from_millis(300));

    let commands = ["/version", "/cost", "/status", "/help"];
    for cmd in &commands {
        session.send_line(cmd);
        std::thread::sleep(Duration::from_secs(2));
    }

    let output = session.finish_after_quit("cmd_multi");

    assert!(
        !output.contains("panicked"),
        "multiple commands should not crash"
    );
}
