use anyhow::Result;
use async_trait::async_trait;
use std::error::Error;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct LearnHandler;

#[async_trait]
impl CommandHandler for LearnHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();
        if args.is_empty() {
            return Ok(CommandResult::Output(usage()));
        }

        let mut parts = args.splitn(3, char::is_whitespace);
        let mode = parts.next().unwrap_or_default();
        let (instruction, source_session_id, source_note) = match mode {
            "conversation" => {
                let instruction = parts.collect::<Vec<_>>().join(" ");
                (instruction, Some(ctx.session_id.as_str().to_string()), None)
            }
            "session" => {
                let session_id = parts.next().unwrap_or_default().trim();
                let instruction = parts.next().unwrap_or_default().trim().to_string();
                if session_id.is_empty() || instruction.is_empty() {
                    return Ok(CommandResult::Output(usage()));
                }
                (
                    instruction,
                    Some(session_id.to_string()),
                    Some(format!("Source session: {session_id}")),
                )
            }
            "docs" => {
                let path = parts.next().unwrap_or_default().trim();
                let instruction = parts.next().unwrap_or_default().trim().to_string();
                if path.is_empty() || instruction.is_empty() {
                    return Ok(CommandResult::Output(usage()));
                }
                (
                    instruction,
                    Some(ctx.session_id.as_str().to_string()),
                    Some(format!("Source docs: {path}")),
                )
            }
            _ => return Ok(CommandResult::Output(usage())),
        };

        if instruction.trim().is_empty() {
            return Ok(CommandResult::Output(usage()));
        }

        let skill_name = skill_name_from_instruction(&instruction);
        let markdown = draft_markdown(&skill_name, &instruction, source_note.as_deref());
        let proposal = allthecodes_skills::stage_skill_proposal(
            allthecodes_skills::SkillProposalDraft {
                action: allthecodes_skills::SkillProposalAction::Create,
                scope: allthecodes_skills::SkillProposalScope::Project,
                skill_name,
                source_session_id,
                markdown,
            },
            &ctx.cwd,
        )
        .map_err(skill_error)?;
        let proposal_id = format!("native:project:{}", proposal.id);

        Ok(CommandResult::Output(format!(
            "Skill proposal {} staged for '{}'.\nProposed path: .allthecodes/skills/{}/SKILL.md\nUse /skills diff {} then /skills approve {} to install it.",
            proposal_id,
            proposal.skill_name,
            proposal.skill_name,
            proposal_id,
            proposal_id
        )))
    }
}

fn usage() -> String {
    "Usage: /learn conversation <instruction>\n       /learn session <session_id> <instruction>\n       /learn docs <path> <instruction>".to_string()
}

fn skill_error(error: Box<dyn Error + Send + Sync + 'static>) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

fn skill_name_from_instruction(instruction: &str) -> String {
    let words = instruction
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| !word.is_empty())
        .take(5)
        .collect::<Vec<_>>();
    let name = words.join("-");
    if allthecodes_skills::is_valid_skill_name(&name) {
        name
    } else {
        "learned-skill".to_string()
    }
}

fn draft_markdown(skill_name: &str, instruction: &str, source_note: Option<&str>) -> String {
    let description = first_sentence(instruction);
    let mut markdown = format!(
        "---\nname: {skill_name}\ndescription: {description}\nuser-invocable: true\n---\n\n# {skill_name}\n\n{instruction}\n"
    );
    if let Some(note) = source_note {
        markdown.push_str("\n## Source\n");
        markdown.push_str(note);
        markdown.push('\n');
    }
    markdown
}

fn first_sentence(instruction: &str) -> String {
    let text = instruction
        .split(['.', '\n'])
        .next()
        .unwrap_or(instruction)
        .trim();
    let truncated = text.chars().take(120).collect::<String>();
    if truncated.is_empty() {
        "Learned workflow".to_string()
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: AppState::default(),
            session_id: SessionId::from_string("learn-test-session"),
        }
    }

    #[tokio::test]
    async fn learn_conversation_creates_proposal_not_active_skill() {
        allthecodes_skills::clear_skills();
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let handler = LearnHandler;
        let mut ctx = test_ctx(cwd.clone());

        let result = handler
            .execute("conversation Review Rust diffs carefully", &mut ctx)
            .await
            .unwrap();

        let text = match result {
            CommandResult::Output(text) => text,
            _ => panic!("Expected Output"),
        };
        assert!(text.contains("Skill proposal"));
        assert!(text.contains("native:project:skill-proposal-"));
        assert!(text.contains("/skills diff native:project:"));
        assert!(!text.contains(&cwd.display().to_string()));

        let proposals = allthecodes_skills::list_skill_proposals(&cwd).unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].skill_name, "review-rust-diffs-carefully");
        assert!(proposals[0]
            .proposed_path
            .starts_with(cwd.join(".allthecodes").join("skills")));
        assert!(!proposals[0].proposed_path.exists());
        assert!(allthecodes_skills::find_skill(&proposals[0].skill_name).is_none());
    }
}
