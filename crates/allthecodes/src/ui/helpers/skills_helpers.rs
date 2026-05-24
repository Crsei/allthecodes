//! Skill display and matching helpers.

use allthecodes_skills::SkillDefinition;

#[cfg(test)]
pub(crate) const SKILL_NAME_TRUNCATE_LEN: usize = 21;

pub(crate) fn skill_display_name(skill: &SkillDefinition) -> &str {
    skill.display_name()
}

pub(crate) fn skill_description(skill: &SkillDefinition) -> &str {
    if let Some(when_to_use) = skill.frontmatter.when_to_use.as_deref() {
        if !when_to_use.trim().is_empty() {
            return when_to_use;
        }
    }
    skill.frontmatter.description.as_str()
}

#[cfg(test)]
pub(crate) fn truncate_skill_name(name: &str) -> String {
    let count = name.chars().count();
    if count <= SKILL_NAME_TRUNCATE_LEN {
        return name.to_string();
    }
    let mut out = name
        .chars()
        .take(SKILL_NAME_TRUNCATE_LEN.saturating_sub(1))
        .collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
pub(crate) fn match_skill(skill: &SkillDefinition, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystacks = [
        skill.name.as_str(),
        skill_display_name(skill),
        skill_description(skill),
    ];
    haystacks
        .iter()
        .any(|value| value.to_ascii_lowercase().contains(&query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_skills::{SkillFrontmatter, SkillSource};

    fn skill() -> SkillDefinition {
        SkillDefinition {
            name: "code-review".to_string(),
            source: SkillSource::Project,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                name: Some("Code Review".to_string()),
                description: "Find correctness risks before commit.".to_string(),
                when_to_use: Some("Use when reviewing Rust changes.".to_string()),
                ..SkillFrontmatter::default()
            },
            prompt_body: "Review the diff.".to_string(),
        }
    }

    #[test]
    fn skill_description_prefers_when_to_use() {
        let skill = skill();
        assert_eq!(skill_display_name(&skill), "Code Review");
        assert_eq!(
            skill_description(&skill),
            "Use when reviewing Rust changes."
        );
    }

    #[test]
    fn skill_description_falls_back_to_description() {
        let mut skill = skill();
        skill.frontmatter.when_to_use = Some("  ".to_string());
        assert_eq!(
            skill_description(&skill),
            "Find correctness risks before commit."
        );
    }

    #[test]
    fn skill_matching_checks_name_display_name_and_description() {
        let skill = skill();
        assert!(match_skill(&skill, ""));
        assert!(match_skill(&skill, "CODE"));
        assert!(match_skill(&skill, "rust changes"));
        assert!(!match_skill(&skill, "shell"));
    }

    #[test]
    fn skill_name_truncation_matches_menu_limit() {
        assert_eq!(truncate_skill_name("short"), "short");
        assert_eq!(
            truncate_skill_name("abcdefghijklmnopqrstuvwxyz"),
            "abcdefghijklmnopqrst..."
        );
    }
}
