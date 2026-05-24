//! /init command -- initializes project config.

use std::fs;

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct InitHandler;

const AGENTS_MD_TEMPLATE: &str = r#"# AGENTS.md

Project instructions for allthecodes.

## Build And Test

- Add project-specific commands here.

## Project Notes

- Add coding conventions, architecture notes, and review expectations here.
"#;

#[async_trait]
impl CommandHandler for InitHandler {
    async fn execute(&self, _args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let config_dir = ctx.cwd.join(".allthecodes");
        let settings_file = config_dir.join("settings.json");
        let agents_md = ctx.cwd.join("AGENTS.md");
        let claude_md = ctx.cwd.join("CLAUDE.md");

        // If AGENTS.md already exists, skip. Also skip if CLAUDE.md exists
        // (user has a legacy instruction file).
        if agents_md.exists() {
            return Ok(CommandResult::Output(
                "AGENTS.md already exists here. Skipping /init to avoid overwriting it."
                    .to_string(),
            ));
        }
        if claude_md.exists() {
            return Ok(CommandResult::Output(
                "CLAUDE.md already exists here. Skipping /init to avoid overwriting it.\n\
                 Tip: rename it to AGENTS.md to use the new primary filename."
                    .to_string(),
            ));
        }

        fs::create_dir_all(&config_dir)?;
        let mut created = Vec::new();
        let mut skipped = Vec::new();

        if !settings_file.exists() {
            fs::write(&settings_file, r#"{"model": null, "theme": null}"#)?;
            created.push(settings_file.display().to_string());
        } else {
            skipped.push(settings_file.display().to_string());
        }

        fs::write(&agents_md, AGENTS_MD_TEMPLATE)?;
        created.push(agents_md.display().to_string());

        let mut lines = vec!["Project initialization complete.".to_string()];
        if !created.is_empty() {
            lines.push(format!("Created: {}", created.join(", ")));
        }
        if !skipped.is_empty() {
            lines.push(format!("Already present: {}", skipped.join(", ")));
        }

        Ok(CommandResult::Output(lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("."),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_init_creates_config() {
        let tmp = std::env::temp_dir().join("cc_rust_init_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let handler = InitHandler;
        let mut ctx = test_ctx();
        ctx.cwd = tmp.clone();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("Created")),
            _ => panic!("Expected Output"),
        }

        let settings = tmp.join(".allthecodes").join("settings.json");
        let agents_md = tmp.join("AGENTS.md");
        assert!(settings.exists());
        assert!(agents_md.exists());
        assert!(fs::read_to_string(&agents_md)
            .unwrap()
            .contains("Project instructions"));
        assert!(fs::read_to_string(&agents_md)
            .unwrap()
            .contains("allthecodes"));

        fs::write(&agents_md, "# Existing instructions\n").unwrap();

        // Existing instructions are user-owned; /init must not rewrite them.
        let result2 = handler.execute("", &mut ctx).await.unwrap();
        match result2 {
            CommandResult::Output(text) => {
                assert!(text.contains("AGENTS.md already exists here"));
                assert!(text.contains("Skipping /init"));
            }
            _ => panic!("Expected Output"),
        }
        assert_eq!(
            fs::read_to_string(&agents_md).unwrap(),
            "# Existing instructions\n"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_init_skips_when_claude_md_exists() {
        let tmp =
            std::env::temp_dir().join(format!("cc_rust_init_claude_skip_{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Pre-create a CLAUDE.md to simulate a legacy project.
        fs::write(tmp.join("CLAUDE.md"), "# Legacy instructions\n").unwrap();

        let handler = InitHandler;
        let mut ctx = test_ctx();
        ctx.cwd = tmp.clone();
        let result = handler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("CLAUDE.md already exists here"));
                assert!(text.contains("Skipping /init"));
            }
            _ => panic!("Expected Output"),
        }

        // AGENTS.md must NOT be created when CLAUDE.md already exists.
        assert!(!tmp.join("AGENTS.md").exists());

        let _ = fs::remove_dir_all(&tmp);
    }
}
