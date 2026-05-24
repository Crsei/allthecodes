use crate::ui::command_surface::adapters::tasks::first_non_empty;
use crate::ui::teams::teams_dialog::{TeamSummary, TeammateStatus};
use allthecodes_engine::types::app_state::AppState;
pub(crate) fn team_summary_from_state(state: &AppState) -> (TeamSummary, bool) {
    let Some(context) = state
        .team_context
        .as_ref()
        .filter(|ctx| !ctx.team_name.is_empty())
    else {
        return (
            TeamSummary {
                name: "No active team".to_string(),
                teammates: Vec::new(),
            },
            false,
        );
    };

    let summary = allthecodes_teams::helpers::read_team_file(&context.team_name)
        .map(|team_file| {
            let snapshots = allthecodes_teams::in_process::InProcessBackend::task_snapshots();
            let teammates = team_file
                .members
                .iter()
                .filter(|member| member.name != allthecodes_teams::constants::TEAM_LEAD_NAME)
                .map(|member| {
                    let matching = snapshots
                        .iter()
                        .filter(|snapshot| snapshot.agent_id == member.agent_id)
                        .collect::<Vec<_>>();
                    let mut status = TeammateStatus::new(
                        member.name.clone(),
                        first_non_empty([
                            member.prompt.as_deref().unwrap_or_default(),
                            member.agent_type.as_deref().unwrap_or("teammate"),
                        ]),
                    );
                    status.mode = member.mode.clone().unwrap_or_else(|| "ask".to_string());
                    status.state = teammate_state_label(member.is_active, &matching);
                    status.assigned_tasks = matching.len();
                    status.backend = member
                        .backend_type
                        .map(|backend| backend.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    status.model = member.model.clone();
                    status.hidden = !member.tmux_pane_id.is_empty()
                        && team_file.hidden_pane_ids.contains(&member.tmux_pane_id);
                    status
                })
                .collect::<Vec<_>>();
            TeamSummary {
                name: team_file.name,
                teammates,
            }
        })
        .unwrap_or_else(|_| {
            let mut teammates = context
                .teammates
                .values()
                .map(|info| {
                    let mut status = TeammateStatus::new(
                        info.name.clone(),
                        info.agent_type.as_deref().unwrap_or("teammate"),
                    );
                    status.backend = if info.tmux_pane_id.is_empty() {
                        "in-process".to_string()
                    } else {
                        "tmux".to_string()
                    };
                    status.hidden = false;
                    status
                })
                .collect::<Vec<_>>();
            teammates.sort_by(|a, b| a.name.cmp(&b.name));
            TeamSummary {
                name: context.team_name.clone(),
                teammates,
            }
        });

    (summary, true)
}

pub(crate) fn teammate_state_label(
    active: Option<bool>,
    snapshots: &[&allthecodes_teams::in_process::TeammateTaskSnapshot],
) -> String {
    if snapshots
        .iter()
        .any(|snapshot| snapshot.has_error || snapshot.error_message.is_some())
    {
        return "error".to_string();
    }
    if snapshots
        .iter()
        .any(|snapshot| snapshot.status == allthecodes_teams::types::TaskStatus::Running)
    {
        if snapshots.iter().all(|snapshot| snapshot.is_idle) {
            return "idle".to_string();
        }
        return "running".to_string();
    }
    match active {
        Some(true) => "active".to_string(),
        Some(false) => "stopped".to_string(),
        None => "unknown".to_string(),
    }
}
