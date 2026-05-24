//! Footer status indicator for active teammates.

pub fn render_team_status(
    total_teammates: usize,
    teams_selected: bool,
    show_hint: bool,
) -> Option<String> {
    if total_teammates == 0 {
        return None;
    }
    let noun = if total_teammates == 1 {
        "teammate"
    } else {
        "teammates"
    };
    let mut status = format!("{total_teammates} {noun}");
    if teams_selected {
        status = format!("[{status}]");
    }
    if teams_selected && show_hint {
        status.push_str(" - Enter to view");
    }
    Some(status)
}
pub fn render_team_summary_status(
    total_teammates: usize,
    active_teammates: usize,
    assigned_tasks: usize,
    teams_selected: bool,
    show_hint: bool,
) -> Option<String> {
    let mut status = render_team_status(total_teammates, false, false)?;
    if active_teammates > 0 || assigned_tasks > 0 {
        status.push_str(&format!(
            " ({} active, {} task{})",
            active_teammates,
            assigned_tasks,
            if assigned_tasks == 1 { "" } else { "s" }
        ));
    }
    if teams_selected {
        status = format!("[{status}]");
    }
    if teams_selected && show_hint {
        status.push_str(" - Enter to view");
    }
    Some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_status_includes_active_and_task_counts() {
        assert_eq!(
            render_team_summary_status(3, 2, 1, true, true).as_deref(),
            Some("[3 teammates (2 active, 1 task)] - Enter to view")
        );
        assert_eq!(render_team_summary_status(0, 2, 1, false, false), None);
    }
}
