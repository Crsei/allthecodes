//! Full-build feature panel registry.

use crossterm::event::KeyEvent;

use super::form_navigation::{FormOption, FormTab, TabbedFormEvent, TabbedFormState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeaturePanelKind {
    Mcp,
    Agents,
    Subagents,
    Teams,
    Lsp,
    Settings,
    Sandbox,
    Plugins,
    Skills,
    Tasks,
}

impl FeaturePanelKind {
    pub fn title(self) -> &'static str {
        match self {
            FeaturePanelKind::Mcp => "MCP",
            FeaturePanelKind::Agents => "Agents",
            FeaturePanelKind::Subagents => "Subagents",
            FeaturePanelKind::Teams => "Teams",
            FeaturePanelKind::Lsp => "LSP",
            FeaturePanelKind::Settings => "Settings",
            FeaturePanelKind::Sandbox => "Sandbox",
            FeaturePanelKind::Plugins => "Plugins",
            FeaturePanelKind::Skills => "Skills",
            FeaturePanelKind::Tasks => "Tasks",
        }
    }
}

impl std::str::FromStr for FeaturePanelKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mcp" => Ok(FeaturePanelKind::Mcp),
            "agents" => Ok(FeaturePanelKind::Agents),
            "subagents" => Ok(FeaturePanelKind::Subagents),
            "agent-runtime" => Ok(FeaturePanelKind::Subagents),
            "teams" => Ok(FeaturePanelKind::Teams),
            "lsp" => Ok(FeaturePanelKind::Lsp),
            "settings" => Ok(FeaturePanelKind::Settings),
            "sandbox" => Ok(FeaturePanelKind::Sandbox),
            "plugins" => Ok(FeaturePanelKind::Plugins),
            "skills" => Ok(FeaturePanelKind::Skills),
            "tasks" => Ok(FeaturePanelKind::Tasks),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    Ready,
    NeedsConfig,
    Running,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePanel {
    pub kind: FeaturePanelKind,
    pub state: PanelState,
    pub badge: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeaturePanelFormState {
    form: TabbedFormState,
}

impl FeaturePanelFormState {
    pub fn new(panels: Vec<FeaturePanel>) -> Self {
        Self {
            form: TabbedFormState::new(
                "Feature panels",
                panels.iter().map(feature_panel_tab).collect(),
            ),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TabbedFormEvent {
        self.form.handle_key(key)
    }

    pub fn selected_panel_action(&self) -> Option<(FeaturePanelKind, String)> {
        let tab = self.form.active_tab()?;
        let option = self.form.selected_option()?;
        if !option.enabled {
            return None;
        }
        Some((tab.id.parse().ok()?, option.id.clone()))
    }
    pub fn render(&self) -> String {
        self.form.render_lines().join("\n")
    }
}

pub fn default_panels() -> Vec<FeaturePanel> {
    [
        FeaturePanelKind::Mcp,
        FeaturePanelKind::Agents,
        FeaturePanelKind::Subagents,
        FeaturePanelKind::Teams,
        FeaturePanelKind::Lsp,
        FeaturePanelKind::Settings,
        FeaturePanelKind::Sandbox,
        FeaturePanelKind::Plugins,
        FeaturePanelKind::Skills,
        FeaturePanelKind::Tasks,
    ]
    .into_iter()
    .map(|kind| FeaturePanel {
        kind,
        state: PanelState::Ready,
        badge: 0,
    })
    .collect()
}

pub fn render_panel_index(panels: &[FeaturePanel]) -> String {
    panels
        .iter()
        .map(|panel| {
            let badge = if panel.badge == 0 {
                String::new()
            } else {
                format!(" ({})", panel.badge)
            };
            format!("{:<9} {:?}{}", panel.kind.title(), panel.state, badge)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn feature_panel_tab(panel: &FeaturePanel) -> FormTab {
    let status = format!("{:?}", panel.state);
    let badge = if panel.badge == 0 {
        "no pending items".to_string()
    } else {
        format!("{} pending item(s)", panel.badge)
    };

    let configure = FormOption::new("configure", "Configure")
        .with_description("available when this panel needs setup");
    let configure = if panel.state == PanelState::NeedsConfig {
        configure
    } else {
        configure.disabled()
    };

    FormTab::new(
        panel_kind_id(panel.kind),
        panel.kind.title(),
        vec![
            FormOption::new("open", format!("Open {}", panel.kind.title()))
                .with_description(format!("status={status}; {badge}")),
            configure,
        ],
    )
}

fn panel_kind_id(kind: FeaturePanelKind) -> &'static str {
    match kind {
        FeaturePanelKind::Mcp => "mcp",
        FeaturePanelKind::Agents => "agents",
        FeaturePanelKind::Subagents => "subagents",
        FeaturePanelKind::Teams => "teams",
        FeaturePanelKind::Lsp => "lsp",
        FeaturePanelKind::Settings => "settings",
        FeaturePanelKind::Sandbox => "sandbox",
        FeaturePanelKind::Plugins => "plugins",
        FeaturePanelKind::Skills => "skills",
        FeaturePanelKind::Tasks => "tasks",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn feature_panel_form_uses_tab_and_selection_keys() {
        let mut panels = default_panels();
        let lsp_index = panels
            .iter()
            .position(|panel| panel.kind == FeaturePanelKind::Lsp)
            .expect("lsp panel");
        panels[lsp_index].state = PanelState::NeedsConfig;
        let mut form = FeaturePanelFormState::new(panels);

        assert_eq!(
            form.handle_key(key(KeyCode::Right)),
            TabbedFormEvent::TabChanged {
                tab_index: 1,
                tab_id: "agents".to_string(),
            }
        );
        form.handle_key(key(KeyCode::Right));
        form.handle_key(key(KeyCode::Right));
        form.handle_key(key(KeyCode::Right));
        form.handle_key(key(KeyCode::Down));

        assert_eq!(
            form.selected_panel_action(),
            Some((FeaturePanelKind::Lsp, "configure".to_string()))
        );
        assert_eq!(form.handle_key(key(KeyCode::Esc)), TabbedFormEvent::Closed);
    }

    #[test]
    fn running_and_error_panels_render_in_index_and_form() {
        let panels = vec![
            FeaturePanel {
                kind: FeaturePanelKind::Tasks,
                state: PanelState::Running,
                badge: 2,
            },
            FeaturePanel {
                kind: FeaturePanelKind::Plugins,
                state: PanelState::Error,
                badge: 0,
            },
        ];
        let index = render_panel_index(&panels);
        assert!(index.contains("Running"));
        assert!(index.contains("Error"));

        let form = FeaturePanelFormState::new(panels);
        assert!(form.render().contains("Feature panels"));
    }
}
