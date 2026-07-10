#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// T01：/logout — 清理认证凭据与 onboarding 状态。
///
/// 离线命令，不依赖 API key。
///
/// 步骤：
/// 1. Command("logout")
/// 2. Wait(2s)
/// 3. AssertNoPanic
/// 4. Snapshot("logout_result")
///
/// 断言：无 panic，显示退出确认
#[test]
fn logout_command() {
    let case = TestCase::new("logout_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("logout".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("logout_result".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02：/advisor — 显示当前 advisor 模型或帮助信息。
///
/// 离线命令，不依赖 API key。
///
/// 步骤：
/// 1. Command("advisor")
/// 2. Wait(2s)
/// 3. AssertNoPanic
///
/// 断言：显示当前 advisor 模型或 "(unset)"
#[test]
fn advisor_show() {
    let case = TestCase::new("advisor_show")
        .log_root(SCRIPTS_LOG_ROOT)
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("advisor".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("advisor_show".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03：/advisor set <model> — 设置 advisor 模型。
///
/// 离线命令，不依赖 API key。
///
/// 步骤：
/// 1. Command("advisor set gpt-5.5")
/// 2. Wait(2s)
/// 3. AssertNoPanic
///
/// 断言：advisor 模型已设置
#[test]
fn advisor_set_model() {
    let case = TestCase::new("advisor_set_model")
        .log_root(SCRIPTS_LOG_ROOT)
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("advisor set gpt-5.5".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("advisor_set_model".into()));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04：/advisor clear — 清除 advisor 模型。
///
/// 离线命令，不依赖 API key。
///
/// 步骤：
/// 1. Command("advisor clear")
/// 2. Wait(2s)
/// 3. AssertNoPanic
///
/// 断言：advisor 已清除
#[test]
fn advisor_clear() {
    let case = TestCase::new("advisor_clear")
        .log_root(SCRIPTS_LOG_ROOT)
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Command("advisor clear".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("advisor_clear".into()));

    TestRunner::new().run(&case).assert_no_errors();
}
