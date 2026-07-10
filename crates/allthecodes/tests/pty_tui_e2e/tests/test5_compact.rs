#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// 测试：/compact 在有对话内容后的行为。
///
/// 流程：启动 TUI（bypass 权限模式）→ 发送一条简单消息 →
///       等待回复 → /compact → 验证 compact 输出 → 检查无 panic
#[test]
#[ignore = "requires real API key"]
fn script_compact_after_conversation() {
    let case = TestCase::new("compact_after_conversation")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Input("Say exactly: COMPACT_TEST_MARKER".into()))
        .step(TestStep::WaitResponseDone(API_TIMEOUT))
        .step(TestStep::Snapshot("conversation_created".into()))
        .step(TestStep::Command("compact".into()))
        .step(TestStep::WaitForAny(
            vec![
                "Compacted".into(),
                "No compaction needed".into(),
                "Conversation compacted".into(),
                "tokens".into(),
            ],
            Duration::from_secs(30),
        ))
        .step(TestStep::Snapshot("compact_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// 测试：/compact 在空对话环境下的行为（刚启动，无对话历史）。
///
/// 流程：启动 TUI（bypass 权限模式）→ 立即 /compact → 验证响应
#[test]
#[ignore = "requires real API key"]
fn script_compact_empty() {
    let case = TestCase::new("compact_empty")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Command("compact".into()))
        .step(TestStep::WaitForAny(
            vec![
                "Nothing to compact".into(),
                "empty".into(),
                "Compacted".into(),
                "No compaction needed".into(),
            ],
            Duration::from_secs(15),
        ))
        .step(TestStep::Snapshot("compact_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
