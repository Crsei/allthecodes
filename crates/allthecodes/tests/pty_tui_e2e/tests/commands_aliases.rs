#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 命令别名与边界情况 E2E 测试
//!
//! 测试目标：验证各命令别名可被识别且不会导致 TUI 崩溃。
//! 所有测试均为 **离线**，不依赖 API。
//!
//! 策略：对每个别名发送斜杠命令并断言无 panic，捕获别名注册错误。

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ─── T01: Group 1 - Safe Commands ───────────────────────────────────────

/// T01: 批量别名冒烟测试（第 1 组 - 安全命令）。
///
/// 按顺序测试 18 个别名（全部返回 Output，无副作用）：
///   /h、/?、/v、/settings、/ctx、/cp、/mem、/perms、/gitbranch、
///   /keys、/shortcuts、/status-line、/term-setup、/terminal、
///   /diagnostics、/diag、/exp、/experiments
///
/// 断言：全部 18 个别名均可解析且无 panic。
#[test]
fn alias_batch_group1() {
    let case = TestCase::new("alias_batch_group1")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        // /h
        .step(TestStep::Command("h".into()))
        .step(TestStep::AssertNoPanic)
        // /?
        .step(TestStep::Command("?".into()))
        .step(TestStep::AssertNoPanic)
        // /v
        .step(TestStep::Command("v".into()))
        .step(TestStep::AssertNoPanic)
        // /settings
        .step(TestStep::Command("settings".into()))
        .step(TestStep::AssertNoPanic)
        // /ctx
        .step(TestStep::Command("ctx".into()))
        .step(TestStep::AssertNoPanic)
        // /cp
        .step(TestStep::Command("cp".into()))
        .step(TestStep::AssertNoPanic)
        // /mem
        .step(TestStep::Command("mem".into()))
        .step(TestStep::AssertNoPanic)
        // /perms
        .step(TestStep::Command("perms".into()))
        .step(TestStep::AssertNoPanic)
        // /gitbranch
        .step(TestStep::Command("gitbranch".into()))
        .step(TestStep::AssertNoPanic)
        // /keys
        .step(TestStep::Command("keys".into()))
        .step(TestStep::AssertNoPanic)
        // /shortcuts
        .step(TestStep::Command("shortcuts".into()))
        .step(TestStep::AssertNoPanic)
        // /status-line
        .step(TestStep::Command("status-line".into()))
        .step(TestStep::AssertNoPanic)
        // /term-setup
        .step(TestStep::Command("term-setup".into()))
        .step(TestStep::AssertNoPanic)
        // /terminal
        .step(TestStep::Command("terminal".into()))
        .step(TestStep::AssertNoPanic)
        // /diagnostics
        .step(TestStep::Command("diagnostics".into()))
        .step(TestStep::AssertNoPanic)
        // /diag
        .step(TestStep::Command("diag".into()))
        .step(TestStep::AssertNoPanic)
        // /exp
        .step(TestStep::Command("exp".into()))
        .step(TestStep::AssertNoPanic)
        // /experiments
        .step(TestStep::Command("experiments".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T02: Group 2 - Exit Commands ───────────────────────────────────────

/// T02: 批量别名冒烟测试（第 2 组 - 退出类命令）。
///
/// 单独测试退出别名 `/q` 和 `/quit`，各自启动独立 PTY 会话。
/// 这两个命令会终止 REPL，因此不在同一会话中连续执行。
///
/// 断言：两者均正常退出且无 panic。
#[test]
fn alias_batch_group2_exit() {
    // Test /q
    let case_q = TestCase::new("alias_batch_group2_q")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("q".into()))
        .step(TestStep::AssertNoPanic);
    TestRunner::new().run(&case_q).assert_no_errors();

    // Test /quit
    let case_quit = TestCase::new("alias_batch_group2_quit")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("quit".into()))
        .step(TestStep::AssertNoPanic);
    TestRunner::new().run(&case_quit).assert_no_errors();
}

// ─── T03: Group 3 - Export/Query Commands ───────────────────────────────

/// T03: 批量别名冒烟测试（第 3 组 - 导出/查询命令）。
///
/// 按顺序测试 13 个别名：
///   /markdown-export、/audit、/sexport、/structured-export、
///   /plugins、/kairos、/logs、/teams、/teamonboarding、
///   /coord、/secreview、/cron、/br
///
/// 断言：全部 13 个别名均可解析且无 panic。
#[test]
fn alias_batch_group3() {
    let case = TestCase::new("alias_batch_group3")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        // /markdown-export
        .step(TestStep::Command("markdown-export".into()))
        .step(TestStep::AssertNoPanic)
        // /audit
        .step(TestStep::Command("audit".into()))
        .step(TestStep::AssertNoPanic)
        // /sexport
        .step(TestStep::Command("sexport".into()))
        .step(TestStep::AssertNoPanic)
        // /structured-export
        .step(TestStep::Command("structured-export".into()))
        .step(TestStep::AssertNoPanic)
        // /plugins
        .step(TestStep::Command("plugins".into()))
        .step(TestStep::AssertNoPanic)
        // /kairos
        .step(TestStep::Command("kairos".into()))
        .step(TestStep::AssertNoPanic)
        // /logs
        .step(TestStep::Command("logs".into()))
        .step(TestStep::AssertNoPanic)
        // /teams
        .step(TestStep::Command("teams".into()))
        .step(TestStep::AssertNoPanic)
        // /teamonboarding
        .step(TestStep::Command("teamonboarding".into()))
        .step(TestStep::AssertNoPanic)
        // /coord
        .step(TestStep::Command("coord".into()))
        .step(TestStep::AssertNoPanic)
        // /secreview
        .step(TestStep::Command("secreview".into()))
        .step(TestStep::AssertNoPanic)
        // /cron
        .step(TestStep::Command("cron".into()))
        .step(TestStep::AssertNoPanic)
        // /br
        .step(TestStep::Command("br".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T04: Unknown Command ───────────────────────────────────────────────

/// T04: 未知命令处理。
///
/// 发送不存在的命令 `/nonexistent-command-xyz`。
/// 断言：未知命令被优雅处理，无 panic。
#[test]
fn unknown_command_no_crash() {
    let case = TestCase::new("unknown_command_no_crash")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("nonexistent-command-xyz".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T05: Empty Slash Command ───────────────────────────────────────────

/// T05: 空命令。
///
/// 输入 "/" 后直接回车（空斜杠命令）。
/// 断言：空命令不会崩溃。
#[test]
fn empty_slash_command() {
    let case = TestCase::new("empty_slash_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::TypeText("/".into()))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T06: Command With Whitespace ───────────────────────────────────────

/// T06: 带多余空白的命令。
///
/// 发送 "  help  "（首尾空格），验证空白被正确去除。
/// 断言：命令正常工作，无 panic。
#[test]
fn command_with_whitespace() {
    let case = TestCase::new("command_with_whitespace")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("  help  ".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── T07: Command Case Sensitivity ──────────────────────────────────────

/// T07: 命令大小写敏感性。
///
/// 发送 "HELP"（全大写），验证大小写处理。
/// 断言：无 panic（大小写可能被折叠处理）。
#[test]
fn command_case_sensitivity() {
    let case = TestCase::new("command_case_sensitivity")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("HELP".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
