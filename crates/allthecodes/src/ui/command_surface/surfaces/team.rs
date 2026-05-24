use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::adapters::team::team_summary_from_state;
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};
use crate::ui::teams::team_status::render_team_summary_status;
use crate::ui::teams::teams_dialog::render_teams_dialog;
use crate::ui::teams::teams_dialog::{TeamSummary, TeammateStatus};
use allthecodes_engine::types::app_state::AppState;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamSurface {
    pub(crate) summary: TeamSummary,
    pub(crate) active: bool,
    pub(crate) selected_index: usize,
}

impl TeamSurface {
    pub(crate) fn new(state: &AppState) -> Self {
        let (summary, active) = team_summary_from_state(state);
        Self {
            summary,
            active,
            selected_index: 0,
        }
    }

    pub(crate) fn render(&self) -> String {
        if self.detail_active() {
            return [
                self.status_line(true),
                String::new(),
                render_teams_dialog(&self.summary, self.list_index()),
                String::new(),
                "Left overview | s send | k kill | p spawn | c create | l list | Esc close"
                    .to_string(),
            ]
            .join("\n");
        }

        let teammates = self
            .summary
            .teammates
            .iter()
            .filter(|teammate| teammate.name != "team-lead")
            .collect::<Vec<_>>();
        let mut detail_lines = if teammates.is_empty() {
            vec!["No teammates".to_string()]
        } else {
            teammates
                .iter()
                .enumerate()
                .map(|(idx, teammate)| {
                    selected_row(
                        &teammate.name,
                        format!(
                            "status={} mode={} tasks={} {}",
                            teammate.state, teammate.mode, teammate.assigned_tasks, teammate.role
                        ),
                        idx == self.selected_index,
                    )
                })
                .collect::<Vec<_>>()
        };
        detail_lines.push(String::new());
        detail_lines.push("Commands".to_string());
        if self.active {
            detail_lines.push(plain_row("Enter:", "/team status"));
            if let Some(teammate) = self.selected_teammate() {
                detail_lines.push(plain_row("s:", format!("/team send {} ", teammate.name)));
                detail_lines.push(plain_row(
                    "k:",
                    format!("/team kill {} direct-execute", teammate.name),
                ));
            }
            detail_lines.push(plain_row("p:", "/team spawn "));
        } else {
            detail_lines.push(plain_row("c:", "/team create "));
            detail_lines.push(plain_row("l:", "/team list"));
        }
        BetterViewPanel::new("Team")
            .summary(format!(
                "{} | team={} active={} members={}",
                self.status_line(false),
                self.summary.name,
                self.active,
                teammates.len()
            ))
            .sections_title("Teammates")
            .sections(vec!["Current team".to_string()], 0)
            .detail_title("Teammate details")
            .detail_lines(detail_lines)
            .footer(if self.active {
                "Up/Down member | Enter/Right details | s send | k kill | p spawn | Esc"
            } else {
                "c create | l list | Esc close"
            })
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up => {
                self.set_list_index(cycle_index(
                    self.list_index(),
                    self.selectable_teammate_count(),
                    -1,
                ));
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.set_list_index(cycle_index(
                    self.list_index(),
                    self.selectable_teammate_count(),
                    1,
                ));
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter | KeyCode::Right => {
                self.open_detail();
                CommandSurfaceOutcome::None
            }
            KeyCode::Left | KeyCode::Backspace => {
                self.close_detail();
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('l') => CommandSurfaceOutcome::Submit("/team list".to_string()),
            KeyCode::Char('c') => CommandSurfaceOutcome::FillPrompt("/team create ".to_string()),
            KeyCode::Char('p') => CommandSurfaceOutcome::FillPrompt("/team spawn ".to_string()),
            KeyCode::Char('s') => self
                .selected_teammate()
                .map(|teammate| {
                    CommandSurfaceOutcome::FillPrompt(format!("/team send {} ", teammate.name))
                })
                .unwrap_or(CommandSurfaceOutcome::None),
            KeyCode::Char('k') => self
                .selected_teammate()
                .map(|teammate| {
                    CommandSurfaceOutcome::Submit(format!("/team kill {}", teammate.name))
                })
                .unwrap_or(CommandSurfaceOutcome::None),
            _ => CommandSurfaceOutcome::None,
        }
    }

    pub(crate) fn selected_teammate(&self) -> Option<&TeammateStatus> {
        self.summary
            .teammates
            .iter()
            .filter(|teammate| teammate.name != "team-lead")
            .nth(self.list_index())
    }

    fn selectable_teammate_count(&self) -> usize {
        self.summary
            .teammates
            .iter()
            .filter(|teammate| teammate.name != "team-lead")
            .count()
    }

    fn status_line(&self, selected: bool) -> String {
        let teammates = self
            .summary
            .teammates
            .iter()
            .filter(|teammate| teammate.name != "team-lead")
            .collect::<Vec<_>>();
        let active = teammates
            .iter()
            .filter(|teammate| teammate.state == "running" || teammate.state == "active")
            .count();
        let assigned_tasks = teammates
            .iter()
            .map(|teammate| teammate.assigned_tasks)
            .sum::<usize>();
        render_team_summary_status(teammates.len(), active, assigned_tasks, selected, true)
            .unwrap_or_else(|| "no teammates".to_string())
    }

    fn list_index(&self) -> usize {
        let count = self.selectable_teammate_count();
        if count == 0 {
            0
        } else {
            self.selected_index % count
        }
    }

    fn detail_active(&self) -> bool {
        let count = self.selectable_teammate_count();
        count > 0 && self.selected_index >= count
    }

    fn set_list_index(&mut self, index: usize) {
        let detail_offset = if self.detail_active() {
            self.selectable_teammate_count()
        } else {
            0
        };
        self.selected_index = index.saturating_add(detail_offset);
    }

    fn open_detail(&mut self) {
        let count = self.selectable_teammate_count();
        if count > 0 && !self.detail_active() {
            self.selected_index = self.list_index() + count;
        }
    }

    fn close_detail(&mut self) {
        self.selected_index = self.list_index();
    }
}
