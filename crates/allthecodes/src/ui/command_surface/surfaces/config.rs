use crossterm::event::KeyEvent;

use crate::ui::better_view_panel::BetterViewPanel;
use crate::ui::command_surface::CommandSurfaceOutcome;
use crate::ui::form_navigation::{FormOption, FormTab, TabbedFormEvent, TabbedFormState};
use crate::ui::selection_surface::{SelectionItem, SelectionSurface, SelectionSurfaceEvent};
use allthecodes_engine::effort::{effort_to_budget_tokens, CapabilityProvenance, EffortTransport};
use allthecodes_engine::types::app_state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSurface {
    pub(crate) state: TabbedFormState,
    model_picker: SelectionSurface,
    theme_picker: SelectionSurface,
    effort_picker: SelectionSurface,
    effort_picker_enabled: bool,
}

impl ConfigSurface {
    pub(crate) fn new(state: &AppState) -> Self {
        let model = if state.main_loop_model.is_empty() {
            "default".to_string()
        } else {
            state.main_loop_model.clone()
        };
        let backend = if state.main_loop_backend.is_empty() {
            "default".to_string()
        } else {
            state.main_loop_backend.clone()
        };
        let output_style = state
            .settings
            .output_style
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let language = state
            .settings
            .language
            .clone()
            .unwrap_or_else(|| "not set".to_string());
        let editor = state
            .settings
            .editor_mode
            .clone()
            .unwrap_or_else(|| "normal".to_string());
        let view_mode = state
            .settings
            .view_mode
            .clone()
            .unwrap_or_else(|| "prompt".to_string());
        let spinner_tips_enabled = state.settings.spinner_tips.enabled.unwrap_or(true);
        let spinner_interval = state.settings.spinner_tips.interval_ms.unwrap_or(10_000);
        let spinner_custom_count = state.settings.spinner_tips.custom_tips.len();
        let fast_mode = if state.fast_mode {
            "true".to_string()
        } else {
            bool_setting_label(state.settings.fast_mode)
        };
        let thinking = state
            .thinking_enabled
            .map(|enabled| enabled.to_string())
            .unwrap_or_else(|| "auto".to_string());

        Self {
            state: TabbedFormState::new(
                "Config",
                vec![
                    FormTab::new(
                        "status",
                        "Status",
                        vec![
                            FormOption::new("show", "Show effective config")
                                .with_description(format!("model={model}; backend={backend}")),
                            FormOption::new("sources", "Show setting sources")
                                .with_description("which layer set each key"),
                        ],
                    ),
                    FormTab::new(
                        "model",
                        "Model",
                        vec![FormOption::new("picker", "Model")
                            .with_description(model_readonly_summary(state))
                            .disabled()],
                    ),
                    FormTab::new(
                        "theme",
                        "Theme",
                        vec![FormOption::new("picker", "Theme picker").disabled()],
                    ),
                    FormTab::new(
                        "usage",
                        "Usage",
                        vec![
                            FormOption::new("cost", "Show session usage")
                                .with_description("token totals and estimated cost"),
                            FormOption::new("extra-usage", "Show extended usage")
                                .with_description("per-message token and cost breakdown"),
                        ],
                    ),
                    FormTab::new(
                        "output",
                        "Output",
                        vec![
                            FormOption::new("style-default", "Use default output style")
                                .with_description(format!("current outputStyle={output_style}")),
                            FormOption::new("style-explanatory", "Use explanatory output style")
                                .with_description(format!("current outputStyle={output_style}")),
                            FormOption::new("style-learning", "Use learning output style")
                                .with_description(format!("current outputStyle={output_style}")),
                            FormOption::new("set-output-style", "Set custom output style")
                                .with_description("fill prompt with /config set outputStyle"),
                            FormOption::new("editor-vim", "Enable vim editor mode")
                                .with_description(format!("current editorMode={editor}")),
                            FormOption::new("editor-normal", "Use normal editor mode")
                                .with_description(format!("current editorMode={editor}")),
                            FormOption::new("view-prompt", "Use prompt view")
                                .with_description(format!("current viewMode={view_mode}")),
                            FormOption::new("view-transcript", "Use transcript view")
                                .with_description(format!("current viewMode={view_mode}")),
                            FormOption::new("view-focus", "Use focus view")
                                .with_description(format!("current viewMode={view_mode}")),
                            FormOption::new("spinner-tips-on", "Enable spinner tips")
                                .with_description(format!(
                                    "enabled={spinner_tips_enabled}; intervalMs={spinner_interval}; customTips={spinner_custom_count}"
                                )),
                            FormOption::new("spinner-tips-off", "Disable spinner tips")
                                .with_description(format!(
                                    "enabled={spinner_tips_enabled}; intervalMs={spinner_interval}; customTips={spinner_custom_count}"
                                )),
                            FormOption::new("spinner-tips-interval", "Set spinner tip interval")
                                .with_description("fill prompt with /config set spinnerTips.intervalMs"),
                            FormOption::new("spinner-tips-custom", "Set custom spinner tips")
                                .with_description("fill prompt with JSON string array"),
                        ],
                    ),
                    FormTab::new(
                        "language",
                        "Language",
                        vec![
                            FormOption::new("set-language", "Set response language")
                                .with_description(format!("current language={language}")),
                            FormOption::new("language-en", "Use English")
                                .with_description(format!("current language={language}")),
                            FormOption::new("language-zh", "Use Chinese")
                                .with_description(format!("current language={language}")),
                        ],
                    ),
                    FormTab::new(
                        "thinking",
                        "Thinking",
                        vec![FormOption::new("picker", "Reasoning effort")
                            .with_description(format!(
                                "current effort={}; thinking={thinking}; fastMode={fast_mode}; use /effort to change",
                                crate::startup_model::resolve_display_effort_label(state)
                                    .as_deref()
                                    .unwrap_or("auto")
                            ))
                            .disabled()],
                    ),
                    FormTab::new(
                        "safety",
                        "Safety",
                        vec![FormOption::new("sources", "Review setting sources")
                            .with_description("managed/user/project/local provenance")],
                    ),
                    FormTab::new(
                        "config",
                        "Config",
                        vec![
                            FormOption::new("set-model", "Set custom model")
                                .with_description("fill prompt with /config set model"),
                            FormOption::new("set-theme", "Set custom theme")
                                .with_description("fill prompt with /config set theme"),
                        ],
                    ),
                ],
            ),
            model_picker: build_model_picker(state),
            theme_picker: build_theme_picker(state),
            effort_picker: build_effort_picker(state),
            effort_picker_enabled: false,
        }
    }

    pub(crate) fn new_thinking_picker(state: &AppState) -> Self {
        let mut surface = Self::new(state);
        surface.effort_picker_enabled = true;
        if let Some(index) = surface
            .state
            .tabs
            .iter()
            .position(|tab| tab.id == "thinking")
        {
            surface.state.active_tab = index;
        }
        surface
    }

    pub(crate) fn render(&self) -> String {
        match self.active_tab_id() {
            Some("model") => {
                let effort = self.current_effort_label();
                self.render_picker(
                    &self.model_picker,
                    &[format!(
                        "Current effort: {}",
                        format_effort(effort.as_deref())
                    )],
                )
            }
            Some("theme") => self.render_picker(&self.theme_picker, &[]),
            Some("thinking") => self.render_picker(&self.effort_picker, &[]),
            _ => self.state.render_lines().join("\n"),
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        if is_tab_navigation_key(&key) {
            let _ = self.state.handle_key(key);
            return CommandSurfaceOutcome::None;
        }

        match self.active_tab_id() {
            Some("model") => {
                let _ = self.model_picker.handle_key(key);
                return CommandSurfaceOutcome::None;
            }
            Some("theme") => {
                return handle_picker_key(&mut self.theme_picker, key, |id| {
                    CommandSurfaceOutcome::Submit(format!("/config set theme {id}"))
                });
            }
            Some("thinking") => {
                if self.effort_picker_enabled {
                    return handle_picker_key(&mut self.effort_picker, key, |id| {
                        CommandSurfaceOutcome::Submit(format!("/effort {id}"))
                    });
                }
                let _ = self.effort_picker.handle_key(key);
                return CommandSurfaceOutcome::None;
            }
            _ => {}
        }

        match self.state.handle_key(key) {
            TabbedFormEvent::Selected { option_id, .. } => match option_id.as_str() {
                "show" => CommandSurfaceOutcome::Submit("/config show".to_string()),
                "sources" => CommandSurfaceOutcome::Submit("/config sources".to_string()),
                "cost" => CommandSurfaceOutcome::Submit("/cost".to_string()),
                "extra-usage" => CommandSurfaceOutcome::Submit("/extra-usage".to_string()),
                "style-default" => {
                    CommandSurfaceOutcome::Submit("/config set outputStyle default".to_string())
                }
                "style-explanatory" => {
                    CommandSurfaceOutcome::Submit("/config set outputStyle explanatory".to_string())
                }
                "style-learning" => {
                    CommandSurfaceOutcome::Submit("/config set outputStyle learning".to_string())
                }
                "set-output-style" => {
                    CommandSurfaceOutcome::FillPrompt("/config set outputStyle ".to_string())
                }
                "set-language" => {
                    CommandSurfaceOutcome::FillPrompt("/config set language ".to_string())
                }
                "language-en" => {
                    CommandSurfaceOutcome::Submit("/config set language English".to_string())
                }
                "language-zh" => {
                    CommandSurfaceOutcome::Submit("/config set language Chinese".to_string())
                }
                "voice-on" => {
                    CommandSurfaceOutcome::Submit("/config set voiceEnabled true".to_string())
                }
                "voice-off" => {
                    CommandSurfaceOutcome::Submit("/config set voiceEnabled false".to_string())
                }
                "editor-vim" => {
                    CommandSurfaceOutcome::Submit("/config set editorMode vim".to_string())
                }
                "editor-normal" => {
                    CommandSurfaceOutcome::Submit("/config set editorMode normal".to_string())
                }
                "view-prompt" => {
                    CommandSurfaceOutcome::Submit("/config set viewMode prompt".to_string())
                }
                "view-transcript" => {
                    CommandSurfaceOutcome::Submit("/config set viewMode transcript".to_string())
                }
                "view-focus" => {
                    CommandSurfaceOutcome::Submit("/config set viewMode focus".to_string())
                }
                "spinner-tips-on" => CommandSurfaceOutcome::Submit(
                    "/config set spinnerTips.enabled true".to_string(),
                ),
                "spinner-tips-off" => CommandSurfaceOutcome::Submit(
                    "/config set spinnerTips.enabled false".to_string(),
                ),
                "spinner-tips-interval" => CommandSurfaceOutcome::FillPrompt(
                    "/config set spinnerTips.intervalMs ".to_string(),
                ),
                "spinner-tips-custom" => CommandSurfaceOutcome::FillPrompt(
                    "/config set spinnerTips.customTips [\"tip\"]".to_string(),
                ),
                "progress-on" => CommandSurfaceOutcome::Submit(
                    "/config set terminalProgressBarEnabled true".to_string(),
                ),
                "progress-off" => CommandSurfaceOutcome::Submit(
                    "/config set terminalProgressBarEnabled false".to_string(),
                ),
                "raw" => CommandSurfaceOutcome::Submit("/config show --raw".to_string()),
                "schema" => CommandSurfaceOutcome::Submit("/config schema".to_string()),
                "set-model" => CommandSurfaceOutcome::FillPrompt("/model ".to_string()),
                "set-theme" => CommandSurfaceOutcome::FillPrompt("/config set theme ".to_string()),
                _ => CommandSurfaceOutcome::None,
            },
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn active_tab_id(&self) -> Option<&str> {
        self.state.active_tab().map(|tab| tab.id.as_str())
    }

    fn current_effort_label(&self) -> Option<String> {
        self.effort_picker
            .items
            .iter()
            .find(|item| item.description.contains("current"))
            .map(|item| item.id.clone())
    }

    fn render_picker(&self, picker: &SelectionSurface, context_lines: &[String]) -> String {
        let mut lines = context_lines.to_vec();
        lines.extend(picker.render_lines(10));
        BetterViewPanel::new(&self.state.title)
            .sections(
                self.state
                    .tabs
                    .iter()
                    .map(|tab| tab.label.clone())
                    .collect::<Vec<_>>(),
                self.state.active_tab,
            )
            .detail_title(picker.title.clone())
            .detail_lines(lines)
            .footer(
                "Type filter | Up/Down navigate | Enter select | Left/Right section | Esc close",
            )
            .render()
    }
}

fn handle_picker_key(
    picker: &mut SelectionSurface,
    key: KeyEvent,
    submit: impl FnOnce(String) -> CommandSurfaceOutcome,
) -> CommandSurfaceOutcome {
    match picker.handle_key(key) {
        SelectionSurfaceEvent::Selected(id) => submit(id),
        SelectionSurfaceEvent::Closed => CommandSurfaceOutcome::Close,
        SelectionSurfaceEvent::None => CommandSurfaceOutcome::None,
    }
}

fn is_tab_navigation_key(key: &KeyEvent) -> bool {
    use crossterm::event::{KeyCode, KeyEventKind};

    if key.kind != KeyEventKind::Press {
        return false;
    }

    matches!(
        key.code,
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab
    ) || matches!(key.code, KeyCode::Char(ch) if ch.is_ascii_digit())
}

pub(super) fn build_model_picker(state: &AppState) -> SelectionSurface {
    let current = if state.main_loop_model.is_empty() {
        AppState::default().main_loop_model
    } else {
        state.main_loop_model.clone()
    };
    let mut items = Vec::new();

    let profile_models = active_profile_model_ids(state);
    if profile_models.is_empty() {
        items.push(SelectionItem {
            id: "unsupported".to_string(),
            label: "No profile models".to_string(),
            description: "current profile has no modelCapabilities".to_string(),
            enabled: false,
            disabled_reason: Some("use /login first".to_string()),
            preview_lines: Vec::new(),
            actions: Vec::new(),
            search_terms: Vec::new(),
        });
    } else {
        for model in profile_models {
            let capability = capability_for_model(state, &model);
            push_model_item(&mut items, &model, &model, &current, capability.as_ref());
        }
    }

    let selected = items
        .iter()
        .position(|item| item.id == current)
        .unwrap_or(0);
    let mut picker = SelectionSurface::new("Model", items);
    picker.selected = selected;
    picker
}

fn push_model_item(
    items: &mut Vec<SelectionItem>,
    id: &str,
    model: &str,
    current: &str,
    capability: Option<&allthecodes_config::settings::ModelCapabilitySettings>,
) {
    if items.iter().any(|item| item.id == id) {
        return;
    }

    let label = capability
        .map(|capability| format!("{} ({model})", capability.display_name_or(model)))
        .unwrap_or_else(|| model.to_string());
    let mut description = Vec::new();
    if model == current {
        description.push("current".to_string());
    }
    if let Some(capability) = capability {
        if let Some(context_window) = capability.context_window {
            description.push(format!("ctx={}k", context_window / 1000));
        }
        if let Some(effort) = capability.default_reasoning_level.as_deref() {
            description.push(format!("default effort={effort}"));
        }
        if capability.supports_fast_mode {
            description.push("fast".to_string());
        }
        if capability.supports_search_tool {
            description.push("search".to_string());
        }
        if capability.supports_parallel_tool_calls {
            description.push("parallel tools".to_string());
        }
    }

    items.push(SelectionItem {
        id: id.to_string(),
        label,
        description: if description.is_empty() {
            "configured".to_string()
        } else {
            description.join("; ")
        },
        enabled: true,
        disabled_reason: None,
        preview_lines: Vec::new(),
        actions: Vec::new(),
        search_terms: Vec::new(),
    });
}

fn active_profile_model_ids(state: &AppState) -> Vec<String> {
    let Some(active) = state
        .settings
        .active_auth_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let Some(profile) = state.settings.auth_profiles.get(active) else {
        return Vec::new();
    };
    let Some(capabilities) = profile
        .model_capabilities
        .as_ref()
        .filter(|capabilities| !capabilities.is_empty())
        .or_else(|| {
            (!state.settings.model_capabilities.is_empty())
                .then_some(&state.settings.model_capabilities)
        })
    else {
        return Vec::new();
    };
    let configured = profile
        .available_models
        .as_deref()
        .filter(|models| !models.is_empty())
        .unwrap_or(&state.settings.available_models);
    let mut models = if configured.is_empty() {
        capabilities.keys().cloned().collect::<Vec<_>>()
    } else {
        configured
            .iter()
            .map(|model| model.trim())
            .filter(|model| !model.is_empty())
            .filter(|model| capabilities.contains_key(*model))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    models.sort();
    models
}

fn capability_for_model(
    state: &AppState,
    model: &str,
) -> Option<allthecodes_config::settings::ModelCapabilitySettings> {
    state
        .settings
        .model_capabilities
        .get(model)
        .cloned()
        .or_else(|| {
            let active = state.settings.active_auth_profile.as_deref()?;
            state
                .settings
                .auth_profiles
                .get(active)?
                .model_capabilities
                .as_ref()?
                .get(model)
                .cloned()
        })
}

fn model_readonly_summary(state: &AppState) -> String {
    let profile = state
        .settings
        .active_auth_profile
        .as_deref()
        .unwrap_or("none");
    let context = capability_for_model(state, &state.main_loop_model)
        .and_then(|capability| capability.context_window)
        .map(|tokens| format!("{}k", tokens / 1000))
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "profile={profile}; model={}; context={context}; use /model to change",
        state.main_loop_model
    )
}

fn build_theme_picker(state: &AppState) -> SelectionSurface {
    let current = state.settings.theme.as_deref().unwrap_or("dark");
    let mut items = vec![
        theme_item("auto", "Auto", "match terminal when supported", current),
        theme_item("dark", "Dark mode", "default ratatui palette", current),
        theme_item("light", "Light mode", "light terminal palette", current),
        theme_item(
            "solarized",
            "Solarized",
            "low-contrast terminal palette",
            current,
        ),
        theme_item(
            "monokai",
            "Monokai",
            "high-contrast editor palette",
            current,
        ),
        theme_item("nord", "Nord", "cool low-saturation palette", current),
    ];

    if !items.iter().any(|item| item.id == current) {
        items.push(theme_item(
            current,
            format!("Custom ({current})"),
            "current custom theme",
            current,
        ));
    }

    let selected = items
        .iter()
        .position(|item| item.id == current)
        .unwrap_or(1);
    let mut picker = SelectionSurface::new("Theme", items);
    picker.selected = selected;
    picker
}

fn theme_item(
    id: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
    current: &str,
) -> SelectionItem {
    let id = id.into();
    let mut description = description.into();
    if id == current {
        if description.is_empty() {
            description = "current".to_string();
        } else {
            description.push_str("; current");
        }
    }
    SelectionItem {
        id,
        label: label.into(),
        description,
        enabled: true,
        disabled_reason: None,
        preview_lines: Vec::new(),
        actions: Vec::new(),
        search_terms: Vec::new(),
    }
}

fn build_effort_picker(state: &AppState) -> SelectionSurface {
    let transport = active_transport(state);
    let (capability_provenance, resolved) = resolved_effort(state);
    let current_effort = resolved.as_deref();
    let Some(capability) = capability_for_model(state, &state.main_loop_model) else {
        let mut picker = SelectionSurface::new(
            "Effort",
            vec![SelectionItem {
                id: "unsupported".to_string(),
                label: "Reasoning levels unavailable".to_string(),
                description: "current profile has no reasoning metadata".to_string(),
                enabled: false,
                disabled_reason: Some("use /login and /model first".to_string()),
                preview_lines: Vec::new(),
                actions: Vec::new(),
                search_terms: Vec::new(),
            }],
        );
        picker.selected = 0;
        return picker;
    };
    if capability.supported_reasoning_levels.is_empty() {
        let mut picker = SelectionSurface::new(
            "Effort",
            vec![SelectionItem {
                id: "unsupported".to_string(),
                label: "Reasoning levels unavailable".to_string(),
                description: "current profile did not configure supportedReasoningLevels"
                    .to_string(),
                enabled: false,
                disabled_reason: Some("read-only".to_string()),
                preview_lines: Vec::new(),
                actions: Vec::new(),
                search_terms: Vec::new(),
            }],
        );
        picker.selected = 0;
        return picker;
    }

    let current = match current_effort {
        Some("max")
            if capability
                .supported_reasoning_levels
                .iter()
                .any(|level| level == "xhigh")
                && !capability
                    .supported_reasoning_levels
                    .iter()
                    .any(|level| level == "max") =>
        {
            Some("xhigh")
        }
        other => other,
    };
    let default = capability
        .default_reasoning_level
        .as_deref()
        .unwrap_or("model default");
    let supported_phrase = match capability_provenance {
        CapabilityProvenance::Bundled => "Bundled levels",
        CapabilityProvenance::UserProfile => "Configured levels",
        CapabilityProvenance::None => "Supported levels",
    };
    let mut items = vec![effort_item(
        "auto",
        "Auto",
        format!("model default: {default}"),
        None,
        transport,
    )];
    for level in &capability.supported_reasoning_levels {
        let label = match level.as_str() {
            "low" => "Low".to_string(),
            "medium" => "Medium".to_string(),
            "high" => "High".to_string(),
            "xhigh" => "Extra high".to_string(),
            "max" => "Maximum".to_string(),
            other => other.to_string(),
        };
        let description = if capability.default_reasoning_level.as_deref() == Some(level.as_str()) {
            "supported; default"
        } else {
            "supported"
        };
        items.push(effort_item(level, label, description, current, transport));
    }
    if let Some(header_idx) = items.first().map(|_| 0) {
        // Tag the first item with the supported-levels provenance so the
        // caller surface can render the headline phrase (Bundled/Configured/
        // Supported levels) without a separate query.
        items[header_idx]
            .search_terms
            .push(supported_phrase.to_string());
    }
    let selected = current
        .and_then(|value| items.iter().position(|item| item.id == value))
        .unwrap_or(0);
    let mut picker = SelectionSurface::new("Effort", items);
    picker.selected = selected;
    picker
}

/// Resolve the effective effort for the active profile via the central
/// resolver so the picker shows the same value the wire builder will send.
///
/// Delegates to [`crate::startup_model::resolve_display_effort`] so the
/// status line, `/effort` display, and picker share one resolution path
/// (plan §3 phase D4).
fn resolved_effort(state: &AppState) -> (CapabilityProvenance, Option<String>) {
    match crate::startup_model::resolve_display_effort(state) {
        Some(resolved) => (resolved.capability_provenance, Some(resolved.level)),
        None => (CapabilityProvenance::None, None),
    }
}

/// The wire transport for the active profile's `api_provider`. Used to gate
/// the misleading "X tokens" suffix to the Anthropic fixed-budget transport
/// only; Codex and passthrough providers show the level alone.
fn active_transport(state: &AppState) -> EffortTransport {
    let api_provider = state
        .settings
        .active_auth_profile
        .as_deref()
        .and_then(|name| state.settings.auth_profiles.get(name))
        .and_then(|profile| profile.api_provider.as_deref());
    EffortTransport::from_api_provider(api_provider)
}

fn effort_item(
    id: impl Into<String>,
    label: impl Into<String>,
    description: impl Into<String>,
    current: Option<&str>,
    transport: EffortTransport,
) -> SelectionItem {
    let id = id.into();
    let mut details = vec![description.into()];
    // Token text is meaningful only for the Anthropic fixed-budget transport,
    // where it represents the local `thinking.budget_tokens` request cap. For
    // Codex (`reasoning.effort`) and passthrough transports the upstream
    // server controls the actual budget, so showing "X tokens" here would be
    // a false claim.
    if transport == EffortTransport::Anthropic {
        if let Some(tokens) = effort_to_budget_tokens(&id) {
            details.push(format!("local request budget: {tokens} tokens"));
        }
    }
    if current == Some(id.as_str()) {
        details.push("current".to_string());
    }

    SelectionItem {
        id,
        label: label.into(),
        description: details.join("; "),
        enabled: true,
        disabled_reason: None,
        preview_lines: Vec::new(),
        actions: Vec::new(),
        search_terms: Vec::new(),
    }
}

/// Format the current effort line in the model tab.
///
/// Token-count text is intentionally omitted here. With the transport-aware
/// resolver, the model tab does not know which wire transport a level came
/// from, and a fixed `(N tokens)` suffix is wrong for the Codex
/// `reasoning.effort` and passthrough transports (plan §3 phase D2). The
/// effort picker's per-item descriptions already carry the localized
/// "local request budget: N tokens" line for the Anthropic fixed-budget case.
fn format_effort(value: Option<&str>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not set".to_string())
}

fn bool_setting_label(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not set".to_string())
}
