#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// 测试：/plan 进入 plan mode，输入需求，等待模型响应，退出 plan mode。
///
/// 验证 plan mode 的基本流程：进入、输入 prompt、模型思考/响应、无 panic。
#[test]
#[ignore = "requires real API key"]
fn script_plan_flow() {
    let case = TestCase::new("plan_flow")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .timeout(Duration::from_secs(300))
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        // 先进入 plan mode（不带 prompt，避免命令面板拦截）
        .step(TestStep::Command("plan".into()))
        // 等待 plan mode 进入（plan mode 状态栏或 Read/Glob 等工具出现）
        .step(TestStep::WaitForAny(
            vec![
                "Plan mode".into(),
                "plan.md".into(),
                "Read".into(),
                "Glob".into(),
            ],
            Duration::from_secs(60),
        ))
        .step(TestStep::Snapshot("plan_entered".into()))
        // 输入计划需求提示
        .step(TestStep::Input(
            "Create a simple plan for adding a hello world function. \
             Use TaskCreate to define one task."
                .into(),
        ))
        // 等待模型开始处理（状态栏变为 BUSY 或出现 Thinking 动画）
        .step(TestStep::WaitForAny(
            vec![
                "BUSY".into(),
                "Thinking".into(),
                "plan.md".into(),
                "TaskCreate".into(),
            ],
            Duration::from_secs(120),
        ))
        .step(TestStep::Snapshot("plan_processing".into()))
        // 退出 plan mode
        .step(TestStep::Command("plan exit".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Snapshot("plan_exited".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
