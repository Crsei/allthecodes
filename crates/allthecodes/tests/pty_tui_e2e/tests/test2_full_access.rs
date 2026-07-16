#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// 测试 2: 先设置 `/permissions full access`，再询问相同项目结构，验证无需对话框即可正常运行。
///
/// 流程：启动 TUI（default 权限模式）→ /permissions full access → 输入相同问题 →
///       验证直接运行（无权限对话框）→ 中止
#[test]
#[ignore = "requires a real API key and network"]
fn script_full_access_structure_no_dialog() {
    let ws = "/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun";
    let case = TestCase::new("full_access_structure_no_dialog")
        .log_root(SCRIPTS_LOG_ROOT)
        .workspace(ws)
        .permission_mode("default")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::SetPermission("full access".into()))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("after_permission".into()))
        .step(TestStep::Input(
            "List the files and directories in this workspace. Use Bash to run: ls".into(),
        ))
        .step(TestStep::WaitForAny(
            vec![
                "Cargo.toml".into(),
                "src/".into(),
                "package.json".into(),
                "index.ts".into(),
            ],
            API_TIMEOUT,
        ))
        .step(TestStep::Snapshot("structure_result".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
