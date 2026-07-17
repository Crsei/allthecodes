use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};

use allthecodes_ipc_protocol::subsystem_events::AgentSettingsCommand;
use allthecodes_ipc_protocol::subsystem_types::{
    AgentDefinitionEntry, AgentDefinitionSource, AgentMemoryScope as IpcAgentMemoryScope,
};
use allthecodes_ipc_protocol::BackendMessage;

use crate::ui::agents::agent_detail::render_agent_detail;
use crate::ui::agents::agent_editor::{
    render_save_change_summary, AgentEditorState, AgentSaveChanges,
};
use crate::ui::agents::agents_list::AgentsListState;
use crate::ui::agents::color_picker::ColorPickerState;
use crate::ui::agents::generate_agent::{
    generate_agent_draft, render_generated_agent_preview, GenerateAgentRequest,
};
use crate::ui::agents::model_selector::{model_options_with_current, ModelOption};
use crate::ui::agents::new_agent_creation::create_agent_wizard::render_create_agent_wizard;
use crate::ui::agents::new_agent_creation::{AgentCreationMethod, AgentWizardData};
use crate::ui::agents::tool_selector::{default_agent_tools, ToolSelectorState};
use crate::ui::agents::types::{AgentDefinition, AgentMemoryScope, AgentSource, AgentSourceFilter};
use crate::ui::agents::utils::{get_agent_source_display_name, selection_marker};
use crate::ui::agents::validate_agent::{render_validation_result, validate_agent_definition};
use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::adapters::agents::{agent_entry_to_ui, agent_source_tabs};
use crate::ui::command_surface::{cycle_index, CommandSurfaceCursorAnchor, CommandSurfaceOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsSurface {
    pub(crate) state: AgentsListState,
    pub(crate) source_tabs: Vec<AgentSourceFilter>,
    pub(crate) source_index: usize,
    pub(crate) mode: AgentsSurfaceMode,
    cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentsSurfaceMode {
    List,
    Detail(Box<AgentDefinition>),
    Edit(AgentEditorState),
    EditColor {
        agent: Box<AgentDefinition>,
        picker: ColorPickerState,
    },
    EditModel {
        agent: Box<AgentDefinition>,
        options: Vec<ModelOption>,
        selected_index: usize,
    },
    EditTools {
        agent: Box<AgentDefinition>,
        selector: ToolSelectorState,
    },
    Create(CreateAgentState),
    SaveSummary {
        agent: Box<AgentDefinition>,
        changes: AgentSaveChanges,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateAgentState {
    data: AgentWizardData,
    step: usize,
    selector: ToolSelectorState,
    color_picker: ColorPickerState,
    model_options: Vec<ModelOption>,
    model_index: usize,
    error: Option<String>,
}

impl CreateAgentState {
    fn new() -> Self {
        let model_options = model_options_with_current(Some("MOTA"));
        Self {
            data: AgentWizardData::empty(),
            step: 0,
            selector: ToolSelectorState::new(default_agent_tools(), None),
            color_picker: ColorPickerState::new("agent", None),
            model_options,
            model_index: 0,
            error: None,
        }
    }

    fn render(&self) -> String {
        let mut sections = vec![render_create_agent_wizard(&self.data, self.step, false)];
        sections.push(match self.step {
            0 => render_create_location_step(self.data.location),
            1 => render_create_method_step(self.data.method),
            2 => render_text_field(
                "Generation goal",
                self.data.generation_goal.as_deref().unwrap_or_default(),
            ),
            3 => render_text_field(
                "Agent name",
                self.data.agent_type.as_deref().unwrap_or_default(),
            ),
            4 => render_text_field(
                "System prompt",
                self.data.system_prompt.as_deref().unwrap_or_default(),
            ),
            5 => render_text_field(
                "Description",
                self.data.when_to_use.as_deref().unwrap_or_default(),
            ),
            6 => self.selector.render(),
            7 => render_model_options(&self.model_options, self.model_index),
            8 => self.color_picker.render(),
            _ => self.render_confirm(),
        });
        if let Some(error) = &self.error {
            sections.push(format!("error: {error}"));
        }
        sections.join("\n\n")
    }

    fn render_confirm(&self) -> String {
        let existing: Vec<String> = Vec::new();
        let validation = validate_agent_definition(
            self.data.agent_type.as_deref().unwrap_or_default(),
            self.data.when_to_use.as_deref().unwrap_or_default(),
            self.data.system_prompt.as_deref().unwrap_or_default(),
            &existing,
        );
        [
            render_validation_result(&validation),
            format!(
                "location: {}",
                self.data
                    .location
                    .map(|source| source.display_name())
                    .unwrap_or("unset")
            ),
            format!(
                "tools: {}",
                crate::ui::agents::utils::tools_label(&self.data.tools)
            ),
            format!("model: {}", self.data.model.as_deref().unwrap_or("default")),
            format!(
                "color: {}",
                self.data.color.as_deref().unwrap_or("automatic")
            ),
        ]
        .join("\n")
    }
}

impl AgentsSurface {
    pub(crate) fn new(cwd: &Path) -> Self {
        let agents: Vec<AgentDefinition> =
            allthecodes_services::agent_definitions::list_all_agents(cwd)
                .into_iter()
                .map(agent_entry_to_ui)
                .collect();
        let source_tabs = agent_source_tabs(&agents);
        let mut state = AgentsListState::new(AgentSourceFilter::All, agents);
        state.show_create_new = create_entry_for_source(state.source);
        state.create_new_selected = state.show_create_new;
        Self {
            state,
            source_tabs,
            source_index: 0,
            mode: AgentsSurfaceMode::List,
            cwd: cwd.to_path_buf(),
        }
    }

    pub(crate) fn render(&self) -> String {
        match &self.mode {
            AgentsSurfaceMode::List => self.render_list(),
            AgentsSurfaceMode::Detail(agent) => self.render_detail(agent),
            AgentsSurfaceMode::Edit(editor) => BetterViewPanel::new("Agents / Edit")
                .summary(format!("agent={}", editor.agent.agent_type))
                .sections_title("Mode")
                .sections(vec!["Edit menu".to_string()], 0)
                .detail_title("Actions")
                .detail_lines(editor.render_menu().lines().map(str::to_string).collect())
                .footer("Up/Down action | Enter select | Backspace back | Esc close")
                .render(),
            AgentsSurfaceMode::EditColor { agent, picker } => self.render_text_mode(
                format!("Agents / {} / Color", agent.agent_type),
                "Color",
                picker.render(),
                "Up/Down color | Enter save | Backspace back | Esc close",
            ),
            AgentsSurfaceMode::EditModel {
                agent,
                options,
                selected_index,
            } => self.render_text_mode(
                format!("Agents / {} / Model", agent.agent_type),
                "Model",
                render_model_options(options, *selected_index),
                "Up/Down model | Enter save | Backspace back | Esc close",
            ),
            AgentsSurfaceMode::EditTools { agent, selector } => self.render_text_mode(
                format!("Agents / {} / Tools", agent.agent_type),
                "Tools",
                selector.render(),
                "Up/Down tool | Space toggle | Enter save | Backspace back | Esc close",
            ),
            AgentsSurfaceMode::Create(create) => self.render_text_mode(
                "Agents / Create".to_string(),
                "Create",
                create.render(),
                "Type text | Enter next/save | Up/Down choose | Space toggle | Backspace edit/back | Esc close",
            ),
            AgentsSurfaceMode::SaveSummary {
                agent,
                changes,
                message,
            } => self.render_text_mode(
                format!("Agents / {} / Saved", agent.agent_type),
                "Saved",
                format!("{message}\n\n{}", render_save_change_summary(agent, changes)),
                "Enter detail | Backspace list | Esc close",
            ),
        }
    }

    fn render_list(&self) -> String {
        let sections = self
            .source_tabs
            .iter()
            .map(|source| get_agent_source_display_name(*source))
            .collect::<Vec<_>>();
        let visible = self.state.visible_agents();
        let mut detail_lines = Vec::new();
        if self.state.show_create_new {
            detail_lines.push(selected_row(
                "Create new agent",
                "Project or user scoped markdown agent",
                self.state.create_new_selected,
            ));
        }
        if visible.is_empty() {
            detail_lines.push("No agents available for this source".to_string());
        } else {
            detail_lines.extend(visible.iter().enumerate().map(|(idx, agent)| {
                let detail = format!("{}  {}", agent.source.display_name(), agent.when_to_use);
                selected_row(
                    &agent.agent_type,
                    detail,
                    !self.state.create_new_selected && idx == self.state.selected_index,
                )
            }));
        }
        if let Some(agent) = self.state.selected_agent() {
            detail_lines.push(String::new());
            detail_lines.push("Details".to_string());
            detail_lines.push(plain_row(
                "command:",
                format!("/agents show {}", agent.agent_type),
            ));
        }
        BetterViewPanel::new("Agents")
            .summary(format!(
                "source={} agents={}",
                sections.get(self.source_index).cloned().unwrap_or_default(),
                visible.len()
            ))
            .sections_title("Sources")
            .sections(sections, self.source_index)
            .detail_title("Agents")
            .detail_lines(detail_lines)
            .footer("Left/Right source | Up/Down agent | Enter select | n new | Esc close")
            .render()
    }

    fn render_detail(&self, agent: &AgentDefinition) -> String {
        let mut detail_lines = vec![format!("Agent detail: {}", agent.agent_type)];
        detail_lines.extend(render_agent_detail(agent, 80).lines().map(str::to_string));
        detail_lines.push(String::new());
        detail_lines.push(format!("Enter submit `/agents show {}`", agent.agent_type));
        if agent.source.is_editable() {
            detail_lines.push("e edit this agent".to_string());
        }
        BetterViewPanel::new(format!("Agents / {}", agent.agent_type))
            .summary(format!("source={}", agent.source.display_name()))
            .sections_title("View")
            .sections(vec!["Agent detail".to_string(), "Commands".to_string()], 0)
            .detail_title("Details")
            .detail_lines(detail_lines)
            .footer("Backspace/b list | e edit | Enter show | Esc close")
            .render()
    }

    fn render_text_mode(
        &self,
        title: String,
        detail_title: &str,
        body: String,
        footer: &str,
    ) -> String {
        BetterViewPanel::new(title)
            .summary(self.cwd.display().to_string())
            .sections_title("Agents")
            .sections(vec!["List".to_string(), detail_title.to_string()], 1)
            .detail_title(detail_title)
            .detail_lines(body.lines().map(str::to_string).collect())
            .footer(footer)
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match self.mode.clone() {
            AgentsSurfaceMode::List => self.handle_list_key(key),
            AgentsSurfaceMode::Detail(agent) => self.handle_detail_key(key, *agent),
            AgentsSurfaceMode::Edit(editor) => self.handle_edit_key(key, editor),
            AgentsSurfaceMode::EditColor { agent, picker } => {
                self.handle_color_key(key, *agent, picker)
            }
            AgentsSurfaceMode::EditModel {
                agent,
                options,
                selected_index,
            } => self.handle_model_key(key, *agent, options, selected_index),
            AgentsSurfaceMode::EditTools { agent, selector } => {
                self.handle_tools_key(key, *agent, selector)
            }
            AgentsSurfaceMode::Create(create) => self.handle_create_key(key, create),
            AgentsSurfaceMode::SaveSummary { agent, .. } => match key.code {
                KeyCode::Enter => {
                    self.mode = AgentsSurfaceMode::Detail(agent);
                    CommandSurfaceOutcome::None
                }
                KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                    self.mode = AgentsSurfaceMode::List;
                    CommandSurfaceOutcome::None
                }
                _ => CommandSurfaceOutcome::None,
            },
        }
    }

    pub(crate) fn cursor_anchor(&self) -> Option<CommandSurfaceCursorAnchor> {
        let AgentsSurfaceMode::Create(create) = &self.mode else {
            return None;
        };
        if !is_text_step(create.step) {
            return None;
        }
        let value = match create.step {
            2 => create.data.generation_goal.as_deref().unwrap_or_default(),
            3 => create.data.agent_type.as_deref().unwrap_or_default(),
            4 => create.data.system_prompt.as_deref().unwrap_or_default(),
            5 => create.data.when_to_use.as_deref().unwrap_or_default(),
            _ => return None,
        };
        Some(CommandSurfaceCursorAnchor::Field {
            marker: "input: ".to_string(),
            value: value.to_string(),
        })
    }

    fn handle_detail_key(
        &mut self,
        key: KeyEvent,
        agent: AgentDefinition,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                self.mode = AgentsSurfaceMode::List;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('e') if agent.source.is_editable() => {
                self.mode = AgentsSurfaceMode::Edit(AgentEditorState::new(agent));
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => {
                CommandSurfaceOutcome::Submit(format!("/agents show {}", agent.agent_type))
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_edit_key(
        &mut self,
        key: KeyEvent,
        mut editor: AgentEditorState,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                self.mode = AgentsSurfaceMode::Detail(Box::new(editor.agent));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                editor.selected_menu_index = if editor.selected_menu_index == 0 {
                    3
                } else {
                    editor.selected_menu_index - 1
                };
                self.mode = AgentsSurfaceMode::Edit(editor);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                editor.selected_menu_index = (editor.selected_menu_index + 1) % 4;
                self.mode = AgentsSurfaceMode::Edit(editor);
            }
            KeyCode::Enter => match editor.selected_menu_index {
                0 => {
                    let file_path =
                        crate::ui::agents::agent_file_utils::get_actual_relative_agent_file_path(
                            &editor.agent,
                        );
                    return CommandSurfaceOutcome::FillPrompt(format!(
                        "Open {} in your editor to change {}",
                        file_path, editor.agent.agent_type
                    ));
                }
                1 => {
                    let mut selector =
                        ToolSelectorState::new(default_agent_tools(), editor.agent.tools.clone());
                    selector.show_individual_tools = true;
                    self.mode = AgentsSurfaceMode::EditTools {
                        agent: Box::new(editor.agent),
                        selector,
                    };
                }
                2 => {
                    let options = model_options_with_current(editor.agent.model.as_deref());
                    let selected_index = options
                        .iter()
                        .position(|option| {
                            Some(option.value.as_str()) == editor.agent.model.as_deref()
                        })
                        .unwrap_or(0);
                    self.mode = AgentsSurfaceMode::EditModel {
                        agent: Box::new(editor.agent),
                        options,
                        selected_index,
                    };
                }
                3 => {
                    let picker = ColorPickerState::new(
                        editor.agent.agent_type.clone(),
                        editor.agent.color.as_deref(),
                    );
                    self.mode = AgentsSurfaceMode::EditColor {
                        agent: Box::new(editor.agent),
                        picker,
                    };
                }
                _ => self.mode = AgentsSurfaceMode::Edit(editor),
            },
            _ => self.mode = AgentsSurfaceMode::Edit(editor),
        }
        CommandSurfaceOutcome::None
    }

    fn handle_color_key(
        &mut self,
        key: KeyEvent,
        mut agent: AgentDefinition,
        mut picker: ColorPickerState,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                self.mode = AgentsSurfaceMode::Edit(AgentEditorState::new(agent));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                picker.move_previous();
                self.mode = AgentsSurfaceMode::EditColor {
                    agent: Box::new(agent),
                    picker,
                };
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                picker.move_next();
                self.mode = AgentsSurfaceMode::EditColor {
                    agent: Box::new(agent),
                    picker,
                };
            }
            KeyCode::Enter => {
                let color = picker.selected_color().map(str::to_string);
                agent.color = color.clone();
                self.persist_agent(
                    agent,
                    AgentSaveChanges {
                        color,
                        ..AgentSaveChanges::default()
                    },
                );
            }
            _ => {
                self.mode = AgentsSurfaceMode::EditColor {
                    agent: Box::new(agent),
                    picker,
                };
            }
        }
        CommandSurfaceOutcome::None
    }

    fn handle_model_key(
        &mut self,
        key: KeyEvent,
        mut agent: AgentDefinition,
        options: Vec<ModelOption>,
        mut selected_index: usize,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                self.mode = AgentsSurfaceMode::Edit(AgentEditorState::new(agent));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                selected_index = if selected_index == 0 {
                    options.len().saturating_sub(1)
                } else {
                    selected_index - 1
                };
                self.mode = AgentsSurfaceMode::EditModel {
                    agent: Box::new(agent),
                    options,
                    selected_index,
                };
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                if !options.is_empty() {
                    selected_index = (selected_index + 1) % options.len();
                }
                self.mode = AgentsSurfaceMode::EditModel {
                    agent: Box::new(agent),
                    options,
                    selected_index,
                };
            }
            KeyCode::Enter => {
                let model = options
                    .get(selected_index)
                    .map(|option| option.value.clone());
                agent.model = model.clone();
                self.persist_agent(
                    agent,
                    AgentSaveChanges {
                        model,
                        ..AgentSaveChanges::default()
                    },
                );
            }
            _ => {
                self.mode = AgentsSurfaceMode::EditModel {
                    agent: Box::new(agent),
                    options,
                    selected_index,
                };
            }
        }
        CommandSurfaceOutcome::None
    }

    fn handle_tools_key(
        &mut self,
        key: KeyEvent,
        mut agent: AgentDefinition,
        mut selector: ToolSelectorState,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Left => {
                self.mode = AgentsSurfaceMode::Edit(AgentEditorState::new(agent));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                selector.move_prev();
                self.mode = AgentsSurfaceMode::EditTools {
                    agent: Box::new(agent),
                    selector,
                };
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                selector.move_next();
                self.mode = AgentsSurfaceMode::EditTools {
                    agent: Box::new(agent),
                    selector,
                };
            }
            KeyCode::Char(' ') => {
                selector.toggle_focused();
                self.mode = AgentsSurfaceMode::EditTools {
                    agent: Box::new(agent),
                    selector,
                };
            }
            KeyCode::Enter => {
                let tools = selector.selected_output();
                agent.tools = tools.clone();
                self.persist_agent(
                    agent,
                    AgentSaveChanges {
                        tools,
                        ..AgentSaveChanges::default()
                    },
                );
            }
            _ => {
                self.mode = AgentsSurfaceMode::EditTools {
                    agent: Box::new(agent),
                    selector,
                };
            }
        }
        CommandSurfaceOutcome::None
    }

    fn handle_create_key(
        &mut self,
        key: KeyEvent,
        mut create: CreateAgentState,
    ) -> CommandSurfaceOutcome {
        create.error = None;
        match key.code {
            KeyCode::Backspace if create.step == 0 => {
                self.mode = AgentsSurfaceMode::List;
            }
            KeyCode::Backspace | KeyCode::Left => {
                if is_text_step(create.step) {
                    edit_current_create_text(&mut create, KeyCode::Backspace);
                } else {
                    create.step = create.step.saturating_sub(1);
                }
                self.mode = AgentsSurfaceMode::Create(create);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_create_choice(&mut create, -1);
                self.mode = AgentsSurfaceMode::Create(create);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                move_create_choice(&mut create, 1);
                self.mode = AgentsSurfaceMode::Create(create);
            }
            KeyCode::Char(' ') if create.step == 6 => {
                create.selector.toggle_focused();
                create.data.tools = create.selector.selected_output();
                self.mode = AgentsSurfaceMode::Create(create);
            }
            KeyCode::Char(ch) if is_text_step(create.step) => {
                edit_current_create_text(&mut create, KeyCode::Char(ch));
                self.mode = AgentsSurfaceMode::Create(create);
            }
            KeyCode::Enter => {
                if create.step == 2 && create.data.method == Some(AgentCreationMethod::Generate) {
                    apply_generated_draft(&mut create);
                }
                if create.step >= 9 {
                    match self.persist_created_agent(&create) {
                        Ok(agent) => {
                            self.refresh_agents(Some(&agent));
                            self.mode = AgentsSurfaceMode::SaveSummary {
                                agent: Box::new(agent),
                                changes: AgentSaveChanges::default(),
                                message: "Agent saved".to_string(),
                            };
                        }
                        Err(error) => {
                            create.error = Some(error);
                            self.mode = AgentsSurfaceMode::Create(create);
                        }
                    }
                } else {
                    create.step += 1;
                    self.mode = AgentsSurfaceMode::Create(create);
                }
            }
            _ => self.mode = AgentsSurfaceMode::Create(create),
        }
        CommandSurfaceOutcome::None
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::Char('[') => {
                self.switch_source_tab(-1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.switch_source_tab(1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.state.move_prev();
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.state.move_next();
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('n') | KeyCode::Char('c') => {
                self.mode = AgentsSurfaceMode::Create(CreateAgentState::new());
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => {
                if self.state.create_new_selected {
                    self.mode = AgentsSurfaceMode::Create(CreateAgentState::new());
                } else if let Some(agent) = self.state.selected_agent() {
                    self.mode = AgentsSurfaceMode::Detail(Box::new(agent));
                }
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn switch_source_tab(&mut self, direction: isize) {
        self.source_index = cycle_index(self.source_index, self.source_tabs.len(), direction);
        if let Some(source) = self.source_tabs.get(self.source_index).copied() {
            self.state.source = source;
            self.state.selected_index = 0;
            self.state.show_create_new = create_entry_for_source(source);
            self.state.create_new_selected = self.state.show_create_new;
            self.mode = AgentsSurfaceMode::List;
        }
    }

    fn persist_agent(&mut self, agent: AgentDefinition, changes: AgentSaveChanges) {
        match upsert_agent(agent.clone()) {
            Ok(saved) => {
                self.refresh_agents(Some(&saved));
                self.mode = AgentsSurfaceMode::SaveSummary {
                    agent: Box::new(saved),
                    changes,
                    message: "Agent saved".to_string(),
                };
            }
            Err(error) => {
                let mut editor = AgentEditorState::new(agent);
                editor.error = Some(error);
                self.mode = AgentsSurfaceMode::Edit(editor);
            }
        }
    }

    fn persist_created_agent(&self, create: &CreateAgentState) -> Result<AgentDefinition, String> {
        let source = create.data.location.unwrap_or(AgentSource::Project);
        let mut agent = AgentDefinition::new(
            create
                .data
                .agent_type
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            create
                .data
                .when_to_use
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            create
                .data
                .system_prompt
                .as_deref()
                .unwrap_or_default()
                .trim()
                .to_string(),
            source,
        );
        agent.tools = create.data.tools.clone();
        agent.model = create.data.model.clone();
        agent.color = create.data.color.clone();
        upsert_agent(agent)
    }

    fn refresh_agents(&mut self, selected: Option<&AgentDefinition>) {
        self.state.agents = allthecodes_services::agent_definitions::list_all_agents(&self.cwd)
            .into_iter()
            .map(agent_entry_to_ui)
            .collect();
        self.source_tabs = agent_source_tabs(&self.state.agents);
        if let Some(selected) = selected {
            self.state.source = AgentSourceFilter::Source(selected.source);
            self.source_index = self
                .source_tabs
                .iter()
                .position(|source| *source == self.state.source)
                .unwrap_or(0);
            let visible = self.state.visible_agents();
            self.state.selected_index = visible
                .iter()
                .position(|agent| agent.agent_type == selected.agent_type)
                .unwrap_or(0);
            self.state.create_new_selected = false;
            self.state.show_create_new = create_entry_for_source(self.state.source);
        }
    }
}

fn create_entry_for_source(source: AgentSourceFilter) -> bool {
    matches!(
        source,
        AgentSourceFilter::All
            | AgentSourceFilter::Source(AgentSource::User)
            | AgentSourceFilter::Source(AgentSource::Project)
    )
}

fn render_model_options(options: &[ModelOption], selected_index: usize) -> String {
    let mut lines =
        vec!["Model determines the agent's reasoning capabilities and speed.".to_string()];
    for (idx, option) in options.iter().enumerate() {
        lines.push(format!(
            "{} {:<12} {}",
            selection_marker(idx == selected_index),
            option.label,
            option.description
        ));
    }
    lines.join("\n")
}

fn render_create_location_step(location: Option<AgentSource>) -> String {
    [AgentSource::Project, AgentSource::User]
        .into_iter()
        .map(|source| {
            format!(
                "{} {}",
                selection_marker(location.unwrap_or(AgentSource::Project) == source),
                source.display_name()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_create_method_step(method: Option<AgentCreationMethod>) -> String {
    [
        (AgentCreationMethod::Generate, "Generate from goal"),
        (AgentCreationMethod::Manual, "Manual"),
    ]
    .into_iter()
    .map(|(value, label)| {
        format!(
            "{} {label}",
            selection_marker(method.unwrap_or(AgentCreationMethod::Generate) == value)
        )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn render_text_field(label: &str, value: &str) -> String {
    format!("{label}\ninput: {value}")
}

fn is_text_step(step: usize) -> bool {
    matches!(step, 2..=5)
}

fn edit_current_create_text(create: &mut CreateAgentState, code: KeyCode) {
    let target = match create.step {
        2 => &mut create.data.generation_goal,
        3 => &mut create.data.agent_type,
        4 => &mut create.data.system_prompt,
        5 => &mut create.data.when_to_use,
        _ => return,
    };
    let text = target.get_or_insert_with(String::new);
    match code {
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(ch) => text.push(ch),
        _ => {}
    }
}

fn move_create_choice(create: &mut CreateAgentState, direction: isize) {
    match create.step {
        0 => {
            create.data.location =
                Some(match create.data.location.unwrap_or(AgentSource::Project) {
                    AgentSource::Project => AgentSource::User,
                    _ => AgentSource::Project,
                });
        }
        1 => {
            create.data.method = Some(
                match create.data.method.unwrap_or(AgentCreationMethod::Generate) {
                    AgentCreationMethod::Generate => AgentCreationMethod::Manual,
                    AgentCreationMethod::Manual => AgentCreationMethod::Generate,
                },
            );
        }
        6 if direction < 0 => {
            create.selector.move_prev();
            create.data.tools = create.selector.selected_output();
        }
        6 => {
            create.selector.move_next();
            create.data.tools = create.selector.selected_output();
        }
        7 if !create.model_options.is_empty() => {
            create.model_index =
                cycle_index(create.model_index, create.model_options.len(), direction);
            create.data.model = create
                .model_options
                .get(create.model_index)
                .map(|option| option.value.clone());
        }
        8 if direction < 0 => {
            create.color_picker.move_previous();
            create.data.color = create.color_picker.selected_color().map(str::to_string);
        }
        8 => {
            create.color_picker.move_next();
            create.data.color = create.color_picker.selected_color().map(str::to_string);
        }
        _ => {}
    }
}

fn apply_generated_draft(create: &mut CreateAgentState) {
    let request = GenerateAgentRequest {
        goal: create
            .data
            .generation_goal
            .clone()
            .unwrap_or_else(|| "handle focused implementation work".to_string()),
        source: create.data.location.unwrap_or(AgentSource::Project),
        preferred_tools: create.selector.selected_output().unwrap_or_else(|| {
            default_agent_tools()
                .into_iter()
                .map(|tool| tool.name)
                .collect()
        }),
    };
    let draft = generate_agent_draft(&request);
    let preview = render_generated_agent_preview(&draft);
    create.data.agent_type = Some(draft.agent.agent_type);
    create.data.when_to_use = Some(draft.agent.when_to_use);
    create.data.system_prompt = Some(draft.agent.system_prompt);
    create.data.tools = draft.agent.tools;
    create.error = Some(preview);
}

fn upsert_agent(agent: AgentDefinition) -> Result<AgentDefinition, String> {
    let entry = agent_to_entry(agent)?;
    let messages = allthecodes_services::agent_definitions::handle(AgentSettingsCommand::Upsert {
        entry: Box::new(entry),
    });
    for message in messages {
        if let BackendMessage::AgentSettingsEvent { event } = message {
            match event {
                allthecodes_ipc_protocol::subsystem_events::AgentSettingsEvent::Changed {
                    entry: Some(entry),
                    ..
                } => return Ok(agent_entry_to_ui(*entry)),
                allthecodes_ipc_protocol::subsystem_events::AgentSettingsEvent::Error {
                    error,
                    ..
                } => {
                    return Err(error);
                }
                _ => {}
            }
        }
    }
    Err("agent save did not return a changed entry".to_string())
}

fn agent_to_entry(agent: AgentDefinition) -> Result<AgentDefinitionEntry, String> {
    let source = match agent.source {
        AgentSource::User => AgentDefinitionSource::User,
        AgentSource::Project | AgentSource::Local => AgentDefinitionSource::Project,
        AgentSource::BuiltIn | AgentSource::Plugin | AgentSource::Policy | AgentSource::Flag => {
            return Err(format!(
                "{} agents are read-only",
                agent.source.display_name()
            ));
        }
    };
    Ok(AgentDefinitionEntry {
        name: agent.agent_type,
        description: agent.when_to_use,
        system_prompt: agent.system_prompt,
        tools: agent.tools.unwrap_or_default(),
        disallowed_tools: Vec::new(),
        model: agent.model,
        color: agent.color,
        permission_mode: None,
        memory: agent.memory.and_then(|memory| match memory {
            AgentMemoryScope::User => Some(IpcAgentMemoryScope::User),
            AgentMemoryScope::Project => Some(IpcAgentMemoryScope::Project),
            AgentMemoryScope::Local => Some(IpcAgentMemoryScope::Local),
            AgentMemoryScope::None => None,
        }),
        max_turns: None,
        effort: agent.effort,
        background: false,
        isolation: None,
        skills: agent.skills,
        hooks: serde_json::Value::Null,
        mcp_servers: Vec::new(),
        initial_prompt: None,
        filename: agent.filename,
        source,
        file_path: agent.base_dir,
    })
}
