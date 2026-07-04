use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_tools::workflow::file_workflow::{
    list_workflow_scripts, workflow_scripts_dir, WORKFLOW_EXTENSIONS,
};

pub struct WorkflowsHandler;

#[async_trait]
impl CommandHandler for WorkflowsHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let trimmed = args.trim();
        if matches!(trimmed, "help" | "-h" | "--help") {
            return Ok(CommandResult::Output(usage()));
        }
        if !trimmed.is_empty() {
            return Ok(CommandResult::Output(format!(
                "Unsupported arguments: {trimmed}\n\n{}",
                usage()
            )));
        }
        Ok(CommandResult::Output(format_workflows_output(&ctx.cwd)?))
    }
}

pub(crate) fn format_workflows_output(cwd: &Path) -> Result<String> {
    let dir = workflow_scripts_dir(cwd);
    let scripts = list_workflow_scripts(cwd)?;
    if scripts.is_empty() {
        return Ok(format!(
            "No workflow scripts found.\nDirectory: {}\nSupported extensions: {}",
            dir.display(),
            WORKFLOW_EXTENSIONS
                .iter()
                .map(|extension| format!(".{extension}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut lines = Vec::new();
    lines.push("Project workflow scripts".to_string());
    lines.push(format!("Directory: {}", dir.display()));
    lines.push(String::new());
    for script in scripts {
        let step_label = if script.step_count == 1 {
            "step"
        } else {
            "steps"
        };
        let mut line = format!(
            "- {} ({}; {} {})",
            script.name, script.file, script.step_count, step_label
        );
        if let Some(error) = script.parse_error {
            line.push_str(&format!("; parse error: {error}"));
        }
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn usage() -> String {
    "Usage: /workflows\n\nLists .md, .yaml, and .yml workflow scripts in the project .allthecodes/workflows directory.".to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;

    use super::*;

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: AppState::default(),
            session_id: SessionId::from_string("workflows-test"),
        }
    }

    #[test]
    fn formats_empty_workflows_directory() {
        let temp = tempfile::tempdir().expect("tempdir");

        let output = format_workflows_output(temp.path()).expect("format");

        assert!(output.contains("No workflow scripts found."));
        assert!(output.contains(".allthecodes/workflows"));
        assert!(output.contains(".md, .yaml, .yml"));
    }

    #[test]
    fn lists_file_workflows_without_json_static_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workflows_dir = temp.path().join(".allthecodes").join("workflows");
        fs::create_dir_all(&workflows_dir).expect("create workflows dir");
        fs::write(workflows_dir.join("release.md"), "- Prepare\n- Publish\n")
            .expect("write markdown");
        fs::write(
            workflows_dir.join("deploy.yaml"),
            "steps:\n  - name: Deploy\n",
        )
        .expect("write yaml");
        fs::write(workflows_dir.join(".hidden.md"), "- Hidden\n").expect("write hidden");
        fs::write(workflows_dir.join("static.json"), "{}").expect("write static json");

        let output = format_workflows_output(temp.path()).expect("format");

        assert!(output.contains("release (release.md; 2 steps)"));
        assert!(output.contains("deploy (deploy.yaml; 1 step)"));
        assert!(!output.contains("static.json"));
        assert!(!output.contains(".hidden.md"));
    }

    #[tokio::test]
    async fn handler_returns_workflow_list_output() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workflows_dir = temp.path().join(".allthecodes").join("workflows");
        fs::create_dir_all(&workflows_dir).expect("create workflows dir");
        fs::write(workflows_dir.join("release.md"), "- Prepare\n").expect("write markdown");
        let mut ctx = test_ctx(temp.path().to_path_buf());

        let result = WorkflowsHandler
            .execute("", &mut ctx)
            .await
            .expect("execute");

        let CommandResult::Output(output) = result else {
            panic!("expected output");
        };
        assert!(output.contains("release (release.md; 1 step)"));
    }
}
