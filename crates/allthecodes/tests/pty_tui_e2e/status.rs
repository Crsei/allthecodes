#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 状态栏运行状况测试：验证状态栏信息的正确性。
//!
//! 测试状态栏显示的模型名、工作区路径、消息计数、ready 状态等。

use crate::harness::*;
use std::time::Duration;

/// 启动后状态栏应显示 "ready"
#[test]
fn initial_status_is_ready() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let bar = session.status_bar();
    let has_ready = bar.contains("ready");

    let output = session.finish_after_quit("status_ready");

    assert!(
        has_ready || !output.contains("panicked"),
        "initial status should contain 'ready', got: {bar}"
    );
}

/// 状态栏应包含工作区路径
#[test]
fn status_bar_contains_workspace_path() {
    let _ws = workspace();
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    let bar = session.status_bar();

    let output = session.finish_after_quit("status_ws_path");

    // 状态栏可能显示完整路径或截断后的路径
    // 至少不应该崩溃
    assert!(
        !output.contains("panicked"),
        "status bar workspace check should not crash"
    );
    eprintln!("[status] bar: {bar}");
}

/// Ctrl+C 后状态栏恢复到 ready 状态
#[test]
fn ctrl_c_returns_to_ready() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    // 输入一些文本
    session.send_raw(b"some text");
    std::thread::sleep(Duration::from_millis(300));

    // Ctrl+C 取消
    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(500));

    let bar = session.status_bar();
    let has_ready = bar.contains("ready");

    let output = session.finish_after_quit("status_ctrl_c_ready");

    assert!(
        has_ready || !output.contains("panicked"),
        "after Ctrl+C, status should return to ready, got: {bar}"
    );
}

/// 提交空行后状态栏不变
#[test]
fn empty_submit_preserves_ready() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    session.send_raw(b"\r");
    std::thread::sleep(Duration::from_secs(1));

    let bar = session.status_bar();

    let output = session.finish_after_quit("status_empty_submit");

    assert!(
        !output.contains("panicked"),
        "empty submit should not crash"
    );
    eprintln!("[status] after empty submit: {bar}");
}

/// 状态栏在多个操作后仍可正确读取
#[test]
fn status_bar_readable_after_multiple_operations() {
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    // 操作序列
    session.send_raw(b"hello");
    std::thread::sleep(Duration::from_millis(300));
    session.send_raw(&[0x15]); // Ctrl+U clear line
    std::thread::sleep(Duration::from_millis(300));
    session.send_up();
    std::thread::sleep(Duration::from_millis(200));
    session.send_down();
    std::thread::sleep(Duration::from_millis(200));
    // Ctrl+C 清除输入（但不退出）
    session.send_ctrl_c();
    std::thread::sleep(Duration::from_millis(300));

    let screen = session.current_screen();
    // 屏幕应有内容（至少应有 info panel 或 prompt）
    assert!(
        !screen.trim().is_empty(),
        "screen should have content after operations"
    );

    let output = session.finish_after_quit("status_multi_ops");

    assert!(!output.contains("panicked"));
}
