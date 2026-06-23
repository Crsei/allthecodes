//! `/chat-mode` command for project chat mode selection and lifecycle.

use allthecodes_services::chat_modes;
use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};

pub struct ChatModeHandler;

#[async_trait]
impl CommandHandler for ChatModeHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts = args.split_whitespace().collect::<Vec<_>>();
        match parts.first().copied().unwrap_or("status") {
            "" | "status" => status(ctx),
            "list" | "ls" => list(ctx),
            "use" => use_mode(ctx, parts.get(1).copied().unwrap_or("")),
            "off" => set_session_override(ctx, Some(chat_modes::NORMAL_CHAT_MODE_ID), "normal"),
            "follow" => set_session_override(ctx, None, "project default"),
            "default" => default_mode(ctx, parts.get(1).copied().unwrap_or("")),
            "enable" => enable_mode(ctx, parts.get(1).copied().unwrap_or("")).await,
            "disable" => disable_mode(ctx, parts.get(1).copied().unwrap_or("")),
            "help" => Ok(CommandResult::Output(help().to_string())),
            other => Ok(CommandResult::Output(format!(
                "Unknown /chat-mode subcommand '{}'.\n{}",
                other,
                help()
            ))),
        }
    }
}

fn status(ctx: &CommandContext) -> Result<CommandResult> {
    let pref = chat_modes::chat_mode_preference_for_cwd_session(
        &ctx.cwd.to_string_lossy(),
        ctx.session_id.as_str(),
    );
    let override_label = pref
        .chat_mode_override
        .as_deref()
        .unwrap_or("(following default)");
    Ok(CommandResult::Output(format!(
        "Chat mode status:\n  default: {}\n  session override: {}\n  effective: {}",
        pref.default_chat_mode, override_label, pref.effective_chat_mode
    )))
}

fn list(ctx: &CommandContext) -> Result<CommandResult> {
    let response = chat_modes::list_modes(&ctx.cwd)?;
    let pref = chat_modes::chat_mode_preference_for_cwd_session(
        &ctx.cwd.to_string_lossy(),
        ctx.session_id.as_str(),
    );
    let mut lines = vec!["Chat modes:".to_string()];
    for mode in response.modes {
        let marker = if mode.bundle.id == pref.effective_chat_mode {
            "*"
        } else {
            " "
        };
        let resources = missing_summary(&mode);
        lines.push(format!(
            "{} {:<16} {:<10} {}{}",
            marker, mode.bundle.id, mode.status, mode.bundle.display_name, resources
        ));
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

fn use_mode(ctx: &CommandContext, id: &str) -> Result<CommandResult> {
    if id.trim().is_empty() {
        bail!("Usage: /chat-mode use <id>");
    }
    let id = chat_modes::normalize_mode_or_normal(Some(id));
    if id != chat_modes::NORMAL_CHAT_MODE_ID {
        chat_modes::resolve_mode_activation(&ctx.cwd, Some(&id))?;
    }
    set_session_override(ctx, Some(&id), &id)
}

fn default_mode(ctx: &CommandContext, id: &str) -> Result<CommandResult> {
    if id.trim().is_empty() {
        bail!("Usage: /chat-mode default <id>");
    }
    let id = chat_modes::set_default_mode(&ctx.cwd, id)?;
    Ok(CommandResult::Output(format!(
        "Project default chat mode set to '{}'.",
        id
    )))
}

async fn enable_mode(ctx: &CommandContext, id: &str) -> Result<CommandResult> {
    if id.trim().is_empty() {
        bail!("Usage: /chat-mode enable <id>");
    }
    let bundle = chat_modes::enable_mode(&ctx.cwd, id, Some(env!("CARGO_PKG_VERSION"))).await?;
    Ok(CommandResult::Output(format!(
        "Chat mode '{}' enabled.",
        bundle.id
    )))
}

fn disable_mode(ctx: &CommandContext, id: &str) -> Result<CommandResult> {
    if id.trim().is_empty() {
        bail!("Usage: /chat-mode disable <id>");
    }
    let bundle = chat_modes::disable_mode(&ctx.cwd, id)?;
    Ok(CommandResult::Output(format!(
        "Chat mode '{}' disabled.",
        bundle.id
    )))
}

fn set_session_override(
    ctx: &CommandContext,
    mode: Option<&str>,
    label: &str,
) -> Result<CommandResult> {
    allthecodes_session::storage::set_session_chat_mode_override(
        ctx.session_id.as_str(),
        mode,
        &ctx.cwd.to_string_lossy(),
    )?;
    Ok(CommandResult::Output(format!(
        "Session chat mode now uses {}.",
        label
    )))
}

fn missing_summary(mode: &chat_modes::ChatModeResolved) -> String {
    let mut parts = Vec::new();
    if !mode.missing_plugins.is_empty() {
        parts.push(format!("plugins [{}]", mode.missing_plugins.join(", ")));
    }
    if !mode.missing_skills.is_empty() {
        parts.push(format!("skills [{}]", mode.missing_skills.join(", ")));
    }
    if !mode.missing_mcp_servers.is_empty() {
        parts.push(format!("MCP [{}]", mode.missing_mcp_servers.join(", ")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" (missing {})", parts.join(", "))
    }
}

fn help() -> &'static str {
    "Usage:\n  \
       /chat-mode status\n  \
       /chat-mode list\n  \
       /chat-mode use <id>\n  \
       /chat-mode off\n  \
       /chat-mode follow\n  \
       /chat-mode default <id>\n  \
       /chat-mode enable <id>\n  \
       /chat-mode disable <id>"
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_config::settings::{project_settings_path, write_settings_file, RawSettings};
    use serial_test::serial;
    use std::path::Path;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn ctx(root: &Path) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: root.to_path_buf(),
            app_state: Default::default(),
            session_id: SessionId::from_string("chat-mode-test-session"),
        }
    }

    fn project() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".allthecodes")).unwrap();
        dir
    }

    fn seed_mode(root: &Path, enabled: bool) {
        let raw = RawSettings {
            extra: std::collections::HashMap::from([(
                "chatModes".to_string(),
                serde_json::json!({
                    "bundles": [{
                        "id": "focus",
                        "displayName": "Focus",
                        "prompt": "Stay focused.",
                        "enabled": enabled
                    }]
                }),
            )]),
            ..Default::default()
        };
        write_settings_file(&project_settings_path(root), &raw).unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn use_and_follow_update_session_override() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let project = project();
        seed_mode(project.path(), true);
        let handler = ChatModeHandler;
        let mut ctx = ctx(project.path());

        let result = handler.execute("use focus", &mut ctx).await.unwrap();
        assert!(matches!(result, CommandResult::Output(_)));
        let info =
            allthecodes_session::storage::load_session_info(ctx.session_id.as_str()).unwrap();
        assert_eq!(info.chat_mode_override.as_deref(), Some("focus"));

        handler.execute("follow", &mut ctx).await.unwrap();
        let info =
            allthecodes_session::storage::load_session_info(ctx.session_id.as_str()).unwrap();
        assert_eq!(info.chat_mode_override, None);
    }

    #[tokio::test]
    #[serial]
    async fn default_rejects_disabled_mode() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let project = project();
        seed_mode(project.path(), false);
        let handler = ChatModeHandler;
        let mut ctx = ctx(project.path());

        match handler.execute("default focus", &mut ctx).await {
            Ok(_) => panic!("disabled mode should not become project default"),
            Err(error) => assert!(error.to_string().contains("disabled")),
        }
    }

    #[tokio::test]
    #[serial]
    async fn status_renders_effective_mode() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let project = project();
        let handler = ChatModeHandler;
        let mut ctx = ctx(project.path());

        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => assert!(text.contains("effective: normal")),
            _ => panic!("expected output"),
        }
    }
}
