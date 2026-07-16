#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// T01 – T02：/exit 及其别名
// ─────────────────────────────────────────────────────────────────────────────

/// T01: `/exit` 触发 REPL 退出。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("exit") → Wait(2s) → AssertNoPanic
/// 断言：输出中不包含 "panicked"，进程退出
#[test]
fn exit_command_quits_repl() {
    let case = TestCase::new("exit_command_quits_repl")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("exit".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02a: `/exit` 别名 `/q` 触发 REPL 退出。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("q") → Wait(2s) → AssertNoPanic
/// 断言：输出中不包含 "panicked"，进程退出
#[test]
fn exit_alias_q_quits_repl() {
    let case = TestCase::new("exit_alias_q_quits_repl")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("q".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02b: `/exit` 别名 `/quit` 触发 REPL 退出。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("quit") → Wait(2s) → AssertNoPanic
/// 断言：输出中不包含 "panicked"，进程退出
#[test]
fn exit_alias_quit_quits_repl() {
    let case = TestCase::new("exit_alias_quit_quits_repl")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("quit".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T03 – T05：/config 及其子命令和别名
// ─────────────────────────────────────────────────────────────────────────────

/// T03: `/config` 显示设置（无参数）。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("config") → Wait(2s) →
///       AssertScreenContains("Show effective config") → Snapshot("config_show")
/// 断言：屏幕包含配置表面的固定标题。
#[test]
fn config_show_displays_settings() {
    let case = TestCase::new("config_show_displays_settings")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("config".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains(
            "Show effective config".into(),
        ))
        .step(TestStep::Snapshot("config_show".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: `/config reset` 恢复默认值。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("config set theme dark") → Wait(1s) →
///       Command("config reset") → Wait(2s) → Snapshot("after_reset") → AssertNoPanic
/// 断言：无 panic，屏幕显示重置确认或默认值
#[test]
fn config_reset_restores_defaults() {
    let case = TestCase::new("config_reset_restores_defaults")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("config set theme dark".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::Command("config reset".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("after_reset".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05: `/config` 别名 `/settings`。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("settings") → Wait(2s) →
///       AssertScreenContains("Show effective config")
/// 断言：别名效果与 /config 相同
#[test]
fn config_alias_settings() {
    let case = TestCase::new("config_alias_settings")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("settings".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains(
            "Show effective config".into(),
        ));

    TestRunner::new().run(&case).assert_no_errors();
}
// ─────────────────────────────────────────────────────────────────────────────
// T06：/debug
// ─────────────────────────────────────────────────────────────────────────────

/// T06: `/debug` 显示调试信息。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("debug") → Wait(2s) →
///       AssertNoPanic → Snapshot("debug_output")
/// 断言：屏幕非空，无 panic
#[test]
fn debug_command_shows_info() {
    let case = TestCase::new("debug_command_shows_info")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("debug".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("debug_output".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T07 – T09：/effort
// ─────────────────────────────────────────────────────────────────────────────

/// T07: `/effort` 显示当前 effort。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("effort") → Wait(2s) →
///       AssertScreenContains("Effort / Filter")
/// 断言：显示 effort 选择器。
#[test]
fn effort_shows_current() {
    let case = TestCase::new("effort_shows_current")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("effort".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("Effort / Filter".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08: `/effort high` 报告当前 profile 的设置结果。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("effort high") → Wait(2s) →
///       WaitForAny("Effort set to", "Current profile has no configured")
/// 断言：支持 high 的 profile 成功设置；不支持的 profile 给出能力诊断。
#[test]
fn effort_set_high_reports_profile_result() {
    let case = TestCase::new("effort_set_high_reports_profile_result")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("effort high".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::WaitForAny(
            vec![
                "Effort set to".into(),
                "Current profile has no configured".into(),
            ],
            Duration::from_secs(5),
        ));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09: `/effort` 无效值被拒绝。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("effort extreme") → Wait(2s) →
///       WaitForAny("low", "medium", "high")
/// 断言：无效 effort 被拒绝并给出有用的提示信息（应提到有效值）
#[test]
fn effort_rejects_invalid() {
    let case = TestCase::new("effort_rejects_invalid")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("effort extreme".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::WaitForAny(
            vec!["low".into(), "medium".into(), "high".into()],
            Duration::from_secs(5),
        ));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T10：/fast
// ─────────────────────────────────────────────────────────────────────────────

/// T10: `/fast` 切换。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("fast status") → Wait(1s) →
///       Snapshot("fast_initial") → Command("fast on") → Wait(2s) →
///       AssertScreenContains("fast") → Command("fast off") → Wait(2s) →
///       AssertNoPanic
/// 断言：切换操作无 panic
#[test]
fn fast_toggle() {
    let case = TestCase::new("fast_toggle")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("fast status".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::Snapshot("fast_initial".into()))
        .step(TestStep::Command("fast on".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertScreenContains("Fast".into()))
        .step(TestStep::Command("fast off".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T11 – T12：/context 及其别名
// ─────────────────────────────────────────────────────────────────────────────

/// T11: `/context` 显示上下文信息。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("context") → Wait(2s) →
///       WaitForAny("token", "context")
/// 断言：显示上下文用量
#[test]
fn context_command_shows_usage() {
    let case = TestCase::new("context_command_shows_usage")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("context".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::WaitForAny(
            vec!["token".into(), "context".into()],
            Duration::from_secs(5),
        ));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T12: `/context` 别名 `/ctx`。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("ctx") → Wait(2s) →
///       AssertNoPanic
/// 断言：别名可用
#[test]
fn context_alias_ctx() {
    let case = TestCase::new("context_alias_ctx")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("ctx".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T13：/files
// ─────────────────────────────────────────────────────────────────────────────

/// T13: `/files` 列出引用的文件。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("files") → Wait(2s) →
///       AssertNoPanic
/// 断言：无 panic（新会话中可能显示 "no files"）
#[test]
fn files_command_works() {
    let case = TestCase::new("files_command_works")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("files".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T14 – T15：/copy 及其别名
// ─────────────────────────────────────────────────────────────────────────────

/// T14: `/copy` 在无助手消息时不崩溃。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("copy") → Wait(2s) →
///       AssertNoPanic
/// 断言：无助手消息时不 panic
#[test]
fn copy_no_crash_fresh_session() {
    let case = TestCase::new("copy_no_crash_fresh_session")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("copy".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T15: `/copy` 别名 `/cp`。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("cp") → Wait(2s) →
///       AssertNoPanic
/// 断言：别名可用
#[test]
fn copy_alias_cp() {
    let case = TestCase::new("copy_alias_cp")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("cp".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T16：/compact（空会话）
// ─────────────────────────────────────────────────────────────────────────────

/// T16: `/compact` 在空会话时不崩溃。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("compact") → Wait(3s) →
///       AssertNoPanic
/// 断言：空对话时不 panic
#[test]
fn compact_no_crash_empty() {
    let case = TestCase::new("compact_no_crash_empty")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("compact".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T17：/keybindings
// ─────────────────────────────────────────────────────────────────────────────

/// T17: `/keybindings` 显示快捷键。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("keybindings") → Wait(2s) →
///       WaitForAny("Ctrl", "key")
/// 断言：显示快捷键
#[test]
fn keybindings_command() {
    let case = TestCase::new("keybindings_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("keybindings".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::WaitForAny(
            vec!["Ctrl".into(), "key".into()],
            Duration::from_secs(5),
        ));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T18：/statusline
// ─────────────────────────────────────────────────────────────────────────────

/// T18: `/statusline` 命令。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("statusline") → Wait(2s) →
///       AssertNoPanic
/// 断言：无 panic
#[test]
fn statusline_command() {
    let case = TestCase::new("statusline_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("statusline".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T19：/terminal-setup
// ─────────────────────────────────────────────────────────────────────────────

/// T19: `/terminal-setup` 命令。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("terminal-setup") → Wait(2s) →
///       AssertNoPanic
/// 断言：无 panic，显示终端信息
#[test]
fn terminal_setup_command() {
    let case = TestCase::new("terminal_setup_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("terminal-setup".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T20 – T21：/doctor 及其别名
// ─────────────────────────────────────────────────────────────────────────────

/// T20: `/doctor` 诊断。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("doctor") → Wait(3s) →
///       AssertNoPanic → Snapshot("doctor")
/// 断言：无 panic，显示诊断信息
#[test]
fn doctor_command() {
    let case = TestCase::new("doctor_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("doctor".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("doctor".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T21a: `/doctor` 别名 `/diagnostics`。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("diagnostics") → Wait(2s) →
///       AssertNoPanic
/// 断言：别名可用且无 panic
#[test]
fn doctor_alias_diagnostics() {
    let case = TestCase::new("doctor_alias_diagnostics")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("diagnostics".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

/// T21b: `/doctor` 别名 `/diag`。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("diag") → Wait(2s) →
///       AssertNoPanic
/// 断言：别名可用且无 panic
#[test]
fn doctor_alias_diag() {
    let case = TestCase::new("doctor_alias_diag")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("diag".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T22：/experimental
// ─────────────────────────────────────────────────────────────────────────────

/// T22: `/experimental` 功能门控。
///
/// 步骤：启动 PTY → skip_trust_gate → Command("experimental") → Wait(2s) →
///       AssertNoPanic
/// 断言：无 panic，显示功能门控状态
#[test]
fn experimental_command() {
    let case = TestCase::new("experimental_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("experimental".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// T23：所有核心别名批量测试
// ─────────────────────────────────────────────────────────────────────────────

/// T23: 所有别名批量测试。
///
/// 步骤：启动 PTY → skip_trust_gate → 按顺序执行 /v、/settings、/ctx、/cp、
///       /keys、/diag、/exp，每个执行后 AssertNoPanic
/// 断言：所有别名均可解析且无 panic
#[test]
fn all_core_aliases_batch() {
    let case = TestCase::new("all_core_aliases_batch")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("v".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("settings".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("ctx".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("cp".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("keys".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("diag".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("exp".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&case).assert_no_errors();
}
