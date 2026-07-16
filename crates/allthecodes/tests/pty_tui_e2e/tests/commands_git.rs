#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

// ========================================================================
// T01 – /diff 显示变更
// ========================================================================

/// T01: `/diff` — 显示当前工作区的变更（staged + unstaged）。
///
/// 使用 bypass 模式，无需 API key。输出可能包含实际 diff 内容或 "No changes detected."。
#[test]
fn script_diff_command() {
    let case = TestCase::new("diff_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("diff".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("diff_output".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T02 – /diff --staged 仅暂存
// ========================================================================

/// T02: `/diff --staged` — 仅显示暂存的变更。
///
/// 使用 bypass 模式，无需 API key。输出可能包含 staged diff 或 "No changes detected."。
#[test]
fn script_diff_staged() {
    let case = TestCase::new("diff_staged")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("diff --staged".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("diff_staged_output".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T03 – /diff --cached（--staged 的别名）
// ========================================================================

/// T03: `/diff --cached` — `--staged` 的别名，行为应完全一致。
///
/// 使用 bypass 模式，无需 API key。
#[test]
fn script_diff_cached_alias() {
    let case = TestCase::new("diff_cached_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("diff --cached".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("diff_cached_output".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T04 – /commit 带消息（离线）
// ========================================================================

/// T04: `/commit test: e2e commit message` — 带消息直接执行 git commit。
///
/// 注意：此测试会修改 git 状态（创建提交）。使用临时工作区可避免影响主仓库。
/// 若无暂存变更，git 会报 "nothing to commit"，命令仍正常返回（无 panic）。
#[test]
fn script_commit_with_message() {
    let case = TestCase::new("commit_with_message")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("commit test: e2e commit message".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("commit_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T05 – /commit 不带消息（在线 — 生成 Query）
// ========================================================================

/// T05: `/commit` — 不带消息，生成 Query 让模型生成 commit message。
///
/// 需要真实 API key，因为命令的输出是构造 Query 发给模型处理。
#[test]
#[ignore = "requires real API key"]
fn script_commit_no_message() {
    let case = TestCase::new("commit_no_message")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("commit".into()))
        .step(TestStep::Wait(Duration::from_secs(5)))
        .step(TestStep::Snapshot("commit_no_message_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T06 – /gbranch 显示 git 分支
// ========================================================================

/// T06: `/gbranch` — 显示本地 git 分支列表。
///
/// 使用 bypass 模式，无需 API key。输出应包含分支名和当前分支标记 `*`。
#[test]
fn script_gbranch_command() {
    let case = TestCase::new("gbranch_command")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("gbranch".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("gbranch_output".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T07 – /gbranch 别名 /gitbranch
// ========================================================================

/// T07: `/gitbranch` — `/gbranch` 的别名，应同样可用。
///
/// 使用 bypass 模式，无需 API key。
#[test]
fn script_gbranch_alias() {
    let case = TestCase::new("gbranch_alias")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("gitbranch".into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Snapshot("gitbranch_output".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

// ========================================================================
// T08 – /branch 分叉对话（在线）
// ========================================================================

/// T08: `/branch test-branch` — 传参时提示使用 /gbranch，不实际分叉。
///
/// 需要先有对话内容（调用 API 获取响应），然后验证 /branch 对参数的响应。
#[test]
#[ignore = "requires real API key"]
fn script_branch_forks_session() {
    let case = TestCase::new("branch_forks_session")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(1)))
        .step(TestStep::Input("Say exactly: BRANCH_MARKER_42".into()))
        .step(TestStep::WaitForText(
            "BRANCH_MARKER_42".into(),
            API_TIMEOUT,
        ))
        .step(TestStep::Snapshot("conversation_created".into()))
        .step(TestStep::Command("branch test-branch".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Snapshot("after_branch".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
