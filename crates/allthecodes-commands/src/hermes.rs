use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::settings::{self, LoadedSettings, RawSettings, SettingsSource};

pub struct HermesHandler;

#[async_trait]
impl CommandHandler for HermesHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts = args.split_whitespace().collect::<Vec<_>>();
        let action = parts.first().copied().unwrap_or("");
        match action {
            "" => {
                let loaded = settings::load_effective(&ctx.cwd)?;
                sync_hermes_state(ctx, &loaded);
                if loaded.effective.hermes_enabled.unwrap_or(false) {
                    Ok(CommandResult::Output(render_status(
                        &loaded, &ctx.cwd, None,
                    )))
                } else {
                    set_hermes(true, HermesScope::Project, ctx)
                }
            }
            "status" | "show" => {
                let loaded = settings::load_effective(&ctx.cwd)?;
                sync_hermes_state(ctx, &loaded);
                Ok(CommandResult::Output(render_status(
                    &loaded, &ctx.cwd, None,
                )))
            }
            "on" | "enable" => {
                let scope = parse_scope(&parts[1..])?;
                set_hermes(true, scope, ctx)
            }
            "off" | "disable" => {
                let scope = parse_scope(&parts[1..])?;
                set_hermes(false, scope, ctx)
            }
            other => Ok(CommandResult::Output(usage(other))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HermesScope {
    User,
    Project,
}

impl HermesScope {
    fn label(self) -> &'static str {
        match self {
            HermesScope::User => "user",
            HermesScope::Project => "project",
        }
    }
}

fn parse_scope(parts: &[&str]) -> Result<HermesScope> {
    let mut scope = HermesScope::Project;
    for part in parts {
        match *part {
            "--user" => scope = HermesScope::User,
            "--project" => scope = HermesScope::Project,
            other if other.starts_with("--") => {
                anyhow::bail!(
                    "Unknown /hermes scope '{}'. Use --project or --user.",
                    other
                );
            }
            other => {
                anyhow::bail!("Unexpected /hermes argument '{}'.\n{}", other, usage_text());
            }
        }
    }
    Ok(scope)
}

fn set_hermes(
    enabled: bool,
    scope: HermesScope,
    ctx: &mut CommandContext,
) -> Result<CommandResult> {
    let path = persist_hermes(scope, enabled, &ctx.cwd)?;
    let loaded = settings::load_effective(&ctx.cwd)?;
    sync_hermes_state(ctx, &loaded);

    let mut note = format!(
        "Hermes {} at {} scope.\n-> persisted to {}",
        if enabled { "enabled" } else { "disabled" },
        scope.label(),
        path.display()
    );
    note.push('\n');
    note.push_str(&render_status(
        &loaded,
        &ctx.cwd,
        Some((
            scope,
            enabled,
            loaded.effective.hermes_enabled.unwrap_or(false),
        )),
    ));

    Ok(CommandResult::Output(note))
}

fn persist_hermes(
    scope: HermesScope,
    enabled: bool,
    cwd: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let path = match scope {
        HermesScope::User => settings::user_settings_path(),
        HermesScope::Project => settings::project_settings_path(cwd),
    };
    let mut raw = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str::<RawSettings>(&text)?
    } else {
        RawSettings::default()
    };
    raw.hermes_enabled = Some(enabled);
    let written = match scope {
        HermesScope::User => settings::write_user_settings(&raw)?,
        HermesScope::Project => settings::write_project_settings(cwd, &raw)?,
    };
    Ok(written)
}

fn sync_hermes_state(ctx: &mut CommandContext, loaded: &LoadedSettings) {
    ctx.app_state.settings.hermes_enabled = loaded.effective.hermes_enabled;
    match loaded.sources.get("hermesEnabled").copied() {
        Some(source) => {
            ctx.app_state
                .settings
                .sources
                .insert("hermesEnabled".to_string(), source);
        }
        None => {
            ctx.app_state.settings.sources.remove("hermesEnabled");
        }
    }
}

fn render_status(
    loaded: &LoadedSettings,
    cwd: &std::path::Path,
    write_result: Option<(HermesScope, bool, bool)>,
) -> String {
    let effective = loaded.effective.hermes_enabled.unwrap_or(false);
    let user = loaded.user.as_ref().and_then(|raw| raw.hermes_enabled);
    let project = loaded.project.as_ref().and_then(|raw| raw.hermes_enabled);
    let source = loaded.source_of("hermesEnabled");

    let mut out = String::new();
    out.push_str("Hermes runtime\n");
    out.push_str("--------------\n");
    out.push_str(&format!(
        "  effective: {}\n",
        if effective { "ON" } else { "OFF" }
    ));
    out.push_str(&format!("  source:    {}\n", source_label(source)));
    out.push_str(&format!(
        "  user:      {} ({})\n",
        bool_layer_label(user),
        settings::user_settings_path().display()
    ));
    out.push_str(&format!(
        "  project:   {} ({})\n",
        bool_layer_label(project),
        settings::project_settings_path(cwd).display()
    ));
    out.push_str("  rule:      On wins across user and project settings\n");
    out.push_str("  controls:  autonomous tools, background review, scheduled dispatch\n");
    out.push_str("  manual:    slash commands remain available\n");

    if let Some((scope, requested, effective_after)) = write_result {
        if !requested && effective_after {
            out.push_str(&format!(
                "  note:      {} scope is off, but another layer keeps Hermes ON\n",
                scope.label()
            ));
        }
    }

    out
}

fn bool_layer_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unset",
    }
}

fn source_label(source: SettingsSource) -> &'static str {
    source.as_str()
}

fn usage(other: &str) -> String {
    format!("Unknown /hermes subcommand '{}'.\n{}", other, usage_text())
}

fn usage_text() -> &'static str {
    "Usage:\n  /hermes                       - enable project Hermes when off, status when on\n  /hermes status                - show effective state and layers\n  /hermes on [--project|--user] - enable Hermes in a settings layer\n  /hermes off [--project|--user] - disable Hermes in a settings layer"
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_config::settings::{self, RawSettings};
    use allthecodes_engine::types::app_state::AppState;
    use std::path::{Path, PathBuf};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &Path) -> Self {
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

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: AppState::default(),
            session_id: SessionId::from_string("hermes-test-session"),
        }
    }

    fn output_text(result: CommandResult) -> String {
        let CommandResult::Output(text) = result else {
            panic!("expected output");
        };
        text
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn bare_command_enables_project_scope_by_default() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let mut ctx = test_ctx(cwd.path().to_path_buf());

        let text = output_text(HermesHandler.execute("", &mut ctx).await.unwrap());

        assert!(text.contains("effective: ON"), "{text}");
        assert_eq!(ctx.app_state.settings.hermes_enabled, Some(true));
        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(cwd.path().join(".allthecodes/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.hermes_enabled, Some(true));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn user_scope_write_sets_user_settings() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        let mut ctx = test_ctx(cwd.path().to_path_buf());

        let text = output_text(HermesHandler.execute("on --user", &mut ctx).await.unwrap());

        assert!(text.contains("user"), "{text}");
        assert_eq!(ctx.app_state.settings.hermes_enabled, Some(true));
        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(home.path().join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.hermes_enabled, Some(true));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn project_off_cannot_override_user_on() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        settings::write_user_settings(&RawSettings {
            hermes_enabled: Some(true),
            ..Default::default()
        })
        .unwrap();
        let mut ctx = test_ctx(cwd.path().to_path_buf());

        let text = output_text(
            HermesHandler
                .execute("off --project", &mut ctx)
                .await
                .unwrap(),
        );

        assert!(text.contains("effective: ON"), "{text}");
        assert!(text.contains("another layer"), "{text}");
        assert_eq!(ctx.app_state.settings.hermes_enabled, Some(true));
        let raw: RawSettings = serde_json::from_str(
            &std::fs::read_to_string(cwd.path().join(".allthecodes/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.hermes_enabled, Some(false));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn status_does_not_write_when_already_on() {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", home.path());
        settings::write_user_settings(&RawSettings {
            hermes_enabled: Some(true),
            ..Default::default()
        })
        .unwrap();
        let mut ctx = test_ctx(cwd.path().to_path_buf());

        let text = output_text(HermesHandler.execute("", &mut ctx).await.unwrap());

        assert!(text.contains("effective: ON"), "{text}");
        assert!(!cwd.path().join(".allthecodes/settings.json").exists());
    }
}
