#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! CommandSurface PTY E2E tests.
//!
//! These tests exercise the interactive slash-command surfaces in a real PTY.
//! Snapshots are still kept as debugging artifacts, but every case asserts a
//! behaviorally meaningful screen, prompt, output, or fixture-file state.

use crate::script::{TestCase, TestKey, TestRunner, TestStep};
use crate::tests::SCRIPTS_LOG_ROOT;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

const SURFACE_WAIT: Duration = Duration::from_secs(3);
const SHORT_WAIT: Duration = Duration::from_millis(300);

struct SurfaceCase<'a> {
    name: &'a str,
    input: &'a str,
    title: &'a str,
    expected: &'a [&'a str],
}

struct SurfaceFixture {
    _workspace: TempDir,
    _home: TempDir,
    settings_path: PathBuf,
    case: TestCase,
}

impl SurfaceFixture {
    fn step(mut self, step: TestStep) -> Self {
        self.case = self.case.step(step);
        self
    }

    fn assert_file_not_contains(self, text: &str) -> Self {
        let path = self.settings_path.display().to_string();
        self.step(TestStep::AssertFileNotContains(path, text.to_string()))
    }
}

fn isolated_surface_case(name: &str) -> SurfaceFixture {
    let workspace = TempDir::new().expect("create command surface workspace");
    let home = TempDir::new().expect("create command surface home");
    let project_config_dir = workspace.path().join(".allthecodes");
    fs::create_dir_all(&project_config_dir).expect("create project .allthecodes dir");
    let settings_path = project_config_dir.join("settings.json");
    fs::write(&settings_path, "{}").expect("write project settings fixture");
    fs::write(home.path().join("settings.json"), "{}").expect("write home settings fixture");

    let case = TestCase::new(name)
        .log_root(SCRIPTS_LOG_ROOT)
        .timeout(Duration::from_secs(30))
        .cols(180)
        .rows(60)
        .workspace(workspace.path().display().to_string())
        .env("ALLTHECODES_HOME", home.path().display().to_string())
        .step(TestStep::SkipTrustGate)
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::Wait(Duration::from_secs(2)))
        .step(TestStep::Wait(Duration::from_millis(500)));

    SurfaceFixture {
        _workspace: workspace,
        _home: home,
        settings_path,
        case,
    }
}

fn mcp_surface_case(name: &str) -> SurfaceFixture {
    let mut fixture = isolated_surface_case(name);
    let project_config_dir = fixture
        .settings_path
        .parent()
        .expect("settings path has parent");
    write_mcp_fixture(project_config_dir, "db", true);
    fixture.case = open_surface_steps(fixture.case, "mcp", "┌ MCP ");
    fixture
}

fn write_mcp_fixture(project_config_dir: &Path, server_name: &str, disabled: bool) {
    let disabled = if disabled { "true" } else { "false" };
    fs::write(
        project_config_dir.join("settings.json"),
        format!(
            r#"{{
  "mcpServers": {{
    "{server_name}": {{
      "type": "stdio",
      "command": "node",
      "args": ["db-server.js"],
      "disabled": {disabled}
}}}}}}
"#
        ),
    )
    .expect("write project MCP fixture");
}

fn open_surface_steps(case: TestCase, cmd: &str, title: &str) -> TestCase {
    case.step(TestStep::Command(cmd.into()))
        .step(TestStep::Wait(Duration::from_millis(500)))
        .step(TestStep::WaitForScreenText(title.into(), SURFACE_WAIT))
}

fn open_surface_case(name: &str, cmd: &str, title: &str) -> SurfaceFixture {
    let mut fixture = isolated_surface_case(name);
    fixture.case = open_surface_steps(fixture.case, cmd, title);
    fixture
}

fn assert_surface_open(name: &str, cmd: &str, title: &str, expected: &[&str]) {
    assert_surface_open_case(SurfaceCase {
        name,
        input: cmd,
        title,
        expected,
    });
}

fn assert_surface_open_case(case: SurfaceCase<'_>) {
    let mut fixture = open_surface_case(case.name, case.input, case.title);
    for text in case.expected {
        fixture = fixture.step(TestStep::AssertScreenContains((*text).to_string()));
    }
    fixture = fixture
        .step(TestStep::Snapshot(format!("{}_surface", case.name)))
        .step(TestStep::AssertNoPanic);
    TestRunner::new().run(&fixture.case).assert_no_errors();
}

fn assert_command_with_args_does_not_open_surface(input: &str, forbidden_title: &str) {
    let name = format!("surface_args_do_not_open_{}", input.replace(' ', "_"));
    let fixture = isolated_surface_case(&name)
        .step(TestStep::Command(input.into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenNotContains(forbidden_title.into()))
        .step(TestStep::Snapshot(format!("{name}_screen")))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

// ─────────────────────────────────────────────────────────────────────────────
// Open-state coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn palette_space_enters_command_arguments() {
    let fixture = isolated_surface_case("palette_space_enters_command_arguments")
        .step(TestStep::OpenPalette)
        .step(TestStep::AssertScreenContains("Enter run".into()))
        .step(TestStep::AssertScreenContains("Space add args".into()))
        .step(TestStep::TypeText("mc".into()))
        .step(TestStep::TypeText(" ".into()))
        .step(TestStep::AssertPromptContains("/mcp ".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_agents_open_content() {
    assert_surface_open(
        "surface_agents",
        "agents",
        "┌ Agents ",
        &["source=Agents", "Create new agent", "Left/Right source"],
    );
}

#[test]
fn surface_config_open_content() {
    assert_surface_open(
        "surface_config",
        "config",
        "┌ Config ",
        &["Show effective config", "model=", "backend="],
    );
}

#[test]
fn surface_diff_open_content() {
    assert_surface_open(
        "surface_diff",
        "diff",
        "Diff",
        &["Diff", "Not a git repository"],
    );
}

#[test]
fn surface_effort_open_content() {
    assert_surface_open(
        "surface_effort",
        "effort",
        "Effort",
        &["Effort / Filter", "Thinking", "current"],
    );
}

#[test]
fn surface_hooks_open_content() {
    assert_surface_open(
        "surface_hooks",
        "hooks",
        "┌ Hooks ",
        &["mode=events", "PreToolUse", "0 hooks configured"],
    );
}

#[test]
fn surface_login_open_content() {
    assert_surface_open(
        "surface_login",
        "login",
        "┌ Login / allthecodes ",
        &[
            "profiles=claude-code,codex,custom",
            "/login status",
            "shortcut letter",
        ],
    );
}

#[test]
fn surface_mcp_open_content() {
    let fixture = mcp_surface_case("surface_mcp")
        .step(TestStep::AssertScreenContains("MCP servers (1)".into()))
        .step(TestStep::AssertScreenContains("db".into()))
        .step(TestStep::AssertScreenContains("Status".into()))
        .step(TestStep::AssertScreenContains("Edit".into()))
        .step(TestStep::AssertScreenContains("Reconnect".into()))
        .step(TestStep::AssertScreenContains("Remove".into()))
        .step(TestStep::Snapshot("surface_mcp_surface".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_memory_open_content() {
    assert_surface_open(
        "surface_memory",
        "memory",
        "┌ Memory ",
        &["action=Edit", "User memory", "/memory edit"],
    );
}

#[test]
fn surface_model_open_content() {
    assert_surface_open(
        "surface_model",
        "model",
        "┌ Model ",
        &["Select the active model", "Model / Filter", "current"],
    );
}

#[test]
fn surface_permissions_open_content() {
    assert_surface_open(
        "surface_permissions",
        "permissions",
        "┌ Permissions ",
        &["mode=", "Default", "Full Access"],
    );
}

#[test]
fn surface_plugin_open_content() {
    assert_surface_open(
        "surface_plugin",
        "plugin",
        "┌ Plugins ",
        &["No installed plugins", "installed_plugins.json", "r reload"],
    );
}

#[test]
fn surface_remote_open_content() {
    assert_surface_open(
        "surface_remote",
        "remote",
        "Remote control gateway",
        &["daemon:", "/remote status", "Left/Right tabs"],
    );
}

#[test]
fn surface_resume_open_content() {
    assert_surface_open(
        "surface_resume",
        "resume",
        "Resume Sessions",
        &["Workspace:", "Resume Sessions"],
    );
}

#[test]
fn surface_sandbox_open_content() {
    assert_surface_open(
        "surface_sandbox",
        "sandbox",
        "┌ Sandbox ",
        &["Show full status", "Network policy", "Left/Right section"],
    );
}

#[test]
fn surface_skills_open_content() {
    assert_surface_open(
        "surface_skills",
        "skills",
        "┌ Skills ",
        &["filter=", "enabled=true source=bundled", "Type filter"],
    );
}

#[test]
fn surface_tasks_open_content() {
    assert_surface_open(
        "surface_tasks",
        "tasks",
        "Background tasks",
        &["Background tasks (0)", "No tasks", "r refresh"],
    );
}

#[test]
fn surface_team_open_content() {
    assert_surface_open(
        "surface_team",
        "team",
        "┌ Team ",
        &["No active team", "/team create", "/team list"],
    );
}

#[test]
fn surface_alias_open_cases() {
    for case in [
        SurfaceCase {
            name: "surface_alias_perms",
            input: "perms",
            title: "┌ Permissions ",
            expected: &["mode=", "Default", "Full Access"],
        },
        SurfaceCase {
            name: "surface_alias_plugins",
            input: "plugins",
            title: "┌ Plugins ",
            expected: &["No installed plugins", "installed_plugins.json", "r reload"],
        },
        SurfaceCase {
            name: "surface_alias_teams",
            input: "teams",
            title: "┌ Team ",
            expected: &["No active team", "/team create", "/team list"],
        },
    ] {
        assert_surface_open_case(case);
    }
}

#[test]
fn commands_with_args_do_not_open_surfaces() {
    for (input, forbidden_title) in [
        ("permissions mode default", "┌ Permissions "),
        ("plugin list", "┌ Plugins "),
        ("team list", "┌ Team "),
        ("mcp status", "┌ MCP "),
        ("memory list", "┌ Memory "),
        ("hooks list", "┌ Hooks "),
        ("sandbox status", "┌ Sandbox "),
    ] {
        assert_command_with_args_does_not_open_surface(input, forbidden_title);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Interaction coverage
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn surface_escape_closes_overlay_and_returns_to_prompt() {
    let fixture = open_surface_case("surface_escape_closes_overlay", "config", "┌ Config ")
        .step(TestStep::Key(TestKey::Escape))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenNotContains("┌ Config ".into()))
        .step(TestStep::TypeText("after-escape".into()))
        .step(TestStep::AssertPromptContains("after-escape".into()))
        .step(TestStep::Snapshot("surface_escape_closed".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_alias_escape_closes_overlay_and_returns_to_prompt() {
    let fixture = open_surface_case(
        "surface_alias_escape_closes_overlay",
        "perms",
        "┌ Permissions ",
    )
    .step(TestStep::Key(TestKey::Escape))
    .step(TestStep::Wait(SHORT_WAIT))
    .step(TestStep::AssertScreenNotContains("┌ Permissions ".into()))
    .step(TestStep::TypeText("after-alias-escape".into()))
    .step(TestStep::AssertPromptContains("after-alias-escape".into()))
    .step(TestStep::Snapshot("surface_alias_escape_closed".into()))
    .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_config_tabbed_navigation_changes_sections() {
    let fixture = open_surface_case("surface_config_navigation", "config", "┌ Config ")
        .step(TestStep::AssertScreenContains("> Status".into()))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Model".into()))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Theme".into()))
        .step(TestStep::Key(TestKey::Left))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Model".into()))
        .step(TestStep::Snapshot("surface_config_navigation".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_alias_permissions_navigation_changes_selection() {
    let fixture = open_surface_case(
        "surface_alias_permissions_navigation",
        "perms",
        "┌ Permissions ",
    )
    .step(TestStep::AssertScreenContains("> Default".into()))
    .step(TestStep::Key(TestKey::Down))
    .step(TestStep::Wait(SHORT_WAIT))
    .step(TestStep::AssertScreenContains("> Auto-review".into()))
    .step(TestStep::Snapshot(
        "surface_alias_permissions_navigation".into(),
    ))
    .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_action_navigation_and_shortcuts() {
    let fixture = mcp_surface_case("surface_mcp_action_navigation")
        .step(TestStep::AssertScreenContains("> Status".into()))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Edit".into()))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Reconnect".into()))
        .step(TestStep::TypeText("v".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("Server details".into()))
        .step(TestStep::Key(TestKey::Left))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Status".into()))
        .step(TestStep::Snapshot("surface_mcp_action_navigation".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_skills_filter_and_clear() {
    let fixture = open_surface_case("surface_skills_filter", "skills", "┌ Skills ")
        .step(TestStep::TypeText("up".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("filter=up".into()))
        .step(TestStep::AssertScreenContains("update-config".into()))
        .step(TestStep::Key(TestKey::Backspace))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::Key(TestKey::Backspace))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("debug".into()))
        .step(TestStep::AssertScreenContains("remember".into()))
        .step(TestStep::Snapshot("surface_skills_filter".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_model_filter_selects_mini() {
    let fixture = open_surface_case("surface_model_filter", "model", "┌ Model ")
        .step(TestStep::TypeText("gpt-5.4-mini".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains(
            "Model / gpt-5.4-mini".into(),
        ))
        .step(TestStep::Key(TestKey::CtrlU))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("No profile models".into()))
        .step(TestStep::Snapshot("surface_model_filter".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_add_shortcut_fills_prompt() {
    let fixture = mcp_surface_case("surface_mcp_add_shortcut")
        .step(TestStep::TypeText("a".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertPromptContains("/mcp add ".into()))
        .step(TestStep::AssertScreenNotContains("┌ MCP ".into()))
        .step(TestStep::Snapshot("surface_mcp_add_shortcut".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_edit_action_fills_prompt() {
    let fixture = mcp_surface_case("surface_mcp_edit_action")
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Edit".into()))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertPromptContains("/mcp edit db ".into()))
        .step(TestStep::AssertScreenNotContains("┌ MCP ".into()))
        .step(TestStep::Snapshot("surface_mcp_edit_action".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_team_create_shortcut_fills_prompt() {
    let fixture = open_surface_case("surface_team_create_shortcut", "team", "┌ Team ")
        .step(TestStep::TypeText("c".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertPromptContains("/team create ".into()))
        .step(TestStep::AssertScreenNotContains("┌ Team ".into()))
        .step(TestStep::Snapshot("surface_team_create_shortcut".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_alias_team_create_shortcut_fills_prompt() {
    let fixture = open_surface_case("surface_alias_team_create_shortcut", "teams", "┌ Team ")
        .step(TestStep::TypeText("c".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertPromptContains("/team create ".into()))
        .step(TestStep::AssertScreenNotContains("┌ Team ".into()))
        .step(TestStep::Snapshot(
            "surface_alias_team_create_shortcut".into(),
        ))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_status_action_submits_status() {
    let fixture = mcp_surface_case("surface_mcp_status_action")
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::WaitForText(
            "MCP server status".into(),
            Duration::from_secs(3),
        ))
        .step(TestStep::AssertTextContains("db".into()))
        .step(TestStep::Snapshot("surface_mcp_status_action".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_login_status_action_submits_status() {
    let fixture = open_surface_case(
        "surface_login_status_action",
        "login",
        "┌ Login / allthecodes ",
    )
    .step(TestStep::Key(TestKey::Enter))
    .step(TestStep::WaitForAny(
        vec![
            "Authentication".into(),
            "Auth".into(),
            "login status".into(),
            "Not authenticated".into(),
        ],
        Duration::from_secs(5),
    ))
    .step(TestStep::Snapshot("surface_login_status_action".into()))
    .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_plugin_empty_row_is_disabled() {
    let fixture = open_surface_case("surface_plugin_empty_disabled", "plugin", "┌ Plugins ")
        .step(TestStep::AssertScreenContains(
            "disabled: no plugins found".into(),
        ))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("┌ Plugins ".into()))
        .step(TestStep::AssertScreenContains(
            "No installed plugins".into(),
        ))
        .step(TestStep::Snapshot("surface_plugin_empty_disabled".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_alias_plugin_empty_row_is_disabled() {
    let fixture = open_surface_case(
        "surface_alias_plugin_empty_disabled",
        "plugins",
        "┌ Plugins ",
    )
    .step(TestStep::AssertScreenContains(
        "disabled: no plugins found".into(),
    ))
    .step(TestStep::Key(TestKey::Enter))
    .step(TestStep::Wait(SHORT_WAIT))
    .step(TestStep::AssertScreenContains("┌ Plugins ".into()))
    .step(TestStep::AssertScreenContains(
        "No installed plugins".into(),
    ))
    .step(TestStep::Snapshot(
        "surface_alias_plugin_empty_disabled".into(),
    ))
    .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_tasks_empty_actions_do_not_submit() {
    let fixture = open_surface_case("surface_tasks_empty_disabled", "tasks", "Background tasks")
        .step(TestStep::AssertScreenContains("No tasks".into()))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::TypeText("k".into()))
        .step(TestStep::TypeText("d".into()))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("Background tasks".into()))
        .step(TestStep::AssertScreenContains("No tasks".into()))
        .step(TestStep::Snapshot("surface_tasks_empty_disabled".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_reconnect_disabled_server_reports_disabled() {
    let fixture = mcp_surface_case("surface_mcp_reconnect_disabled")
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Reconnect".into()))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::WaitForAny(
            vec![
                "MCP server `db` is disabled".into(),
                "Cannot reconnect MCP server `db`".into(),
            ],
            Duration::from_secs(5),
        ))
        .step(TestStep::Snapshot("surface_mcp_reconnect_disabled".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}

#[test]
fn surface_mcp_remove_action_updates_project_settings() {
    let fixture = mcp_surface_case("surface_mcp_remove_action")
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::Key(TestKey::Right))
        .step(TestStep::Wait(SHORT_WAIT))
        .step(TestStep::AssertScreenContains("> Remove".into()))
        .step(TestStep::Key(TestKey::Enter))
        .step(TestStep::WaitForText(
            "Removed MCP server `db`".into(),
            Duration::from_secs(3),
        ))
        .assert_file_not_contains("\"db\"")
        .step(TestStep::Snapshot("surface_mcp_remove_action".into()))
        .step(TestStep::AssertNoPanic);

    TestRunner::new().run(&fixture.case).assert_no_errors();
}
