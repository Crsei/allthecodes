//! Skill improvement via side-channel LLM analysis.
//!
//! During skill execution, periodically analyzes user messages for
//! preferences, corrections, and improvement suggestions, then
//! autonomously updates the skill definition file.
//!
//! Port of TypeScript `skillImprovement.ts`.
//!
//! NOTE: This is a structural stub. Full implementation requires:
//!   - Integration with cc-api for LLM calls
//!   - Integration with post-sampling hooks
//!   - File system access for reading/writing skill files
//!   - Langfuse observability integration

use serde::{Deserialize, Serialize};

/// A single suggested improvement to a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdate {
    pub section: String,
    pub change: String,
    pub reason: String,
}

/// Whether skill improvement is enabled.
pub fn is_skill_improvement_enabled() -> bool {
    let explicit = std::env::var("SKILL_IMPROVEMENT_ENABLED").ok();
    match explicit.as_deref() {
        Some("0") | Some("false") => false,
        Some("1") | Some("true") => true,
        _ => {
            matches!(
                std::env::var("SKILL_LEARNING_ENABLED").ok().as_deref(),
                Some("1") | Some("true")
            )
        }
    }
}

/// How many user messages between skill analysis runs.
pub const TURN_BATCH_SIZE: usize = 5;

/// Initialize the skill improvement system.
/// Registers a post-sampling hook that periodically analyzes conversations.
pub fn init_skill_improvement() {
    if !is_skill_improvement_enabled() {
        return;
    }

    // TODO: Register post-sampling hook with create_api_query_hook config
    // See TypeScript `createSkillImprovementHook()` for the full implementation.
    //
    // The hook should:
    // 1. Check if a project skill is active
    // 2. Every TURN_BATCH_SIZE user messages, analyze recent messages
    // 3. Parse <updates> tags for improvement suggestions
    // 4. Update AppState.skillImprovement with suggestions
}

/// Apply skill improvements by making an LLM call to rewrite the skill file.
/// Fire-and-forget — does not block the main conversation.
pub async fn apply_skill_improvement(skill_name: &str, _updates: &[SkillUpdate]) {
    if skill_name.is_empty() {
        return;
    }

    // TODO: Full implementation:
    //
    // 1. Read skill file from .allthecodes/skills/{skill_name}/SKILL.md
    // 2. Build LLM prompt with current content + suggested updates
    // 3. Call queryModelWithoutStreaming (via cc-api)
    // 4. Parse <updated_file> tags
    // 5. Write updated content back to file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_update_serialization() {
        let update = SkillUpdate {
            section: "step 1".into(),
            change: "ask about energy levels".into(),
            reason: "user requested it".into(),
        };

        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("step 1"));
        assert!(json.contains("ask about energy levels"));

        let deserialized: SkillUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.section, "step 1");
    }

    #[test]
    fn test_is_skill_improvement_enabled_default() {
        // Without env var, should be false
        assert!(!is_skill_improvement_enabled());
    }
}
