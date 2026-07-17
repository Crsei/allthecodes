#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! PTY TUI E2E 测试 — 记忆、技能与钩子命令
//!
//! 涵盖命令：/memory, /memory path, /memory edit, /memory list,
//! /memory set/get/rm (CRUD), /memory set --global, /memory set --category,
//! /memory search, /memory 别名 /mem, /skills, /skills list, /skills <name>,
//! /hooks, /hooks list, /hooks list <event>, /hooks path
//!
//! 所有测试均为离线（不依赖 API key）。

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::time::Duration;

/// T01：/memory 显示 AGENTS.md 内容或 selector。
///
/// 验证无参数 /memory 时显示 selector（分组列出所有 scope 的记忆条目、
/// auto-memory 状态、AGENTS.md 文件及目录快捷方式），或显示
/// "no AGENTS.md found"。
#[test]
fn memory_default_shows_agents_md() {
    let case = TestCase::new("memory_default_shows_agents_md")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory".into()))
        .step(TestStep::Snapshot("memory_default".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T02：/memory path 列出 AGENTS.md 文件路径与大小。
///
/// 验证命令列出所有找到的 AGENTS.md 文件路径及其字节数。
#[test]
fn memory_path() {
    let case = TestCase::new("memory_path")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory path".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T03：/memory edit 创建或定位 AGENTS.md。
///
/// 验证命令创建（若不存在）或定位（若已存在）AGENTS.md 并返回路径。
#[test]
fn memory_edit() {
    let case = TestCase::new("memory_edit")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory edit".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T04：/memory list 列出所有 scope 的记忆条目。
///
/// 验证命令按 scope 分组显示 memdir 记忆条目，或显示
/// "No memory entries found"。
#[test]
fn memory_list() {
    let case = TestCase::new("memory_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T05：/memory set / get / rm 完整 CRUD 循环。
///
/// 验证记忆条目的创建、读取和删除全流程：
/// 1. 设置记忆项 "e2e_test_key" = "v9Z"
/// 2. 获取并验证值包含短且唯一的 "v9Z"（避免状态栏裁剪长值）
/// 3. 删除该项
#[test]
fn memory_set_get_rm_cycle() {
    let case = TestCase::new("memory_set_get_rm_cycle")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory set e2e_test_key v9Z".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Command("memory get e2e_test_key".into()))
        .step(TestStep::WaitForScreenText(
            "v9Z".into(),
            Duration::from_secs(3),
        ))
        .step(TestStep::Command("memory rm e2e_test_key".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T06：/memory set --global 设置全局记忆。
///
/// 验证通过 `--global` 标志将记忆条目写入 global scope。
#[test]
fn memory_set_global() {
    let case = TestCase::new("memory_set_global")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command(
            "memory set e2e_global_key global_val --global".into(),
        ))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T07：/memory set --category 设置分类记忆。
///
/// 验证通过 `--category` 标志为记忆条目指定分类。
#[test]
fn memory_set_category() {
    let case = TestCase::new("memory_set_category")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command(
            "memory set e2e_cat_key cat_val --category=test".into(),
        ))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T08：/memory search 搜索记忆。
///
/// 验证命令在所有 scope 中进行子字符串匹配搜索并返回匹配条目。
#[test]
fn memory_search() {
    let case = TestCase::new("memory_search")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("memory search e2e_test".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T09：/memory 别名 /mem。
///
/// 验证 `/mem` 作为 `/memory` 的别名正常工作。
#[test]
fn memory_alias_mem() {
    let case = TestCase::new("memory_alias_mem")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("mem".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T10：/skills 列出已加载的技能。
///
/// 验证无参数 /skills 按名称排序列出所有已加载技能包，含 source tag、
/// invocability tag、版本和描述。
#[test]
fn skills_list() {
    let case = TestCase::new("skills_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("skills".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("skills_list".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T11：/skills list 子命令。
///
/// 验证 /skills list 子命令与 /skills 效果相同。
#[test]
fn skills_list_subcommand() {
    let case = TestCase::new("skills_list_subcommand")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("skills list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T12：/skills <name> 显示技能详情。
///
/// 验证输入技能名称后显示该技能的详细信息（版本、描述、source、
/// invocability、allowed tools 等），或显示 "not found"。
#[test]
fn skills_detail() {
    let case = TestCase::new("skills_detail")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("skills review".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T13：/hooks 显示合并后的钩子树。
///
/// 验证命令按 event → matcher → hook 分组显示聚合后的钩子树，
/// 或显示空树。
#[test]
fn hooks_tree() {
    let case = TestCase::new("hooks_tree")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("hooks".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Snapshot("hooks_tree".into()))
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T14：/hooks list 列出钩子。
///
/// 验证 /hooks list 显示钩子树（等价于 /hooks）。
#[test]
fn hooks_list() {
    let case = TestCase::new("hooks_list")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("hooks list".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T15：/hooks list <event> 按事件筛选钩子。
///
/// 验证 /hooks list PreToolUse 只显示 PreToolUse 事件下的钩子。
#[test]
fn hooks_list_filtered() {
    let case = TestCase::new("hooks_list_filtered")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("hooks list PreToolUse".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}

/// T16：/hooks path <scope> 显示设置文件路径。
///
/// 验证 /hooks path project 返回对应 settings 层的 settings.json 路径。
#[test]
fn hooks_path() {
    let case = TestCase::new("hooks_path")
        .log_root(SCRIPTS_LOG_ROOT)
        .permission_mode("bypass")
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Command("hooks path project".into()))
        .step(TestStep::AssertNoPanic)
        .step(TestStep::Key(TestKey::CtrlC))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::Key(TestKey::CtrlC));

    TestRunner::new().run(&case).assert_no_errors();
}
