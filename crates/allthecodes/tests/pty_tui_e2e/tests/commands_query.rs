#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ─── Online tests (require real API key) ────────────────────────────────

/// T01: /review 生成 PR 审查查询。
///
/// 注入 PR 审查提示词到会话中，引导模型通过 gh CLI 获取 PR 上下文并执行代码审查。
/// 步骤：启动 TUI → 设置 full access 权限 → /review → 等待模型处理 → 验证无 panic。
#[test]
#[ignore = "requires real API key"]
fn review_command_generates_query() {
    let case = TestCase::new("review_command_generates_query")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("review".into()))
        .step(TestStep::Wait(Duration::from_secs(10)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("review_output".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02: /security-review 生成安全审查查询。
///
/// 收集当前 git 仓库的分支名、工作区状态和最近提交记录，拼装成安全审查提示词注入会话。
/// 步骤：启动 TUI → 设置 full access 权限 → /security-review → 等待模型处理 → 验证无 panic。
#[test]
#[ignore = "requires real API key"]
fn security_review_command() {
    let case = TestCase::new("security_review_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("security-review".into()))
        .step(TestStep::Wait(Duration::from_secs(10)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("security_review_output".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03: /security-review 别名 /secreview。
///
/// 验证 /secreview 作为 /security-review 的别名可用，行为完全相同。
/// 步骤：启动 TUI → 设置 full access 权限 → /secreview → 等待模型处理 → 验证无 panic。
#[test]
#[ignore = "requires real API key"]
fn security_review_alias() {
    let case = TestCase::new("security_review_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("secreview".into()))
        .step(TestStep::Wait(Duration::from_secs(10)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("secreview_output".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04: /recap 总结会话。
///
/// 先与模型对话建立会话上下文，然后通过 /recap 请求会话摘要。
/// 步骤：启动 TUI → 设置 full access 权限 → 发送消息 "Hello, my name is TestBot" →
///       等待模型回复 → /recap → 等待模型生成摘要 → 验证无 panic。
#[test]
#[ignore = "requires real API key"]
fn recap_command() {
    let case = TestCase::new("recap_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Input("Hello, my name is TestBot".into()))
        .step(TestStep::WaitForText("TestBot".into(), API_TIMEOUT))
        .step(TestStep::Snapshot("conversation_created".into()))
        .step(TestStep::Command("recap".into()))
        .step(TestStep::Wait(Duration::from_secs(10)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("recap_output".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ─── Offline tests (no API key needed) ──────────────────────────────────

/// T05: /loop 帮助。
///
/// 验证 /loop 无参数时显示帮助文本（列出子命令和用法），不触发任何 API 调用。
/// 步骤：启动 TUI → /loop → 等待渲染 → 验证无 panic → 截图保存。
#[test]
fn loop_command_help() {
    let case = TestCase::new("loop_command_help")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("loop".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("loop_help".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06: /schedule 帮助。
///
/// 验证 /schedule 无参数时显示帮助/列表文本（显示任务列表或用法提示），
/// 不触发任何 API 调用。
/// 步骤：启动 TUI → /schedule → 等待渲染 → 验证无 panic → 截图保存。
#[test]
fn schedule_command_help() {
    let case = TestCase::new("schedule_command_help")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("schedule".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("schedule_help".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07: /schedule 别名 /cron。
///
/// 验证 /cron 作为 /schedule 的别名可用，行为完全相同。
/// 步骤：启动 TUI → /cron → 等待渲染 → 验证无 panic → 截图保存。
#[test]
fn schedule_alias_cron() {
    let case = TestCase::new("schedule_alias_cron")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("cron".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("cron_help".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
