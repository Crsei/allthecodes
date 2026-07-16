#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ─── 离线测试（T01-T14）：代理 / 团队 / 任务 / 协调器 / 计划 ──────

/// T01: /agents — 列出代理（按来源分组，如 built-in、user、MCP 等）。
#[test]
fn agents_list() {
    let case = TestCase::new("agents_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("agents".into()))
        .step(TestStep::WaitForScreenText("agent".into(), Duration::from_secs(3)))
        .step(TestStep::Snapshot("agents_list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02: /agents list — 子命令 list 列出代理。
#[test]
fn agents_list_subcommand() {
    let case = TestCase::new("agents_list_subcommand")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("agents list".into()))
        .step(TestStep::Snapshot("agents_list_subcommand".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03: /agents show default — 显示默认代理详情或 "not found"。
#[test]
fn agents_show() {
    let case = TestCase::new("agents_show")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("agents show default".into()))
        .step(TestStep::Snapshot("agents_show_detail".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: /agents sources — 显示代理加载路径与来源分组。
#[test]
fn agents_sources() {
    let case = TestCase::new("agents_sources")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("agents sources".into()))
        .step(TestStep::Snapshot("agents_sources".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05: /team — 显示团队状态（无参数在 Rust TUI 中打开 TeamSurface）。
#[test]
fn team_status() {
    let case = TestCase::new("team_status")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("team".into()))
        .step(TestStep::Snapshot("team_status".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06: /team list — 列出团队或显示 "none"。
#[test]
fn team_list() {
    let case = TestCase::new("team_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("team list".into()))
        .step(TestStep::Snapshot("team_list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07: /team create + /team delete — 创建并删除测试团队。
#[test]
fn team_create_and_delete() {
    let case = TestCase::new("team_create_and_delete")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command(
            "team create test-e2e-team E2E test team".into(),
        ))
        .step(TestStep::Snapshot("team_create".into()))
        .step(TestStep::Command("team delete test-e2e-team".into()))
        .step(TestStep::Snapshot("team_delete".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08: /team-onboarding — 生成 teammate 入手指南。
#[test]
fn team_onboarding() {
    let case = TestCase::new("team_onboarding")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("team-onboarding".into()))
        .step(TestStep::Snapshot("team_onboarding".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09: /tasks — 列出后台任务（tool tasks + team tasks）。
#[test]
fn tasks_list() {
    let case = TestCase::new("tasks_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("tasks".into()))
        .step(TestStep::Snapshot("tasks_list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10: /tasks show <invalid> — 不存在的任务 ID 应显示 "not found"。
#[test]
fn tasks_show_invalid() {
    let case = TestCase::new("tasks_show_invalid")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("tasks show nonexistent-task-123".into()))
        .step(TestStep::Snapshot("tasks_show_invalid".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11: /tasks delete <invalid> — 删除不存在的任务应显示 "not found"。
#[test]
fn tasks_delete_invalid() {
    let case = TestCase::new("tasks_delete_invalid")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command(
            "tasks delete nonexistent-task-123".into(),
        ))
        .step(TestStep::Snapshot("tasks_delete_invalid".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T12: /coordinator — 显示协调器状态。
#[test]
fn coordinator_command() {
    let case = TestCase::new("coordinator_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("coordinator".into()))
        .step(TestStep::Snapshot("coordinator_command".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T13: /coord (alias) — 验证 /coordinator 别名可用。
#[test]
fn coordinator_alias() {
    let case = TestCase::new("coordinator_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("coord".into()))
        .step(TestStep::Snapshot("coordinator_alias".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T14: /plan — 进入计划模式。
#[test]
fn plan_command() {
    let case = TestCase::new("plan_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("plan".into()))
        .step(TestStep::Snapshot("plan_command".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── 在线测试（T15-T16）：需要真实 API Key ─────────────────────────

/// T15: /btw <question> — 分叉代理用于附带问题（单轮、无工具）。
#[test]
#[ignore = "requires real API key"]
fn btw_command() {
    let case = TestCase::new("btw_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .timeout(Duration::from_secs(90))
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("btw what is 2+2?".into()))
        .step(TestStep::WaitForText("4".into(), API_TIMEOUT))
        .step(TestStep::Snapshot("btw_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T16: /simplify — 开始多代理代码简化审查。
#[test]
#[ignore = "requires real API key"]
fn simplify_command() {
    let case = TestCase::new("simplify_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .timeout(Duration::from_secs(120))
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("simplify".into()))
        .step(TestStep::Snapshot("simplify_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
