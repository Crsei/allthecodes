use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::surfaces::tasks::{TaskSurfaceItem, TaskSurfaceSource};
use super::*;
use crate::ui::diff::diff_dialog::{DiffDialogMode, DiffSource};
use crate::ui::diff::{DiffData, DiffFile, DiffStats};
use crate::ui::mcp::index::{McpServer, McpServerKind};
use crate::ui::mcp::mcp_list_panel::McpListPanelState;
use crate::ui::memory::memory_file_selector::{
    MemoryFileKind, MemoryFileOption, MemoryFileSelectorState,
};
use crate::ui::selection_surface::{SelectionItem, SelectionSurface};
use crate::ui::skills::skills_menu::SkillMenuItem;
use crate::ui::tasks::{
    TaskKind as UiTaskKind, TaskState as UiTaskState, TaskStatus as UiTaskStatus,
};
use crate::ui::teams::teams_dialog::{TeamSummary, TeammateStatus};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_ipc_protocol::subsystem_types::LspRecommendationPayload;
use allthecodes_ipc_protocol::BackendMessage;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl_key(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn section(label: &str, body: String) -> String {
    let body = body
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    format!("-- {label} --\n{body}")
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn add_codex_profile(state: &mut AppState, model: &str) {
    let profile = allthecodes_config::settings::ProviderProfileSettings {
        backend: Some("codex".to_string()),
        api_provider: Some("openai-codex".to_string()),
        model: Some(model.to_string()),
        available_models: Some(allthecodes_config::settings::codex_model_ids()),
        model_capabilities: Some(allthecodes_config::settings::codex_model_capabilities()),
        ..Default::default()
    };
    state.main_loop_model = model.to_string();
    state.settings.api_provider = Some("openai-codex".to_string());
    state.settings.active_auth_profile = Some("codex".to_string());
    state
        .settings
        .auth_profiles
        .insert("codex".to_string(), profile);
    state.settings.available_models = allthecodes_config::settings::codex_model_ids();
    state.settings.model_capabilities = allthecodes_config::settings::codex_model_capabilities();
}

#[test]
fn agents_surface_switches_between_list_and_detail() {
    let mut surface = CommandSurface::Agents(AgentsSurface::new(Path::new(".")));
    assert!(surface.render().contains("Agents"));
    assert!(surface.render().contains("Create new agent"));

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Built-in agents"));
    assert!(!surface.render().contains("Create new agent"));
    assert!(surface.render().contains("general-purpose"));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    let detail = surface.render();
    assert!(detail.contains("Agent detail:"));
    assert!(detail.contains("Built-in"));
    assert!(detail.contains("Enter submit `/agents show"));

    match surface.handle_key(key(KeyCode::Enter)) {
        CommandSurfaceOutcome::Submit(command) => assert!(command.starts_with("/agents show ")),
        other => panic!("expected selected agent detail command, got {other:?}"),
    }

    surface.handle_key(key(KeyCode::Backspace));
    assert!(surface.render().contains("> Built-in agents"));
}

#[test]
fn hooks_surface_browses_read_only_config() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "PreToolUse".to_string(),
        serde_json::json!([
            {
                "matcher": "Bash",
                "hooks": [
                    {
                        "type": "command",
                        "command": "cargo test",
                        "statusMessage": "Run tests"
                    },
                    { "type": "prompt", "prompt": "Review the tool input" },
                    { "type": "agent", "prompt": "Inspect the repository" },
                    { "type": "http", "url": "https://hooks.example/run" }
                ]
            }
        ]),
    );
    let mut surface = CommandSurface::Hooks(HooksSurface::new(&hooks));

    let event_list = surface.render();
    assert!(event_list.contains("Read-only"));
    assert!(event_list.contains("PostToolUseFailure"));
    assert!(event_list.contains("PermissionRequest"));
    assert!(event_list.contains("InstructionsLoaded"));
    assert!(event_list.contains("CwdChanged"));
    assert!(event_list.contains("FileChanged"));

    assert_eq!(
        surface.handle_key(key(KeyCode::Char('l'))),
        CommandSurfaceOutcome::Submit("/hooks list PreToolUse".to_string())
    );

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    let matcher_list = surface.render();
    assert!(matcher_list.contains("PreToolUse - Matchers"));
    assert!(matcher_list.contains("Bash"));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    let hook_list = surface.render();
    assert!(hook_list.contains("[command] Run tests"));
    assert!(hook_list.contains("[prompt ] Review the tool input"));
    assert!(hook_list.contains("[agent  ] Inspect the repository"));
    assert!(hook_list.contains("[http   ] https://hooks.example/run"));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    let detail = surface.render();
    assert!(detail.contains("Hook details"));
    assert!(detail.contains("Type: command"));
    assert!(detail.contains("Command:"));
    assert!(detail.contains("cargo test"));
    assert!(detail.contains("Status message: Run tests"));

    assert_eq!(
        surface.handle_key(key(KeyCode::Esc)),
        CommandSurfaceOutcome::None
    );
    assert!(surface.render().contains("PreToolUse - Matcher: Bash"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Backspace)),
        CommandSurfaceOutcome::None
    );
    assert!(surface.render().contains("PreToolUse - Matchers"));

    surface.handle_key(key(KeyCode::Right));
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('o'))),
        CommandSurfaceOutcome::Submit("/hooks open project".to_string())
    );
}

#[test]
fn hooks_surface_non_matcher_events_open_hook_list_directly() {
    let mut hooks = HashMap::new();
    hooks.insert(
        "Stop".to_string(),
        serde_json::json!([{ "hooks": [{ "type": "prompt", "prompt": "Summarize the turn" }] }]),
    );
    let mut surface = CommandSurface::Hooks(HooksSurface::new(&hooks));

    while !surface.render().contains("> Stop") {
        surface.handle_key(key(KeyCode::Down));
    }

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    let hook_list = surface.render();
    assert!(hook_list.contains("Exit code 0 - stdout/stderr not shown"));
    assert!(hook_list.contains("[prompt ] Summarize the turn"));
    assert!(!hook_list.contains("Stop - Matchers"));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceKind {
    Agents,
    Config,
    Diff,
    Hooks,
    Kairos,
    Login,
    Mcp,
    Memory,
    Model,
    Permissions,
    Plugin,
    Providers,
    Remote,
    Resume,
    Sandbox,
    Skills,
    Subagents,
    Tasks,
    Team,
}

fn surface_kind(surface: &CommandSurface) -> SurfaceKind {
    match surface {
        CommandSurface::Agents(_) => SurfaceKind::Agents,
        CommandSurface::Config(_) => SurfaceKind::Config,
        CommandSurface::Diff(_) => SurfaceKind::Diff,
        CommandSurface::Hooks(_) => SurfaceKind::Hooks,
        CommandSurface::Kairos(_) => SurfaceKind::Kairos,
        CommandSurface::Login(_) => SurfaceKind::Login,
        CommandSurface::Mcp(_) => SurfaceKind::Mcp,
        CommandSurface::Memory(_) => SurfaceKind::Memory,
        CommandSurface::Model(_) => SurfaceKind::Model,
        CommandSurface::Permissions(_) => SurfaceKind::Permissions,
        CommandSurface::Plugin(_) => SurfaceKind::Plugin,
        CommandSurface::Providers(_) => SurfaceKind::Providers,
        CommandSurface::Remote(_) => SurfaceKind::Remote,
        CommandSurface::Resume(_) => SurfaceKind::Resume,
        CommandSurface::Sandbox(_) => SurfaceKind::Sandbox,
        CommandSurface::Skills(_) => SurfaceKind::Skills,
        CommandSurface::Subagents(_) => SurfaceKind::Subagents,
        CommandSurface::Tasks(_) => SurfaceKind::Tasks,
        CommandSurface::Team(_) => SurfaceKind::Team,
        CommandSurface::LspRecommendation(_) => {
            panic!("slash-command matrix should not create LSP recommendations")
        }
    }
}

#[test]
fn surface_entry_matrix_matches_expected_aliases() {
    let state = AppState::default();
    let cwd = std::env::current_dir().expect("current dir");

    for (command, expected) in [
        ("agents", SurfaceKind::Agents),
        ("config", SurfaceKind::Config),
        ("diff", SurfaceKind::Diff),
        ("effort", SurfaceKind::Config),
        ("hooks", SurfaceKind::Hooks),
        ("kairos", SurfaceKind::Kairos),
        ("login", SurfaceKind::Login),
        ("mcp", SurfaceKind::Mcp),
        ("memory", SurfaceKind::Memory),
        ("model", SurfaceKind::Model),
        ("permissions", SurfaceKind::Permissions),
        ("plugin", SurfaceKind::Plugin),
        ("providers", SurfaceKind::Providers),
        ("remote", SurfaceKind::Remote),
        ("resume", SurfaceKind::Resume),
        ("sandbox", SurfaceKind::Sandbox),
        ("skills", SurfaceKind::Skills),
        ("subagents", SurfaceKind::Subagents),
        ("agent-runtime", SurfaceKind::Subagents),
        ("tasks", SurfaceKind::Tasks),
        ("team", SurfaceKind::Team),
        ("perms", SurfaceKind::Permissions),
        ("plugins", SurfaceKind::Plugin),
        ("teams", SurfaceKind::Team),
    ] {
        let surface = CommandSurface::for_slash_command(command, "", &state, &cwd)
            .unwrap_or_else(|| panic!("{command} should open a command surface"));
        assert_eq!(
            surface_kind(&surface),
            expected,
            "{command} should map to the expected command surface"
        );
        assert!(
            CommandSurface::for_slash_command(command, "status", &state, &cwd).is_none(),
            "{command} with args should keep the normal slash-command path"
        );
    }

    for command in ["agent", "permission", "settings", "sessions"] {
        assert!(
            CommandSurface::for_slash_command(command, "", &state, &cwd).is_none(),
            "/{command} is not a CommandSurface alias"
        );
    }
}

#[test]
#[serial_test::serial]
fn subagents_surface_routes_summary_print() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = EnvGuard::set("ALLTHECODES_HOME", temp.path().to_str().unwrap());
    let state = AppState::default();
    let cwd = std::env::current_dir().expect("current dir");
    let mut surface = CommandSurface::for_slash_command_with_session(
        "subagents",
        "",
        &state,
        &cwd,
        Some("surface-test-session"),
    )
    .expect("subagents surface");

    assert!(surface.render().contains("Subagents"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/subagents summary".to_string())
    );
}

#[test]
fn diff_surface_renders_explicit_escape_close_hint() {
    let surface = DiffSurface {
        sources: vec![DiffSource::current()],
        source_index: 0,
        selected_index: 0,
        mode: DiffDialogMode::List,
        error: None,
    };

    assert!(surface.render().contains("Esc close"));
}

#[test]
fn permissions_command_surface_routes_confirmed_safety_modes() {
    let mut state = AppState::default();
    state
        .tool_permission_context
        .always_allow_rules
        .insert("project".to_string(), vec!["Bash(cargo test*)".to_string()]);
    state
        .tool_permission_context
        .is_bypass_permissions_mode_available = true;
    let cwd = std::env::current_dir().expect("current dir");
    let mut surface =
        CommandSurface::for_slash_command("permissions", "", &state, &cwd).expect("surface");

    assert!(surface.render().contains("Permissions"));
    assert!(surface.render().contains("Default (current)"));
    assert!(surface.render().contains("Auto-review"));
    assert!(surface.render().contains("Full Access"));
    surface.handle_key(key(KeyCode::Down));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/permissions mode auto --confirm".to_string())
    );
    surface.handle_key(key(KeyCode::Down));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/permissions mode bypass --confirm".to_string())
    );
}

#[test]
fn login_surface_routes_auth_actions() {
    let mut surface = CommandSurface::Login(LoginSurface { action_index: 0 });

    assert!(surface.render().contains("Login / allthecodes"));
    assert!(surface
        .render()
        .contains("profiles=claude-code,codex,custom"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/login status".to_string())
    );

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Claude Code"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/login claude-code".to_string())
    );

    assert_eq!(
        surface.handle_key(key(KeyCode::Char('i'))),
        CommandSurfaceOutcome::Submit("/login codex-cli".to_string())
    );
}

#[test]
fn skills_surface_filters_and_opens_selected_skill() {
    let mut surface = CommandSurface::Skills(SkillsSurface {
        items: vec![
            SkillMenuItem::new("debug", "diagnose failures"),
            SkillMenuItem::new("remember", "save memory"),
        ],
        selected_index: 0,
        filter: String::new(),
    });

    surface.handle_key(key(KeyCode::Char('m')));
    assert!(surface.render().contains("remember"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/skills remember".to_string())
    );
}

#[test]
fn skills_surface_searches_filter_without_replacing_exact_enter() {
    let mut surface = CommandSurface::Skills(SkillsSurface {
        items: vec![
            SkillMenuItem::new("debug", "diagnose failures"),
            SkillMenuItem::new("remember", "save memory"),
        ],
        selected_index: 0,
        filter: String::new(),
    });

    surface.handle_key(key(KeyCode::Char('m')));
    assert_eq!(
        surface.handle_key(ctrl_key('s')),
        CommandSurfaceOutcome::Submit("/skills search m".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/skills remember".to_string())
    );
}

#[test]
fn tasks_surface_routes_selected_task_actions() {
    let mut surface = CommandSurface::Tasks(TasksSurface {
        items: vec![TaskSurfaceItem {
            task: UiTaskStatus::new("task-1", "cargo test", UiTaskKind::Shell),
            source: TaskSurfaceSource::Tool,
        }],
        selected_index: 0,
    });

    assert!(surface.render().contains("Background tasks"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/tasks show task-1".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('d'))),
        CommandSurfaceOutcome::Submit("/tasks delete task-1".to_string())
    );
}

#[test]
fn tasks_surface_shows_fork_context_and_live_path() {
    let mut surface = TasksSurface::from_items(Vec::new());
    surface.handle_event(&BackendMessage::AgentEvent {
        event: allthecodes_types::agent_events::AgentEvent::Spawned {
            agent_id: "fork-1".to_string(),
            parent_agent_id: None,
            description: "follow parent".to_string(),
            agent_type: Some("fork".to_string()),
            model: None,
            is_background: true,
            depth: 1,
            chain_id: "chain".to_string(),
            fork_metadata: Some(allthecodes_types::agent_types::ForkLaunchMetadata {
                is_fork: true,
                context: allthecodes_types::agent_types::ForkContextMode::LiveReadonly,
                live_channel: Some(allthecodes_types::agent_types::LiveParentContextPaths {
                    directory: "/tmp/fork-1".to_string(),
                    snapshot: "/tmp/fork-1/parent-context.md".to_string(),
                    updates: "/tmp/fork-1/parent-updates.ndjson".to_string(),
                    latest_diff: None,
                    latest_seq: 1,
                }),
            }),
        },
    });

    surface.handle_key(key(KeyCode::Right));
    let rendered = surface.render();
    assert!(rendered.contains("fork · context: live readonly"));
    assert!(rendered.contains("parent-context.md"));
}

#[test]
fn tasks_surface_snapshots_tool_and_team_integration() {
    let mut tool = UiTaskStatus::new("task-1", "cargo test", UiTaskKind::Shell);
    tool.state = UiTaskState::Running;
    tool.progress = Some((1, 2));
    tool.summary = "running tests".into();
    tool.elapsed_ms = 3_200;

    let mut teammate =
        UiTaskStatus::new("team-1", "builder (ui-port)", UiTaskKind::InProcessTeammate);
    teammate.state = UiTaskState::Running;
    teammate.summary = "implementing task panel".into();

    let mut surface = CommandSurface::Tasks(TasksSurface {
        items: vec![
            TaskSurfaceItem {
                task: tool,
                source: TaskSurfaceSource::Tool,
            },
            TaskSurfaceItem {
                task: teammate,
                source: TaskSurfaceSource::Team {
                    teammate_name: "builder".into(),
                },
            },
        ],
        selected_index: 0,
    });

    let mut rendered = vec![section("tool-selected", surface.render())];
    surface.handle_key(key(KeyCode::Down));
    rendered.push(section("team-selected", surface.render()));

    insta::assert_snapshot!("tasks_surface_tool_team_integration", rendered.join("\n\n"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('s'))),
        CommandSurfaceOutcome::Submit("/team kill builder".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('d'))),
        CommandSurfaceOutcome::None
    );
}

#[test]
fn team_surface_routes_teammate_actions() {
    let mut surface = CommandSurface::Team(TeamSurface {
        summary: TeamSummary {
            name: "ui-port".to_string(),
            teammates: vec![TeammateStatus::new("builder", "Implement changes")],
        },
        active: true,
        selected_index: 0,
    });

    assert!(surface.render().contains("team=ui-port"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('s'))),
        CommandSurfaceOutcome::FillPrompt("/team send builder ".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Char('k'))),
        CommandSurfaceOutcome::Submit("/team kill builder".to_string())
    );
}

#[test]
fn diff_surface_opens_selected_file_detail() {
    let source = DiffSource::with_label(
        "Current",
        DiffData {
            stats: Some(DiffStats::new(1, 1, 0)),
            files: vec![DiffFile::new(
                "src/main.rs",
                1,
                0,
                false,
                false,
                false,
                false,
            )],
            hunks: HashMap::from([("src/main.rs".to_string(), vec!["+fn main() {}".to_string()])]),
            loading: false,
        },
    );
    let mut surface = CommandSurface::Diff(DiffSurface {
        sources: vec![source],
        source_index: 0,
        selected_index: 0,
        mode: DiffDialogMode::List,
        error: None,
    });

    assert!(surface.render().contains("src/main.rs"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
    assert!(surface.render().contains("+fn main() {}"));
}

#[test]
fn config_surface_uses_tab_navigation_and_selection() {
    let mut surface = CommandSurface::Config(ConfigSurface::new(&AppState::default()));
    assert!(surface.render().contains("> Status"));

    surface.handle_key(key(KeyCode::Char('9')));
    assert!(surface.render().contains("> Config"));
    surface.handle_key(key(KeyCode::Down));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::FillPrompt("/config set theme ".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Esc)),
        CommandSurfaceOutcome::Close
    );
}

#[test]
fn effort_command_opens_thinking_picker() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");
    state.effort_value = Some("high".into());
    let cwd = std::env::current_dir().expect("current dir");
    let mut surface =
        CommandSurface::for_slash_command("effort", "", &state, &cwd).expect("surface");

    let rendered = surface.render();
    assert!(rendered.contains("Effort"));
    assert!(rendered.contains("High - supported"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/effort high".to_string())
    );
}

#[test]
fn effort_command_maps_settings_output_config_effort() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");
    // Codex transport must NOT consult `output_config.effort` (plan §3.1).
    // Setting it to any value -- including 'low', which the legacy
    // Anthropic-collapse previously re-mapped to 'high' -- must not shift the
    // Codex picker off the resolved Codex baseline. With no profile baseline
    // set, the picker falls back to the bundled default for gpt-5.5
    // (`medium`) in all three subcases.
    state.settings.output_config = Some(serde_json::json!({"effort": "low"}));
    let cwd = std::env::current_dir().expect("current dir");
    let mut surface =
        CommandSurface::for_slash_command("effort", "", &state, &cwd).expect("surface");
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/effort medium".to_string())
    );

    state.settings.output_config = Some(serde_json::json!({"effort": "xhigh"}));
    let mut surface =
        CommandSurface::for_slash_command("effort", "", &state, &cwd).expect("surface");
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/effort medium".to_string())
    );

    state.settings.output_config = Some(serde_json::json!({"effort": "ultra"}));
    let mut surface =
        CommandSurface::for_slash_command("effort", "", &state, &cwd).expect("surface");
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/effort medium".to_string())
    );
}

#[test]
fn effort_command_codex_profile_baseline_wins_over_output_config_effort() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");
    // Codex profile baseline (`modelReasoningEffort`) must win deterministically
    // over the legacy `output_config.effort` field, since Codex transport does
    // not consume `output_config.effort` at all (plan §3.1, §6 acceptance #1).
    state
        .settings
        .auth_profiles
        .get_mut("codex")
        .expect("codex profile")
        .model_reasoning_effort = Some("low".into());
    state.settings.output_config = Some(serde_json::json!({"effort": "high"}));
    let cwd = std::env::current_dir().expect("current dir");
    let mut surface =
        CommandSurface::for_slash_command("effort", "", &state, &cwd).expect("surface");
    let rendered = surface.render();
    assert!(rendered.contains("Low - supported"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/effort low".to_string())
    );
}

#[test]
fn config_surface_exposes_model_theme_and_effort_pickers() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");
    state.settings.theme = Some("light".into());
    state.settings.output_style = Some("explanatory".into());
    state.settings.language = Some("English".into());
    state.effort_value = Some("medium".into());

    let mut surface = CommandSurface::Config(ConfigSurface::new(&state));
    let mut rendered = vec![section("status", surface.render())];

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("model", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("theme", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("usage", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("output", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("language", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("thinking", surface.render()));

    surface.handle_key(key(KeyCode::Right));
    rendered.push(section("safety", surface.render()));

    insta::assert_snapshot!("config_surface_model_theme_effort", rendered.join("\n\n"));
}

#[test]
fn config_surface_model_picker_uses_active_profile_capabilities() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");

    let mut surface = CommandSurface::Config(ConfigSurface::new(&state));
    surface.handle_key(key(KeyCode::Right));
    let rendered = surface.render();

    assert!(rendered.contains("GPT-5.5 (gpt-5.5)"));
    assert!(rendered.contains("ctx=272k"));
    assert!(rendered.contains("default effort=medium"));
}

#[test]
fn model_surface_without_profile_shows_no_profile_models() {
    let state = AppState::default();

    let surface = CommandSurface::Model(ModelSurface::new(&state));
    let rendered = surface.render();

    assert!(rendered.contains("No profile models"));
    assert!(rendered.contains("use /login first"));
}

#[test]
fn model_surface_uses_active_profile_capabilities() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");

    let surface = CommandSurface::Model(ModelSurface::new(&state));
    let rendered = surface.render();

    assert!(rendered.contains("GPT-5.5 (gpt-5.5)"));
    assert!(rendered.contains("gpt-5.4"));
    assert!(rendered.contains("search"));
}

#[test]
#[serial_test::serial]
fn model_surface_codex_provider_ignores_anthropic_alias_env_mapping() {
    let _sota = EnvGuard::set("ANTHROPIC_DEFAULT_SOTA_MODEL", "deepseek-v4-pro");
    let _mota = EnvGuard::set("ANTHROPIC_DEFAULT_MOTA_MODEL", "deepseek-v4-pro");
    let _fota = EnvGuard::set("ANTHROPIC_DEFAULT_FOTA_MODEL", "deepseek-v4-flash");
    let mut state = AppState {
        main_loop_model: "gpt-5.5".into(),
        ..Default::default()
    };
    add_codex_profile(&mut state, "gpt-5.5");

    let surface = CommandSurface::Model(ModelSurface::new(&state));
    let rendered = surface.render();

    assert!(rendered.contains("GPT-5.5 (gpt-5.5)"));
    assert!(rendered.contains("gpt-5.4"));
    assert!(!rendered.contains("deepseek-v4-pro"));
    assert!(!rendered.contains("deepseek-v4-flash"));
}

#[test]
#[serial_test::serial]
fn model_surface_ignores_settings_alias_models_without_capabilities() {
    let _sota = EnvGuard::set("ANTHROPIC_DEFAULT_SOTA_MODEL", "deepseek-v4-pro");
    let mut state = AppState {
        main_loop_model: "custom-sota".into(),
        ..Default::default()
    };
    state.settings.api_provider = Some("openai-codex".into());
    state.settings.sota_model = Some("custom-sota".into());
    state.settings.mota_model = Some("custom-mota".into());
    state.settings.fota_model = Some("custom-fota".into());
    state.settings.available_models = vec!["SOTA".into(), "MOTA".into(), "FOTA".into()];

    let surface = CommandSurface::Model(ModelSurface::new(&state));
    let rendered = surface.render();

    assert!(rendered.contains("No profile models"));
    assert!(!rendered.contains("custom-sota"));
    assert!(!rendered.contains("deepseek-v4-pro"));
}

#[test]
fn config_surface_picker_selection_submits_config_set_commands() {
    let mut state = AppState::default();
    add_codex_profile(&mut state, "gpt-5.5");
    state.settings.theme = Some("light".into());
    state.effort_value = Some("medium".into());

    let mut model_surface = CommandSurface::Config(ConfigSurface::new(&state));
    model_surface.handle_key(key(KeyCode::Right));
    assert_eq!(
        model_surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );

    let mut theme_surface = CommandSurface::Config(ConfigSurface::new(&state));
    theme_surface.handle_key(key(KeyCode::Right));
    theme_surface.handle_key(key(KeyCode::Right));
    assert_eq!(
        theme_surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/config set theme light".to_string())
    );

    let mut effort_surface = CommandSurface::Config(ConfigSurface::new(&state));
    for _ in 0..6 {
        effort_surface.handle_key(key(KeyCode::Right));
    }
    assert_eq!(
        effort_surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::None
    );
}

#[test]
fn sandbox_surface_uses_tab_navigation_and_selection() {
    let mut surface = CommandSurface::Sandbox(SandboxSurface::new(&AppState::default()));

    let rendered = surface.render();
    assert!(rendered.contains("> Config"));
    assert!(rendered.contains("Network policy"));

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Dependencies"));
    assert!(surface.render().contains("OS-level sandbox"));
    assert!(surface.render().contains("Network proxy runtime"));
    surface.handle_key(key(KeyCode::Down));
    surface.handle_key(key(KeyCode::Down));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/sandbox require".to_string())
    );
    surface.handle_key(key(KeyCode::Down));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/sandbox optional".to_string())
    );

    surface.handle_key(key(KeyCode::Char('6')));
    assert!(surface.render().contains("> Network"));
    surface.handle_key(key(KeyCode::Up));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/sandbox network on".to_string())
    );
}

#[test]
fn mcp_surface_action_tabs_apply_to_selected_server() {
    let mut server = McpServer::new("db", McpServerKind::Stdio);
    server.command_or_url = "node db-server.js".to_string();
    let mut surface = CommandSurface::Mcp(McpSurface {
        state: McpListPanelState::new(vec![server]),
        action_index: 0,
    });

    assert!(surface.render().contains("> Status"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/mcp status".to_string())
    );

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Edit"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::FillPrompt("/mcp edit db ".to_string())
    );

    surface.handle_key(key(KeyCode::Right));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/mcp reconnect db".to_string())
    );

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Remove"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/mcp remove db".to_string())
    );

    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Search"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::FillPrompt("/mcp search ".to_string())
    );

    surface.handle_key(key(KeyCode::Char('v')));
    assert!(surface.render().contains("Server details"));
}

#[test]
fn plugin_surface_searches_filter_without_replacing_exact_enter() {
    let mut surface = CommandSurface::Plugin(PluginSurface {
        picker: SelectionSurface::new(
            "Plugins",
            vec![SelectionItem::new("rust-tools", "Rust Tools")],
        ),
    });

    if let CommandSurface::Plugin(plugin) = &mut surface {
        plugin.picker.set_filter("rust");
    }

    assert_eq!(
        surface.handle_key(ctrl_key('s')),
        CommandSurfaceOutcome::Submit("/plugin search rust".to_string())
    );
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/plugin info rust-tools".to_string())
    );
}

#[test]
fn memory_surface_action_tabs_apply_to_selected_target() {
    let cwd = PathBuf::from(".");
    let home = PathBuf::from(".");
    let mut surface = CommandSurface::Memory(MemorySurface {
        state: MemoryFileSelectorState::new(vec![
            MemoryFileOption::new("AGENTS.md", MemoryFileKind::Project),
            MemoryFileOption::new(".allthecodes/auto-memory", MemoryFileKind::Folder)
                .with_description("auto-memory folder"),
        ]),
        cwd,
        home,
        action_index: 0,
    });

    surface.handle_key(key(KeyCode::Right));
    surface.handle_key(key(KeyCode::Right));
    surface.handle_key(key(KeyCode::Right));
    assert!(surface.render().contains("> Open"));
    surface.handle_key(key(KeyCode::Down));

    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::Submit("/memory open auto".to_string())
    );
}

#[test]
fn lsp_recommendation_yes_populates_plugin_install_prompt() {
    let mut surface = CommandSurface::lsp_recommendation(LspRecommendationPayload {
        request_id: "req-1".to_string(),
        plugin_name: "rust-analyzer".to_string(),
        plugin_description: None,
        file_extension: ".rs".to_string(),
        language_id: Some("rust".to_string()),
    });
    assert!(surface.render().contains("Yes, install rust-analyzer"));
    assert_eq!(
        surface.handle_key(key(KeyCode::Enter)),
        CommandSurfaceOutcome::LspRecommendationResponse {
            request_id: "req-1".to_string(),
            plugin_name: "rust-analyzer".to_string(),
            decision: "yes".to_string(),
            install_prompt: Some("/plugin install rust-analyzer ".to_string()),
        }
    );
}
