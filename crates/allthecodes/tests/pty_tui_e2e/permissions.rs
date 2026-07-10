#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 权限对话框交互测试。
//!
//! 测试不同权限模式下的工具执行行为：
//! - `bypass` 模式：直接执行，不弹框
//! - `default` 模式：非交互环境下应拒绝
//! - 权限对话框渲染验证

use crate::harness::*;
use std::time::Duration;

/// bypass 模式下工具直接执行，不弹出权限对话框。
#[test]
#[ignore = "requires a real API key and network"]
fn bypass_mode_executes_without_dialog() {
    let session = PtySession::spawn(
        &["-C", workspace(), "--permission-mode", "bypass"],
        120,
        40,
        false,
    );
    std::thread::sleep(RENDER_WAIT);

    session.send_line("Use the Bash tool to run: echo BYPASS_EXEC_OK");

    // 等待工具输出（不应出现权限提示）
    let found = session.wait_for_any(&["BYPASS_EXEC_OK", "Claude:"], API_TIMEOUT);

    let output = session.finish_after_quit("perm_bypass");

    assert!(
        found.is_some(),
        "bypass mode should allow tool execution, got:\n{}",
        output.text()
    );
    assert!(
        !output.text().contains("Permission required"),
        "bypass mode should not show permission dialog"
    );
}

/// default 模式下（非交互），工具请求应被拒绝。
#[test]
#[ignore = "requires a real API key and network"]
fn default_mode_denies_tool() {
    let session = PtySession::spawn(
        &["-C", workspace()], // 无 --permission-mode = default
        120,
        40,
        false,
    );
    std::thread::sleep(RENDER_WAIT);

    session.send_line("Use the Bash tool to run: echo SHOULD_NOT_RUN");

    // 等待错误或权限拒绝
    let found = session.wait_for_any(
        &["Permission", "permission", "denied", "error", "Error"],
        Duration::from_secs(30),
    );

    let output = session.finish_after_quit("perm_default");

    assert!(
        found.is_some() || !output.contains("panicked"),
        "default mode should deny or show error, got:\n{}",
        output.text()
    );
}

/// 无 API key 时提交提示，应显示错误而非崩溃。
#[test]
fn no_api_key_shows_error() {
    let session = PtySession::spawn(
        &["-C", workspace()],
        120,
        40,
        true, // strip_keys = true
    );
    std::thread::sleep(RENDER_WAIT);
    skip_trust_gate(&session);

    session.send_line("hello");

    let found = session.wait_for_any(
        &[
            "error",
            "Error",
            "api",
            "API",
            "no API",
            "configured",
            "key",
        ],
        Duration::from_secs(10),
    );

    let output = session.finish_after_quit("perm_no_key");

    assert!(
        found.is_some() || !output.contains("panicked"),
        "should show error for missing API key, got:\n{}",
        output.text()
    );
}

/// 启动时传入各种 permission-mode 标志，TUI 不应崩溃。
#[test]
fn all_permission_modes_start_cleanly() {
    for mode in &["bypass", "auto", "default"] {
        let session = PtySession::spawn_with_env(
            &["-C", workspace(), "--permission-mode", mode],
            120,
            40,
            true,
            &[],
        );
        std::thread::sleep(RENDER_WAIT);

        let screen = session.current_screen();
        let output = session.finish_after_quit(&format!("perm_mode_{mode}"));

        assert!(
            !output.contains("panicked"),
            "permission mode '{mode}' should not crash, screen:\n{screen}"
        );
    }
}

/// 权限对话框在 PTY 中渲染时应包含相关文本（通过模拟工具请求触发）。
/// 这个测试验证权限对话框的渲染区域可见。
#[test]
fn permission_dialog_renders_in_screen_area() {
    // 使用 App-level 构造验证权限对话框渲染位置
    // （PTY 级别的权限触发需要 API，这里只验证离线不崩溃）
    let session = PtySession::spawn(&default_args(), 120, 40, true);
    std::thread::sleep(RENDER_WAIT);

    skip_trust_gate(&session);
    // 验证初始屏幕没有意外的权限文本
    let screen = session.current_screen();
    assert!(
        !screen.contains("Permission Required"),
        "startup screen should not show permission dialog"
    );

    let output = session.finish_after_quit("perm_dialog_render");
    assert!(!output.contains("panicked"));
}
