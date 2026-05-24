//! Rust-side team UI surfaces.
pub mod team_status;
pub mod teams_dialog;

#[cfg(test)]
mod tests {
    use super::team_status::render_team_status;
    use super::teams_dialog::{render_teams_dialog, TeamSummary, TeammateStatus};

    #[test]
    fn snapshot_team_surfaces() {
        let mut reviewer = TeammateStatus::new("reviewer", "Review implementation risks");
        reviewer.mode = "plan".to_string();
        reviewer.assigned_tasks = 2;
        let mut builder = TeammateStatus::new("builder", "Implement bounded changes");
        builder.hidden = true;
        let team = TeamSummary {
            name: "ui-port".to_string(),
            teammates: vec![
                TeammateStatus::new("team-lead", "Coordinate"),
                reviewer,
                builder,
            ],
        };
        let rendered = format!(
            "## status\n{}\n\n## dialog\n{}",
            render_team_status(2, true, true).unwrap(),
            render_teams_dialog(&team, 0)
        );
        insta::assert_snapshot!("team_surfaces", rendered);
    }
}
