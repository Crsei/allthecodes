//! Teams dialog rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateStatus {
    pub name: String,
    pub role: String,
    pub mode: String,
    pub state: String,
    pub assigned_tasks: usize,
    pub backend: String,
    pub model: Option<String>,
    pub hidden: bool,
}

impl TeammateStatus {
    pub fn new(name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            role: role.into(),
            mode: "ask".to_string(),
            state: "running".to_string(),
            assigned_tasks: 0,
            backend: "unknown".to_string(),
            model: None,
            hidden: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamSummary {
    pub name: String,
    pub teammates: Vec<TeammateStatus>,
}

pub fn render_teams_dialog(team: &TeamSummary, selected_index: usize) -> String {
    let mut lines = vec![format!("Team: {}", team.name)];
    if team.teammates.is_empty() {
        lines.push("No teammates".to_string());
        return lines.join("\n");
    }

    if let Some(coordinator) = team
        .teammates
        .iter()
        .find(|member| member.name == "team-lead")
    {
        lines.push(format!(
            "Coordinator: {} state={} mode={} backend={}",
            coordinator.name, coordinator.state, coordinator.mode, coordinator.backend
        ));
    }

    let display_teammates = team
        .teammates
        .iter()
        .filter(|member| member.name != "team-lead")
        .collect::<Vec<_>>();
    let visible = display_teammates
        .iter()
        .filter(|member| !member.hidden)
        .count();
    let running = display_teammates
        .iter()
        .filter(|member| member.state == "running" || member.state == "active")
        .count();
    let idle = display_teammates
        .iter()
        .filter(|member| member.state == "idle")
        .count();
    let errored = display_teammates
        .iter()
        .filter(|member| member.state == "error")
        .count();
    let stopped = display_teammates
        .iter()
        .filter(|member| member.state == "stopped")
        .count();
    let task_count: usize = display_teammates
        .iter()
        .map(|member| member.assigned_tasks)
        .sum();
    lines.push(format!(
        "Summary: {} members ({} visible), {} active, {} idle, {} error, {} stopped, {} assigned task(s)",
        display_teammates.len(),
        visible,
        running,
        idle,
        errored,
        stopped,
        task_count
    ));

    for (idx, teammate) in display_teammates.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        let hidden = if teammate.hidden { " hidden" } else { "" };
        lines.push(format!(
            "{marker} {:<14} {:<10} mode={:<5} backend={:<10} tasks={}{}",
            teammate.name,
            teammate.state,
            teammate.mode,
            teammate.backend,
            teammate.assigned_tasks,
            hidden
        ));
        let model = teammate.model.as_deref().unwrap_or("inherit");
        lines.push(format!("  role: {} | model: {}", teammate.role, model));
    }
    lines.push("k kill | s shutdown | h hide | m cycle mode".to_string());
    lines.join("\n")
}
