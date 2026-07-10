#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// 测试：Claude 创建任务（TaskCreate/TodoWrite）并可能调用子代理（Agent 工具）。
///
/// 流程：启动 TUI（bypass 权限模式）→ 输入任务创建提示 →
///       等待任务相关工具调用 → 等待子代理或执行 → 验证无 panic
#[test]
#[ignore = "requires real API key"]
fn script_task_execution_with_subagent() {
    let case = TestCase::new("task_execution_with_subagent")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .timeout(Duration::from_secs(300))
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::Input(
            "Create a plan with 3 implementation tasks for adding a greeting function. \
             Break it into: 1) design, 2) implement, 3) test. \
             Use TaskCreate for each task."
                .into(),
        ))
        // 等待任务创建或子代理调用
        .step(TestStep::WaitForAny(
            vec![
                "TaskCreate".into(),
                "Updated todos".into(),
                "[ ]".into(),
                "[x]".into(),
                "Created task".into(),
                "Agent".into(),
            ],
            Duration::from_secs(180),
        ))
        .step(TestStep::Snapshot("tasks_created".into()))
        // 等待任务执行或子代理完成
        .step(TestStep::Wait(Duration::from_secs(10)))
        .step(TestStep::WaitForAny(
            vec![
                "Agent".into(),
                "[x]".into(),
                "completed".into(),
                "Bash".into(),
                "implement".into(),
            ],
            Duration::from_secs(120),
        ))
        .step(TestStep::Snapshot("execution".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
