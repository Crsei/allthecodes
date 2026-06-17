//! /config command -- show and modify configuration settings.
//!
//! Subcommands:
//! - `/config show`                       -- show effective settings + sources
//! - `/config show --raw`                 -- show every loaded layer separately
//! - `/config sources`                    -- print per-key origin map
//! - `/config schema`                     -- print JSON Schema for settings
//! - `/config set <key> <value> [--scope] -- set a value (default scope: user)
//! - `/config reset [--scope]`            -- reset specific layer or all to defaults
//!
//! Scope flags: `--user` (default), `--project`, `--local`.

use std::path::Path;

use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_config::settings::{self, RawSettings, SettingsSource};
use allthecodes_engine::types::app_state::AppState;
use allthecodes_engine::types::tool::PermissionMode;

mod show;

const NATIVE_BACKEND_NAME: &str = "native";
const CODEX_BACKEND_NAME: &str = "codex";

fn normalize_backend(value: Option<&str>) -> String {
    let raw = value.unwrap_or(NATIVE_BACKEND_NAME).trim();
    if raw.eq_ignore_ascii_case(CODEX_BACKEND_NAME) {
        CODEX_BACKEND_NAME.to_string()
    } else {
        NATIVE_BACKEND_NAME.to_string()
    }
}

/// Handler for the `/config` slash command.
pub struct ConfigHandler;

#[async_trait]
impl CommandHandler for ConfigHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        match parts.first().copied() {
            Some("show") | None => show::handle_show(&parts[parts.first().map_or(0, |_| 1)..], ctx),
            Some("set") => handle_set(&parts[1..], ctx),
            Some("reset") => handle_reset(&parts[1..], ctx),
            Some("sources") => show::handle_sources(ctx),
            Some("schema") => show::handle_schema(),
            Some(sub) => Ok(CommandResult::Output(format!(
                "Unknown config subcommand: '{}'\n{}",
                sub,
                usage_text()
            ))),
        }
    }
}

fn usage_text() -> &'static str {
    "Usage:\n  \
       /config show [--raw]                  -- show effective config + sources\n  \
       /config sources                       -- list which layer set each key\n  \
       /config schema                        -- print JSON Schema for settings\n  \
       /config set <key> <value> [scope]     -- set a value (scope: --user|--project|--local)\n  \
       /config reset [scope]                 -- reset to defaults"
}

// ---------------------------------------------------------------------------
// /config show
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum WriteScope {
    User,
    Project,
    Local,
}

fn parse_scope(parts: &mut Vec<&str>) -> WriteScope {
    let mut scope = WriteScope::User;
    parts.retain(|p| match *p {
        "--user" => {
            scope = WriteScope::User;
            false
        }
        "--project" => {
            scope = WriteScope::Project;
            false
        }
        "--local" => {
            scope = WriteScope::Local;
            false
        }
        _ => true,
    });
    scope
}

fn handle_set(parts: &[&str], ctx: &mut CommandContext) -> Result<CommandResult> {
    let mut parts: Vec<&str> = parts.to_vec();
    let scope = parse_scope(&mut parts);

    if parts.len() < 2 {
        return Ok(CommandResult::Output(format!(
            "Usage: /config set <key> <value> [--user|--project|--local]\n\n\
             Available keys: backend, theme, verbose, permissionMode,\n  \
               apiProvider,\n  \
               outputStyle, language, voiceEnabled, soundEffects, editorMode, viewMode,\n  \
               terminalProgressBarEnabled, effortLevel, model_reasoning_effort, fastMode,\n  \
               defaultModel, fallbackModel, fastModel,\n  \
               sotaModel, motaModel, fotaModel, fastModePerSessionOptIn,\n  \
               teammateMode, claudeInChromeDefaultEnabled,\n  \
               autoMemoryEnabled\n\n{}",
            usage_text()
        )));
    }

    let key = parts[0];
    let value = parts[1..].join(" ");

    if matches!(
        key,
        "model"
            | "effortLevel"
            | "effort_level"
            | "model_reasoning_effort"
            | "modelReasoningEffort"
    ) {
        return Ok(CommandResult::Output(format!(
            "{} is read-only in /config. Use /login to select the auth profile, /model to select the model, and /effort to select reasoning effort.",
            key
        )));
    }

    // Stage the in-memory update before touching disk so validation errors are
    // still visible, but do not publish it until persistence succeeds. This
    // keeps a present-but-invalid settings file from creating a transient
    // empty/default session override.
    let mut staged_state = ctx.app_state.clone();
    let in_memory_msg = apply_set_in_memory(key, &value, &mut staged_state)?;

    // 2. Persist to the chosen scope's file (with backup).
    let persist_value = match key {
        "model" => staged_state.main_loop_model.clone(),
        _ => value.clone(),
    };
    let file_msg = persist_set(scope, key, &persist_value, &ctx.cwd)?;

    // 3. Update the source map so /config sources reflects the change.
    let src = match scope {
        WriteScope::User => SettingsSource::User,
        WriteScope::Project => SettingsSource::Project,
        WriteScope::Local => SettingsSource::Local,
    };
    staged_state.settings.sources.insert(key.to_string(), src);
    ctx.app_state = staged_state;

    Ok(CommandResult::Output(format!(
        "{}\n{}",
        in_memory_msg, file_msg
    )))
}

/// Apply a key=value to the live AppState. Returns a user-facing message.
fn apply_set_in_memory(key: &str, value: &str, app_state: &mut AppState) -> Result<String> {
    let s = &mut app_state.settings;

    match key {
        "model" => {
            let available = s.available_models.clone();
            let resolved = crate::model::resolve_model_alias_with_settings(value, s);
            crate::model::check_available_with_settings(&resolved, &available, s)
                .map_err(anyhow::Error::msg)?;
            app_state.main_loop_model = resolved.clone();
            s.model = Some(resolved.clone());
            Ok(format!("Model set to: {}", resolved))
        }
        "backend" => {
            let normalized = normalize_backend(Some(value));
            app_state.main_loop_backend = normalized.clone();
            s.backend = Some(normalized.clone());
            Ok(format!("Backend set to: {}", normalized))
        }
        "apiProvider" | "api_provider" => {
            let provider = allthecodes_config::settings::normalize_api_provider(value)
                .ok_or_else(|| anyhow::anyhow!("Unknown apiProvider: {}", value))?;
            s.api_provider = Some(provider.to_string());
            Ok(format!("API provider set to: {}", provider))
        }
        "activeAuthProfile" | "active_auth_profile" => {
            let profile = value.trim();
            if profile.is_empty() {
                anyhow::bail!("activeAuthProfile cannot be empty");
            }
            s.active_auth_profile = Some(profile.to_string());
            Ok(format!("Active auth profile set to: {}", profile))
        }
        "theme" => {
            s.theme = Some(value.to_string());
            Ok(format!("Theme set to: {}", value))
        }
        "verbose" => {
            let v = parse_config_bool(key, value)?;
            app_state.verbose = v;
            s.verbose = Some(v);
            Ok(format!("Verbose set to: {}", v))
        }
        "permissionMode" | "permission_mode" => {
            let mode = PermissionMode::parse_configured(Some(value))?;
            if mode == PermissionMode::Auto && !app_state.tool_permission_context.allows_auto_mode()
            {
                bail!("Auto mode is disabled by configuration (permissions.enableAutoMode=false).");
            }
            let canonical = mode.as_str().to_string();
            s.permission_mode = Some(canonical.clone());
            s.permissions.default_mode = Some(canonical.clone());
            allthecodes_permissions::dangerous::set_permission_mode_with_auto_mode_safety(
                &mut app_state.tool_permission_context,
                mode,
            );
            Ok(format!("Permission mode set to: {}", canonical))
        }
        "outputStyle" | "output_style" => {
            s.output_style = Some(value.to_string());
            Ok(format!("Output style set to: {}", value))
        }
        "language" => {
            s.language = Some(value.to_string());
            Ok(format!("Language set to: {}", value))
        }
        "voiceEnabled" | "voice_enabled" => {
            let v = parse_config_bool(key, value)?;
            s.voice_enabled = Some(v);
            Ok(format!("Voice enabled: {}", v))
        }
        "soundEffects" | "sound_effects" => {
            let v = parse_config_bool(key, value)?;
            s.sound_effects = Some(v);
            Ok(format!("Sound effects: {}", v))
        }
        "editorMode" | "editor_mode" => {
            s.editor_mode = Some(value.to_string());
            Ok(format!("Editor mode set to: {}", value))
        }
        "viewMode" | "view_mode" => {
            s.view_mode = Some(value.to_string());
            Ok(format!("View mode set to: {}", value))
        }
        "terminalProgressBarEnabled" | "terminal_progress_bar_enabled" => {
            let v = parse_config_bool(key, value)?;
            s.terminal_progress_bar_enabled = Some(v);
            Ok(format!("Terminal progress bar: {}", v))
        }
        "effortLevel" | "effort_level" => {
            s.effort_level = Some(value.to_string());
            app_state.effort_value = Some(value.to_string());
            Ok(format!("Effort level set to: {}", value))
        }
        "model_reasoning_effort" | "modelReasoningEffort" => {
            let effort = normalize_model_reasoning_effort(value)?;
            s.model_reasoning_effort = Some(effort.clone());
            Ok(format!("Codex reasoning effort set to: {}", effort))
        }
        "defaultModel" | "default_model" => {
            let resolved = crate::model::resolve_model_alias_with_settings(value, s);
            s.default_model = Some(resolved.clone());
            Ok(format!("Default model set to: {}", resolved))
        }
        "fallbackModel" | "fallback_model" => {
            let resolved = crate::model::resolve_model_alias_with_settings(value, s);
            s.fallback_model = Some(resolved.clone());
            Ok(format!("Fallback model set to: {}", resolved))
        }
        "fastModel" | "fast_model" => {
            let resolved = crate::model::resolve_model_alias_with_settings(value, s);
            s.fast_model = Some(resolved.clone());
            Ok(format!("Fast model set to: {}", resolved))
        }
        "sotaModel" | "sota_model" => {
            s.sota_model = Some(value.to_string());
            Ok(format!("SOTA model set to: {}", value))
        }
        "motaModel" | "mota_model" => {
            s.mota_model = Some(value.to_string());
            Ok(format!("MOTA model set to: {}", value))
        }
        "fotaModel" | "fota_model" => {
            s.fota_model = Some(value.to_string());
            Ok(format!("FOTA model set to: {}", value))
        }
        "fastMode" | "fast_mode" => {
            let v = parse_config_bool(key, value)?;
            s.fast_mode = Some(v);
            app_state.fast_mode = v;
            Ok(format!("Fast mode set to: {}", v))
        }
        "fastModePerSessionOptIn" | "fast_mode_per_session_opt_in" => {
            let v = parse_config_bool(key, value)?;
            s.fast_mode_per_session_opt_in = Some(v);
            Ok(format!("Fast mode per-session opt-in: {}", v))
        }
        "teammateMode" | "teammate_mode" => {
            let v = parse_config_bool(key, value)?;
            s.teammate_mode = Some(v);
            Ok(format!("Teammate mode: {}", v))
        }
        "claudeInChromeDefaultEnabled" | "claude_in_chrome_default_enabled" => {
            let v = parse_config_bool(key, value)?;
            s.claude_in_chrome_default_enabled = Some(v);
            Ok(format!("Claude-in-Chrome default: {}", v))
        }
        "autoMemoryEnabled" | "auto_memory_enabled" => {
            let v = parse_config_bool(key, value)?;
            s.auto_memory_enabled = Some(v);
            Ok(format!("Auto-memory enabled: {}", v))
        }
        _ => anyhow::bail!(
            "Unknown config key: '{}'. Run `/config show` to see available keys.",
            key
        ),
    }
}

/// Persist a key=value into the given scope's settings.json (with backup).
fn persist_set(scope: WriteScope, key: &str, value: &str, cwd: &Path) -> Result<String> {
    // 1. Read current file content (or empty default).
    let path = scope_path(scope, cwd);
    let mut raw = if path.exists() {
        let txt = std::fs::read_to_string(&path)?;
        serde_json::from_str::<RawSettings>(&txt)?
    } else {
        RawSettings::default()
    };

    // 2. Patch the key in-place.
    apply_set_to_raw(&mut raw, key, value)?;

    // 3. Write back via the scope-specific helper (handles backups + dir
    //    creation + atomic rename).
    let written = match scope {
        WriteScope::User => settings::write_user_settings(&raw)?,
        WriteScope::Project => settings::write_project_settings(cwd, &raw)?,
        WriteScope::Local => settings::write_local_settings(cwd, &raw)?,
    };
    Ok(format!(
        "-> persisted to {} (backups kept)",
        written.display()
    ))
}

/// Compute the on-disk path for a scope without touching the filesystem.
fn scope_path(scope: WriteScope, cwd: &Path) -> std::path::PathBuf {
    match scope {
        WriteScope::User => settings::user_settings_path(),
        WriteScope::Project => settings::project_settings_path(cwd),
        WriteScope::Local => settings::local_settings_path(cwd),
    }
}

fn apply_set_to_raw(raw: &mut RawSettings, key: &str, value: &str) -> Result<()> {
    match key {
        "model" => raw.model = Some(value.into()),
        "backend" => {
            raw.backend = Some(normalize_backend(Some(value)).to_string());
        }
        "apiProvider" | "api_provider" => {
            let provider = allthecodes_config::settings::normalize_api_provider(value)
                .ok_or_else(|| anyhow::anyhow!("Unknown apiProvider: {}", value))?;
            raw.api_provider = Some(provider.to_string());
        }
        "activeAuthProfile" | "active_auth_profile" => {
            let profile = value.trim();
            if profile.is_empty() {
                anyhow::bail!("activeAuthProfile cannot be empty");
            }
            raw.active_auth_profile = Some(profile.to_string());
        }
        "theme" => raw.theme = Some(value.into()),
        "verbose" => raw.verbose = Some(parse_config_bool(key, value)?),
        "permissionMode" | "permission_mode" => {
            let mode = PermissionMode::parse_configured(Some(value))?;
            let canonical = mode.as_str().to_string();
            raw.permission_mode = Some(canonical.clone());
            let mut perms = raw.permissions.take().unwrap_or_default();
            perms.default_mode = Some(canonical);
            raw.permissions = Some(perms);
        }
        "outputStyle" | "output_style" => raw.output_style = Some(value.into()),
        "language" => raw.language = Some(value.into()),
        "voiceEnabled" | "voice_enabled" => {
            raw.voice_enabled = Some(parse_config_bool(key, value)?)
        }
        "soundEffects" | "sound_effects" => {
            raw.sound_effects = Some(parse_config_bool(key, value)?)
        }
        "editorMode" | "editor_mode" => raw.editor_mode = Some(value.into()),
        "viewMode" | "view_mode" => raw.view_mode = Some(value.into()),
        "terminalProgressBarEnabled" | "terminal_progress_bar_enabled" => {
            raw.terminal_progress_bar_enabled = Some(parse_config_bool(key, value)?);
        }
        "effortLevel" | "effort_level" => raw.effort_level = Some(value.into()),
        "model_reasoning_effort" | "modelReasoningEffort" => {
            raw.model_reasoning_effort = Some(normalize_model_reasoning_effort(value)?)
        }
        "defaultModel" | "default_model" => raw.default_model = Some(value.into()),
        "fallbackModel" | "fallback_model" => raw.fallback_model = Some(value.into()),
        "fastModel" | "fast_model" => raw.fast_model = Some(value.into()),
        "sotaModel" | "sota_model" => raw.sota_model = Some(value.into()),
        "motaModel" | "mota_model" => raw.mota_model = Some(value.into()),
        "fotaModel" | "fota_model" => raw.fota_model = Some(value.into()),
        "fastMode" | "fast_mode" => raw.fast_mode = Some(parse_config_bool(key, value)?),
        "fastModePerSessionOptIn" | "fast_mode_per_session_opt_in" => {
            raw.fast_mode_per_session_opt_in = Some(parse_config_bool(key, value)?);
        }
        "teammateMode" | "teammate_mode" => {
            raw.teammate_mode = Some(parse_config_bool(key, value)?)
        }
        "claudeInChromeDefaultEnabled" | "claude_in_chrome_default_enabled" => {
            raw.claude_in_chrome_default_enabled = Some(parse_config_bool(key, value)?);
        }
        "autoMemoryEnabled" | "auto_memory_enabled" => {
            raw.auto_memory_enabled = Some(parse_config_bool(key, value)?);
        }
        // Unknown keys get stuffed in `extra` so users can experiment with
        // future fields without losing data.
        _ => {
            raw.extra
                .insert(key.to_string(), serde_json::Value::String(value.into()));
        }
    }
    Ok(())
}

fn parse_config_bool(key: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => anyhow::bail!(
            "Invalid boolean for {}: '{}'. Use true/false or 1/0.",
            key,
            value
        ),
    }
}

fn normalize_model_reasoning_effort(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" => Ok(normalized),
        _ => anyhow::bail!(
            "Invalid model_reasoning_effort: '{}'. Use none, minimal, low, medium, high, or xhigh.",
            value
        ),
    }
}

mod output_style {
    use std::path::{Path, PathBuf};

    use allthecodes_config::paths::data_root;

    pub struct OutputStyleResolution {
        pub style: OutputStyle,
        pub fallback_diagnostic: Option<String>,
    }

    pub enum OutputStyle {
        Default,
        Explanatory,
        Learning,
        Custom { name: String },
    }

    impl OutputStyle {
        pub fn name(&self) -> &str {
            match self {
                OutputStyle::Default => "default",
                OutputStyle::Explanatory => "explanatory",
                OutputStyle::Learning => "learning",
                OutputStyle::Custom { name } => name.as_str(),
            }
        }
    }

    pub fn resolve_with_diagnostic(name: &str, cwd: &Path) -> OutputStyleResolution {
        let trimmed = name.trim();
        let style = match trimmed.to_ascii_lowercase().as_str() {
            "" | "default" => OutputStyle::Default,
            "explanatory" => OutputStyle::Explanatory,
            "learning" => OutputStyle::Learning,
            _ => {
                if custom_style_exists(trimmed, cwd) {
                    OutputStyle::Custom {
                        name: trimmed.to_string(),
                    }
                } else {
                    return OutputStyleResolution {
                        style: OutputStyle::Default,
                        fallback_diagnostic: Some(format!(
                            "Configured output style `{}` was not found in project or user \
                             output-styles directories; using Default.",
                            trimmed
                        )),
                    };
                }
            }
        };
        OutputStyleResolution {
            style,
            fallback_diagnostic: None,
        }
    }

    fn custom_style_exists(name: &str, cwd: &Path) -> bool {
        candidate_paths(name, cwd)
            .into_iter()
            .any(|path| std::fs::read_to_string(path).is_ok())
    }

    fn candidate_paths(name: &str, cwd: &Path) -> Vec<PathBuf> {
        let safe_name = sanitize_name(name);
        vec![
            cwd.join(".allthecodes/output-styles")
                .join(format!("{}.md", safe_name)),
            data_root()
                .join("output-styles")
                .join(format!("{}.md", safe_name)),
        ]
    }

    fn sanitize_name(name: &str) -> String {
        name.chars()
            .filter(|c| !matches!(c, '/' | '\\' | '\0'))
            .collect::<String>()
            .replace("..", "")
            .trim()
            .to_string()
    }
}

// Small helper to allow `take()` on Option<PermissionsSettings> without
// pulling in extra crates. Using `Option::take` works directly above; this
// trait is intentionally not defined; we only need the inherent `take`.

// ---------------------------------------------------------------------------
// /config reset
// ---------------------------------------------------------------------------

fn handle_reset(parts: &[&str], ctx: &mut CommandContext) -> Result<CommandResult> {
    let mut parts: Vec<&str> = parts.to_vec();
    let scope = if parts.is_empty() {
        None
    } else {
        Some(parse_scope(&mut parts))
    };

    // Reset in-memory state (always).
    let default_state = AppState::default();
    ctx.app_state.settings = default_state.settings;
    ctx.app_state.main_loop_model = default_state.main_loop_model;
    ctx.app_state.main_loop_backend = default_state.main_loop_backend;
    ctx.app_state.verbose = default_state.verbose;
    ctx.app_state.fast_mode = default_state.fast_mode;
    ctx.app_state.effort_value = default_state.effort_value;
    ctx.app_state.thinking_enabled = default_state.thinking_enabled;

    // Optional: also rewrite a scope's file to defaults (backed up).
    if let Some(scope) = scope {
        let path = match scope {
            WriteScope::User => settings::user_settings_path(),
            WriteScope::Project => settings::project_settings_path(&ctx.cwd),
            WriteScope::Local => settings::local_settings_path(&ctx.cwd),
        };
        settings::write_settings_file(&path, &RawSettings::default())?;
        return Ok(CommandResult::Output(format!(
            "In-memory configuration reset; {} rewritten to defaults (backup kept).",
            path.display()
        )));
    }

    Ok(CommandResult::Output(
        "In-memory configuration reset to defaults. Files on disk untouched; \
         pass --user / --project / --local to also rewrite a file."
            .into(),
    ))
}

#[cfg(test)]
mod tests;
