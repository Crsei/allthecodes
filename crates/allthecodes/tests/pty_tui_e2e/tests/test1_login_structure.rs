#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use crate::harness::API_TIMEOUT;
use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// 测试 1: `/login claude-code` 登录后询问项目结构，检查权限对话框是否弹出，批准后验证正常运行。
///
/// 流程：启动 TUI（default 权限模式）→ /login claude-code → 输入问题 →
///       等待权限对话框或响应 → 批准对话框 → 验证输出 → 关闭
#[test]
fn script_login_structure_with_permissions() {
    let ws = "/data2-HDD-SATA-20T/Digital_avatar/haoweiyao/claude-code-bun";
    let case = TestCase::new("login_structure_with_permissions")
        .log_root(SCRIPTS_LOG_ROOT)
        .workspace(ws)
        .permission_mode("default")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Snapshot("initial".into()))
        .step(TestStep::LoginSwitch("claude-code".into()))
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Snapshot("after_login".into()))
        .step(TestStep::Input(
            "List the files and directories in this workspace. Use Bash to run: ls".into(),
        ))
        .step(TestStep::WaitForAny(
            vec![
                "Permission".into(),
                "permission".into(),
                "Cargo.toml".into(),
                "src/".into(),
            ],
            Duration::from_secs(30),
        ))
        .step(TestStep::Snapshot("dialog_or_response".into()))
        .step(TestStep::ApproveDialog)
        .step(TestStep::Wait(Duration::from_secs(3)))
        .step(TestStep::Snapshot("after_approve".into()))
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
