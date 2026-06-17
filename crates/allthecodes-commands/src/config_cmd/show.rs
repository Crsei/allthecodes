use super::*;

// ---------------------------------------------------------------------------

pub(super) fn handle_show(parts: &[&str], ctx: &CommandContext) -> Result<CommandResult> {
    let raw_mode = parts.contains(&"--raw");
    if raw_mode {
        return handle_show_raw(ctx);
    }

    let state = &ctx.app_state;
    let mut lines = Vec::new();
    lines.push("Effective configuration:".into());
    lines.push(String::new());

    let src = |key: &str| -> String {
        state
            .settings
            .sources
            .get(key)
            .map(|s| format!("[{}]", s.as_str()))
            .unwrap_or_else(|| "[default]".into())
    };

    let row = |k: &str, v: String, src_key: &str, lines: &mut Vec<String>| {
        lines.push(format!("  {:<32} {:<10} {}", k, src(src_key), v));
    };

    row("model", state.main_loop_model.clone(), "model", &mut lines);
    row(
        "backend",
        state.main_loop_backend.clone(),
        "backend",
        &mut lines,
    );
    row(
        "apiProvider",
        opt_str(state.settings.api_provider.clone()),
        "apiProvider",
        &mut lines,
    );
    row(
        "activeAuthProfile",
        opt_str(state.settings.active_auth_profile.clone()),
        "activeAuthProfile",
        &mut lines,
    );
    row(
        "theme",
        state
            .settings
            .theme
            .clone()
            .unwrap_or_else(|| "(default)".into()),
        "theme",
        &mut lines,
    );
    row("verbose", state.verbose.to_string(), "verbose", &mut lines);
    row(
        "permissionMode",
        state
            .settings
            .permission_mode
            .clone()
            .unwrap_or_else(|| format!("{:?}", state.tool_permission_context.mode).to_lowercase()),
        "permissionMode",
        &mut lines,
    );
    row(
        "permissions.allow",
        join_or_dash(&state.settings.permissions.allow),
        "permissions",
        &mut lines,
    );
    row(
        "permissions.deny",
        join_or_dash(&state.settings.permissions.deny),
        "permissions",
        &mut lines,
    );
    row(
        "permissions.ask",
        join_or_dash(&state.settings.permissions.ask),
        "permissions",
        &mut lines,
    );
    row(
        "sandbox.enabled",
        opt_str(state.settings.sandbox.enabled.map(|b| b.to_string())),
        "sandbox",
        &mut lines,
    );
    row(
        "statusLine.type",
        opt_str(state.settings.status_line.r#type.clone()),
        "statusLine",
        &mut lines,
    );
    row(
        "spinnerTips.enabled",
        opt_str(state.settings.spinner_tips.enabled.map(|b| b.to_string())),
        "spinnerTips",
        &mut lines,
    );
    row(
        "spinnerTips.intervalMs",
        opt_str(
            state
                .settings
                .spinner_tips
                .interval_ms
                .map(|n| n.to_string()),
        ),
        "spinnerTips",
        &mut lines,
    );
    row(
        "spinnerTips.customTips",
        join_or_dash(&state.settings.spinner_tips.custom_tips),
        "spinnerTips",
        &mut lines,
    );
    row(
        "outputStyle",
        opt_str(state.settings.output_style.clone()),
        "outputStyle",
        &mut lines,
    );
    // Resolve the configured style now so the user sees what will actually
    // be injected; built-ins always show as the canonical name; custom
    // styles surface as their resolved name (or fall back to default if
    // the file can't be loaded).
    if let Some(name) = state.settings.output_style.as_deref() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let resolution = output_style::resolve_with_diagnostic(name, &cwd);
        lines.push(format!(
            "  outputStyle (resolved): {}",
            resolution.style.name()
        ));
        if let Some(diagnostic) = resolution.fallback_diagnostic {
            lines.push(format!("  outputStyle diagnostic: {}", diagnostic));
        }
    }
    row(
        "language",
        opt_str(state.settings.language.clone()),
        "language",
        &mut lines,
    );
    row(
        "voiceEnabled",
        opt_str(state.settings.voice_enabled.map(|b| b.to_string())),
        "voiceEnabled",
        &mut lines,
    );
    row(
        "soundEffects",
        state.settings.sound_effects.unwrap_or(true).to_string(),
        "soundEffects",
        &mut lines,
    );
    row(
        "editorMode",
        opt_str(state.settings.editor_mode.clone()),
        "editorMode",
        &mut lines,
    );
    row(
        "viewMode",
        opt_str(state.settings.view_mode.clone()),
        "viewMode",
        &mut lines,
    );
    row(
        "terminalProgressBarEnabled",
        state
            .settings
            .terminal_progress_bar_enabled
            .unwrap_or(true)
            .to_string(),
        "terminalProgressBarEnabled",
        &mut lines,
    );
    row(
        "availableModels",
        join_or_dash(&state.settings.available_models),
        "availableModels",
        &mut lines,
    );
    row(
        "defaultModel",
        opt_str(state.settings.default_model.clone()),
        "defaultModel",
        &mut lines,
    );
    row(
        "fallbackModel",
        opt_str(state.settings.fallback_model.clone()),
        "fallbackModel",
        &mut lines,
    );
    row(
        "fastModel",
        opt_str(state.settings.fast_model.clone()),
        "fastModel",
        &mut lines,
    );
    row(
        "sotaModel",
        opt_str(state.settings.sota_model.clone()),
        "sotaModel",
        &mut lines,
    );
    row(
        "motaModel",
        opt_str(state.settings.mota_model.clone()),
        "motaModel",
        &mut lines,
    );
    row(
        "fotaModel",
        opt_str(state.settings.fota_model.clone()),
        "fotaModel",
        &mut lines,
    );
    row(
        "effortLevel",
        opt_str(state.settings.effort_level.clone()),
        "effortLevel",
        &mut lines,
    );
    row(
        "thinking",
        opt_json(state.settings.thinking.clone()),
        "thinking",
        &mut lines,
    );
    row(
        "output_config",
        opt_json(state.settings.output_config.clone()),
        "output_config",
        &mut lines,
    );
    row(
        "model_reasoning_effort",
        opt_str(state.settings.model_reasoning_effort.clone()),
        "model_reasoning_effort",
        &mut lines,
    );
    row(
        "fastMode",
        opt_str(state.settings.fast_mode.map(|b| b.to_string())),
        "fastMode",
        &mut lines,
    );
    row(
        "fastModePerSessionOptIn",
        opt_str(
            state
                .settings
                .fast_mode_per_session_opt_in
                .map(|b| b.to_string()),
        ),
        "fastModePerSessionOptIn",
        &mut lines,
    );
    row(
        "teammateMode",
        opt_str(state.settings.teammate_mode.map(|b| b.to_string())),
        "teammateMode",
        &mut lines,
    );
    row(
        "claudeInChromeDefaultEnabled",
        opt_str(
            state
                .settings
                .claude_in_chrome_default_enabled
                .map(|b| b.to_string()),
        ),
        "claudeInChromeDefaultEnabled",
        &mut lines,
    );
    row(
        "autoMemoryEnabled",
        opt_str(state.settings.auto_memory_enabled.map(|b| b.to_string())),
        "autoMemoryEnabled",
        &mut lines,
    );

    lines.push(String::new());
    lines.push("File locations:".into());
    lines.push(format!(
        "  user:    {}",
        settings::user_settings_path().display()
    ));
    lines.push(format!(
        "  project: {}",
        settings::project_settings_path(&ctx.cwd).display()
    ));
    lines.push(format!(
        "  local:   {}",
        settings::local_settings_path(&ctx.cwd).display()
    ));
    lines.push(format!(
        "  managed: {}",
        settings::managed_settings_path().display()
    ));

    Ok(CommandResult::Output(lines.join("\n")))
}

fn opt_str(v: Option<String>) -> String {
    v.unwrap_or_else(|| "(unset)".into())
}

fn opt_json(v: Option<serde_json::Value>) -> String {
    v.map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "(invalid json)".to_string()))
        .unwrap_or_else(|| "(unset)".into())
}

fn join_or_dash(v: &[String]) -> String {
    if v.is_empty() {
        "-".to_string()
    } else {
        v.join(", ")
    }
}

fn handle_show_raw(ctx: &CommandContext) -> Result<CommandResult> {
    let loaded = settings::load_effective(&ctx.cwd)?;
    let mut lines = Vec::new();
    lines.push("Raw settings layers (lowest -> highest priority):".into());
    lines.push(String::new());

    let render = |label: &str, raw: &Option<RawSettings>, lines: &mut Vec<String>| {
        lines.push(format!("== {} ==", label));
        match raw {
            None => lines.push("  (none)".into()),
            Some(r) => match serde_json::to_string_pretty(r) {
                Ok(json) => {
                    for l in json.lines() {
                        lines.push(format!("  {}", l));
                    }
                }
                Err(e) => lines.push(format!("  (failed to serialize: {})", e)),
            },
        }
        lines.push(String::new());
    };

    render("managed", &loaded.managed, &mut lines);
    render("user", &loaded.user, &mut lines);
    render("project", &loaded.project, &mut lines);
    render("local", &loaded.local, &mut lines);

    Ok(CommandResult::Output(lines.join("\n")))
}

// ---------------------------------------------------------------------------
// /config sources
// ---------------------------------------------------------------------------

pub(super) fn handle_sources(ctx: &CommandContext) -> Result<CommandResult> {
    if ctx.app_state.settings.sources.is_empty() {
        return Ok(CommandResult::Output(
            "(no settings overrides recorded; every key is default)".into(),
        ));
    }
    let mut lines = vec!["Per-key sources:".into(), String::new()];
    for (k, v) in &ctx.app_state.settings.sources {
        lines.push(format!("  {:<36} {}", k, v.as_str()));
    }
    Ok(CommandResult::Output(lines.join("\n")))
}

// ---------------------------------------------------------------------------
// /config schema
// ---------------------------------------------------------------------------

pub(super) fn handle_schema() -> Result<CommandResult> {
    let schema = settings::settings_schema();
    let pretty =
        serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "(failed to render)".into());
    Ok(CommandResult::Output(pretty))
}

// ---------------------------------------------------------------------------
// /config set
