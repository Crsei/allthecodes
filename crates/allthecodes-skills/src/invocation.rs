//! Skill invocation core shared by root-facing tool adapters.
//!
//! This module intentionally avoids depending on `cc-engine`, `cc-tools`, or
//! the binary crate. Callers adapt the prepared invocation into their concrete
//! tool runtime and fork-agent implementation.

use allthecodes_types::message::{Message, MessageContent, UserMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{SkillContext, SkillDefinition};

/// Input accepted by a Skill tool adapter.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInvocationInput {
    /// The skill name, with or without a leading slash.
    pub skill: String,
    /// Optional arguments to pass to the skill.
    #[serde(default)]
    pub args: Option<String>,
}

/// Validation error with stable numeric codes used by tool adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInvocationError {
    pub message: String,
    pub error_code: i32,
}

impl SkillInvocationError {
    pub fn new(message: impl Into<String>, error_code: i32) -> Self {
        Self {
            message: message.into(),
            error_code,
        }
    }
}

/// A fork execution request that root/main must adapt to the engine runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillForkRequest {
    pub skill: String,
    pub expanded_prompt: String,
    pub allowed_tools: Vec<String>,
    pub model: String,
    pub fallback_model: Option<String>,
    pub max_turns: usize,
}

/// Prepared skill invocation.
#[derive(Debug, Clone)]
pub enum PreparedSkillInvocation {
    Inline {
        data: Value,
        new_messages: Vec<Message>,
    },
    Fork {
        data: Value,
        request: SkillForkRequest,
    },
}

/// Build the prompt listing available model-invocable skills.
pub fn build_model_skills_listing() -> String {
    let skills = crate::get_model_invocable_skills();
    if skills.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("Available skills:".to_string());
    for skill in &skills {
        let desc = &skill.frontmatter.description;
        let when = skill.frontmatter.when_to_use.as_deref().unwrap_or("");
        if when.is_empty() {
            lines.push(format!("- {}: {}", skill.name, desc));
        } else {
            lines.push(format!("- {}: {} (use when: {})", skill.name, desc, when));
        }
    }
    lines.join("\n")
}

/// Build the complete Skill tool prompt text.
pub fn build_skill_tool_prompt() -> String {
    let listing = build_model_skills_listing();
    let base = concat!(
        "Execute a skill by name. Skills are user-defined or bundled extensions ",
        "that provide specialized capabilities. When a skill matches the user's ",
        "request, invoke it with the Skill tool before generating other responses.",
    );

    if listing.is_empty() {
        base.to_string()
    } else {
        format!("{}\n\n{}", base, listing)
    }
}

/// Validate and resolve a model-invocable skill.
pub fn validate_model_invocation(
    skill_name: Option<&str>,
) -> Result<SkillDefinition, SkillInvocationError> {
    let Some(raw_name) = skill_name else {
        return Err(SkillInvocationError::new(
            "Missing required parameter 'skill'.",
            1,
        ));
    };

    let name = raw_name.trim_start_matches('/');
    if name.is_empty() {
        return Err(SkillInvocationError::new("Skill name cannot be empty.", 2));
    }

    match crate::find_skill(name) {
        None => Err(SkillInvocationError::new(
            format!(
                "Skill '{}' not found. Available skills: {}",
                name,
                crate::get_model_invocable_skills()
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            3,
        )),
        Some(skill) if !skill.is_model_invocable() => Err(SkillInvocationError::new(
            format!(
                "Skill '{}' cannot be invoked by the model (model invocation disabled).",
                name,
            ),
            4,
        )),
        Some(skill) => Ok(skill),
    }
}

/// Create a meta user message containing the expanded skill prompt.
pub fn make_skill_message(
    skill: &SkillDefinition,
    args: &str,
    session_id: Option<&str>,
) -> Message {
    Message::User(UserMessage {
        uuid: Uuid::new_v4(),
        timestamp: chrono::Utc::now().timestamp(),
        role: "user".to_string(),
        content: MessageContent::Text(skill.expand_prompt(args, session_id)),
        is_meta: true,
        tool_use_result: None,
        source_tool_assistant_uuid: None,
    })
}

/// Prepare a skill invocation without touching engine-owned runtime state.
///
/// Records usage via `crate::record_skill_usage` on successful preparation.
pub fn prepare_skill_invocation(
    skill: &SkillDefinition,
    args: &str,
    main_loop_model: &str,
    session_id: Option<&str>,
) -> PreparedSkillInvocation {
    // Record usage for this skill invocation
    crate::record_skill_usage(&skill.name);

    match skill.frontmatter.context {
        SkillContext::Inline => {
            let mut data = json!({
                "success": true,
                "skill": skill.name,
                "message": format!(
                    "Skill '{}' invoked. Follow the instructions in the injected prompt.",
                    skill.name
                ),
            });

            if !skill.frontmatter.allowed_tools.is_empty() {
                data["allowed_tools"] = json!(skill.frontmatter.allowed_tools);
            }
            if let Some(model) = &skill.frontmatter.model {
                data["model"] = json!(model);
            }

            PreparedSkillInvocation::Inline {
                data,
                new_messages: vec![make_skill_message(skill, args, session_id)],
            }
        }
        SkillContext::Fork => {
            let model = skill
                .frontmatter
                .model
                .clone()
                .unwrap_or_else(|| main_loop_model.to_string());
            let request = SkillForkRequest {
                skill: skill.name.clone(),
                expanded_prompt: skill.expand_prompt(args, session_id),
                allowed_tools: skill.frontmatter.allowed_tools.clone(),
                model,
                fallback_model: Some(main_loop_model.to_string()),
                max_turns: 30,
            };

            PreparedSkillInvocation::Fork {
                data: json!({
                    "skill": skill.name,
                    "context": "fork",
                    "allowed_tools": request.allowed_tools.clone(),
                    "model": request.model.clone(),
                    "max_turns": request.max_turns,
                }),
                request,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SkillFrontmatter, SkillSource};

    #[test]
    fn builds_inline_meta_message() {
        let skill = SkillDefinition {
            name: "test".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Test".to_string(),
                ..Default::default()
            },
            prompt_body: "Hello $ARGUMENTS!".to_string(),
        };

        let message = make_skill_message(&skill, "world", None);
        match message {
            Message::User(user) => {
                assert!(user.is_meta);
                assert!(
                    matches!(user.content, MessageContent::Text(text) if text == "Hello world!")
                );
            }
            _ => panic!("expected user message"),
        }
    }

    #[test]
    fn plans_fork_without_runtime_dependencies() {
        let skill = SkillDefinition {
            name: "simplify".to_string(),
            source: SkillSource::Bundled,
            base_dir: None,
            frontmatter: SkillFrontmatter {
                description: "Simplify".to_string(),
                context: SkillContext::Fork,
                allowed_tools: vec!["Read".to_string(), "Edit".to_string()],
                model: Some("special".to_string()),
                ..Default::default()
            },
            prompt_body: "Do it: $ARGUMENTS".to_string(),
        };

        let prepared = prepare_skill_invocation(&skill, "now", "main", None);
        match prepared {
            PreparedSkillInvocation::Fork { request, .. } => {
                assert_eq!(request.model, "special");
                assert_eq!(request.fallback_model.as_deref(), Some("main"));
                assert_eq!(request.allowed_tools, vec!["Read", "Edit"]);
                assert_eq!(request.expanded_prompt, "Do it: now");
            }
            _ => panic!("expected fork request"),
        }
    }
}
