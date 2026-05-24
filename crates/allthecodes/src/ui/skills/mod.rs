//! Rust-side skills UI surfaces.
pub mod skills_menu;

#[cfg(test)]
mod tests {
    use super::skills_menu::{render_skills_menu, SkillMenuItem};

    #[test]
    fn snapshot_skills_menu() {
        let mut reviewer = SkillMenuItem::new("code-review", "Review diffs for regressions");
        reviewer.source = "built-in".to_string();
        let mut visual = SkillMenuItem::new("visual-verdict", "Compare screenshots");
        visual.enabled = false;
        let rendered = render_skills_menu(&[reviewer, visual], 0, "review");
        insta::assert_snapshot!("skills_menu", rendered);
    }
}
