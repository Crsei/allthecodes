use super::*;
use allthecodes_bootstrap::SessionId;
use allthecodes_engine::types::app_state::AppState;

fn test_builtin_entries() -> Vec<crate::runtime::BuiltinAgentEntry> {
    vec![
        crate::runtime::BuiltinAgentEntry {
            name: "general-purpose".to_string(),
            description: "General-purpose agent".to_string(),
        },
        crate::runtime::BuiltinAgentEntry {
            name: "Explore".to_string(),
            description: "Explore code".to_string(),
        },
        crate::runtime::BuiltinAgentEntry {
            name: "Plan".to_string(),
            description: "Plan changes".to_string(),
        },
    ]
}

fn test_builtin_prompt(name: &str) -> Option<String> {
    (name == "general-purpose").then(|| "General system prompt".to_string())
}

fn install_test_runtime() {
    crate::runtime::set_agent_runtime_providers(test_builtin_entries, test_builtin_prompt);
}

fn make_ctx() -> CommandContext {
    install_test_runtime();
    CommandContext {
        messages: vec![],
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        app_state: AppState::default(),
        session_id: SessionId::new(),
    }
}

#[tokio::test]
async fn default_lists_builtins_at_minimum() {
    let handler = AgentsHandler;
    let mut ctx = make_ctx();
    let result = handler.execute("", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(s) => {
            assert!(s.contains("Agents"));
            assert!(s.contains("general-purpose"));
            assert!(s.contains("Explore"));
            assert!(s.contains("Plan"));
            assert!(s.contains("Built-in subagent types"));
        }
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn show_requires_name() {
    let handler = AgentsHandler;
    let mut ctx = make_ctx();
    let result = handler.execute("show", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(s) => assert!(s.contains("Usage")),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn show_returns_details_for_builtin() {
    let handler = AgentsHandler;
    let mut ctx = make_ctx();
    let result = handler
        .execute("show general-purpose", &mut ctx)
        .await
        .unwrap();
    match result {
        CommandResult::Output(s) => {
            assert!(s.contains("Agent: general-purpose"));
            assert!(s.contains("Source:"));
            assert!(s.contains("built-in"));
        }
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn show_unknown_reports_missing() {
    let handler = AgentsHandler;
    let mut ctx = make_ctx();
    let result = handler.execute("show nope-no-way", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(s) => assert!(s.contains("No agent named")),
        _ => panic!("expected Output"),
    }
}

#[tokio::test]
async fn unknown_subcommand_lists_usage() {
    let handler = AgentsHandler;
    let mut ctx = make_ctx();
    let result = handler.execute("banana", &mut ctx).await.unwrap();
    match result {
        CommandResult::Output(s) => {
            assert!(s.contains("Unknown /agents"));
            assert!(s.contains("/agents show"));
        }
        _ => panic!("expected Output"),
    }
}

#[test]
fn agent_source_group_order_is_stable() {
    assert!(AgentSource::Builtin.group_order() < AgentSource::Team.group_order());
    assert!(
        AgentSource::Skill(SkillSource::Bundled).group_order()
            < AgentSource::Skill(SkillSource::Project).group_order()
    );
}

#[test]
fn tree_marks_shadowed_when_name_appears_twice() {
    let agents = vec![
        AgentEntry {
            name: "dup".into(),
            description: "builtin".into(),
            source: AgentSource::Builtin,
            path: None,
            active: true,
            execution: "built-in",
        },
        AgentEntry {
            name: "dup".into(),
            description: "user".into(),
            source: AgentSource::Skill(SkillSource::User),
            path: None,
            active: true,
            execution: "fork",
        },
    ];
    let out = render_agent_tree(&agents);
    assert!(out.contains("overrides"));
    assert!(out.contains("shadowed"));
}
