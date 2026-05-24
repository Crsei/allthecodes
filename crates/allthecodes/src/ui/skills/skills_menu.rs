//! Skills menu rendering.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMenuItem {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source: String,
}

impl SkillMenuItem {
    #[cfg(test)]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            enabled: true,
            source: "project".to_string(),
        }
    }
}

#[cfg(test)]
pub fn render_skills_menu(items: &[SkillMenuItem], selected_index: usize, filter: &str) -> String {
    let filter_lc = filter.to_ascii_lowercase();
    let visible = items
        .iter()
        .filter(|item| {
            filter_lc.is_empty()
                || item.name.to_ascii_lowercase().contains(&filter_lc)
                || item.description.to_ascii_lowercase().contains(&filter_lc)
        })
        .collect::<Vec<_>>();

    if visible.is_empty() {
        return format!("Skills\nfilter: {filter}\nNo matching skills");
    }

    let mut lines = vec![format!("Skills ({})", visible.len())];
    if !filter.is_empty() {
        lines.push(format!("filter: {filter}"));
    }
    for (idx, item) in visible.iter().enumerate() {
        let marker = if idx == selected_index { ">" } else { " " };
        let enabled = if item.enabled { "enabled" } else { "disabled" };
        lines.push(format!(
            "{marker} {:<18} {:<8} {}",
            item.name, enabled, item.description
        ));
        lines.push(format!("  source: {}", item.source));
    }
    lines.join("\n")
}
