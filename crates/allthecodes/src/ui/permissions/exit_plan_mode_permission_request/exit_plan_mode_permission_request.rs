//! Exit-plan-mode permission request rendering.

pub fn render_exit_plan_mode_permission_request(
    plan_summary: &str,
    tests: &[impl AsRef<str>],
) -> String {
    let mut lines = vec![
        "Exit plan mode".to_string(),
        format!("plan: {plan_summary}"),
        "verification:".to_string(),
    ];
    if tests.is_empty() {
        lines.push("  - <not specified>".to_string());
    } else {
        lines.extend(tests.iter().map(|test| format!("  - {}", test.as_ref())));
    }
    lines.join("\n")
}
