//! Bundled skills compiled into the binary.

use super::{SkillContext, SkillDefinition, SkillFrontmatter, SkillSource};

/// Register all bundled skills in the global registry.
///
/// This compatibility helper is kept for existing tests and legacy call sites.
/// Startup uses [`bundled_skills`] so the package resolver can validate and
/// order every source in one pass.
pub fn register_bundled_skills() {
    for skill in bundled_skills() {
        super::register_skill(skill);
    }
}

/// Return bundled skills without mutating global state.
pub fn bundled_skills() -> Vec<SkillDefinition> {
    vec![
        SkillDefinition {
            name: "simplify".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description:
                    "Simplify and refine code for clarity, consistency, and maintainability."
                        .to_string(),
                when_to_use: Some(
                    "When the user asks to simplify, clean up, or refactor code for readability."
                        .to_string(),
                ),
                allowed_tools: vec![
                    "Read".into(),
                    "Grep".into(),
                    "Glob".into(),
                    "Edit".into(),
                    "Write".into(),
                    "Bash".into(),
                ],
                user_invocable: true,
                context: SkillContext::Fork,
                agent: Some("code-simplifier".to_string()),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                ..Default::default()
            },
            prompt_body: concat!(
                "Simplify and refine the recently modified code for clarity, consistency, ",
                "and maintainability while preserving all functionality. Focus on:\n",
                "- Removing unnecessary complexity\n",
                "- Improving naming and readability\n",
                "- Consolidating duplicated logic\n",
                "- Ensuring consistent style\n\n",
                "Only modify code that was recently changed unless instructed otherwise.",
            )
            .to_string(),
        },
        SkillDefinition {
            name: "remember".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Save a piece of information to the project's AGENTS.md instructions."
                    .to_string(),
                when_to_use: Some(
                    "When the user wants to save a preference, convention, or instruction for future sessions."
                        .to_string(),
                ),
                allowed_tools: vec!["Read".into(), "Edit".into(), "Write".into()],
                user_invocable: true,
                argument_hint: Some("What to remember".to_string()),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                ..Default::default()
            },
            prompt_body: concat!(
                "The user wants you to remember something by saving it to the project's ",
                "AGENTS.md file. Read the existing AGENTS.md (if any), then append the new ",
                "instruction or information. If AGENTS.md doesn't exist, create it.\n\n",
                "Information to remember: $ARGUMENTS",
            )
            .to_string(),
        },
        SkillDefinition {
            name: "debug".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Diagnose and fix issues by reading logs, errors, and code."
                    .to_string(),
                when_to_use: Some(
                    "When the user reports a bug or error and needs help debugging.".to_string(),
                ),
                allowed_tools: vec!["Read".into(), "Grep".into(), "Glob".into(), "Bash".into()],
                user_invocable: true,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                ..Default::default()
            },
            prompt_body: concat!(
                "Help the user debug an issue. Steps:\n",
                "1. Read any error messages or logs mentioned\n",
                "2. Search the codebase for relevant code\n",
                "3. Identify the root cause\n",
                "4. Suggest a fix with specific code changes\n\n",
                "Context: $ARGUMENTS",
            )
            .to_string(),
        },
        SkillDefinition {
            name: "stuck".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Get help when you're stuck on a problem.".to_string(),
                when_to_use: Some(
                    "When the user says they are stuck or need a different approach.".to_string(),
                ),
                user_invocable: true,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                ..Default::default()
            },
            prompt_body: concat!(
                "The user is stuck. Help them by:\n",
                "1. Reviewing what has been tried so far\n",
                "2. Identifying alternative approaches\n",
                "3. Suggesting the most promising next step\n",
                "4. Breaking the problem into smaller, manageable parts\n\n",
                "Additional context: $ARGUMENTS",
            )
            .to_string(),
        },
        SkillDefinition {
            name: "update-config".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Update allthecodes configuration settings.".to_string(),
                allowed_tools: vec!["Read".into(), "Edit".into(), "Write".into()],
                user_invocable: true,
                disable_model_invocation: true,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                ..Default::default()
            },
            prompt_body: concat!(
                "Help the user update their allthecodes configuration. ",
                "Read the current settings, understand what they want to change, ",
                "and make the appropriate modifications.\n\n",
                "Settings request: $ARGUMENTS",
            )
            .to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_bundled_skills() {
        crate::clear_skills();

        register_bundled_skills();
        let all = crate::get_all_skills();

        assert!(
            all.len() >= 5,
            "Expected >= 5 bundled skills, got {}",
            all.len()
        );

        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"simplify"));
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"debug"));
        assert!(names.contains(&"stuck"));
        assert!(names.contains(&"update-config"));
    }

    #[test]
    fn test_bundled_skills_properties() {
        let all = bundled_skills();
        let simplify = all.iter().find(|s| s.name == "simplify").unwrap();
        assert_eq!(simplify.source, SkillSource::Bundled);
        assert!(simplify.is_user_invocable());
        assert!(simplify.is_model_invocable());
        assert_eq!(simplify.frontmatter.context, SkillContext::Fork);
        assert_eq!(simplify.effective_version(), env!("CARGO_PKG_VERSION"));

        let update_config = all.iter().find(|s| s.name == "update-config").unwrap();
        assert!(!update_config.is_model_invocable());
        assert!(update_config.is_user_invocable());
    }

    #[test]
    fn test_bundled_skill_prompts_not_empty() {
        for skill in bundled_skills() {
            assert!(
                !skill.prompt_body.is_empty(),
                "Skill '{}' has empty prompt",
                skill.name,
            );
        }
    }
}
