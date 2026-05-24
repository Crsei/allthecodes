use crossterm::event::KeyEvent;

use crate::ui::better_view_panel::{selected_row, BetterViewPanel};
use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::form_navigation::{FormOption, FormTab, TabbedFormEvent, TabbedFormState};
use crate::ui::permissions::rules::add_permission_rules::render_add_permission_rules;
use crate::ui::permissions::rules::add_workspace_directory::render_add_workspace_directory;
use crate::ui::permissions::rules::permission_rule_description::render_permission_rule_description;
use crate::ui::permissions::rules::permission_rule_input::PermissionRuleInputState;
use crate::ui::permissions::rules::permission_rule_list::render_permission_rule_list;
use crate::ui::permissions::rules::recent_denials_tab::render_recent_denials_tab;
use crate::ui::permissions::rules::remove_workspace_directory::render_remove_workspace_directory;
use crate::ui::permissions::rules::workspace_tab::render_workspace_tab;
use crate::ui::permissions::rules::{PermissionRule, RecentDenial, WorkspaceDirectory};
use crate::ui::permissions::utils::{PermissionDecision, PermissionScope};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::{
    PermissionMode, ToolPermissionContext, ToolPermissionRulesBySource,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsSurface {
    pub(crate) state: TabbedFormState,
    rules: Vec<PermissionRule>,
    workspace_directories: Vec<WorkspaceDirectory>,
    recent_denials: Vec<RecentDenial>,
}

impl PermissionsSurface {
    pub(crate) fn new(app_state: &AppState) -> Self {
        let perm = &app_state.tool_permission_context;
        let mut rules = Vec::new();
        collect_rules(
            &mut rules,
            &perm.always_deny_rules,
            PermissionDecision::Deny,
        );
        collect_rules(&mut rules, &perm.always_ask_rules, PermissionDecision::Ask);
        collect_rules(
            &mut rules,
            &perm.always_allow_rules,
            PermissionDecision::AlwaysAllow,
        );
        collect_rules(
            &mut rules,
            &perm.session_allow_rules,
            PermissionDecision::Allow,
        );
        let mut workspace_directories = perm
            .additional_working_directories
            .values()
            .map(|directory| WorkspaceDirectory {
                path: directory.path.clone(),
                trusted: !directory.read_only,
            })
            .collect::<Vec<_>>();
        workspace_directories.sort_by(|a, b| a.path.cmp(&b.path));

        Self {
            state: TabbedFormState::new("Permissions", vec![modes_tab(perm)]),
            rules,
            workspace_directories,
            recent_denials: Vec::new(),
        }
    }

    pub(crate) fn render(&self) -> String {
        let sections = self
            .state
            .tabs
            .iter()
            .map(|tab| tab.label.clone())
            .collect::<Vec<_>>();
        let mut detail_lines = if let Some(tab) = self.state.active_tab() {
            tab.options
                .iter()
                .enumerate()
                .map(|(idx, option)| {
                    selected_row(
                        &option.label,
                        &option.description,
                        idx == self.state.selected_index,
                    )
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if self.active_tab_id() == Some("rules") {
            detail_lines.push(String::new());
            detail_lines.extend(
                render_permission_rule_list(&self.rules, self.state.selected_index)
                    .lines()
                    .map(str::to_string),
            );
            if let Some(rule) = self.rules.get(
                self.state
                    .selected_index
                    .min(self.rules.len().saturating_sub(1)),
            ) {
                detail_lines.push(String::new());
                detail_lines.extend(
                    render_permission_rule_description(rule)
                        .lines()
                        .map(str::to_string),
                );
            }
        } else if self.active_tab_id() == Some("workspace") {
            detail_lines.push(String::new());
            detail_lines.extend(
                render_workspace_tab(&self.workspace_directories, self.state.selected_index)
                    .lines()
                    .map(str::to_string),
            );
            detail_lines.push(String::new());
            detail_lines.extend(
                render_add_workspace_directory("<path>", true)
                    .lines()
                    .map(str::to_string),
            );
            if let Some(directory) = self.workspace_directories.get(
                self.state
                    .selected_index
                    .min(self.workspace_directories.len().saturating_sub(1)),
            ) {
                detail_lines.push(String::new());
                detail_lines.extend(
                    render_remove_workspace_directory(&directory.path, self.rules.len())
                        .lines()
                        .map(str::to_string),
                );
            }
        } else if self.active_tab_id() == Some("denials") {
            detail_lines.push(String::new());
            detail_lines.extend(
                render_recent_denials_tab(&self.recent_denials)
                    .lines()
                    .map(str::to_string),
            );
        } else if self.active_tab_id() == Some("mutate") {
            detail_lines.push(String::new());
            let inputs = [
                PermissionRuleInputState::new("Bash(cargo test*)", PermissionDecision::AlwaysAllow),
                PermissionRuleInputState::new("Bash(*)", PermissionDecision::Ask),
                PermissionRuleInputState::new("Write(/tmp/**)", PermissionDecision::Deny),
            ];
            detail_lines.extend(
                render_add_permission_rules(&inputs)
                    .lines()
                    .map(str::to_string),
            );
        }
        BetterViewPanel::new("Permissions")
            .summary(format!(
                "mode={} rules={}",
                self.state
                    .active_tab()
                    .and_then(|tab| tab
                        .options
                        .iter()
                        .find(|option| option.description == "current"))
                    .map(|option| option.label.trim_end_matches(" (current)"))
                    .unwrap_or("default"),
                self.rules.len()
            ))
            .sections(sections, self.state.active_tab)
            .detail_title(
                self.state
                    .active_tab()
                    .map(|tab| tab.label.clone())
                    .unwrap_or_else(|| "Detail".to_string()),
            )
            .detail_lines(detail_lines)
            .footer("Left/Right section | Up/Down navigate | Enter select | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match self.state.handle_key(key) {
            TabbedFormEvent::Selected { option_id, .. } => match option_id.as_str() {
                "show" => CommandSurfaceOutcome::Submit("/permissions".to_string()),
                "mode-default" => {
                    CommandSurfaceOutcome::Submit("/permissions mode default".to_string())
                }
                "mode-auto" => {
                    CommandSurfaceOutcome::Submit("/permissions mode auto --confirm".to_string())
                }
                "mode-bypass" => {
                    CommandSurfaceOutcome::Submit("/permissions mode bypass --confirm".to_string())
                }
                "mode-plan" => CommandSurfaceOutcome::Submit("/permissions mode plan".to_string()),
                "mode-accept-edits" => {
                    CommandSurfaceOutcome::Submit("/permissions mode acceptEdits".to_string())
                }
                "mode-dont-ask" => {
                    CommandSurfaceOutcome::Submit("/permissions mode dontAsk".to_string())
                }
                "add-allow" => CommandSurfaceOutcome::FillPrompt(
                    "/permissions allow <rule> --project".to_string(),
                ),
                "add-ask" => CommandSurfaceOutcome::FillPrompt(
                    "/permissions ask <rule> --project".to_string(),
                ),
                "add-deny" => CommandSurfaceOutcome::FillPrompt(
                    "/permissions deny <rule> --project".to_string(),
                ),
                "session-grant" => {
                    CommandSurfaceOutcome::FillPrompt("/permissions session-grant ".to_string())
                }
                "clear-session" => {
                    CommandSurfaceOutcome::Submit("/permissions clear-session-grants".to_string())
                }
                "reset" => CommandSurfaceOutcome::Submit("/permissions reset".to_string()),
                _ => CommandSurfaceOutcome::None,
            },
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn active_tab_id(&self) -> Option<&str> {
        self.state.active_tab().map(|tab| tab.id.as_str())
    }
}

fn modes_tab(perm: &ToolPermissionContext) -> FormTab {
    let mut auto = FormOption::new("mode-auto", "Auto mode")
        .with_description("classifier reviews permission prompts");
    if perm.is_auto_mode_available == Some(false) {
        auto = auto.disabled();
    }
    let mut bypass = FormOption::new("mode-bypass", "Full Access")
        .with_description("requires explicit danger confirmation");
    if !perm.is_bypass_permissions_mode_available {
        bypass = bypass.disabled();
    }

    FormTab::new(
        "modes",
        "Mode",
        vec![
            mode_option("mode-default", "Default", PermissionMode::Default, perm),
            mode_option_from_option(auto, PermissionMode::Auto, perm, "Auto-review"),
            mode_option_from_option(bypass, PermissionMode::Bypass, perm, "Full Access"),
        ],
    )
}

fn mode_option(
    id: impl Into<String>,
    label: impl Into<String>,
    mode: PermissionMode,
    perm: &ToolPermissionContext,
) -> FormOption {
    let current = if perm.mode == mode {
        "current"
    } else {
        "select"
    };
    let mut label = label.into();
    if perm.mode == mode {
        label.push_str(" (current)");
    }
    FormOption::new(id, label).with_description(current)
}

fn mode_option_from_option(
    option: FormOption,
    mode: PermissionMode,
    perm: &ToolPermissionContext,
    label: &str,
) -> FormOption {
    let mut option = option;
    option.label = if perm.mode == mode {
        format!("{label} (current)")
    } else {
        label.to_string()
    };
    option.description = if perm.mode == mode {
        "current".to_string()
    } else {
        option.description
    };
    option
}

fn collect_rules(
    out: &mut Vec<PermissionRule>,
    rules_by_source: &ToolPermissionRulesBySource,
    decision: PermissionDecision,
) {
    let mut sorted: Vec<(&String, &Vec<String>)> = rules_by_source.iter().collect();
    sorted.sort_by_key(|(source, _)| (*source).clone());
    for (source, rules) in sorted {
        for rule in rules {
            out.push(PermissionRule::new(
                rule,
                decision,
                scope_for_source(source),
                source,
            ));
        }
    }
}

fn scope_for_source(source: &str) -> PermissionScope {
    match source {
        "session" => PermissionScope::Session,
        "local" => PermissionScope::Local,
        "policy" | "managed" => PermissionScope::Policy,
        _ => PermissionScope::Project,
    }
}
