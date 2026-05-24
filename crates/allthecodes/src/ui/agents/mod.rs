//! Rust-side agent management surfaces mirrored from upstream agents.
pub mod agent_detail;
pub mod agent_editor;
pub mod agent_file_utils;
pub mod agent_navigation_footer;
pub mod agents_list;
pub mod agents_menu;
pub mod color_picker;
pub mod generate_agent;
pub mod model_selector;
pub mod new_agent_creation;
pub mod tool_selector;
pub mod types;
pub mod utils;
pub mod validate_agent;

const _: fn() = production_symbol_anchors;

fn production_symbol_anchors() {
    use std::path::Path;

    use agent_editor::AgentEditMode;
    use agent_file_utils::{
        format_agent_as_markdown, get_new_agent_file_path, render_agent_file_summary,
        sanitize_agent_filename,
    };
    use agent_navigation_footer::AgentNavigationFooter;
    use agents_list::AgentsListState;
    use agents_menu::AgentsMenuState;
    use new_agent_creation::wizard_steps::{
        color_step::render_color_step, confirm_step::render_confirm_step,
        confirm_step_wrapper::render_confirm_step_wrapper,
        description_step::render_description_step, generate_step::render_generate_step,
        location_step::render_location_step, memory_step::render_memory_step,
        method_step::render_method_step, model_step::render_model_step,
        prompt_step::render_prompt_step, render_step_frame, tools_step::render_tools_step,
        type_step::render_type_step,
    };
    use new_agent_creation::{AgentCreationMethod, AgentWizardData};
    use tool_selector::{ToolBucketKind, ToolOption};
    use types::{
        AgentDefinition, AgentMemoryScope, AgentModeState, AgentSource, AgentSourceFilter,
    };
    use utils::{group_agents_by_source, indent_lines, truncate_middle};

    let agent = AgentDefinition::new(
        "anchor",
        "Use for anchor checks.",
        "Anchor prompt.",
        AgentSource::Project,
    )
    .with_tools(["Read"])
    .with_model("MOTA")
    .with_memory(AgentMemoryScope::Project)
    .with_color("blue")
    .with_base_dir(".allthecodes/agents")
    .with_filename("anchor");
    let _ = agent.is_plugin();
    let _ = AgentMemoryScope::None.display_name();
    let _ = [
        AgentEditMode::Menu,
        AgentEditMode::EditTools,
        AgentEditMode::EditColor,
        AgentEditMode::EditModel,
    ];
    let _ = format_agent_as_markdown(&agent);
    let _ = get_new_agent_file_path(
        AgentSource::Project,
        "anchor",
        Path::new("."),
        Path::new("."),
        Path::new("."),
    );
    let _ = render_agent_file_summary(&agent);
    let _ = sanitize_agent_filename("Anchor Agent");
    let _ = AgentNavigationFooter {
        can_create: true,
        can_edit: true,
        can_delete: false,
        in_selection: true,
    }
    .render();
    let mut list = AgentsListState::new(AgentSourceFilter::All, vec![agent.clone()]);
    list.show_create_new = false;
    let _ = list.render();
    let mut menu = AgentsMenuState::default_with_counts(1, 0, 0, 1);
    menu.move_next();
    menu.move_prev();
    let _ = menu.selected_filter();
    let _ = model_selector::render_model_selector(Some("MOTA"));
    let mut wizard = AgentWizardData::empty();
    wizard.location = Some(AgentSource::Project);
    wizard.method = Some(AgentCreationMethod::Manual);
    wizard.agent_type = Some("anchor".to_string());
    wizard.when_to_use = Some("Use for anchor checks.".to_string());
    wizard.system_prompt = Some("Anchor prompt with enough detail for validation.".to_string());
    let _ = render_step_frame("Anchor", "body", true);
    let _ = render_color_step("anchor", Some("blue"));
    let _ = render_confirm_step(&agent);
    let _ = render_confirm_step_wrapper(&wizard);
    let _ = render_description_step(wizard.when_to_use.as_deref(), 40);
    let _ = render_generate_step(Some("anchor goal"), true);
    let _ = render_location_step(Some(AgentSource::Project));
    let _ = render_memory_step(Some(AgentMemoryScope::None));
    let _ = render_method_step(Some(AgentCreationMethod::Manual));
    let _ = render_model_step(Some("MOTA"));
    let _ = render_prompt_step(wizard.system_prompt.as_deref(), 40);
    let _ = render_tools_step(Some(vec!["Read".to_string()]), true);
    let _ = render_type_step(
        wizard.agent_type.as_deref(),
        &["existing".to_string()],
        wizard.when_to_use.as_deref().unwrap_or_default(),
        wizard.system_prompt.as_deref().unwrap_or_default(),
    );
    let _ = ToolOption::new("OtherTool", ToolBucketKind::Other);
    let _ = [
        AgentModeState::MainMenu,
        AgentModeState::ListAgents {
            source: AgentSourceFilter::All,
        },
        AgentModeState::AgentMenu {
            agent_type: "anchor".to_string(),
            previous: Box::new(AgentModeState::MainMenu),
        },
        AgentModeState::ViewAgent {
            agent_type: "anchor".to_string(),
            previous: Box::new(AgentModeState::MainMenu),
        },
        AgentModeState::CreateAgent,
        AgentModeState::EditAgent {
            agent_type: "anchor".to_string(),
            previous: Box::new(AgentModeState::MainMenu),
        },
        AgentModeState::DeleteConfirm {
            agent_type: "anchor".to_string(),
            previous: Box::new(AgentModeState::MainMenu),
        },
    ];
    let _ = group_agents_by_source(&[agent]);
    let _ = truncate_middle("abcdefghijkl", 8);
    let _ = indent_lines(["one".to_string()], 2);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::agent_detail::render_agent_detail;
    use super::agent_editor::{
        render_save_change_summary, AgentEditMode, AgentEditorState, AgentSaveChanges,
    };
    use super::agent_file_utils::{
        format_agent_as_markdown, get_new_agent_file_path, render_agent_file_summary,
        sanitize_agent_filename,
    };
    use super::agent_navigation_footer::AgentNavigationFooter;
    use super::agents_list::AgentsListState;
    use super::agents_menu::AgentsMenuState;
    use super::color_picker::ColorPickerState;
    use super::generate_agent::{
        generate_agent_draft, render_generated_agent_preview, GenerateAgentRequest,
    };
    use super::model_selector::render_model_selector;
    use super::new_agent_creation::create_agent_wizard::render_create_agent_wizard;
    use super::new_agent_creation::wizard_steps::color_step::render_color_step;
    use super::new_agent_creation::wizard_steps::confirm_step_wrapper::render_confirm_step_wrapper;
    use super::new_agent_creation::wizard_steps::description_step::render_description_step;
    use super::new_agent_creation::wizard_steps::generate_step::render_generate_step;
    use super::new_agent_creation::wizard_steps::location_step::render_location_step;
    use super::new_agent_creation::wizard_steps::memory_step::render_memory_step;
    use super::new_agent_creation::wizard_steps::method_step::render_method_step;
    use super::new_agent_creation::wizard_steps::model_step::render_model_step;
    use super::new_agent_creation::wizard_steps::prompt_step::render_prompt_step;
    use super::new_agent_creation::wizard_steps::tools_step::render_tools_step;
    use super::new_agent_creation::wizard_steps::type_step::render_type_step;
    use super::new_agent_creation::{AgentCreationMethod, AgentWizardData};
    use super::tool_selector::{
        default_agent_tools, ToolBucketKind, ToolOption, ToolSelectorState,
    };
    use super::types::{
        AgentDefinition, AgentMemoryScope, AgentModeState, AgentSource, AgentSourceFilter,
    };
    use super::utils::{indent_lines, truncate_middle};
    use super::validate_agent::{render_validation_result, validate_agent_definition};

    fn sample_agent() -> AgentDefinition {
        let mut hooks = BTreeMap::new();
        hooks.insert("PreToolUse".to_string(), vec!["check".to_string()]);
        let mut agent = AgentDefinition::new(
            "reviewer",
            "Use when code changes need a focused review before commit.",
            "Review the assigned diff, identify correctness risks, and report concise findings.",
            AgentSource::Project,
        )
        .with_tools(["Read", "Grep", "Bash"])
        .with_model("MOTA")
        .with_memory(AgentMemoryScope::Project)
        .with_color("blue")
        .with_base_dir("./.allthecodes/agents")
        .with_filename("reviewer");
        agent.permission_mode = Some("ask".to_string());
        agent.skills = vec!["code-review".to_string(), "security-review".to_string()];
        agent.hooks = hooks;
        agent
    }

    #[test]
    fn snapshot_agents_core_surfaces() {
        let agent = sample_agent();
        let built_in = AgentDefinition::new(
            "general-purpose",
            "Use for ordinary work.",
            "You are a helpful general agent.",
            AgentSource::BuiltIn,
        );
        let mut list = AgentsListState::new(
            AgentSourceFilter::All,
            vec![built_in.clone(), agent.clone()],
        );
        list.move_next();

        let mut color = ColorPickerState::new("reviewer", Some("blue"));
        color.move_next();

        let mut tool_state = ToolSelectorState::new(
            default_agent_tools(),
            Some(vec!["Read".to_string(), "Bash".to_string()]),
        );
        tool_state.show_individual_tools = true;

        let validation = validate_agent_definition(
            "reviewer-2",
            &agent.when_to_use,
            &agent.system_prompt,
            &["reviewer".to_string()],
        );
        let save = AgentSaveChanges {
            tools: Some(vec!["Read".to_string(), "Grep".to_string()]),
            color: Some("green".to_string()),
            model: Some("SOTA".to_string()),
        };

        let rendered = [
            section(
                "menu",
                AgentsMenuState::default_with_counts(2, 1, 0, 1).render(),
            ),
            section("list", list.render()),
            section("detail", render_agent_detail(&agent, 72)),
            section("editor", AgentEditorState::new(agent.clone()).render_menu()),
            section("file-summary", render_agent_file_summary(&agent)),
            section("markdown", format_agent_as_markdown(&agent)),
            section("color", color.render()),
            section("model", render_model_selector(Some("custom-model-id"))),
            section("tools", tool_state.render()),
            section(
                "footer",
                AgentNavigationFooter {
                    can_create: true,
                    can_edit: true,
                    can_delete: false,
                    in_selection: true,
                }
                .render(),
            ),
            section("validation", render_validation_result(&validation)),
            section("save", render_save_change_summary(&agent, &save)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("agents_core_surfaces", rendered);
    }

    #[test]
    fn snapshot_agent_generation_and_wizard() {
        let draft = generate_agent_draft(&GenerateAgentRequest {
            goal: "review unsafe Rust changes".to_string(),
            source: AgentSource::Project,
            preferred_tools: vec!["Read".to_string(), "Grep".to_string()],
        });

        let mut data = AgentWizardData::empty();
        data.location = Some(AgentSource::Project);
        data.method = Some(AgentCreationMethod::Generate);
        data.generation_goal = Some("review unsafe Rust changes".to_string());
        data.agent_type = Some(draft.agent.agent_type.clone());
        data.system_prompt = Some(draft.agent.system_prompt.clone());
        data.when_to_use = Some(draft.agent.when_to_use.clone());
        data.tools = draft.agent.tools.clone();
        data.model = Some("MOTA".to_string());
        data.color = Some("purple".to_string());
        data.memory = Some(AgentMemoryScope::Project);

        let rendered = [
            section("draft", render_generated_agent_preview(&draft)),
            section("wizard", render_create_agent_wizard(&data, 3, true)),
            section("location", render_location_step(data.location)),
            section("method", render_method_step(data.method)),
            section(
                "generate",
                render_generate_step(data.generation_goal.as_deref(), true),
            ),
            section(
                "type",
                render_type_step(
                    data.agent_type.as_deref(),
                    &["reviewer".to_string()],
                    data.when_to_use.as_deref().unwrap_or_default(),
                    data.system_prompt.as_deref().unwrap_or_default(),
                ),
            ),
            section(
                "prompt",
                render_prompt_step(data.system_prompt.as_deref(), 64),
            ),
            section(
                "description",
                render_description_step(data.when_to_use.as_deref(), 64),
            ),
            section("tools", render_tools_step(data.tools.clone(), false)),
            section("model", render_model_step(data.model.as_deref())),
            section(
                "color",
                render_color_step(
                    data.agent_type.as_deref().unwrap_or("agent"),
                    data.color.as_deref(),
                ),
            ),
            section("memory", render_memory_step(data.memory)),
            section("confirm", render_confirm_step_wrapper(&data)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("agent_generation_and_wizard", rendered);
    }

    #[test]
    fn agent_menu_navigation_wraps_and_reports_filter() {
        let mut menu = AgentsMenuState::default_with_counts(4, 1, 1, 2);
        assert_eq!(menu.selected_filter(), Some(AgentSourceFilter::All));

        menu.move_prev();
        assert_eq!(
            menu.selected_filter(),
            Some(AgentSourceFilter::Source(AgentSource::Project))
        );

        menu.move_next();
        assert_eq!(menu.selected_filter(), Some(AgentSourceFilter::All));
    }

    #[test]
    fn agent_editor_modes_are_distinct_states() {
        let modes = [
            AgentEditMode::Menu,
            AgentEditMode::EditTools,
            AgentEditMode::EditColor,
            AgentEditMode::EditModel,
        ];
        assert_eq!(modes.len(), 4);
        assert_ne!(modes[1], modes[2]);
    }

    #[test]
    fn color_picker_previous_wraps_to_last_color() {
        let mut picker = ColorPickerState::new("reviewer", None);
        picker.move_previous();
        assert_eq!(picker.selected_color(), Some("pink"));
    }

    #[test]
    fn tool_selector_toggles_and_renders_other_bucket() {
        let tools = vec![
            ToolOption::new("Read", ToolBucketKind::ReadOnly),
            ToolOption::new("CustomTool", ToolBucketKind::Other),
        ];
        let mut selector = ToolSelectorState::new(tools, Some(vec!["Read".to_string()]));

        assert_eq!(selector.selected_output(), Some(vec!["Read".to_string()]));
        selector.toggle_tool("CustomTool");
        assert_eq!(selector.selected_output(), None);

        selector.toggle_tool("Read");
        assert_eq!(
            selector.selected_output(),
            Some(vec!["CustomTool".to_string()])
        );
        assert!(selector.render().contains("Other tools"));
    }

    #[test]
    fn agent_file_path_helpers_sanitize_by_source() {
        assert_eq!(sanitize_agent_filename("Review Agent!!"), "review-agent");

        let path = get_new_agent_file_path(
            AgentSource::User,
            "Review Agent!!",
            std::path::Path::new("/workspace"),
            std::path::Path::new("/home/user"),
            std::path::Path::new("/managed"),
        );

        assert_eq!(
            path,
            std::path::Path::new("/home/user")
                .join(".allthecodes")
                .join("agents")
                .join("review-agent.md")
        );
    }

    #[test]
    fn agent_type_flags_and_mode_state_cover_edit_flow() {
        let project = sample_agent();
        let plugin = AgentDefinition::new(
            "plugin-reviewer",
            "Use for plugin reviews.",
            "Review plugin output.",
            AgentSource::Plugin,
        );

        assert!(AgentSource::Project.is_editable());
        assert!(!AgentSource::Plugin.is_editable());
        assert!(plugin.is_plugin());
        assert!(!project.is_plugin());

        let state = AgentModeState::EditAgent {
            agent_type: project.agent_type.clone(),
            previous: Box::new(AgentModeState::ViewAgent {
                agent_type: project.agent_type.clone(),
                previous: Box::new(AgentModeState::ListAgents {
                    source: AgentSourceFilter::All,
                }),
            }),
        };

        if let AgentModeState::EditAgent { previous, .. } = state {
            assert!(matches!(*previous, AgentModeState::ViewAgent { .. }));
        } else {
            panic!("expected edit state");
        }
    }

    #[test]
    fn agent_mode_state_variants_preserve_navigation_context() {
        let states = [
            AgentModeState::MainMenu,
            AgentModeState::ListAgents {
                source: AgentSourceFilter::BuiltIn,
            },
            AgentModeState::AgentMenu {
                agent_type: "reviewer".to_string(),
                previous: Box::new(AgentModeState::MainMenu),
            },
            AgentModeState::ViewAgent {
                agent_type: "reviewer".to_string(),
                previous: Box::new(AgentModeState::ListAgents {
                    source: AgentSourceFilter::All,
                }),
            },
            AgentModeState::CreateAgent,
            AgentModeState::EditAgent {
                agent_type: "reviewer".to_string(),
                previous: Box::new(AgentModeState::ViewAgent {
                    agent_type: "reviewer".to_string(),
                    previous: Box::new(AgentModeState::MainMenu),
                }),
            },
            AgentModeState::DeleteConfirm {
                agent_type: "reviewer".to_string(),
                previous: Box::new(AgentModeState::AgentMenu {
                    agent_type: "reviewer".to_string(),
                    previous: Box::new(AgentModeState::MainMenu),
                }),
            },
        ];

        let labels = states
            .iter()
            .map(mode_state_label)
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            labels,
            "main,list:Built-in agents,menu:reviewer,view:reviewer,create,edit:reviewer,delete:reviewer"
        );
    }

    #[test]
    fn formatting_helpers_truncate_and_indent() {
        assert_eq!(truncate_middle("abcdefghijkl", 8), "ab...jkl");
        assert_eq!(truncate_middle("abc", 8), "abc");
        assert_eq!(
            indent_lines(["one".to_string(), "two".to_string()], 2),
            vec!["  one".to_string(), "  two".to_string()]
        );
    }

    fn section(name: &str, body: impl AsRef<str>) -> String {
        format!("## {name}\n{}", body.as_ref())
    }

    fn mode_state_label(state: &AgentModeState) -> String {
        match state {
            AgentModeState::MainMenu => "main".to_string(),
            AgentModeState::ListAgents { source } => {
                format!(
                    "list:{}",
                    super::utils::get_agent_source_display_name(*source)
                )
            }
            AgentModeState::AgentMenu {
                agent_type,
                previous,
            } => {
                let _ = previous.as_ref();
                format!("menu:{agent_type}")
            }
            AgentModeState::ViewAgent {
                agent_type,
                previous,
            } => {
                let _ = previous.as_ref();
                format!("view:{agent_type}")
            }
            AgentModeState::CreateAgent => "create".to_string(),
            AgentModeState::EditAgent {
                agent_type,
                previous,
            } => {
                let _ = previous.as_ref();
                format!("edit:{agent_type}")
            }
            AgentModeState::DeleteConfirm {
                agent_type,
                previous,
            } => {
                let _ = previous.as_ref();
                format!("delete:{agent_type}")
            }
        }
    }
}
